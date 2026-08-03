"""Qwen OpenAI-compatible streaming LLM application Node Pack."""

from __future__ import annotations

import json
import os
import re
import urllib.request
from typing import Any, Callable, Iterable

import voxa


class _SseClient:
    def stream(self, endpoint: str, api_key: str, payload: dict[str, Any]) -> Iterable[str]:
        request = urllib.request.Request(
            endpoint,
            data=json.dumps(payload, separators=(",", ":")).encode(),
            headers={"Authorization": f"Bearer {api_key}", "Content-Type": "application/json"},
            method="POST",
        )
        with urllib.request.urlopen(request, timeout=60) as response:
            for raw_line in response:
                line = raw_line.decode("utf-8").strip()
                if not line.startswith("data: ") or line == "data: [DONE]":
                    continue
                event = json.loads(line[6:])
                choices = event.get("choices", [])
                if choices:
                    text = choices[0].get("delta", {}).get("content", "")
                    if text:
                        yield text


class QwenLlmStreamNode:
    def __init__(
        self,
        config: dict[str, Any] | None = None,
        client_factory: Callable[[], Any] = _SseClient,
    ) -> None:
        self.config = config or {}
        self._client = client_factory()
        self._history: list[dict[str, str]] = []

    def on_process(self, frame: Any, ctx: Any) -> None:
        api_key, workspace = _credentials()
        self._history.append({"role": "user", "content": frame.text})
        messages = [{"role": "system", "content": self.config.get(
            "system_prompt",
            "You are Voxa, a warm, concise real-time voice assistant. Respond in the user's language.",
        )}, *self._history[-12:]]
        payload = {
            "model": self.config.get("model", "qwen-flash"),
            "messages": messages,
            "temperature": float(self.config.get("temperature", 0.6)),
            "stream": True,
        }
        endpoint = f"https://{workspace}.cn-beijing.maas.aliyuncs.com/compatible-mode/v1/chat/completions"
        answer: list[str] = []
        for sentence in sentence_chunks(self._client.stream(endpoint, api_key, payload)):
            answer.append(sentence)
            # A sentence-sized chunk keeps captions responsive and gives TTS a
            # stable commit boundary instead of synthesizing token fragments.
            ctx.emit("text_out", voxa.TextFrame(sentence, sequence=frame.sequence))
            ctx.emit(
                "client_event_out",
                voxa.EventFrame(
                    "voxa.voice.response.delta",
                    json.dumps({"text": sentence}, separators=(",", ":"), ensure_ascii=False),
                    source="qwen.llm_stream",
                    sequence=frame.sequence,
                ),
            )
            ctx.publish_event("voxa.voice.response.delta", {"text": sentence})
        if answer:
            completed = "".join(answer)
            self._history.append({"role": "assistant", "content": completed})
            ctx.emit(
                "client_event_out",
                voxa.EventFrame(
                    "voxa.voice.response.completed",
                    json.dumps({"text": completed}, separators=(",", ":"), ensure_ascii=False),
                    source="qwen.llm_stream",
                    sequence=frame.sequence,
                ),
            )
            ctx.publish_event("voxa.voice.response.completed", {"text": completed})


def _credentials() -> tuple[str, str]:
    api_key = os.environ.get("DASHSCOPE_API_KEY", "")
    workspace = os.environ.get("DASHSCOPE_WORKSPACE_ID", "")
    if not api_key or not workspace:
        raise RuntimeError("configure DashScope in Studio Connections")
    if re.fullmatch(r"[A-Za-z0-9-]{1,128}", workspace) is None:
        raise ValueError("DASHSCOPE_WORKSPACE_ID has an invalid format")
    return api_key, workspace


def sentence_chunks(deltas: Iterable[str]) -> Iterable[str]:
    buffer = ""
    boundaries = "。！？.!?\n"
    for delta in deltas:
        buffer += delta
        while True:
            positions = [buffer.find(mark) for mark in boundaries if mark in buffer]
            if not positions and len(buffer) < 80:
                break
            end = min(positions) + 1 if positions else 80
            yield buffer[:end]
            buffer = buffer[end:]
    if buffer:
        yield buffer
