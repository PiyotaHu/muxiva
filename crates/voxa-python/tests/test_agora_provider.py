import ctypes
from types import SimpleNamespace

import pytest

from voxa.providers.agora import AgoraAudioIngress, AgoraRtcClient


class FakeObserver:
    pass


class FakeEngine:
    def __init__(self):
        self.calls = []

    def initEventHandler(self, _handler):
        self.calls.append("handler")
        return 0

    def initialize(self, app_id, _context, _area):
        self.calls.append(("initialize", app_id))
        return 0

    def setPlaybackAudioFrameParameters(self, rate, channels, mode, samples):
        self.calls.append(("audio", rate, channels, mode, samples))
        return 0

    def joinChannel(self, token, channel, info, uid):
        self.calls.append(("join", token, channel, info, uid))
        return 0

    def leaveChannel(self):
        self.calls.append("leave")
        return 0

    def release(self):
        self.calls.append("release")


def fake_agora():
    engine = FakeEngine()
    module = SimpleNamespace(
        AudioFrameObserver=FakeObserver,
        RtcEngineEventHandlerBase=object,
        AREA_CODE_GLOB=0xFFFFFFFF,
        RAW_AUDIO_FRAME_OP_MODE_READ_ONLY=0,
        createRtcEngineBridge=lambda: engine,
    )
    module.registerAudioFrameObserver = lambda _engine, observer: setattr(module, "observer", observer)
    module.unregisterAudioFrameObserver = lambda _engine, _observer: setattr(module, "unregistered", True)
    return module, engine


def test_audio_callback_copies_pcm_into_a_bounded_voxa_queue():
    module, _ = fake_agora()
    ingress = AgoraAudioIngress(capacity=1)
    observer = ingress.create_observer(module)
    samples = (ctypes.c_int16 * 2)(1, 2)
    observer.onPlaybackAudioFrame(0, 2, 2, 1, 16000, ctypes.addressof(samples), 7, 0)
    samples[0] = 99
    observer.onPlaybackAudioFrame(0, 2, 2, 1, 16000, ctypes.addressof(samples), 8, 0)

    frame = ingress.try_pop()
    assert frame is not None
    assert frame.data == b"\x01\x00\x02\x00"
    assert frame.timestamp_ns == 7_000_000
    assert ingress.stats.accepted == 1
    assert ingress.stats.full_dropped == 1

    ingress.close()
    observer.onPlaybackAudioFrame(0, 2, 2, 1, 16000, ctypes.addressof(samples), 9, 0)
    assert ingress.stats.closed_dropped == 1


def test_client_owns_registration_join_leave_and_release():
    module, engine = fake_agora()
    client = AgoraRtcClient("app", agora=module)
    client.join("room", "token", 7)
    assert ("join", "token", "room", "", 7) in engine.calls
    assert client.close()
    assert not client.close()
    assert module.unregistered
    assert engine.calls[-2:] == ["leave", "release"]


def test_client_releases_engine_when_initialization_fails():
    module, engine = fake_agora()
    engine.initialize = lambda *_args: -4
    with pytest.raises(RuntimeError, match="initialize failed with code -4"):
        AgoraRtcClient("app", agora=module)
    assert engine.calls[-1] == "release"
