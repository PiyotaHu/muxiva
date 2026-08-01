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
        self.handler = _handler
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

    def renewToken(self, token):
        self.calls.append(("renew", token))
        return 0 if token == "fresh-token" else -9

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


def test_client_tracks_reconnect_token_and_quality_without_leaking_credentials():
    module, engine = fake_agora()
    client = AgoraRtcClient("app", agora=module, event_capacity=16)
    engine.handler.onConnectionStateChanged(2, 0)
    engine.handler.onConnectionStateChanged(3, 0)
    engine.handler.onTokenPrivilegeWillExpire("secret-old-token")
    client.renew_token("fresh-token")
    engine.handler.onConnectionStateChanged(4, 2)
    engine.handler.onConnectionLost()
    engine.handler.onConnectionStateChanged(3, 0)
    engine.handler.onRejoinChannelSuccess("room", 7, 123)
    engine.handler.onNetworkQuality(42, 3, 5)
    engine.handler.onRtcStats(
        SimpleNamespace(
            duration=60,
            txBytes=1000,
            rxBytes=2000,
            userCount=2,
            lastmileDelay=45,
        )
    )

    events = []
    while (event := client.try_pop_event()) is not None:
        events.append(event)
    assert [event.kind for event in events] == [
        "connection_state",
        "connection_state",
        "token_will_expire",
        "connection_state",
        "connection_lost",
        "connection_state",
        "rejoined",
        "network_quality",
        "rtc_stats",
    ]
    assert "secret-old-token" not in repr(events)
    assert "fresh-token" not in repr(events)
    stats = client.rtc_stats
    assert stats.connection_epoch == 2
    assert stats.reconnects == 1
    assert stats.connection_losses == 1
    assert stats.token_expiring == 1
    assert stats.token_renewals == 1
    assert stats.network_quality_samples == 1
    assert stats.worst_tx_quality == 3
    assert stats.worst_rx_quality == 5
    assert stats.rtc_stats_samples == 1
    assert stats.duration_seconds == 60
    assert stats.tx_bytes == 1000
    assert stats.rx_bytes == 2000
    assert stats.user_count == 2
    assert stats.lastmile_delay_ms == 45
    client.close()


def test_control_event_queue_is_bounded_and_renewal_failures_are_counted():
    module, engine = fake_agora()
    client = AgoraRtcClient("app", agora=module, event_capacity=1)
    engine.handler.onRequestToken()
    engine.handler.onRequestToken()
    with pytest.raises(RuntimeError, match="renewToken failed with code -9"):
        client.renew_token("bad-token")
    assert client.rtc_stats.events_accepted == 1
    assert client.rtc_stats.events_dropped == 1
    assert client.rtc_stats.token_required == 2
    assert client.rtc_stats.token_renewal_failures == 1
    client.close()
