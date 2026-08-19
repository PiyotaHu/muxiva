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
import uuid

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

        self._ingress = queue.Queue(maxsize=512)
        self._events = queue.Queue(maxsize=256)
        self._egress = queue.Queue(maxsize=512)
        self._messages = queue.Queue(maxsize=256)

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
        """Encode assistant PCM and queue it for paced delivery."""
        if not self.has_client():
            return
        try:
            packet = self._encoder.encode(pcm)
        except opus_codec.OpusError:
            return
        try:
            self._egress.put_nowait(packet)
        except queue.Full:
            pass

    def publish_message(self, payload: dict) -> None:
        if not self.has_client():
            return
        payload = dict(payload)
        payload.setdefault("session_id", self._client_id or "")
        try:
            self._messages.put_nowait(json.dumps(payload, ensure_ascii=False))
        except queue.Full:
            pass

    def reset_egress(self) -> None:
        """Drop queued assistant audio after a barge-in."""
        while True:
            try:
                self._egress.get_nowait()
            except queue.Empty:
                return

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
        except Exception:
            pass
        finally:
            self._ws = None
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
            if sent_message:
                await asyncio.sleep(0.002)
                continue
            try:
                packet = self._egress.get_nowait()
            except queue.Empty:
                await asyncio.sleep(0.005)
                continue
            if self._ws is not None:
                try:
                    await self._ws.send(packet)
                except Exception:
                    pass
            await asyncio.sleep(frame_seconds)

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
                        elif op == "message" and role == "events":
                            gateway.publish_message(command.get("payload") or {})
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
