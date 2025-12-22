import time

from flitzis_looper_audio import AudioEngine, AudioMessage


def wait_for_msg(engine: AudioEngine, timeout_ms: int = 100) -> AudioMessage | None:
    deadline = time.monotonic() + timeout_ms / 1000

    while time.monotonic() < deadline:
        result = engine.receive_msg()
        if result is not None:
            return result
        time.sleep(0.001)

    return None


def test_ping_pong(audio_engine: AudioEngine) -> None:
    audio_engine.ping()
    msg = wait_for_msg(audio_engine)
    assert isinstance(msg, AudioMessage.Pong)


def test_multiple_pings(audio_engine: AudioEngine) -> None:
    for _ in range(5):
        audio_engine.ping()

    time.sleep(0.1)

    for _ in range(5):
        msg = wait_for_msg(audio_engine)
        assert isinstance(msg, AudioMessage.Pong)
