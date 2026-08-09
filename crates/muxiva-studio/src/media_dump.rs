use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread,
};

use muxiva_core::{FrameObservation, FrameObservationDirection, PortName, RuntimeObserver};
use muxiva_types::{
    AudioData, AudioLayout, Frame, NodeId, PcmSampleFormat, VideoData, VideoLayout,
};
use serde::{Deserialize, Serialize};

const CAPTURE_QUEUE_FRAMES: usize = 256;
const MAX_SESSION_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;
const RETAINED_SESSIONS: usize = 4;
const MANIFEST_FILE: &str = "manifest.json";

#[derive(Clone, Deserialize, Serialize)]
pub struct MediaArtifact {
    pub id: String,
    pub node_id: String,
    pub direction: String,
    pub port: String,
    pub kind: String,
    pub format: String,
    pub file_name: String,
    pub content_type: String,
    pub frames: u64,
    pub bytes: u64,
    pub duration_ms: u64,
    pub playable: bool,
    pub ready: bool,
    pub truncated: bool,
    pub details: serde_json::Value,
}

#[derive(Clone, Deserialize, Serialize)]
struct MediaSessionStatus {
    run_id: String,
    status: String,
    bytes: u64,
    frames: u64,
    truncated: bool,
    last_error: Option<String>,
    artifacts: Vec<MediaArtifact>,
}

impl MediaSessionStatus {
    fn new(run_id: String) -> Self {
        Self {
            run_id,
            status: "running".into(),
            bytes: 0,
            frames: 0,
            truncated: false,
            last_error: None,
            artifacts: Vec::new(),
        }
    }
}

struct CapturedFrame {
    node_id: NodeId,
    port: PortName,
    direction: FrameObservationDirection,
    frame: Frame,
}

enum Command {
    Start(String),
    Capture(Box<CapturedFrame>),
    Finish(String),
}

/// Studio-owned bounded media diagnostics. Runtime workers only perform one
/// cheap Frame clone and a non-blocking channel send.
pub struct MediaDumpStore {
    root: PathBuf,
    enabled: AtomicBool,
    dropped_frames: AtomicU64,
    sender: SyncSender<Command>,
    sessions: Arc<Mutex<BTreeMap<String, MediaSessionStatus>>>,
    active: Mutex<Option<(String, bool)>>,
}

impl MediaDumpStore {
    pub fn new(graph: &Path) -> Self {
        let root = graph
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(".muxiva/observability/media");
        let sessions = Arc::new(Mutex::new(load_sessions(&root)));
        let (sender, receiver) = mpsc::sync_channel(CAPTURE_QUEUE_FRAMES);
        let worker_sessions = sessions.clone();
        let worker_root = root.clone();
        thread::Builder::new()
            .name("muxiva-media-dump".into())
            .spawn(move || writer_loop(receiver, &worker_root, &worker_sessions))
            .expect("Studio media dump worker must start");
        Self {
            root,
            enabled: AtomicBool::new(false),
            dropped_frames: AtomicU64::new(0),
            sender,
            sessions,
            active: Mutex::new(None),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
    }

    pub fn start_session(&self, run_id: &str) {
        self.dropped_frames.store(0, Ordering::Relaxed);
        *self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some((run_id.to_owned(), false));
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        sessions.insert(
            run_id.to_owned(),
            MediaSessionStatus::new(run_id.to_owned()),
        );
        while sessions.len() > RETAINED_SESSIONS {
            let Some(oldest) = sessions.keys().next().cloned() else {
                break;
            };
            sessions.remove(&oldest);
        }
        drop(sessions);
        let _ = self.sender.send(Command::Start(run_id.to_owned()));
    }

    pub fn finish_session(&self, run_id: &str) {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some((current, finished)) = active.as_mut() else {
            return;
        };
        if current != run_id || *finished {
            return;
        }
        *finished = true;
        let _ = self.sender.send(Command::Finish(run_id.to_owned()));
    }

    pub fn status_json(&self, requested_run_id: Option<&str>) -> serde_json::Value {
        let sessions = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let active_run_id = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .map(|(run_id, _)| run_id.clone());
        let selected = match requested_run_id {
            Some(run_id) => sessions.get(run_id),
            None => active_run_id
                .as_deref()
                .and_then(|run_id| sessions.get(run_id))
                .or_else(|| sessions.values().next_back()),
        }
        .cloned();
        serde_json::json!({
            "enabled": self.enabled.load(Ordering::Acquire),
            "active_run_id": active_run_id,
            "dropped_frames": self.dropped_frames.load(Ordering::Relaxed),
            "limits": {
                "session_bytes": MAX_SESSION_BYTES,
                "artifact_bytes": MAX_ARTIFACT_BYTES,
                "queue_frames": CAPTURE_QUEUE_FRAMES,
                "retained_sessions": RETAINED_SESSIONS,
            },
            "session": selected,
        })
    }

    pub fn read_artifact(
        &self,
        run_id: &str,
        artifact_id: &str,
    ) -> Result<(String, String, Vec<u8>), String> {
        if !safe_component(run_id) || !safe_component(artifact_id) {
            return Err("invalid media artifact path".into());
        }
        let artifact = self
            .sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(run_id)
            .and_then(|session| session.artifacts.iter().find(|item| item.id == artifact_id))
            .cloned()
            .ok_or_else(|| "media artifact not found".to_owned())?;
        if !artifact.ready || !safe_component(&artifact.file_name) {
            return Err("media artifact is not ready".into());
        }
        let bytes = fs::read(self.root.join(run_id).join(&artifact.file_name))
            .map_err(|error| format!("failed to read media artifact: {error}"))?;
        if bytes.len() as u64 > MAX_ARTIFACT_BYTES + 44 {
            return Err("media artifact exceeds its configured limit".into());
        }
        Ok((artifact.content_type, artifact.file_name, bytes))
    }
}

impl RuntimeObserver for MediaDumpStore {
    fn observe_frame(&self, observation: FrameObservation<'_>) {
        if !self.enabled.load(Ordering::Acquire)
            || (!matches!(observation.frame(), Frame::Audio(_))
                && !matches!(observation.frame(), Frame::Video(_)))
        {
            return;
        }
        let command = Command::Capture(Box::new(CapturedFrame {
            node_id: observation.node_id().clone(),
            port: observation.port().clone(),
            direction: observation.direction(),
            frame: observation.frame().clone(),
        }));
        if let Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) =
            self.sender.try_send(command)
        {
            self.dropped_frames.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn writer_loop(
    receiver: mpsc::Receiver<Command>,
    root: &Path,
    sessions: &Arc<Mutex<BTreeMap<String, MediaSessionStatus>>>,
) {
    let mut current: Option<SessionWriter> = None;
    while let Ok(command) = receiver.recv() {
        match command {
            Command::Start(run_id) => {
                if let Some(mut writer) = current.take() {
                    writer.finish();
                    writer.publish(sessions);
                }
                current = match SessionWriter::new(root, run_id.clone()) {
                    Ok(writer) => Some(writer),
                    Err(error) => {
                        if let Some(status) = sessions
                            .lock()
                            .unwrap_or_else(|value| value.into_inner())
                            .get_mut(&run_id)
                        {
                            status.status = "failed".into();
                            status.last_error = Some(error);
                        }
                        None
                    }
                };
                prune_sessions(root);
            }
            Command::Capture(captured) => {
                if let Some(writer) = current.as_mut() {
                    writer.capture(*captured);
                    writer.publish(sessions);
                }
            }
            Command::Finish(run_id) => {
                if current
                    .as_ref()
                    .is_some_and(|writer| writer.status.run_id == run_id)
                {
                    let mut writer = current.take().expect("checked current writer");
                    writer.finish();
                    writer.publish(sessions);
                }
            }
        }
    }
    if let Some(mut writer) = current {
        writer.finish();
        writer.publish(sessions);
    }
}

struct SessionWriter {
    directory: PathBuf,
    status: MediaSessionStatus,
    writers: BTreeMap<String, ArtifactWriter>,
    next_artifact: u64,
}

impl SessionWriter {
    fn new(root: &Path, run_id: String) -> Result<Self, String> {
        let directory = root.join(&run_id);
        fs::create_dir_all(&directory)
            .map_err(|error| format!("failed to create media dump directory: {error}"))?;
        Ok(Self {
            directory,
            status: MediaSessionStatus::new(run_id),
            writers: BTreeMap::new(),
            next_artifact: 0,
        })
    }

    fn capture(&mut self, captured: CapturedFrame) {
        if self.status.bytes >= MAX_SESSION_BYTES {
            self.status.truncated = true;
            return;
        }
        let (key, specification) = match ArtifactSpecification::from_capture(&captured) {
            Some(value) => value,
            None => return,
        };
        if !self.writers.contains_key(&key) {
            self.next_artifact += 1;
            let id = format!("media-{:04}", self.next_artifact);
            match ArtifactWriter::new(&self.directory, id, &captured, specification) {
                Ok(writer) => {
                    self.status.artifacts.push(writer.metadata.clone());
                    self.writers.insert(key.clone(), writer);
                }
                Err(error) => {
                    self.status.last_error = Some(error);
                    return;
                }
            }
        }
        let Some(writer) = self.writers.get_mut(&key) else {
            return;
        };
        let remaining_session = MAX_SESSION_BYTES.saturating_sub(self.status.bytes);
        match writer.write_frame(&captured.frame, remaining_session) {
            Ok(written) => {
                self.status.bytes = self.status.bytes.saturating_add(written);
                if written != 0 {
                    self.status.frames = self.status.frames.saturating_add(1);
                }
                if let Some(metadata) = self
                    .status
                    .artifacts
                    .iter_mut()
                    .find(|item| item.id == writer.metadata.id)
                {
                    *metadata = writer.metadata.clone();
                }
            }
            Err(error) => self.status.last_error = Some(error),
        }
        if writer.metadata.truncated || self.status.bytes >= MAX_SESSION_BYTES {
            self.status.truncated = true;
        }
    }

    fn finish(&mut self) {
        for writer in self.writers.values_mut() {
            if let Err(error) = writer.finish() {
                self.status.last_error = Some(error);
            }
            if let Some(metadata) = self
                .status
                .artifacts
                .iter_mut()
                .find(|item| item.id == writer.metadata.id)
            {
                *metadata = writer.metadata.clone();
            }
        }
        self.status.status = if self.status.last_error.is_some() {
            "completed-with-errors".into()
        } else {
            "completed".into()
        };
        if let Err(error) = self.persist_manifest() {
            self.status.status = "completed-with-errors".into();
            self.status.last_error = Some(error);
            let _ = self.persist_manifest();
        }
    }

    fn persist_manifest(&self) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(&self.status)
            .map_err(|error| format!("failed to serialize media dump manifest: {error}"))?;
        let temporary = self.directory.join("manifest.json.tmp");
        fs::write(&temporary, bytes)
            .map_err(|error| format!("failed to write media dump manifest: {error}"))?;
        fs::rename(temporary, self.directory.join(MANIFEST_FILE))
            .map_err(|error| format!("failed to publish media dump manifest: {error}"))
    }

    fn publish(&self, sessions: &Arc<Mutex<BTreeMap<String, MediaSessionStatus>>>) {
        sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(self.status.run_id.clone(), self.status.clone());
    }
}

#[derive(Clone)]
enum ArtifactSpecification {
    Audio {
        sample_rate_hz: u32,
        channels: u16,
        sample_format: PcmSampleFormat,
    },
    Video {
        details: serde_json::Value,
    },
}

impl ArtifactSpecification {
    fn from_capture(captured: &CapturedFrame) -> Option<(String, Self)> {
        let direction = direction_name(captured.direction);
        match &captured.frame {
            Frame::Audio(frame) => {
                let data = frame.data();
                let key = format!(
                    "{}|{}|{}|audio|{}|{}|{:?}",
                    captured.node_id,
                    direction,
                    captured.port,
                    data.sample_rate_hz(),
                    data.channels(),
                    data.sample_format()
                );
                Some((
                    key,
                    Self::Audio {
                        sample_rate_hz: data.sample_rate_hz(),
                        channels: data.channels(),
                        sample_format: data.sample_format(),
                    },
                ))
            }
            Frame::Video(frame) => {
                let data = frame.data();
                let (format, planes) = video_details(data);
                let plane_signature = serde_json::to_string(&planes).ok()?;
                let key = format!(
                    "{}|{}|{}|video|{}|{}|{}|{}|{}",
                    captured.node_id,
                    direction,
                    captured.port,
                    data.width(),
                    data.height(),
                    format,
                    data.buffer().len(),
                    plane_signature,
                );
                Some((
                    key,
                    Self::Video {
                        details: serde_json::json!({
                            "width": data.width(),
                            "height": data.height(),
                            "pixel_format": format,
                            "frame_bytes": data.buffer().len(),
                            "planes": planes,
                        }),
                    },
                ))
            }
            _ => None,
        }
    }
}

struct ArtifactWriter {
    file: File,
    metadata: MediaArtifact,
    kind: ArtifactWriterKind,
    data_bytes: u64,
    duration_ns: u64,
}

#[derive(Clone, Copy)]
enum ArtifactWriterKind {
    Audio {
        sample_rate_hz: u32,
        channels: u16,
        sample_format: PcmSampleFormat,
    },
    Video,
}

impl ArtifactWriter {
    fn new(
        directory: &Path,
        id: String,
        captured: &CapturedFrame,
        specification: ArtifactSpecification,
    ) -> Result<Self, String> {
        let direction = direction_name(captured.direction).to_owned();
        let (extension, content_type, format, details, kind) = match specification {
            ArtifactSpecification::Audio {
                sample_rate_hz,
                channels,
                sample_format,
            } => (
                "wav",
                "audio/wav",
                format!("{:?}", sample_format).to_lowercase(),
                serde_json::json!({
                    "sample_rate_hz": sample_rate_hz,
                    "channels": channels,
                    "sample_format": format!("{:?}", sample_format).to_lowercase(),
                    "layout": "interleaved",
                }),
                ArtifactWriterKind::Audio {
                    sample_rate_hz,
                    channels,
                    sample_format,
                },
            ),
            ArtifactSpecification::Video { details } => (
                "rawvideo",
                "application/octet-stream",
                details["pixel_format"].as_str().unwrap_or("raw").to_owned(),
                details,
                ArtifactWriterKind::Video,
            ),
        };
        let file_name = format!("{id}.{extension}");
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .read(true)
            .open(directory.join(&file_name))
            .map_err(|error| format!("failed to create media artifact: {error}"))?;
        if let ArtifactWriterKind::Audio {
            sample_rate_hz,
            channels,
            sample_format,
        } = kind
        {
            write_wav_header(&mut file, sample_rate_hz, channels, sample_format, 0)?;
        }
        Ok(Self {
            file,
            metadata: MediaArtifact {
                id,
                node_id: captured.node_id.as_str().to_owned(),
                direction,
                port: captured.port.as_str().to_owned(),
                kind: match kind {
                    ArtifactWriterKind::Audio { .. } => "audio",
                    ArtifactWriterKind::Video => "video",
                }
                .into(),
                format,
                file_name,
                content_type: content_type.into(),
                frames: 0,
                bytes: if matches!(kind, ArtifactWriterKind::Audio { .. }) {
                    44
                } else {
                    0
                },
                duration_ms: 0,
                playable: true,
                ready: false,
                truncated: false,
                details,
            },
            kind,
            data_bytes: 0,
            duration_ns: 0,
        })
    }

    fn write_frame(&mut self, frame: &Frame, remaining_session: u64) -> Result<u64, String> {
        let (bytes, duration_ns) = match (&self.kind, frame) {
            (ArtifactWriterKind::Audio { .. }, Frame::Audio(frame)) => {
                (interleaved_audio(frame.data())?, frame.data().duration_ns())
            }
            (ArtifactWriterKind::Video, Frame::Video(frame)) => {
                (frame.data().buffer().as_slice().to_vec(), 0)
            }
            _ => return Err("media artifact received an incompatible Frame".into()),
        };
        let allowed = MAX_ARTIFACT_BYTES
            .saturating_sub(self.data_bytes)
            .min(remaining_session);
        if bytes.len() as u64 > allowed {
            self.metadata.truncated = true;
            return Ok(0);
        }
        self.file
            .write_all(&bytes)
            .map_err(|error| format!("failed to write media artifact: {error}"))?;
        self.data_bytes = self.data_bytes.saturating_add(bytes.len() as u64);
        self.duration_ns = self.duration_ns.saturating_add(duration_ns);
        self.metadata.frames = self.metadata.frames.saturating_add(1);
        self.metadata.bytes = self.data_bytes
            + if matches!(self.kind, ArtifactWriterKind::Audio { .. }) {
                44
            } else {
                0
            };
        self.metadata.duration_ms = self.duration_ns / 1_000_000;
        Ok(bytes.len() as u64)
    }

    fn finish(&mut self) -> Result<(), String> {
        if let ArtifactWriterKind::Audio {
            sample_rate_hz,
            channels,
            sample_format,
        } = self.kind
        {
            self.file
                .seek(SeekFrom::Start(0))
                .map_err(|error| format!("failed to finalize WAV: {error}"))?;
            write_wav_header(
                &mut self.file,
                sample_rate_hz,
                channels,
                sample_format,
                self.data_bytes,
            )?;
        } else if self.metadata.frames > 1 && self.metadata.duration_ms == 0 {
            self.metadata.duration_ms = self.metadata.frames.saturating_mul(33);
        }
        self.file
            .flush()
            .map_err(|error| format!("failed to flush media artifact: {error}"))?;
        self.metadata.ready = true;
        Ok(())
    }
}

fn interleaved_audio(data: &AudioData) -> Result<Vec<u8>, String> {
    if data.layout() == AudioLayout::Interleaved {
        return Ok(data.buffer().as_slice().to_vec());
    }
    let sample_width = data.sample_format().bytes_per_sample();
    let samples = usize::try_from(data.samples_per_channel())
        .map_err(|_| "audio sample count is too large to dump".to_owned())?;
    let channels = usize::from(data.channels());
    let mut output = Vec::with_capacity(data.buffer().len());
    let planes = (0..data.channels())
        .map(|channel| data.plane_bytes(channel).map_err(|error| error.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    for sample in 0..samples {
        let offset = sample.saturating_mul(sample_width);
        for plane in planes.iter().take(channels) {
            output.extend_from_slice(&plane[offset..offset + sample_width]);
        }
    }
    Ok(output)
}

fn write_wav_header(
    file: &mut File,
    sample_rate_hz: u32,
    channels: u16,
    sample_format: PcmSampleFormat,
    data_bytes: u64,
) -> Result<(), String> {
    let data_bytes = u32::try_from(data_bytes).map_err(|_| "WAV data exceeds 4 GiB".to_owned())?;
    let bits = u16::try_from(sample_format.bytes_per_sample() * 8)
        .map_err(|_| "invalid WAV sample width".to_owned())?;
    let format_code = if matches!(
        sample_format,
        PcmSampleFormat::F32Le | PcmSampleFormat::F64Le
    ) {
        3_u16
    } else {
        1_u16
    };
    let block_align = channels
        .checked_mul(bits / 8)
        .ok_or_else(|| "invalid WAV block alignment".to_owned())?;
    let byte_rate = sample_rate_hz
        .checked_mul(u32::from(block_align))
        .ok_or_else(|| "invalid WAV byte rate".to_owned())?;
    let riff_size = data_bytes.saturating_add(36);
    let mut header = Vec::with_capacity(44);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&riff_size.to_le_bytes());
    header.extend_from_slice(b"WAVEfmt ");
    header.extend_from_slice(&16_u32.to_le_bytes());
    header.extend_from_slice(&format_code.to_le_bytes());
    header.extend_from_slice(&channels.to_le_bytes());
    header.extend_from_slice(&sample_rate_hz.to_le_bytes());
    header.extend_from_slice(&byte_rate.to_le_bytes());
    header.extend_from_slice(&block_align.to_le_bytes());
    header.extend_from_slice(&bits.to_le_bytes());
    header.extend_from_slice(b"data");
    header.extend_from_slice(&data_bytes.to_le_bytes());
    file.write_all(&header)
        .map_err(|error| format!("failed to write WAV header: {error}"))
}

fn video_details(data: &VideoData) -> (&'static str, serde_json::Value) {
    match data.layout() {
        VideoLayout::Rgba8 { plane } => ("rgba8", serde_json::json!([plane_json(plane)])),
        VideoLayout::Yuv420p { y, u, v } => (
            "yuv420p",
            serde_json::json!([plane_json(y), plane_json(u), plane_json(v)]),
        ),
    }
}

fn plane_json(plane: &muxiva_types::VideoPlane) -> serde_json::Value {
    serde_json::json!({
        "offset": plane.offset(),
        "stride": plane.stride(),
        "row_bytes": plane.row_bytes(),
        "rows": plane.rows(),
    })
}

fn direction_name(direction: FrameObservationDirection) -> &'static str {
    match direction {
        FrameObservationDirection::Input => "input",
        FrameObservationDirection::Output => "output",
    }
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn load_sessions(root: &Path) -> BTreeMap<String, MediaSessionStatus> {
    let Ok(entries) = fs::read_dir(root) else {
        return BTreeMap::new();
    };
    let mut sessions = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let bytes = fs::read(entry.path().join(MANIFEST_FILE)).ok()?;
            let status = serde_json::from_slice::<MediaSessionStatus>(&bytes).ok()?;
            safe_component(&status.run_id).then_some((status.run_id.clone(), status))
        })
        .collect::<BTreeMap<_, _>>();
    while sessions.len() > RETAINED_SESSIONS {
        let Some(oldest) = sessions.keys().next().cloned() else {
            break;
        };
        sessions.remove(&oldest);
    }
    sessions
}

fn prune_sessions(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    let mut directories = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    directories.sort();
    let remove = directories.len().saturating_sub(RETAINED_SESSIONS);
    for directory in directories.into_iter().take(remove) {
        let _ = fs::remove_dir_all(directory);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        load_sessions, write_wav_header, CapturedFrame, FrameObservationDirection, PortName,
        SessionWriter,
    };
    use muxiva_types::{NodeId, PcmSampleFormat};
    use std::{fs, io::Write};

    #[test]
    fn wav_header_is_standard_and_bounded() {
        let path = std::env::temp_dir().join(format!(
            "muxiva-media-dump-wav-{}-{}.wav",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let mut file = fs::File::create(&path).unwrap();
        write_wav_header(&mut file, 16_000, 1, PcmSampleFormat::I16Le, 4).unwrap();
        file.write_all(&[0, 0, 1, 0]).unwrap();
        drop(file);
        let bytes = fs::read(&path).unwrap();
        let _ = fs::remove_file(path);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 4);
        assert_eq!(bytes.len(), 48);
    }

    #[test]
    fn session_splits_node_input_and_output_into_playable_tracks() {
        let root = std::env::temp_dir().join(format!(
            "muxiva-media-dump-session-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut writer = SessionWriter::new(&root, "run-1".into()).unwrap();
        for direction in [
            FrameObservationDirection::Input,
            FrameObservationDirection::Output,
        ] {
            writer.capture(CapturedFrame {
                node_id: NodeId::new("audio-node").unwrap(),
                port: PortName::new(if direction == FrameObservationDirection::Input {
                    "audio_in"
                } else {
                    "audio_out"
                })
                .unwrap(),
                direction,
                frame: muxiva_testkit::audio_frame(1),
            });
        }
        writer.finish();
        assert_eq!(writer.status.artifacts.len(), 2);
        assert!(writer
            .status
            .artifacts
            .iter()
            .all(|artifact| artifact.ready && artifact.content_type == "audio/wav"));
        assert!(writer.status.artifacts.iter().all(|artifact| {
            fs::read(root.join("run-1").join(&artifact.file_name))
                .is_ok_and(|bytes| bytes.starts_with(b"RIFF"))
        }));
        drop(writer);

        let restored = load_sessions(&root);
        let restored_session = restored.get("run-1").expect("persisted media session");
        assert_eq!(restored_session.status, "completed");
        assert_eq!(restored_session.artifacts.len(), 2);
        assert!(restored_session
            .artifacts
            .iter()
            .all(|artifact| artifact.ready));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_preserves_raw_video_frames_and_layout_metadata() {
        let root = std::env::temp_dir().join(format!(
            "muxiva-media-dump-video-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut writer = SessionWriter::new(&root, "run-video".into()).unwrap();
        for sequence in [1, 2] {
            writer.capture(CapturedFrame {
                node_id: NodeId::new("camera-node").unwrap(),
                port: PortName::new("video_out").unwrap(),
                direction: FrameObservationDirection::Output,
                frame: muxiva_testkit::rgba_video_frame(sequence, 2, 2),
            });
        }
        writer.finish();
        let artifact = writer.status.artifacts.first().unwrap();
        assert_eq!(artifact.kind, "video");
        assert_eq!(artifact.format, "rgba8");
        assert_eq!(artifact.frames, 2);
        assert_eq!(artifact.bytes, 32);
        assert_eq!(artifact.details["frame_bytes"], 16);
        assert_eq!(artifact.details["planes"][0]["stride"], 8);
        let bytes = fs::read(root.join("run-video").join(&artifact.file_name)).unwrap();
        assert_eq!(&bytes[..16], &[1; 16]);
        assert_eq!(&bytes[16..], &[2; 16]);
        drop(writer);
        let _ = fs::remove_dir_all(root);
    }
}
