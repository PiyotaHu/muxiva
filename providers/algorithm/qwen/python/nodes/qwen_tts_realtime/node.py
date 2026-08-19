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
import uuid
from typing import Any, Callable, Iterable
from urllib.parse import quote

import muxiva


class _WebSocketTransport:
    def __init__(
        self,
        endpoint: str,
        api_key: str,
        workspace_id: str,
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
                f"X-DashScope-WorkSpace: {workspace_id}",
            ],
            timeout=10,
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
            if value is None:
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
        if input_port in (None, "text_in") and hasattr(frame, "text"):
            text = frame.text.strip()
            if text:
                sequence = int(frame.sequence)
                with self._lock:
                    if sequence <= self._cancelled_through_sequence:
                        self._log(
                            "text.dropped",
                            sequence=sequence,
                            reason="cancelled_turn",
                        )
                        return
                try:
                    self._jobs.put_nowait((self._current_generation(), text, sequence))
                except queue.Full as error:
                    raise RuntimeError("Qwen TTS pending text queue is full") from error
                with self._lock:
                    self._pending_jobs += 1
                ctx.schedule_next_tick(20)
            return
        if input_port == "tick_in" or (input_port is None and frame is None):
            self._drain(ctx)
            if self._has_pending_work():
                ctx.schedule_next_tick(20)
            return
        raise ValueError(f"Qwen TTS received unsupported input port: {input_port}")

    def on_signal(self, signal: Any, _ctx: Any = None) -> None:
        if getattr(signal, "name", "") != "muxiva.voice.speech.started":
            return
        with self._lock:
            self._cancelled_through_sequence = max(
                self._cancelled_through_sequence,
                int(getattr(signal, "sequence", 0)),
            )
            actively_synthesizing = self._pending_jobs > 0 or not self._results.empty()
        cancelled = self._invalidate()
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
        transport_generation = -1
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
                if transport is None or transport_generation != generation:
                    if transport is not None:
                        self._close_transport(transport)
                        self._forget_transport(transport)
                    try:
                        transport = self._open_transport()
                        transport_generation = generation
                        with self._lock:
                            if generation != self._generation:
                                self._close_transport(transport)
                                transport = None
                                continue
                            self._active_transport = transport
                    except Exception as error:
                        transport = None
                        if generation == self._current_generation() and not self._closing.is_set():
                            self._put_result(
                                (generation, "error", str(error)[:512], sequence)
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
                return
            if generation != self._current_generation():
                continue
            if kind == "audio":
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
                raise RuntimeError(f"Qwen TTS failed: {value}")

    def _invalidate(self) -> bool:
        with self._lock:
            self._generation += 1
            self._pending_jobs = 0
            transport = self._active_transport
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

    def _complete_job(self) -> None:
        with self._lock:
            self._pending_jobs = max(0, self._pending_jobs - 1)

    def _has_pending_work(self) -> bool:
        with self._lock:
            return self._pending_jobs > 0 or not self._results.empty()

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
