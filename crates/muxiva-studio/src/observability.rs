use std::{
    collections::{BTreeMap, VecDeque},
    env,
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;

const SAMPLE_INTERVAL_MS: u64 = 5_000;
const MAX_HISTORY_SAMPLES: usize = 5_000;
const COMPACT_AT_BYTES: u64 = 16 * 1024 * 1024;
const COMPACT_TO_SAMPLES: usize = 2_500;

pub(crate) struct ObservabilityStore {
    history_path: Option<PathBuf>,
    records: Mutex<VecDeque<serde_json::Value>>,
    recording: Mutex<RecordingState>,
    exporter: Arc<OtlpExporter>,
}

#[derive(Default)]
struct RecordingState {
    run_id: String,
    last_sample_ms: u64,
    terminal_saved: bool,
}

#[derive(Clone, Serialize)]
pub(crate) struct ExportStatus {
    configured: bool,
    endpoint: Option<String>,
    protocol: &'static str,
    interval_ms: u64,
    last_attempt_unix_ms: Option<u64>,
    last_success_unix_ms: Option<u64>,
    last_error: Option<String>,
}

struct OtlpExporter {
    endpoint: Option<String>,
    headers: Vec<(String, String)>,
    interval_ms: u64,
    in_flight: AtomicBool,
    state: Mutex<ExportAttempt>,
}

#[derive(Default)]
struct ExportAttempt {
    last_attempt_ms: u64,
    last_success_ms: Option<u64>,
    last_error: Option<String>,
}

#[derive(Default, Serialize)]
struct SessionSummary {
    run_id: String,
    graph_id: String,
    started_at_unix_ms: u64,
    ended_at_unix_ms: u64,
    duration_ms: u64,
    status: String,
    samples: usize,
    node_process_total: u64,
    frame_total: u64,
    max_queued: u64,
    drops_total: u64,
    max_node_process_ms: f64,
    health: String,
}

impl ObservabilityStore {
    pub(crate) fn new(graph: &Path) -> Self {
        let directory = graph
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(".muxiva/observability");
        let history_path = match fs::create_dir_all(&directory) {
            Ok(()) => Some(directory.join("history.jsonl")),
            Err(error) => {
                eprintln!(
                    "[MUXIVA][OBSERVE][WARN] history=memory-only reason={}",
                    safe_log(&error.to_string())
                );
                None
            }
        };
        let records = history_path
            .as_deref()
            .map(load_history)
            .unwrap_or_default();
        Self {
            history_path,
            records: Mutex::new(records),
            recording: Mutex::new(RecordingState::default()),
            exporter: Arc::new(OtlpExporter::from_environment()),
        }
    }

    pub(crate) fn observe(&self, snapshot: &serde_json::Value) {
        let Some(run_id) = snapshot["run_id"].as_str() else {
            return;
        };
        let now_ms = unix_time_ms();
        let terminal = matches!(
            snapshot["status"].as_str(),
            Some("completed" | "aborted" | "stopped")
        );
        let mut state = self
            .recording
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.run_id != run_id {
            state.run_id = run_id.to_owned();
            state.last_sample_ms = 0;
            state.terminal_saved = false;
        }
        let due = now_ms.saturating_sub(state.last_sample_ms) >= SAMPLE_INTERVAL_MS;
        if (!terminal && !due) || (terminal && state.terminal_saved) {
            return;
        }
        state.last_sample_ms = now_ms;
        state.terminal_saved |= terminal;
        drop(state);

        let mut record = snapshot.clone();
        record["observed_at_unix_ms"] = now_ms.into();
        record["health"] = snapshot_health(snapshot).into();
        self.append_record(record.clone());
        self.exporter.maybe_export(record);
    }

    fn append_record(&self, record: serde_json::Value) {
        let mut records = self
            .records
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if records.len() == MAX_HISTORY_SAMPLES {
            records.pop_front();
        }
        records.push_back(record.clone());
        let Some(path) = &self.history_path else {
            return;
        };
        let result = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut file| {
                serde_json::to_writer(&mut file, &record).map_err(std::io::Error::other)?;
                file.write_all(b"\n")
            });
        if let Err(error) = result {
            eprintln!(
                "[MUXIVA][OBSERVE][WARN] history=append-failed reason={}",
                safe_log(&error.to_string())
            );
            return;
        }
        if fs::metadata(path).is_ok_and(|metadata| metadata.len() > COMPACT_AT_BYTES) {
            compact_history(path, &records);
        }
    }

    pub(crate) fn history_index(&self) -> serde_json::Value {
        let records = self
            .records
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut sessions = BTreeMap::<String, SessionSummary>::new();
        for record in records.iter() {
            let run_id = record["run_id"].as_str().unwrap_or("unknown").to_owned();
            let observed = record["observed_at_unix_ms"].as_u64().unwrap_or(0);
            let entry = sessions
                .entry(run_id.clone())
                .or_insert_with(|| SessionSummary {
                    run_id,
                    graph_id: record["graph_id"].as_str().unwrap_or("unknown").to_owned(),
                    started_at_unix_ms: record["started_at_unix_ms"].as_u64().unwrap_or(observed),
                    ..SessionSummary::default()
                });
            entry.samples += 1;
            entry.ended_at_unix_ms = observed;
            entry.duration_ms = record["elapsed_ms"].as_u64().unwrap_or(entry.duration_ms);
            entry.status = record["status"].as_str().unwrap_or("unknown").to_owned();
            entry.node_process_total = sum_array_field(record, "nodes", "process_total");
            entry.frame_total = sum_array_field(record, "edges", "enqueue_total");
            entry.drops_total = sum_array_field(record, "edges", "drop_total");
            entry.max_queued = entry
                .max_queued
                .max(sum_array_field(record, "edges", "queue_len"));
            entry.max_node_process_ms = entry
                .max_node_process_ms
                .max(max_node_average_process_ms(record));
            entry.health = worst_health(
                &entry.health,
                record["health"].as_str().unwrap_or("healthy"),
            );
        }
        let mut sessions = sessions.into_values().collect::<Vec<_>>();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.started_at_unix_ms));
        serde_json::json!({
            "sessions": sessions,
            "retention": {
                "sample_interval_ms": SAMPLE_INTERVAL_MS,
                "max_samples": MAX_HISTORY_SAMPLES,
                "compact_at_bytes": COMPACT_AT_BYTES,
            },
            "storage": self.history_path.as_ref().map(|path| path.display().to_string()),
            "export": self.export_status(),
        })
    }

    pub(crate) fn history_session(&self, run_id: &str) -> Option<serde_json::Value> {
        if run_id.is_empty()
            || run_id.len() > 96
            || !run_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return None;
        }
        let records = self
            .records
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let samples = records
            .iter()
            .filter(|record| record["run_id"].as_str() == Some(run_id))
            .cloned()
            .collect::<Vec<_>>();
        (!samples.is_empty()).then(|| serde_json::json!({"run_id": run_id, "samples": samples}))
    }

    pub(crate) fn export_status(&self) -> ExportStatus {
        self.exporter.status()
    }
}

impl OtlpExporter {
    fn from_environment() -> Self {
        let signal_endpoint = env::var("OTEL_EXPORTER_OTLP_METRICS_ENDPOINT")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let base_endpoint = env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let endpoint = signal_endpoint.or_else(|| {
            base_endpoint.map(|base| format!("{}/v1/metrics", base.trim_end_matches('/')))
        });
        let protocol = env::var("OTEL_EXPORTER_OTLP_METRICS_PROTOCOL")
            .or_else(|_| env::var("OTEL_EXPORTER_OTLP_PROTOCOL"))
            .unwrap_or_else(|_| "http/json".into());
        let configuration_error = (protocol != "http/json" && endpoint.is_some())
            .then(|| format!("unsupported protocol {protocol}; expected http/json"));
        let endpoint = if protocol == "http/json" {
            endpoint
        } else {
            if endpoint.is_some() {
                eprintln!(
                    "[MUXIVA][OTLP][WARN] exporter=disabled protocol={} supported=http/json",
                    safe_log(&protocol)
                );
            }
            None
        };
        let headers = env::var("OTEL_EXPORTER_OTLP_METRICS_HEADERS")
            .or_else(|_| env::var("OTEL_EXPORTER_OTLP_HEADERS"))
            .ok()
            .map(|value| parse_headers(&value))
            .unwrap_or_default();
        let interval_ms = env::var("OTEL_METRIC_EXPORT_INTERVAL")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| (1_000..=3_600_000).contains(value))
            .unwrap_or(10_000);
        Self {
            endpoint,
            headers,
            interval_ms,
            in_flight: AtomicBool::new(false),
            state: Mutex::new(ExportAttempt {
                last_error: configuration_error,
                ..ExportAttempt::default()
            }),
        }
    }

    fn maybe_export(self: &Arc<Self>, snapshot: serde_json::Value) {
        let Some(endpoint) = self.endpoint.clone() else {
            return;
        };
        let now_ms = unix_time_ms();
        {
            let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            if now_ms.saturating_sub(state.last_attempt_ms) < self.interval_ms {
                return;
            }
            state.last_attempt_ms = now_ms;
        }
        if self.in_flight.swap(true, Ordering::AcqRel) {
            return;
        }
        let exporter = self.clone();
        thread::spawn(move || {
            let payload = otlp_metrics(&snapshot);
            let mut request = ureq::post(&endpoint).header("Content-Type", "application/json");
            for (name, value) in &exporter.headers {
                request = request.header(name, value);
            }
            let result = request.send_json(&payload).map(|_| ());
            let completed_ms = unix_time_ms();
            let mut state = exporter
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            match result {
                Ok(()) => {
                    state.last_success_ms = Some(completed_ms);
                    state.last_error = None;
                }
                Err(error) => {
                    let message = safe_log(&error.to_string());
                    state.last_error = Some(message.clone());
                    eprintln!("[MUXIVA][OTLP][WARN] export=failed reason={message}");
                }
            }
            exporter.in_flight.store(false, Ordering::Release);
        });
    }

    fn status(&self) -> ExportStatus {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        ExportStatus {
            configured: self.endpoint.is_some(),
            endpoint: self.endpoint.clone(),
            protocol: "http/json",
            interval_ms: self.interval_ms,
            last_attempt_unix_ms: (state.last_attempt_ms > 0).then_some(state.last_attempt_ms),
            last_success_unix_ms: state.last_success_ms,
            last_error: state.last_error.clone(),
        }
    }
}

pub(crate) fn prometheus(snapshot: Option<&serde_json::Value>) -> String {
    let mut output = String::from(
        "# HELP muxiva_runtime_up Whether the local Studio Runtime exporter is available.\n# TYPE muxiva_runtime_up gauge\nmuxiva_runtime_up 1\n",
    );
    let Some(snapshot) = snapshot else {
        return output;
    };
    let mut declared = BTreeMap::new();
    let graph = label(snapshot["graph_id"].as_str().unwrap_or("unknown"));
    let run = label(snapshot["run_id"].as_str().unwrap_or("unknown"));
    let status = label(snapshot["status"].as_str().unwrap_or("unknown"));
    output.push_str("# TYPE muxiva_runtime_session_info gauge\n");
    output.push_str(&format!(
        "muxiva_runtime_session_info{{graph_id=\"{graph}\",run_id=\"{run}\",status=\"{status}\"}} 1\n"
    ));
    metric(
        &mut output,
        &mut declared,
        "muxiva_runtime_elapsed_seconds",
        "gauge",
        &[],
        snapshot["elapsed_ms"].as_f64().unwrap_or(0.0) / 1_000.0,
    );
    for node in snapshot["nodes"].as_array().into_iter().flatten() {
        let node_id = label(node["node_id"].as_str().unwrap_or("unknown"));
        let labels = [
            ("graph_id", graph.as_str()),
            ("run_id", run.as_str()),
            ("node_id", node_id.as_str()),
        ];
        metric(
            &mut output,
            &mut declared,
            "muxiva_node_process_total",
            "counter",
            &labels,
            node["process_total"].as_f64().unwrap_or(0.0),
        );
        metric(
            &mut output,
            &mut declared,
            "muxiva_node_process_duration_seconds_total",
            "counter",
            &labels,
            node["process_duration_ns"].as_f64().unwrap_or(0.0) / 1e9,
        );
        metric(
            &mut output,
            &mut declared,
            "muxiva_node_process_duration_seconds_max",
            "gauge",
            &labels,
            node["max_process_duration_ns"].as_f64().unwrap_or(0.0) / 1e9,
        );
        metric(
            &mut output,
            &mut declared,
            "muxiva_node_error_total",
            "counter",
            &labels,
            node["error_total"].as_f64().unwrap_or(0.0),
        );
        metric(
            &mut output,
            &mut declared,
            "muxiva_node_panic_total",
            "counter",
            &labels,
            node["panic_total"].as_f64().unwrap_or(0.0),
        );
        for custom in node["custom_metrics"].as_array().into_iter().flatten() {
            let name = label(custom["name"].as_str().unwrap_or("unknown"));
            let custom_labels = [
                ("graph_id", graph.as_str()),
                ("run_id", run.as_str()),
                ("node_id", node_id.as_str()),
                ("metric", name.as_str()),
            ];
            let (metric_name, metric_type) = if custom["kind"].as_str() == Some("counter") {
                ("muxiva_node_custom_counter_total", "counter")
            } else {
                ("muxiva_node_custom_gauge", "gauge")
            };
            metric(
                &mut output,
                &mut declared,
                metric_name,
                metric_type,
                &custom_labels,
                custom["value"].as_f64().unwrap_or(0.0),
            );
        }
    }
    for edge in snapshot["edges"].as_array().into_iter().flatten() {
        let edge_id = label(edge["edge_id"].as_str().unwrap_or("unknown"));
        let labels = [
            ("graph_id", graph.as_str()),
            ("run_id", run.as_str()),
            ("edge_id", edge_id.as_str()),
        ];
        for (name, field, kind, divisor) in [
            ("muxiva_edge_queue_length", "queue_len", "gauge", 1.0),
            ("muxiva_edge_queue_capacity", "queue_capacity", "gauge", 1.0),
            (
                "muxiva_edge_queue_high_watermark",
                "high_watermark",
                "gauge",
                1.0,
            ),
            ("muxiva_edge_enqueue_total", "enqueue_total", "counter", 1.0),
            ("muxiva_edge_dequeue_total", "dequeue_total", "counter", 1.0),
            ("muxiva_edge_drop_total", "drop_total", "counter", 1.0),
            ("muxiva_edge_full_total", "full_total", "counter", 1.0),
            (
                "muxiva_edge_payload_bytes_total",
                "payload_bytes_total",
                "counter",
                1.0,
            ),
            (
                "muxiva_edge_blocked_seconds_total",
                "blocked_duration_ns",
                "counter",
                1e9,
            ),
            (
                "muxiva_edge_oldest_frame_age_seconds",
                "oldest_frame_age_ns",
                "gauge",
                1e9,
            ),
            (
                "muxiva_edge_audio_duration_seconds_total",
                "audio_duration_ns_total",
                "counter",
                1e9,
            ),
        ] {
            metric(
                &mut output,
                &mut declared,
                name,
                kind,
                &labels,
                edge[field].as_f64().unwrap_or(0.0) / divisor,
            );
        }
    }
    output
}

fn metric(
    output: &mut String,
    declared: &mut BTreeMap<String, String>,
    name: &str,
    kind: &str,
    labels: &[(&str, &str)],
    value: f64,
) {
    if !declared.contains_key(name) {
        declared.insert(name.to_owned(), kind.to_owned());
        output.push_str(&format!("# TYPE {name} {kind}\n"));
    }
    output.push_str(name);
    if !labels.is_empty() {
        output.push('{');
        for (index, (key, value)) in labels.iter().enumerate() {
            if index > 0 {
                output.push(',');
            }
            output.push_str(&format!("{key}=\"{value}\""));
        }
        output.push('}');
    }
    output.push_str(&format!(" {value}\n"));
}

fn otlp_metrics(snapshot: &serde_json::Value) -> serde_json::Value {
    let time_ns = unix_time_ns().to_string();
    let start_ns = snapshot["started_at_unix_ms"]
        .as_u64()
        .unwrap_or(0)
        .saturating_mul(1_000_000)
        .to_string();
    let graph = snapshot["graph_id"].as_str().unwrap_or("unknown");
    let run = snapshot["run_id"].as_str().unwrap_or("unknown");
    let mut metrics = Vec::new();
    let resource = vec![
        attribute("service.name", "muxiva-studio"),
        attribute("muxiva.graph.id", graph),
        attribute("muxiva.run.id", run),
    ];
    let mut add = |name: &str, kind: &str, monotonic: bool, points: Vec<serde_json::Value>| {
        if points.is_empty() {
            return;
        }
        let data = if kind == "sum" {
            serde_json::json!({"sum":{"aggregationTemporality":2,"isMonotonic":monotonic,"dataPoints":points}})
        } else {
            serde_json::json!({"gauge":{"dataPoints":points}})
        };
        let mut value = serde_json::json!({"name":name});
        value
            .as_object_mut()
            .unwrap()
            .extend(data.as_object().unwrap().clone());
        metrics.push(value);
    };
    let node_points = |field: &str, divisor: f64| {
        snapshot["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|node| {
                point(
                    node[field].as_f64().unwrap_or(0.0) / divisor,
                    &start_ns,
                    &time_ns,
                    vec![attribute(
                        "muxiva.node.id",
                        node["node_id"].as_str().unwrap_or("unknown"),
                    )],
                )
            })
            .collect::<Vec<_>>()
    };
    add(
        "muxiva.node.process",
        "sum",
        true,
        node_points("process_total", 1.0),
    );
    add(
        "muxiva.node.process.duration",
        "sum",
        true,
        node_points("process_duration_ns", 1e9),
    );
    add(
        "muxiva.node.process.duration.max",
        "gauge",
        false,
        node_points("max_process_duration_ns", 1e9),
    );
    add(
        "muxiva.node.errors",
        "sum",
        true,
        node_points("error_total", 1.0),
    );
    add(
        "muxiva.node.panics",
        "sum",
        true,
        node_points("panic_total", 1.0),
    );
    let mut custom_counters = Vec::new();
    let mut custom_gauges = Vec::new();
    for node in snapshot["nodes"].as_array().into_iter().flatten() {
        for custom in node["custom_metrics"].as_array().into_iter().flatten() {
            let target = if custom["kind"].as_str() == Some("counter") {
                &mut custom_counters
            } else {
                &mut custom_gauges
            };
            target.push(point(
                custom["value"].as_f64().unwrap_or(0.0),
                &start_ns,
                &time_ns,
                vec![
                    attribute(
                        "muxiva.node.id",
                        node["node_id"].as_str().unwrap_or("unknown"),
                    ),
                    attribute(
                        "muxiva.metric.name",
                        custom["name"].as_str().unwrap_or("unknown"),
                    ),
                ],
            ));
        }
    }
    add("muxiva.node.custom.counter", "sum", true, custom_counters);
    add("muxiva.node.custom.gauge", "gauge", false, custom_gauges);
    let edge_points = |field: &str, divisor: f64| {
        snapshot["edges"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|edge| {
                point(
                    edge[field].as_f64().unwrap_or(0.0) / divisor,
                    &start_ns,
                    &time_ns,
                    vec![attribute(
                        "muxiva.edge.id",
                        edge["edge_id"].as_str().unwrap_or("unknown"),
                    )],
                )
            })
            .collect::<Vec<_>>()
    };
    add(
        "muxiva.edge.queue.size",
        "gauge",
        false,
        edge_points("queue_len", 1.0),
    );
    add(
        "muxiva.edge.queue.capacity",
        "gauge",
        false,
        edge_points("queue_capacity", 1.0),
    );
    add(
        "muxiva.edge.frames",
        "sum",
        true,
        edge_points("enqueue_total", 1.0),
    );
    add(
        "muxiva.edge.frames.dequeued",
        "sum",
        true,
        edge_points("dequeue_total", 1.0),
    );
    add(
        "muxiva.edge.drops",
        "sum",
        true,
        edge_points("drop_total", 1.0),
    );
    add(
        "muxiva.edge.queue.full",
        "sum",
        true,
        edge_points("full_total", 1.0),
    );
    add(
        "muxiva.edge.payload.size",
        "sum",
        true,
        edge_points("payload_bytes_total", 1.0),
    );
    add(
        "muxiva.edge.blocked.duration",
        "sum",
        true,
        edge_points("blocked_duration_ns", 1e9),
    );
    add(
        "muxiva.edge.oldest_frame.age",
        "gauge",
        false,
        edge_points("oldest_frame_age_ns", 1e9),
    );
    add(
        "muxiva.edge.audio.duration",
        "sum",
        true,
        edge_points("audio_duration_ns_total", 1e9),
    );
    serde_json::json!({"resourceMetrics":[{"resource":{"attributes":resource},"scopeMetrics":[{"scope":{"name":"io.muxiva.studio","version":env!("CARGO_PKG_VERSION")},"metrics":metrics}]}]})
}

fn point(
    value: f64,
    start_ns: &str,
    time_ns: &str,
    attributes: Vec<serde_json::Value>,
) -> serde_json::Value {
    serde_json::json!({"attributes":attributes,"startTimeUnixNano":start_ns,"timeUnixNano":time_ns,"asDouble":value})
}

fn attribute(key: &str, value: &str) -> serde_json::Value {
    serde_json::json!({"key":key,"value":{"stringValue":value}})
}

fn load_history(path: &Path) -> VecDeque<serde_json::Value> {
    let Ok(file) = fs::File::open(path) else {
        return VecDeque::new();
    };
    let values = BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect::<Vec<_>>();
    let skip = values.len().saturating_sub(MAX_HISTORY_SAMPLES);
    values.into_iter().skip(skip).collect()
}

fn compact_history(path: &Path, records: &VecDeque<serde_json::Value>) {
    let temporary = path.with_extension("jsonl.tmp");
    let result = fs::File::create(&temporary)
        .and_then(|mut file| {
            for record in records.iter().rev().take(COMPACT_TO_SAMPLES).rev() {
                serde_json::to_writer(&mut file, record).map_err(std::io::Error::other)?;
                file.write_all(b"\n")?;
            }
            file.sync_all()
        })
        .and_then(|()| fs::rename(&temporary, path));
    if let Err(error) = result {
        eprintln!(
            "[MUXIVA][OBSERVE][WARN] history=compact-failed reason={}",
            safe_log(&error.to_string())
        );
    }
}

fn parse_headers(value: &str) -> Vec<(String, String)> {
    value
        .split(',')
        .filter_map(|pair| pair.split_once('='))
        .filter_map(|(name, value)| {
            let name = name.trim();
            let value = value.trim();
            (!name.is_empty() && !value.contains(['\r', '\n']))
                .then(|| (name.to_owned(), percent_decode(value)))
        })
        .collect()
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = |byte| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            };
            if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                decoded.push(high * 16 + low);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(decoded).unwrap_or_else(|_| value.to_owned())
}

fn sum_array_field(value: &serde_json::Value, array: &str, field: &str) -> u64 {
    value[array]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item[field].as_u64())
        .sum()
}

fn max_node_average_process_ms(value: &serde_json::Value) -> f64 {
    value["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|node| {
            let total = node["process_total"].as_u64().unwrap_or(0);
            if total == 0 {
                0.0
            } else {
                node["process_duration_ns"].as_f64().unwrap_or(0.0) / total as f64 / 1e6
            }
        })
        .fold(0.0, f64::max)
}

fn snapshot_health(snapshot: &serde_json::Value) -> &'static str {
    for node in snapshot["nodes"].as_array().into_iter().flatten() {
        let custom = |name: &str| {
            node["custom_metrics"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|metric| metric["name"].as_str() == Some(name))
                .and_then(|metric| metric["value"].as_u64())
                .unwrap_or(0)
        };
        if node["error_total"].as_u64().unwrap_or(0) > 0
            || node["panic_total"].as_u64().unwrap_or(0) > 0
            || custom("ingress.dropped_frames") > 0
            || custom("ingress.queue_duration_ms") >= 1_000
        {
            return "critical";
        }
    }
    for edge in snapshot["edges"].as_array().into_iter().flatten() {
        let len = edge["queue_len"].as_f64().unwrap_or(0.0);
        let capacity = edge["queue_capacity"].as_f64().unwrap_or(0.0);
        let ratio = if capacity > 0.0 { len / capacity } else { 0.0 };
        if edge["drop_total"].as_u64().unwrap_or(0) > 0
            || ratio >= 0.8
            || edge["oldest_frame_age_ns"].as_u64().unwrap_or(0) >= 1_000_000_000
        {
            return "critical";
        }
    }
    if max_node_average_process_ms(snapshot) >= 50.0 {
        return "critical";
    }
    for node in snapshot["nodes"].as_array().into_iter().flatten() {
        let queue_ms = node["custom_metrics"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|metric| metric["name"].as_str() == Some("ingress.queue_duration_ms"))
            .and_then(|metric| metric["value"].as_u64())
            .unwrap_or(0);
        if queue_ms >= 200 {
            return "warning";
        }
    }
    for edge in snapshot["edges"].as_array().into_iter().flatten() {
        let len = edge["queue_len"].as_f64().unwrap_or(0.0);
        let capacity = edge["queue_capacity"].as_f64().unwrap_or(0.0);
        let ratio = if capacity > 0.0 { len / capacity } else { 0.0 };
        if ratio >= 0.4 || edge["oldest_frame_age_ns"].as_u64().unwrap_or(0) >= 200_000_000 {
            return "warning";
        }
    }
    if max_node_average_process_ms(snapshot) >= 10.0 {
        "warning"
    } else {
        "healthy"
    }
}

fn worst_health(current: &str, candidate: &str) -> String {
    let rank = |value| match value {
        "critical" => 3,
        "warning" => 2,
        "healthy" => 1,
        _ => 0,
    };
    if rank(candidate) > rank(current) {
        candidate.to_owned()
    } else {
        current.to_owned()
    }
}

fn label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn safe_log(value: &str) -> String {
    value
        .chars()
        .filter(|character| !matches!(character, '\r' | '\n'))
        .take(512)
        .collect()
}

pub(crate) fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn unix_time_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        time::Duration,
    };

    fn snapshot(status: &str, run_id: &str) -> serde_json::Value {
        serde_json::json!({
            "run_id": run_id, "graph_id":"test", "started_at_unix_ms":1, "status":status,
            "elapsed_ms":100, "nodes":[{"node_id":"a","process_total":2,"process_duration_ns":4_000_000,"max_process_duration_ns":3_000_000,"error_total":0,"panic_total":0,"custom_metrics":[]}],
            "edges":[{"edge_id":"a-b","queue_len":1,"queue_capacity":8,"high_watermark":2,"enqueue_total":3,"dequeue_total":2,"drop_total":0,"full_total":0,"payload_bytes_total":12,"blocked_duration_ns":0,"oldest_frame_age_ns":20_000_000,"audio_duration_ns_total":0}]
        })
    }

    #[test]
    fn prometheus_output_has_stable_names_and_labels() {
        let output = prometheus(Some(&snapshot("running", "run-1")));
        assert!(output.contains(
            "muxiva_node_process_total{graph_id=\"test\",run_id=\"run-1\",node_id=\"a\"} 2"
        ));
        assert!(output.contains("muxiva_edge_queue_length"));
        assert!(!output.contains("# TYPE muxiva_runtime_up counter"));
    }

    #[test]
    fn otlp_json_uses_lower_camel_case_and_string_nanoseconds() {
        let value = otlp_metrics(&snapshot("running", "run-1"));
        assert!(value["resourceMetrics"][0]["scopeMetrics"][0]["metrics"].is_array());
        assert!(value.to_string().contains("timeUnixNano"));
        assert!(!value.to_string().contains("time_unix_nano"));
        assert_eq!(
            parse_headers("authorization=Bearer%20token")[0].1,
            "Bearer token"
        );
    }

    #[test]
    fn otlp_http_json_export_reaches_a_collector_without_blocking_the_caller() {
        let listener = match TcpListener::bind(("127.0.0.1", 0)) {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => return,
            Err(error) => panic!("collector fixture failed: {error}"),
        };
        let endpoint = format!("http://{}/v1/metrics", listener.local_addr().unwrap());
        let collector = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_http_request(&mut stream)
        });
        let exporter = Arc::new(OtlpExporter {
            endpoint: Some(endpoint),
            headers: vec![("X-Test".into(), "muxiva".into())],
            interval_ms: 1_000,
            in_flight: AtomicBool::new(false),
            state: Mutex::new(ExportAttempt::default()),
        });
        let started = std::time::Instant::now();
        exporter.maybe_export(snapshot("running", "run-1"));
        assert!(started.elapsed() < Duration::from_millis(100));
        let request = collector.join().unwrap();
        assert!(request.contains("POST /v1/metrics HTTP/1.1"));
        assert!(request
            .to_ascii_lowercase()
            .contains("content-type: application/json"));
        assert!(request.contains("\"resourceMetrics\""));
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while exporter.status().last_success_unix_ms.is_none() {
            assert!(std::time::Instant::now() < deadline);
            thread::yield_now();
        }
    }

    #[test]
    fn terminal_history_survives_a_store_restart() {
        let directory = std::env::temp_dir().join(format!(
            "muxiva-observability-history-{}-{}",
            std::process::id(),
            unix_time_ns()
        ));
        fs::create_dir_all(&directory).unwrap();
        let graph = directory.join("graph.json");
        fs::write(&graph, "{}").unwrap();
        let store = ObservabilityStore::new(&graph);
        store.observe(&snapshot("completed", "persisted-1"));
        drop(store);

        let restored = ObservabilityStore::new(&graph);
        let index = restored.history_index();
        assert_eq!(index["sessions"][0]["run_id"], "persisted-1");
        assert_eq!(index["sessions"][0]["status"], "completed");
        fs::remove_dir_all(directory).unwrap();
    }

    fn read_http_request(stream: &mut TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        let expected = loop {
            let count = stream.read(&mut buffer).unwrap();
            bytes.extend_from_slice(&buffer[..count]);
            if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..position + 4]);
                let length = headers
                    .lines()
                    .find(|line| line.to_ascii_lowercase().starts_with("content-length:"))
                    .and_then(|line| line.split_once(':'))
                    .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                break position + 4 + length;
            }
        };
        while bytes.len() < expected {
            let count = stream.read(&mut buffer).unwrap();
            bytes.extend_from_slice(&buffer[..count]);
        }
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").unwrap();
        String::from_utf8(bytes).unwrap()
    }
}
