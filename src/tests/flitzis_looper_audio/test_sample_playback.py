import time
import wave
from typing import TYPE_CHECKING

import pytest

from flitzis_looper_audio import AudioEngine
from tests.conftest import write_mono_pcm16_wav

if TYPE_CHECKING:
    from pathlib import Path


def _wait_for_loader_event(
    audio_engine: AudioEngine,
    sample_id: int,
    event_type: str,
    *,
    task: str | None = None,
) -> dict[str, object]:
    deadline = time.monotonic() + 2.0
    while time.monotonic() < deadline:
        event = audio_engine.poll_loader_events()
        if event is None:
            time.sleep(0.01)
            continue

        if event.get("id") != sample_id or event.get("type") != event_type:
            continue
        if task is not None and event.get("task") != task:
            continue
        return event

    pytest.fail(f"timed out waiting for {event_type!r} event")


def _wait_for_input_event(audio_engine: AudioEngine) -> dict[str, object]:
    deadline = time.monotonic() + 1.0
    while time.monotonic() < deadline:
        event = audio_engine.poll_input_events()
        if event is not None:
            return event
        time.sleep(0.01)

    pytest.fail("timed out waiting for input event")


@pytest.mark.parametrize("sample_rate_hz", [48_000, 44_100])
def test_load_and_play_sample_smoke(
    sample_rate_hz: int, audio_engine: AudioEngine, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)

    wav_path = tmp_path / "sample.wav"
    cached_path: str | None = None
    write_mono_pcm16_wav(wav_path, sample_rate_hz)

    audio_engine.load_sample_async(0, str(wav_path), run_analysis=True)

    deadline = time.monotonic() + 2.0
    error_msg: str | None = None

    while time.monotonic() < deadline:
        event = audio_engine.poll_loader_events()
        if event is None:
            time.sleep(0.01)
            continue

        if event.get("id") != 0:
            continue

        if event.get("type") == "success":
            cached = event.get("cached_path")
            if isinstance(cached, str):
                cached_path = cached
            break

        if event.get("type") == "error":
            msg = event.get("msg")
            if isinstance(msg, str):
                error_msg = msg
            break

    if error_msg is not None:
        if "sample rate mismatch" in error_msg:
            pytest.skip("No matching sample rate for output device")
        else:
            pytest.fail(f"expected sample load success, got error: {error_msg!r}")

    assert cached_path is not None
    assert (tmp_path / cached_path).is_file()

    audio_engine.play_sample(0, 1.0)
    audio_engine.stop_all()
    audio_engine.unload_sample(0)


def test_sample_slot_id_range_is_0_to_215(audio_engine: AudioEngine) -> None:
    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.load_sample_async(216, "does-not-matter.wav", run_analysis=True)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.play_sample(216, 1.0)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.play_sample_exclusive(216, 1.0)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.anchor_transport_phase_from_pad(216)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.generate_stems_async(216, "source", "samples/stems/cache")

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.publish_prepared_stems(216, "source", "samples/stems/cache")

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.set_stem_mix_mode(216, "full_mix")

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.set_stem_enabled_mask(216, 1, "source")

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.stop_sample(216)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.unload_sample(216)

    audio_engine.load_sample_async(215, "file-does-not-exist.wav", run_analysis=True)

    deadline = time.monotonic() + 1.0
    saw_error = False
    while time.monotonic() < deadline:
        event = audio_engine.poll_loader_events()
        if event is None:
            time.sleep(0.01)
            continue

        if event.get("type") == "error" and event.get("id") == 215:
            saw_error = True
            break

    assert saw_error

    # Shouldn't crash
    audio_engine.anchor_transport_phase_from_pad(215)
    audio_engine.play_sample(215, 1.0)
    audio_engine.play_sample_exclusive(215, 1.0)
    audio_engine.stop_sample(215)
    audio_engine.unload_sample(215)


def test_stop_all_requires_initialized_engine() -> None:
    try:
        engine = AudioEngine()
    except RuntimeError as exc:
        pytest.skip(f"AudioEngine unavailable: {exc}")

    with pytest.raises(RuntimeError, match=r"Audio engine not initialized"):
        engine.stop_all()


def test_play_sample_exclusive_requires_initialized_engine() -> None:
    engine = AudioEngine()

    with pytest.raises(RuntimeError, match=r"Audio engine not initialized"):
        engine.play_sample_exclusive(0, 1.0)


def test_stop_all_is_safe_when_nothing_playing(audio_engine: AudioEngine) -> None:
    audio_engine.stop_all()


def test_set_trigger_quantization_accepts_supported_modes(audio_engine: AudioEngine) -> None:
    modes = (
        "immediate",
        "disabled",
        "off",
        "next_beat",
        "next-beat",
        "beat",
        "next_bar",
        "next-bar",
        "bar",
    )

    for mode in modes:
        audio_engine.set_trigger_quantization(mode)


def test_set_trigger_quantization_rejects_invalid_mode(audio_engine: AudioEngine) -> None:
    with pytest.raises(ValueError, match=r"trigger quantization mode"):
        audio_engine.set_trigger_quantization("half_note")


def test_set_trigger_quantization_requires_initialized_engine() -> None:
    engine = AudioEngine()

    with pytest.raises(RuntimeError, match=r"Audio engine not initialized"):
        engine.set_trigger_quantization("next_beat")


def test_injected_midi_input_reports_normalized_mapping_event(
    audio_engine: AudioEngine,
) -> None:
    audio_engine.set_input_mapping_enabled(True)
    audio_engine.set_input_mapping_snapshot([("midi:note:1:60", "ui.select_bank:2")])

    assert audio_engine.inject_midi_input_for_test([0x90, 60, 100]) is True
    event = _wait_for_input_event(audio_engine)

    assert event["source"] == "midi"
    assert event["binding_key"] == "midi:note:1:60"
    assert event["action_key"] == "ui.select_bank:2"
    assert event["direct"] is False
    assert event["dispatched"] is True
    assert isinstance(event["received_at_ns"], int)

    assert audio_engine.inject_midi_input_for_test([0x90, 60, 0]) is False
    assert audio_engine.poll_input_events() is None


def test_set_stem_mix_mode_accepts_supported_modes(audio_engine: AudioEngine) -> None:
    audio_engine.set_stem_mix_mode(0, "full_mix")
    audio_engine.set_stem_mix_mode(0, "all_stems", "source-version")


def test_set_stem_mix_mode_rejects_invalid_mode(audio_engine: AudioEngine) -> None:
    with pytest.raises(ValueError, match=r"stem mix mode"):
        audio_engine.set_stem_mix_mode(0, "half_stems")


def test_set_stem_mix_mode_all_stems_requires_source_version(
    audio_engine: AudioEngine,
) -> None:
    with pytest.raises(ValueError, match=r"source_version"):
        audio_engine.set_stem_mix_mode(0, "all_stems")


def test_set_stem_mix_mode_requires_initialized_engine() -> None:
    engine = AudioEngine()

    with pytest.raises(RuntimeError, match=r"Audio engine not initialized"):
        engine.set_stem_mix_mode(0, "full_mix")


def test_set_stem_enabled_mask_accepts_component_mask(audio_engine: AudioEngine) -> None:
    audio_engine.set_stem_enabled_mask(0, 0b1111, "source-version")


def test_set_stem_enabled_mask_rejects_unsupported_mask(audio_engine: AudioEngine) -> None:
    with pytest.raises(ValueError, match=r"stem enabled mask"):
        audio_engine.set_stem_enabled_mask(0, 0b1_0000, "source-version")


def test_set_stem_enabled_mask_rejects_empty_source_version(
    audio_engine: AudioEngine,
) -> None:
    with pytest.raises(ValueError, match=r"source_version"):
        audio_engine.set_stem_enabled_mask(0, 0b1111, "")


def test_set_stem_enabled_mask_requires_initialized_engine() -> None:
    engine = AudioEngine()

    with pytest.raises(RuntimeError, match=r"Audio engine not initialized"):
        engine.set_stem_enabled_mask(0, 0b1111, "source-version")


def test_set_pad_timing_metadata_accepts_finite_anchor(audio_engine: AudioEngine) -> None:
    audio_engine.set_pad_timing_metadata(0, 1.25)


def test_set_pad_timing_metadata_rejects_invalid_anchor(audio_engine: AudioEngine) -> None:
    with pytest.raises(ValueError, match=r"phase_anchor_s out of range"):
        audio_engine.set_pad_timing_metadata(0, float("nan"))

    with pytest.raises(ValueError, match=r"phase_anchor_s out of range"):
        audio_engine.set_pad_timing_metadata(0, -1.0)


def test_set_pad_timing_metadata_requires_initialized_engine() -> None:
    engine = AudioEngine()

    with pytest.raises(RuntimeError, match=r"Audio engine not initialized"):
        engine.set_pad_timing_metadata(0, 1.25)


def test_anchor_transport_phase_from_pad_requires_initialized_engine() -> None:
    engine = AudioEngine()

    with pytest.raises(RuntimeError, match=r"Audio engine not initialized"):
        engine.anchor_transport_phase_from_pad(0)


def test_generate_stems_async_requires_initialized_engine() -> None:
    engine = AudioEngine()

    with pytest.raises(RuntimeError, match=r"Audio engine not initialized"):
        engine.generate_stems_async(0, "source", "samples/stems/cache")


def test_loaded_sample_shape_requires_initialized_engine() -> None:
    engine = AudioEngine()

    with pytest.raises(RuntimeError, match=r"Audio engine not initialized"):
        engine.loaded_sample_shape(0)


def test_publish_prepared_stems_requires_initialized_engine() -> None:
    engine = AudioEngine()

    with pytest.raises(RuntimeError, match=r"Audio engine not initialized"):
        engine.publish_prepared_stems(0, "source", "samples/stems/cache")


def test_generate_stems_async_rejects_empty_source_version(audio_engine: AudioEngine) -> None:
    with pytest.raises(ValueError, match=r"source_version"):
        audio_engine.generate_stems_async(0, "", "samples/stems/cache")


def test_generate_stems_async_rejects_invalid_cache_dir(audio_engine: AudioEngine) -> None:
    with pytest.raises(ValueError, match=r"stem cache directory"):
        audio_engine.generate_stems_async(0, "source", "../samples/stems/cache")


def test_publish_prepared_stems_rejects_invalid_cache_dir(audio_engine: AudioEngine) -> None:
    with pytest.raises(ValueError, match=r"stem cache directory"):
        audio_engine.publish_prepared_stems(0, "source", "../samples/stems/cache")


def test_generate_stems_async_writes_project_cache_artifacts(
    audio_engine: AudioEngine, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    wav_path = tmp_path / "sample.wav"
    write_mono_pcm16_wav(wav_path, audio_engine.output_sample_rate())

    audio_engine.load_sample_async(0, str(wav_path), run_analysis=False)
    _wait_for_loader_event(audio_engine, 0, "success")

    sample_rate, channels, frame_count = audio_engine.loaded_sample_shape(0)
    assert sample_rate == audio_engine.output_sample_rate()
    assert channels >= 1
    assert frame_count == 128

    cache_dir = "samples/stems/cache"
    audio_engine.generate_stems_async(0, "source-version", cache_dir)
    _wait_for_loader_event(audio_engine, 0, "task_success", task="stem_generation")

    for stem_name in ("vocals", "melody", "bass", "drums", "instrumental"):
        path = tmp_path / cache_dir / f"{stem_name}.wav"
        assert path.is_file()
        with wave.open(str(path), "rb") as wav:
            assert wav.getframerate() == audio_engine.output_sample_rate()
            assert wav.getnframes() == 128
            assert wav.getnchannels() >= 1

    audio_engine.publish_prepared_stems(0, "source-version", cache_dir)


def test_publish_prepared_stems_rejects_misaligned_cache_artifacts(
    audio_engine: AudioEngine, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.chdir(tmp_path)
    wav_path = tmp_path / "sample.wav"
    output_rate = audio_engine.output_sample_rate()
    write_mono_pcm16_wav(wav_path, output_rate)

    audio_engine.load_sample_async(0, str(wav_path), run_analysis=False)
    _wait_for_loader_event(audio_engine, 0, "success")

    cache_dir = tmp_path / "samples" / "stems" / "cache"
    cache_dir.mkdir(parents=True)
    for stem_name in ("vocals", "melody", "bass", "drums", "instrumental"):
        rate = output_rate + 1 if stem_name == "vocals" else output_rate
        write_mono_pcm16_wav(cache_dir / f"{stem_name}.wav", rate)

    with pytest.raises(ValueError, match=r"sample rate mismatch"):
        audio_engine.publish_prepared_stems(0, "source-version", "samples/stems/cache")


def test_load_sample_async_emits_started_and_error_for_missing_file(
    audio_engine: AudioEngine,
) -> None:
    audio_engine.load_sample_async(0, "file-does-not-exist.wav", run_analysis=True)

    deadline = time.monotonic() + 1.0
    seen: dict[str, dict[str, object]] = {}

    while time.monotonic() < deadline and ("started" not in seen or "error" not in seen):
        event = audio_engine.poll_loader_events()
        if event is None:
            time.sleep(0.01)
            continue

        event_type = event.get("type")
        if isinstance(event_type, str):
            seen[event_type] = event

    assert "started" in seen
    assert seen["started"].get("id") == 0

    assert "error" in seen
    assert seen["error"].get("id") == 0
    assert isinstance(seen["error"].get("msg"), str)


def test_poll_loader_events_returns_none_when_empty(audio_engine: AudioEngine) -> None:
    # Ensure the queue is drained.
    deadline = time.monotonic() + 0.2
    while time.monotonic() < deadline and audio_engine.poll_loader_events() is not None:
        pass

    assert audio_engine.poll_loader_events() is None
