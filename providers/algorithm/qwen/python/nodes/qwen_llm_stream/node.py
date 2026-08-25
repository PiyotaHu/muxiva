"""Cancellable Qwen streaming LLM application Node Pack for Muxiva.

Provider I/O runs on a background worker. The Node asks the Runtime for short,
internal callbacks that drain bounded results, so ``on_signal`` remains
responsive without exposing a clock Node in the application Graph.
"""

from __future__ import annotations

import json
import os
import queue
import re
import sys
import threading
import urllib.request
from typing import Any, Callable, Iterable

import muxiva


class _SseClient:
    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._response: Any | None = None

    def stream(
        self,
        endpoint: str,
        api_key: str,
        payload: dict[str, Any],
        cancelled: threading.Event,
    ) -> Iterable[str]:
        request = urllib.request.Request(
            endpoint,
            data=json.dumps(payload, separators=(",", ":")).encode(),
            headers={"Authorization": f"Bearer {api_key}", "Content-Type": "application/json"},
            method="POST",
        )
        response = urllib.request.urlopen(request, timeout=60)
        with self._lock:
            self._response = response
        try:
            with response:
                for raw_line in response:
                    if cancelled.is_set():
                        return
                    line = raw_line.decode("utf-8").strip()
                    if not line.startswith("data: ") or line == "data: [DONE]":
                        continue
                    event = json.loads(line[6:])
                    choices = event.get("choices", [])
                    if choices:
                        text = choices[0].get("delta", {}).get("content", "")
                        if text:
                            yield text
        finally:
            with self._lock:
                if self._response is response:
                    self._response = None

    def cancel(self) -> None:
        with self._lock:
            response = self._response
        if response is not None:
            response.close()


class QwenLlmStreamNode:
    def __init__(
        self,
        config: dict[str, Any] | None = None,
        client_factory: Callable[[], Any] = _SseClient,
    ) -> None:
        self.config = config or {}
        self._client_factory = client_factory
        self._lock = threading.Lock()
        self._results: queue.Queue[tuple[int, str, Any, int]] = queue.Queue(maxsize=512)
        self._generation = 0
        self._cancelled: threading.Event | None = None
        self._active_client: Any | None = None
        self._worker: threading.Thread | None = None
        self._history: list[dict[str, str]] = []
        self._closed = False

    @staticmethod
    def _log(event: str, **fields: Any) -> None:
        detail = " ".join(f"{key}={value}" for key, value in fields.items())
        print(f"[MUXIVA][QWEN-LLM][{event}] {detail}".rstrip(), file=sys.stderr, flush=True)

    def on_process(self, frame: Any, ctx: Any) -> None:
        input_port = getattr(ctx, "input_port", None)
        if input_port in (None, "text_in") and hasattr(frame, "text"):
            self._start_generation(frame.text, frame.sequence)
            ctx.schedule_next_tick(20)
            return
        if input_port == "tick_in" or (input_port is None and frame is None):
            self._drain(ctx)
            if self._has_pending_work():
                ctx.schedule_next_tick(20)
            return
        raise ValueError(f"Qwen LLM received unsupported input port: {input_port}")

    def on_signal(self, signal: Any, _ctx: Any = None) -> None:
        if getattr(signal, "name", "") not in {
            "muxiva.turn.cancelled",
            "muxiva.voice.speech.started",  # pre-controller compatibility
        }:
            return
        self._cancel_current()
        self._log("generation.cancelled", sequence=getattr(signal, "sequence", 0))

    def on_finish(self, _ctx: Any = None) -> None:
        self._closed = True
        self._cancel_current()
        worker = self._worker
        if worker is not None and worker.is_alive():
            worker.join(timeout=2)

    def on_abort(self, _reason: str, ctx: Any = None) -> None:
        self.on_finish(ctx)

    def _start_generation(self, text: str, sequence: int) -> None:
        text = text.strip()
        if not text:
            return
        if self._closed:
            raise RuntimeError("Qwen LLM Node is closed")
        self._cancel_current()
        api_key, workspace = _credentials()
        with self._lock:
            self._generation += 1
            generation = self._generation
            cancelled = threading.Event()
            self._cancelled = cancelled
            history = list(self._history[-12:])
        system_prompt = self.config.get(
            "system_prompt",
            "You are a capable assistant. Respond in the user's language and use provided context accurately.",
        )
        payload = {
            "model": self.config.get("model", "qwen-flash"),
            "messages": [
                {"role": "system", "content": system_prompt},
                *history,
                {"role": "user", "content": text},
            ],
            "temperature": float(self.config.get("temperature", 0.6)),
            "stream": True,
        }
        endpoint = (
            f"https://{workspace}.cn-beijing.maas.aliyuncs.com/"
            "compatible-mode/v1/chat/completions"
        )
        worker = threading.Thread(
            target=self._run_generation,
            args=(generation, cancelled, endpoint, api_key, payload, text, sequence),
            name=f"muxiva-qwen-llm-{generation}",
            daemon=True,
        )
        self._worker = worker
        worker.start()
        self._log("generation.started", generation=generation, sequence=sequence)

    def _run_generation(
        self,
        generation: int,
        cancelled: threading.Event,
        endpoint: str,
        api_key: str,
        payload: dict[str, Any],
        user_text: str,
        sequence: int,
    ) -> None:
        client = self._client_factory()
        with self._lock:
            if generation != self._generation or cancelled.is_set():
                return
            self._active_client = client
        answer: list[str] = []
        try:
            deltas = client.stream(endpoint, api_key, payload, cancelled)
            for delta in semantic_deltas(deltas, cancelled):
                if cancelled.is_set() or generation != self._current_generation():
                    return
                if not delta:
                    continue
                answer.append(delta)
                self._put_result((generation, "delta", delta, sequence), cancelled)
            if not cancelled.is_set() and generation == self._current_generation():
                self._put_result(
                    (generation, "done", {"user": user_text, "answer": "".join(answer)}, sequence),
                    cancelled,
                )
        except Exception as error:
            if not cancelled.is_set() and generation == self._current_generation():
                self._put_result((generation, "error", str(error)[:512], sequence), cancelled)
        finally:
            with self._lock:
                if self._active_client is client:
                    self._active_client = None

    def _drain(self, ctx: Any) -> None:
        maximum = int(
            self.config.get(
                "max_results_per_wakeup",
                self.config.get("max_results_per_tick", 32),
            )
        )
        for _ in range(maximum):
            try:
                generation, kind, value, sequence = self._results.get_nowait()
            except queue.Empty:
                return
            if generation != self._current_generation():
                continue
            if kind == "delta":
                ctx.emit("text_out", muxiva.TextFrame(value, sequence=sequence))
                ctx.publish_notification("muxiva.model.response.delta", {"text": value})
            elif kind == "done":
                answer = value["answer"]
                if answer:
                    with self._lock:
                        self._history.extend([
                            {"role": "user", "content": value["user"]},
                            {"role": "assistant", "content": answer},
                        ])
                        self._history = self._history[-12:]
                    self._emit_event(
                        ctx, "muxiva.model.response.completed", {"text": answer}, sequence
                    )
                self._log("generation.completed", generation=generation, chars=len(answer))
            elif kind == "error":
                raise RuntimeError(f"Qwen LLM stream failed: {value}")

    @staticmethod
    def _emit_event(ctx: Any, topic: str, payload: dict[str, Any], sequence: int) -> None:
        ctx.emit(
            "event_out",
            muxiva.EventFrame(
                topic,
                json.dumps(payload, separators=(",", ":"), ensure_ascii=False),
                source="qwen.llm_stream",
                sequence=sequence,
            ),
        )
        ctx.publish_notification(topic, payload)

    def _cancel_current(self) -> None:
        with self._lock:
            self._generation += 1
            cancelled = self._cancelled
            client = self._active_client
            self._cancelled = None
            self._active_client = None
        if cancelled is not None:
            cancelled.set()
        cancel = getattr(client, "cancel", None)
        if cancel is not None:
            try:
                cancel()
            except Exception:
                pass
        self._clear_results()

    def _current_generation(self) -> int:
        with self._lock:
            return self._generation

    def _has_pending_work(self) -> bool:
        worker = self._worker
        return (worker is not None and worker.is_alive()) or not self._results.empty()

    def _put_result(
        self,
        value: tuple[int, str, Any, int],
        cancelled: threading.Event,
    ) -> None:
        while not cancelled.is_set():
            try:
                self._results.put(value, timeout=0.05)
                return
            except queue.Full:
                continue

    def _clear_results(self) -> None:
        while True:
            try:
                self._results.get_nowait()
            except queue.Empty:
                return


def _credentials() -> tuple[str, str]:
    api_key = os.environ.get("DASHSCOPE_API_KEY", "")
    workspace = os.environ.get("DASHSCOPE_WORKSPACE_ID", "")
    if not api_key or not workspace:
        raise RuntimeError("configure DashScope in Studio Connections")
    if re.fullmatch(r"[A-Za-z0-9-]{1,128}", workspace) is None:
        raise ValueError("DASHSCOPE_WORKSPACE_ID has an invalid format")
    return api_key, workspace

def semantic_deltas(
    deltas: Iterable[str], cancelled: threading.Event | None = None
) -> Iterable[str]:
    """Forward provider deltas without applying presentation or sentence policy."""
    for delta in deltas:
        if cancelled is not None and cancelled.is_set():
            return
        if delta:
            yield delta
