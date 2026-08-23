"""Cancellable Qwen streaming TTS application Node Pack for Muxiva.

Text jobs are synthesized on a background worker and PCM is emitted from
short Runtime tick callbacks. A speech-start Signal closes the active vendor
session and clears both pending text and generated audio immediately.
"""

from __future__ import annotations

import base64
import json
import os
import queue
import re
import ssl
import sys
import threading
import time
import uuid
from typing import Any, Callable, Iterable
from urllib.parse import quote

import muxiva


class _WebSocketTransport:
    def __init__(
        self,
        endpoint: str,
        api_key: str,
        _workspace_id: str,
        session: dict[str, Any],
    ) -> None:
        try:
            import websocket
        except ImportError as error:
            raise RuntimeError("install this Node Pack's requirements.txt") from error
        self._websocket = websocket
        self._socket = websocket.create_connection(
            endpoint,
            header=[
                f"Authorization: Bearer {api_key}",
                f"X-DashScope-WorkSpace: {_workspace_id}",
            ],
            timeout=5,
            enable_multithread=True,
        )
        self._socket.send(json.dumps(session, separators=(",", ":")))
        self._socket.settimeout(15)

    def send(self, event: dict[str, Any]) -> None:
        self._socket.send(json.dumps(event, separators=(",", ":")))

    def poll(self, maximum: int = 1024) -> Iterable[dict[str, Any]]:
        for _ in range(maximum):
            try:
                value = self._socket.recv()
            except (self._websocket.WebSocketTimeoutException, BlockingIOError, ssl.SSLWantReadError):
                return
            if not value:
                return
            event = json.loads(value)
            if not isinstance(event, dict) or not isinstance(event.get("type"), str):
                raise RuntimeError("Qwen TTS event requires a string type")
            yield event
            if event["type"] in {"response.done", "session.finished"}:
                return

    def close(self) -> None:
        self._socket.close()


class QwenTtsRealtimeNode:
    def __init__(
        self,
        config: dict[str, Any] | None = None,
        transport_factory: Callable[[str, str, str, dict[str, Any]], Any] = _WebSocketTransport,
    ) -> None:
        self.config = config or {}
        self._factory = transport_factory
        self._jobs: queue.Queue[tuple[int, str, int] | None] = queue.Queue(maxsize=128)
        self._results: queue.Queue[tuple[int, str, Any, int]] = queue.Queue(maxsize=1024)
        self._lock = threading.Lock()
        self._generation = 0
        self._pending_jobs = 0
        self._terminal_sequence: int | None = None
        self._terminal_generation: int | None = None
        self._drain_not_before = 0.0
        self._emitted_audio_frames: dict[int, int] = {}
        self._cancelled_through_sequence = -1
        self._active_transport: Any | None = None
        self._worker: threading.Thread | None = None
        self._closing = threading.Event()
        self._credentials: tuple[str, str] | None = None

    @staticmethod
    def _log(event: str, **fields: Any) -> None:
        detail = " ".join(f"{key}={value}" for key, value in fields.items())
        print(f"[MUXIVA][QWEN-TTS][{event}] {detail}".rstrip(), file=sys.stderr, flush=True)

    def on_prepare(self, _ctx: Any = None) -> None:
        self._credentials = _credentials()
        self._worker = threading.Thread(
            target=self._run,
            name="muxiva-qwen-tts",
            daemon=True,
        )
        self._worker.start()
        self._log(
            "worker.started",
            model=self.config.get("model", "qwen3-tts-flash-realtime"),
            audio="pcm_s16le/24000/mono",
        )

    def on_process(self, frame: Any, ctx: Any) -> None:
        input_port = getattr(ctx, "input_port", None)
        if input_port == "event_in":
            if getattr(frame, "topic", "") not in {
                "muxiva.agent.response.completed",
                "muxiva.voice.response.completed",
            }:
                return
            sequence = int(getattr(frame, "sequence", 0))
            with self._lock:
                if sequence < self._cancelled_through_sequence:
                    return
                self._terminal_sequence = sequence
                self._terminal_generation = self._generation
                self._drain_not_before = time.monotonic() + self._end_of_turn_grace_seconds()
            self._maybe_emit_drained(ctx)
            if self._has_pending_work():
                ctx.schedule_next_tick(20)
            return
        if input_port in (None, "text_in") and hasattr(frame, "text"):
            text = normalize_tts_text(frame.text.strip())
            if text:
                sequence = int(frame.sequence)
                with self._lock:
                    # A validated final transcript emits the barge-in Signal and
                    # the new prompt with the same Runtime sequence.  The Signal
                    # must invalidate older synthesis, but it must not suppress
                    # the response belonging to that new turn.
                    if sequence < self._cancelled_through_sequence:
                        self._log(
                            "text.dropped",
                            sequence=sequence,
                            reason="cancelled_turn",
                        )
                        return
                    generation = self._generation
                    self._pending_jobs += 1
                    # Agent events and text travel over separate graph edges.
                    # If the terminal event wins that race, late text extends
                    # the barrier so it cannot be mistaken for a drained Turn.
                    if (
                        self._terminal_generation == generation
                        and self._terminal_sequence == sequence
                    ):
                        self._drain_not_before = (
                            time.monotonic() + self._end_of_turn_grace_seconds()
                        )
                try:
                    self._jobs.put_nowait((generation, text, sequence))
                except queue.Full as error:
                    self._complete_job()
                    raise RuntimeError("Qwen TTS pending text queue is full") from error
                ctx.schedule_next_tick(20)
            return
        if input_port == "tick_in" or (input_port is None and frame is None):
            self._drain(ctx)
            if self._has_pending_work():
                ctx.schedule_next_tick(20)
            return
        raise ValueError(f"Qwen TTS received unsupported input port: {input_port}")

    def on_signal(self, signal: Any, _ctx: Any = None) -> None:
        if getattr(signal, "name", "") not in {
            "muxiva.turn.cancelled",
            "muxiva.voice.speech.started",  # pre-controller compatibility
        }:
            return
        with self._lock:
            self._cancelled_through_sequence = max(
                self._cancelled_through_sequence,
                int(getattr(signal, "sequence", 0)),
            )
            actively_synthesizing = self._pending_jobs > 0 or not self._results.empty()
        # A completed Turn leaves a reusable idle vendor session behind.  Do
        # not tear it down merely because the next validated transcript has
        # arrived: reconnecting on every turn caused 20-second first-audio
        # stalls during transient TLS/WebSocket failures.  An actively running
        # synthesis is still closed immediately for real barge-in semantics.
        cancelled = self._invalidate(close_transport=actively_synthesizing)
        self._log(
            "synthesis.cancelled",
            sequence=getattr(signal, "sequence", 0),
            active_session=cancelled,
            actively_synthesizing=actively_synthesizing,
        )

    def on_finish(self, _ctx: Any = None) -> None:
        self._closing.set()
        self._invalidate()
        try:
            self._jobs.put_nowait(None)
        except queue.Full:
            self._clear_queue(self._jobs)
            self._jobs.put_nowait(None)
        worker = self._worker
        if worker is not None and worker.is_alive():
            worker.join(timeout=3)

    def on_abort(self, _reason: str, ctx: Any = None) -> None:
        self.on_finish(ctx)

    def _run(self) -> None:
        transport: Any | None = None
        try:
            while not self._closing.is_set():
                try:
                    job = self._jobs.get(timeout=0.1)
                except queue.Empty:
                    continue
                if job is None:
                    return
                generation, text, sequence = job
                if generation != self._current_generation():
                    continue
                if transport is None:
                    last_error: Exception | None = None
                    retry_count = max(1, int(self.config.get("connect_retries", 3)))
                    for attempt in range(1, retry_count + 1):
                        try:
                            transport = self._open_transport()
                            with self._lock:
                                if generation != self._generation:
                                    self._close_transport(transport)
                                    transport = None
                                    break
                                self._active_transport = transport
                            last_error = None
                            break
                        except Exception as error:
                            transport = None
                            last_error = error
                            self._log(
                                "connect.retry",
                                attempt=attempt,
                                maximum=retry_count,
                                error=str(error)[:160],
                            )
                            if attempt < retry_count and not self._closing.is_set():
                                time.sleep(min(0.5 * attempt, 1.5))
                    if transport is None:
                        if (
                            last_error is not None
                            and generation == self._current_generation()
                            and not self._closing.is_set()
                        ):
                            self._put_result(
                                (generation, "error", str(last_error)[:512], sequence)
                            )
                        continue
                try:
                    self._synthesize(transport, generation, text, sequence)
                except Exception as error:
                    if generation == self._current_generation() and not self._closing.is_set():
                        self._put_result((generation, "error", str(error)[:512], sequence))
                    self._close_transport(transport)
                    self._forget_transport(transport)
                    transport = None
                if generation != self._current_generation() and transport is not None:
                    self._close_transport(transport)
                    self._forget_transport(transport)
                    transport = None
        finally:
            if transport is not None:
                try:
                    transport.send({"event_id": _event_id(), "type": "session.finish"})
                except Exception:
                    pass
                self._close_transport(transport)
            with self._lock:
                self._active_transport = None

    def _open_transport(self) -> Any:
        if self._credentials is None:
            raise RuntimeError("TTS credentials are unavailable")
        key, workspace = self._credentials
        model = str(self.config.get("model", "qwen3-tts-flash-realtime"))
        endpoint = (
            "wss://dashscope.aliyuncs.com/api-ws/v1/realtime"
            f"?model={quote(model, safe='-._')}"
        )
        return self._factory(endpoint, key, workspace, session_update(self.config))

    def _synthesize(
        self,
        transport: Any,
        generation: int,
        text: str,
        sequence: int,
    ) -> None:
        transport.send({
            "event_id": _event_id(),
            "type": "input_text_buffer.append",
            "text": text,
        })
        transport.send({"event_id": _event_id(), "type": "input_text_buffer.commit"})
        for event in transport.poll():
            if generation != self._current_generation():
                return
            kind = event["type"]
            if kind == "response.audio.delta":
                pcm = base64.b64decode(event.get("delta", ""), validate=True)
                if not pcm or len(pcm) > 256 * 1024 or len(pcm) % 2:
                    raise RuntimeError("Qwen TTS returned invalid PCM")
                self._put_result((generation, "audio", pcm, sequence))
            elif kind == "error":
                error = event.get("error", {})
                raise RuntimeError(str(error.get("message", "request failed"))[:512])
        if generation == self._current_generation():
            self._put_result((generation, "done", len(text), sequence))

    @staticmethod
    def _close_transport(transport: Any) -> None:
        try:
            transport.close()
        except Exception:
            pass

    def _forget_transport(self, transport: Any) -> None:
        with self._lock:
            if self._active_transport is transport:
                self._active_transport = None

    def _drain(self, ctx: Any) -> None:
        maximum = int(
            self.config.get(
                "max_results_per_wakeup",
                self.config.get("max_results_per_tick", 64),
            )
        )
        for _ in range(maximum):
            try:
                generation, kind, value, sequence = self._results.get_nowait()
            except queue.Empty:
                break
            if generation != self._current_generation():
                continue
            if kind == "audio":
                with self._lock:
                    self._emitted_audio_frames[sequence] = (
                        self._emitted_audio_frames.get(sequence, 0) + 1
                    )
                ctx.emit(
                    "audio_out",
                    muxiva.AudioFrame(
                        value,
                        sample_rate_hz=24_000,
                        channels=1,
                        sequence=sequence,
                    ),
                )
            elif kind == "done":
                self._complete_job()
                self._log("synthesis.completed", generation=generation, chars=value)
            elif kind == "error":
                self._complete_job()
                # A transient cloud TTS failure must not abort the whole voice
                # graph.  The text has already reached the screen; later turns
                # should be able to reconnect and speak normally.
                self._log("synthesis.failed", generation=generation, error=value)
        self._maybe_emit_drained(ctx)

    def _maybe_emit_drained(self, ctx: Any) -> bool:
        now = time.monotonic()
        with self._lock:
            generation = self._generation
            sequence = self._terminal_sequence
            if (
                sequence is None
                or self._terminal_generation != generation
                or self._pending_jobs > 0
                or not self._jobs.empty()
                or not self._results.empty()
            ):
                return False
            if now < self._drain_not_before:
                return False
            self._terminal_sequence = None
            self._terminal_generation = None
            self._drain_not_before = 0.0
            audio_frames = self._emitted_audio_frames.pop(sequence, 0)
        self._emit_drained(ctx, generation, sequence, audio_frames)
        return True

    @staticmethod
    def _emit_drained(
        ctx: Any,
        generation: int,
        sequence: int,
        audio_frames: int,
    ) -> None:
        # The event and PCM travel over independent Graph edges.  Include the
        # exact frame count so the final transport sink can prevent the control
        # event from overtaking audio still buffered in the resampler path.
        payload = {
            "generation": generation,
            "sequence": sequence,
            "audio_frames": audio_frames,
        }
        ctx.emit(
            "event_out",
            muxiva.EventFrame(
                "muxiva.voice.tts.drained",
                json.dumps(payload, separators=(",", ":")),
                source="qwen.tts_realtime",
                sequence=sequence,
            ),
        )

    def _invalidate(self, *, close_transport: bool = True) -> bool:
        with self._lock:
            self._generation += 1
            self._pending_jobs = 0
            self._terminal_sequence = None
            self._terminal_generation = None
            self._drain_not_before = 0.0
            self._emitted_audio_frames.clear()
            transport = self._active_transport if close_transport else None
            if close_transport:
                self._active_transport = None
        self._clear_queue(self._jobs)
        self._clear_queue(self._results)
        if transport is not None:
            try:
                transport.close()
            except Exception:
                pass
        return transport is not None

    def _current_generation(self) -> int:
        with self._lock:
            return self._generation

    def _complete_job(self) -> int:
        with self._lock:
            self._pending_jobs = max(0, self._pending_jobs - 1)
            return self._pending_jobs

    def _has_pending_work(self) -> bool:
        with self._lock:
            return (
                self._pending_jobs > 0
                or not self._results.empty()
                or self._terminal_sequence is not None
            )

    def _end_of_turn_grace_seconds(self) -> float:
        return max(0, int(self.config.get("end_of_turn_grace_ms", 300))) / 1000.0

    def _put_result(self, value: tuple[int, str, Any, int]) -> None:
        while not self._closing.is_set() and value[0] == self._current_generation():
            try:
                self._results.put(value, timeout=0.05)
                return
            except queue.Full:
                continue

    @staticmethod
    def _clear_queue(target: queue.Queue[Any]) -> None:
        while True:
            try:
                target.get_nowait()
            except queue.Empty:
                return


def session_update(config: dict[str, Any]) -> dict[str, Any]:
    return {
        "event_id": _event_id(),
        "type": "session.update",
        "session": {
            "voice": config.get("voice", "Cherry"),
            "mode": "commit",
            "language_type": config.get("language_type", "Auto"),
            "response_format": "pcm",
            "sample_rate": 24_000,
        },
    }


def normalize_tts_text(text: str) -> str:
    """Make numeric decimals unambiguous to Chinese speech synthesis."""
    return re.sub(r"(?<=\d)\.(?=\d)", "点", text)


def _credentials() -> tuple[str, str]:
    key = os.environ.get("DASHSCOPE_API_KEY", "")
    workspace = os.environ.get("DASHSCOPE_WORKSPACE_ID", "")
    if not key or not workspace:
        raise RuntimeError("configure DashScope in Studio Connections")
    if re.fullmatch(r"[A-Za-z0-9-]{1,128}", workspace) is None:
        raise ValueError("DASHSCOPE_WORKSPACE_ID has an invalid format")
    return key, workspace


def _event_id() -> str:
    return f"event_muxiva_{uuid.uuid4().hex}"
