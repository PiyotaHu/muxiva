"""Shared Xiaozhi transport gateway and loopback control client.

Architecture
------------
Every Muxiva Python Node runs in its own process. The Xiaozhi transport is a
Source Node, so it owns the one place that can host a long-lived WebSocket
server: the ``xiaozhi_audio_source`` process runs :class:`XiaozhiGateway` on a
background thread. The Sink and Event Encoder Nodes run in separate processes
and talk to that gateway through a loopback JSON-lines TCP control socket using
:class:`XiaozhiControlClient`.

The graph therefore stays a pure Muxiva graph; only PCM Frames and control
Signals/Events cross the runtime, while Opus and the Xiaozhi WebSocket protocol
remain inside this transport provider.
"""

from __future__ import annotations

import asyncio
import json
import queue
import socketserver
import sys
import threading
import time
import uuid
import math

import opus_codec

HELLO_TYPE = "hello"
PING_TYPE = "ping"
PONG_TYPE = "pong"
ABORT_TYPE = "abort"
LISTEN_TYPE = "listen"
STT_TYPE = "stt"
TTS_TYPE = "tts"


class XiaozhiGateway:
    """WebSocket device server plus a loopback control server.

    Runs entirely on background threads so the Muxiva Python Host thread keeps
    servicing ``on_process`` callbacks without blocking.
    """

    def __init__(self, config: dict) -> None:
        self.ws_host = str(config.get("ws_host", "0.0.0.0"))
        self.ws_port = int(config.get("ws_port", 8888))
        self.control_host = str(config.get("control_host", "127.0.0.1"))
        self.control_port = int(config.get("control_port", 8889))
        self.sample_rate = int(config.get("sample_rate", 16_000))
        self.frame_duration_ms = int(config.get("frame_duration_ms", 60))
        self.prebuffer_ms = max(
            self.frame_duration_ms,
            int(config.get("playback_prebuffer_ms", 1200)),
        )
        self.playback_initial_burst_frames = max(
            1,
            int(config.get("playback_initial_burst_frames", 5)),
        )
        self.playback_initial_burst_interval_ms = min(
            self.frame_duration_ms - 1,
            max(0, int(config.get("playback_initial_burst_interval_ms", 12))),
        )
        self.playback_queue_ms = max(
            self.prebuffer_ms,
            int(config.get("playback_queue_ms", 120_000)),
        )
        self.playback_stop_grace_ms = max(
            0,
            int(config.get("playback_stop_grace_ms", 1_000)),
        )
        self.playback_no_audio_stop_timeout_ms = max(
            self.playback_stop_grace_ms,
            int(config.get("playback_no_audio_stop_timeout_ms", 5_000)),
        )

        self._ingress = queue.Queue(maxsize=512)
        self._events = queue.Queue(maxsize=256)
        self._egress = queue.Queue(
            maxsize=max(1, math.ceil(self.playback_queue_ms / self.frame_duration_ms))
        )
        self._messages = queue.Queue(maxsize=256)
        self._egress_drop_count = 0

        # The LLM completion event can arrive before the asynchronous TTS node
        # has finished publishing its audio.  Sending ``tts/stop`` immediately
        # makes Xiaozhi firmware leave the speaking state and discard every
        # binary packet that follows.  Hold the stop marker until the current
        # turn's audio queue has drained and stayed quiet for a short interval.
        self._tts_lock = threading.Lock()
        self._tts_audio_seen = False
        self._tts_last_audio_at = 0.0
        self._tts_last_packet_sent_at = 0.0
        self._tts_stop_message: str | None = None
        self._tts_stop_requested_at = 0.0
        self._tts_playback_started = False
        self._prebuffer_frames = max(
            1, math.ceil(self.prebuffer_ms / self.frame_duration_ms)
        )

        self._stopped = threading.Event()
        self._async_stop = None
        self._ws = None
        self._ws_loop = None
        self._client_id = None
        self._encoder = opus_codec.OpusEncoder(
            sample_rate=self.sample_rate, frame_duration_ms=self.frame_duration_ms
        )
        self._decoder = opus_codec.OpusDecoder(
            sample_rate=self.sample_rate, frame_duration_ms=self.frame_duration_ms
        )
        self._threads: list[threading.Thread] = []
        self._pcm_lock = threading.Lock()
        self._egress_pcm = bytearray()
        self._pcm_frame_bytes = (
            self.sample_rate * self.frame_duration_ms // 1000 * 2
        )

    # ------------------------------------------------------------------ lifecycle
    def start(self) -> None:
        ws_thread = threading.Thread(
            target=self._run_ws_server, name="muxiva-xiaozhi-ws", daemon=True
        )
        control_thread = threading.Thread(
            target=self._run_control_server, name="muxiva-xiaozhi-control", daemon=True
        )
        self._threads = [ws_thread, control_thread]
        ws_thread.start()
        control_thread.start()

    def stop(self) -> None:
        self._stopped.set()
        if self._ws_loop is not None and self._async_stop is not None:
            self._ws_loop.call_soon_threadsafe(self._async_stop.set)
        self._encoder.close()
        self._decoder.close()

    # ------------------------------------------------------------------ source side
    def poll_audio(self) -> list[bytes]:
        frames = []
        while True:
            try:
                frames.append(self._ingress.get_nowait())
            except queue.Empty:
                return frames

    def poll_events(self) -> list[dict]:
        events = []
        while True:
            try:
                events.append(self._events.get_nowait())
            except queue.Empty:
                return events

    def has_client(self) -> bool:
        return self._ws is not None

    # ------------------------------------------------------------------ sink/event side
    def publish_audio(self, pcm: bytes) -> None:
        """Reblock arbitrary PCM chunks into exact, paced Opus frames.

        Vendor streaming deltas are not guaranteed to be one protocol frame.
        Encoding each delta directly used to truncate long deltas and pad short
        ones, which made replies sound skipped or unnaturally fast/slow.
        """
        if not self.has_client():
            return
        frames = []
        with self._pcm_lock:
            self._egress_pcm.extend(pcm)
            while len(self._egress_pcm) >= self._pcm_frame_bytes:
                frames.append(bytes(self._egress_pcm[: self._pcm_frame_bytes]))
                del self._egress_pcm[: self._pcm_frame_bytes]
        with self._tts_lock:
            self._tts_audio_seen = True
            self._tts_last_audio_at = time.monotonic()
        for frame in frames:
            self._encode_and_queue(frame)

    def _encode_and_queue(self, pcm: bytes) -> bool:
        try:
            packet = self._encoder.encode(pcm)
            self._egress.put_nowait(packet)
            return True
        except opus_codec.OpusError:
            return False
        except queue.Full:
            self._egress_drop_count += 1
            if self._egress_drop_count == 1 or self._egress_drop_count % 100 == 0:
                print(
                    "[MUXIVA][XIAOZHI][playback.queue_full] "
                    f"dropped_frames={self._egress_drop_count} "
                    f"capacity_frames={self._egress.maxsize}",
                    file=sys.stderr,
                    flush=True,
                )
            return False

    def _flush_pcm_tail(self) -> bool:
        """Pad and queue the final partial frame after TTS becomes quiet."""
        with self._pcm_lock:
            if not self._egress_pcm:
                return False
            tail = bytes(self._egress_pcm)
            self._egress_pcm.clear()
        return self._encode_and_queue(tail)

    def publish_message(self, payload: dict) -> None:
        if not self.has_client():
            return
        payload = dict(payload)
        payload.setdefault("session_id", self._client_id or "")
        message = json.dumps(payload, ensure_ascii=False)
        if payload.get("type") == TTS_TYPE:
            state = payload.get("state")
            if state == "start":
                with self._tts_lock:
                    self._tts_audio_seen = False
                    self._tts_last_audio_at = 0.0
                    self._tts_last_packet_sent_at = 0.0
                    self._tts_stop_message = None
                    self._tts_stop_requested_at = 0.0
                    self._tts_playback_started = False
            elif state == "stop":
                with self._tts_lock:
                    self._tts_stop_message = message
                    self._tts_stop_requested_at = time.monotonic()
                return
        try:
            self._messages.put_nowait(message)
        except queue.Full:
            pass

    def reset_egress(self) -> None:
        """Drop queued assistant audio after a barge-in."""
        while True:
            try:
                self._egress.get_nowait()
            except queue.Empty:
                break
        with self._pcm_lock:
            self._egress_pcm.clear()
        with self._tts_lock:
            self._tts_stop_message = None
            self._tts_stop_requested_at = 0.0
            self._tts_audio_seen = False
            self._tts_last_audio_at = 0.0
            self._tts_last_packet_sent_at = 0.0
            self._tts_playback_started = False

    # ------------------------------------------------------------------ websocket server
    def _run_ws_server(self) -> None:
        try:
            import websockets
        except ImportError as error:  # pragma: no cover - depends on target host
            print(
                f"[MUXIVA][XIAOZHI][fatal] websockets is not installed: {error}",
                file=sys.stderr,
                flush=True,
            )
            return

        async def serve() -> None:
            async with websockets.serve(
                self._ws_handler, self.ws_host, self.ws_port
            ):
                self._ws_loop = asyncio.get_running_loop()
                self._async_stop = asyncio.Event()
                sender = asyncio.create_task(self._ws_sender())
                try:
                    await self._async_stop.wait()
                finally:
                    sender.cancel()

        try:
            asyncio.run(serve())
        except OSError as error:
            print(
                f"[MUXIVA][XIAOZHI][ws.bind.failed] {self.ws_host}:{self.ws_port}: {error}",
                file=sys.stderr,
                flush=True,
            )

    async def _ws_handler(self, ws) -> None:
        # Never let audio/control data from a dead session leak into a newly
        # connected device session.
        self.reset_egress()
        self._ws = ws
        self._client_id = str(uuid.uuid4())
        print(
            f"[MUXIVA][XIAOZHI][device.connected] id={self._client_id}",
            file=sys.stderr,
            flush=True,
        )
        try:
            async for message in ws:
                if isinstance(message, (bytes, bytearray)):
                    self._on_binary(bytes(message))
                elif isinstance(message, str):
                    await self._on_text(message)
        except Exception as error:
            print(
                "[MUXIVA][XIAOZHI][device.connection_error] "
                f"type={type(error).__name__} detail={str(error)[:240]}",
                file=sys.stderr,
                flush=True,
            )
        finally:
            self._ws = None
            self.reset_egress()
            print(
                f"[MUXIVA][XIAOZHI][device.disconnected] id={self._client_id}",
                file=sys.stderr,
                flush=True,
            )

    def _on_binary(self, packet: bytes) -> None:
        try:
            pcm = self._decoder.decode(packet)
        except opus_codec.OpusError:
            return
        try:
            self._ingress.put_nowait(pcm)
        except queue.Full:
            pass

    async def _on_text(self, message: str) -> None:
        try:
            data = json.loads(message)
        except json.JSONDecodeError:
            return
        message_type = data.get("type")
        if message_type == HELLO_TYPE:
            await self._send_json(self._server_hello())
        elif message_type == PING_TYPE:
            await self._send_json({"type": PONG_TYPE})
        elif message_type in (ABORT_TYPE, LISTEN_TYPE):
            try:
                self._events.put_nowait(data)
            except queue.Full:
                pass
            if message_type == ABORT_TYPE:
                self.reset_egress()

    def _server_hello(self) -> dict:
        return {
            "type": HELLO_TYPE,
            "version": 1,
            "transport": "websocket",
            "audio_params": {
                "format": "opus",
                "sample_rate": self.sample_rate,
                "channels": 1,
                "frame_duration": self.frame_duration_ms,
            },
            "session_id": self._client_id or str(uuid.uuid4()),
        }

    async def _send_json(self, payload: dict) -> None:
        if self._ws is not None:
            await self._ws.send(json.dumps(payload, ensure_ascii=False))

    async def _ws_sender(self) -> None:
        frame_seconds = self.frame_duration_ms / 1000.0
        quiet_before_stop_seconds = self.playback_stop_grace_ms / 1000.0
        stop_without_audio_timeout_seconds = (
            self.playback_no_audio_stop_timeout_ms / 1000.0
        )
        next_audio_send_at: float | None = None
        initial_burst_remaining = 0
        underrun_started_at: float | None = None
        while not self._stopped.is_set():
            # Control/display messages are sent immediately, before paced audio.
            sent_message = False
            while True:
                try:
                    message = self._messages.get_nowait()
                except queue.Empty:
                    break
                sent_message = True
                if self._ws is not None:
                    try:
                        await self._ws.send(message)
                    except Exception:
                        pass
            # Control messages share the WebSocket, but must not consume an
            # audio clock slot. Continue into the playout scheduler in the
            # same iteration so display traffic cannot stretch speech timing.
            # Do not start playback on the first small cloud-TTS burst. A 120ms
            # quiet gap does not mean a short reply has finished; it is often
            # merely the gap before the next vendor delta, and starting there
            # drains the queue inside a word (heard as “小—主—人”).  Start when
            # the jitter reservoir is full, or when TTS has explicitly drained.
            with self._tts_lock:
                playback_started = self._tts_playback_started
                stop_pending = self._tts_stop_message is not None
                short_reply_ready = (
                    self._tts_audio_seen
                    and stop_pending
                )
            if not playback_started:
                queued_frames = self._egress.qsize()
                if queued_frames < self._prebuffer_frames and not (
                    queued_frames > 0 and short_reply_ready
                ):
                    await asyncio.sleep(0.005)
                    continue
                with self._tts_lock:
                    self._tts_playback_started = True
                next_audio_send_at = time.monotonic()
                initial_burst_remaining = min(
                    self.playback_initial_burst_frames,
                    max(1, queued_frames),
                )
                underrun_started_at = None
                print(
                    "[MUXIVA][XIAOZHI][playback.started] "
                    f"queued_frames={queued_frames} "
                    f"prebuffer_frames={self._prebuffer_frames} "
                    f"initial_burst_frames={initial_burst_remaining} "
                    f"initial_burst_interval_ms={self.playback_initial_burst_interval_ms}",
                    file=sys.stderr,
                    flush=True,
                )
            try:
                packet = self._egress.get_nowait()
            except queue.Empty:
                pending_stop = None
                flush_tail = False
                now = time.monotonic()
                with self._tts_lock:
                    if self._tts_stop_message is not None:
                        audio_drained = (
                            self._tts_audio_seen
                            and now
                            - max(
                                self._tts_last_audio_at,
                                self._tts_last_packet_sent_at,
                            )
                            >= quiet_before_stop_seconds
                        )
                        audio_never_arrived = (
                            not self._tts_audio_seen
                            and now - self._tts_stop_requested_at
                            >= stop_without_audio_timeout_seconds
                        )
                        if audio_drained or audio_never_arrived:
                            flush_tail = audio_drained
                            if not flush_tail:
                                pending_stop = self._tts_stop_message
                                self._tts_stop_message = None
                                self._tts_stop_requested_at = 0.0
                if flush_tail and self._flush_pcm_tail():
                    # The padded tail is now an ordinary paced packet. Keep the
                    # stop marker pending until that packet has been sent.
                    continue
                if flush_tail:
                    with self._tts_lock:
                        pending_stop = self._tts_stop_message
                        self._tts_stop_message = None
                        self._tts_stop_requested_at = 0.0
                if pending_stop is not None and self._ws is not None:
                    try:
                        await self._ws.send(pending_stop)
                        with self._tts_lock:
                            self._tts_playback_started = False
                            self._tts_audio_seen = False
                    except Exception:
                        pass
                elif playback_started and not stop_pending:
                    if underrun_started_at is None:
                        underrun_started_at = now
                    elif now - underrun_started_at >= frame_seconds * 2:
                        print(
                            "[MUXIVA][XIAOZHI][playback.underrun] "
                            f"gap_ms={round((now - underrun_started_at) * 1000)}",
                            file=sys.stderr,
                            flush=True,
                        )
                        # Log once per empty interval; a new packet rearms it.
                        underrun_started_at = float("inf")
                await asyncio.sleep(0.005)
                continue
            underrun_started_at = None
            if self._ws is not None:
                try:
                    await self._ws.send(packet)
                    with self._tts_lock:
                        self._tts_last_packet_sent_at = time.monotonic()
                except Exception:
                    pass
            # Seed the firmware jitter buffer with a small, bounded lead, but
            # space those leading packets so constrained ESP32 receive queues
            # are not hit by an instantaneous burst. After that startup window,
            # every exact Opus frame follows one absolute real-time clock.
            if initial_burst_remaining > 0:
                initial_burst_remaining -= 1
                if initial_burst_remaining > 0:
                    if self.playback_initial_burst_interval_ms > 0:
                        await asyncio.sleep(
                            self.playback_initial_burst_interval_ms / 1000.0
                        )
                    continue
                next_audio_send_at = time.monotonic() + frame_seconds
                await asyncio.sleep(
                    max(0.0, next_audio_send_at - time.monotonic())
                )
                continue
            # Sleeping a full frame after each send would accumulate
            # encoding/event-loop overhead, so pace against deadlines.
            now = time.monotonic()
            if (
                next_audio_send_at is None
                or next_audio_send_at < now - frame_seconds
            ):
                next_audio_send_at = now
            next_audio_send_at += frame_seconds
            await asyncio.sleep(max(0.0, next_audio_send_at - time.monotonic()))

    # ------------------------------------------------------------------ control server
    def _run_control_server(self) -> None:
        gateway = self

        class Handler(socketserver.StreamRequestHandler):
            def handle(self) -> None:
                role = None
                try:
                    for line in self.rfile:
                        line = line.strip()
                        if not line:
                            continue
                        try:
                            command = json.loads(line)
                        except json.JSONDecodeError:
                            continue
                        op = command.get("op")
                        if op == "hello":
                            role = command.get("role")
                            self.wfile.write(b'{"ok":true}\n')
                            self.wfile.flush()
                            continue
                        if op == "audio" and role == "sink":
                            pcm = bytes.fromhex(command.get("pcm_hex", ""))
                            gateway.publish_audio(pcm)
                        elif op == "message":
                            payload = command.get("payload") or {}
                            # Event Encoder owns ordinary display/control
                            # messages.  The audio Sink owns exactly one
                            # control transition: the deferred tts/stop emitted
                            # after its cross-edge PCM frame barrier releases.
                            sink_stop = (
                                role == "sink"
                                and payload.get("type") == TTS_TYPE
                                and payload.get("state") == "stop"
                            )
                            if role == "events" or sink_stop:
                                gateway.publish_message(payload)
                        elif op == "reset":
                            gateway.reset_egress()
                except (ConnectionError, OSError):
                    pass

        class Server(socketserver.ThreadingTCPServer):
            allow_reuse_address = True
            daemon_threads = True

        with Server((self.control_host, self.control_port), Handler) as server:
            server.serve_forever()


class XiaozhiControlClient:
    """Loopback client used by the Sink and Event Encoder Nodes."""

    def __init__(self, host: str, port: int, role: str) -> None:
        self.host = host
        self.port = port
        self.role = role
        self._socket = None

    def connect(self) -> bool:
        import socket

        try:
            self._socket = socket.create_connection((self.host, self.port), timeout=2.0)
            self._socket.sendall(
                json.dumps({"op": "hello", "role": self.role}).encode() + b"\n"
            )
            return True
        except OSError:
            self._socket = None
            return False

    def is_connected(self) -> bool:
        return self._socket is not None

    def send(self, command: dict) -> None:
        if self._socket is None:
            return
        try:
            self._socket.sendall(json.dumps(command).encode() + b"\n")
        except OSError:
            self._socket = None

    def close(self) -> None:
        if self._socket is not None:
            try:
                self._socket.close()
            except OSError:
                pass
            self._socket = None
