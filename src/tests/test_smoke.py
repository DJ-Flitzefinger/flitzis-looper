from flitzis_looper_rs import AudioEngine


def test_smoke():
    engine = AudioEngine()
    engine.play()
    engine.stop()
