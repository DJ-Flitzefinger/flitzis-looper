import time
import wave
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.constants import (
    NUM_SAMPLES,
    PAD_EQ_DB_MAX,
    PAD_EQ_DB_MIN,
    PAD_GAIN_DB_MAX,
    PAD_GAIN_DB_MIN,
    SPEED_MAX,
    SPEED_MIN,
    VOLUME_MAX,
    VOLUME_MIN,
)
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


def test_transport_slot_id_range_rejects_project_slot_count(audio_engine: AudioEngine) -> None:
    invalid_id = NUM_SAMPLES

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.load_sample_async(invalid_id, "does-not-matter.wav", run_analysis=True)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.play_sample(invalid_id, 1.0)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.play_sample_exclusive(invalid_id, 1.0)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.pause_sample(invalid_id)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.resume_sample(invalid_id)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.seek_sample(invalid_id, 0.0)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.anchor_transport_phase_from_pad(invalid_id)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.set_pad_bpm(invalid_id, 120.0)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.set_pad_timing_metadata(invalid_id, 0.0)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.set_pad_loop_region(invalid_id, 0.0, None)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.set_pad_gain(invalid_id, 1.0)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.set_pad_eq(invalid_id, 0.0, 0.0, 0.0)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.stop_sample(invalid_id)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.unload_sample(invalid_id)


def test_stem_slot_id_range_rejects_project_slot_count(audio_engine: AudioEngine) -> None:
    invalid_id = NUM_SAMPLES

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.loaded_sample_shape(invalid_id)

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.generate_stems_async(invalid_id, "source", "samples/stems/cache")

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.publish_prepared_stems(invalid_id, "source", "samples/stems/cache")

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.set_stem_mix_mode(invalid_id, "full_mix")

    with pytest.raises(ValueError, match=r"id out of range"):
        audio_engine.set_stem_enabled_mask(invalid_id, 1, "source")


def test_last_sample_slot_accepts_control_messages(audio_engine: AudioEngine) -> None:
    valid_last_id = NUM_SAMPLES - 1

    audio_engine.load_sample_async(valid_last_id, "file-does-not-exist.wav", run_analysis=True)

    deadline = time.monotonic() + 1.0
    saw_error = False
    while time.monotonic() < deadline:
        event = audio_engine.poll_loader_events()
        if event is None:
            time.sleep(0.01)
            continue

        if event.get("type") == "error" and event.get("id") == valid_last_id:
            saw_error = True
            break

    assert saw_error

    # Shouldn't crash
    audio_engine.set_pad_bpm(valid_last_id, 120.0)
    audio_engine.set_pad_timing_metadata(valid_last_id, 0.0)
    audio_engine.set_pad_loop_region(valid_last_id, 0.0, None)
    audio_engine.set_pad_gain(valid_last_id, 0.0)
    audio_engine.set_pad_eq(valid_last_id, 0.0, 0.0, 0.0)
    audio_engine.anchor_transport_phase_from_pad(valid_last_id)
    audio_engine.play_sample(valid_last_id, 1.0)
    audio_engine.play_sample_exclusive(valid_last_id, 1.0)
    audio_engine.pause_sample(valid_last_id)
    audio_engine.seek_sample(valid_last_id, 0.0)
    audio_engine.resume_sample(valid_last_id)
    audio_engine.stop_sample(valid_last_id)
    audio_engine.unload_sample(valid_last_id)


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


def test_seek_sample_requires_initialized_engine() -> None:
    engine = AudioEngine()

    with pytest.raises(RuntimeError, match=r"Audio engine not initialized"):
        engine.seek_sample(0, 1.0)


def test_seek_sample_rejects_invalid_positions(audio_engine: AudioEngine) -> None:
    with pytest.raises(ValueError, match=r"position_s out of range"):
        audio_engine.seek_sample(0, float("nan"))

    with pytest.raises(ValueError, match=r"position_s out of range"):
        audio_engine.seek_sample(0, -0.01)


def test_stop_all_is_safe_when_nothing_playing(audio_engine: AudioEngine) -> None:
    audio_engine.stop_all()


def test_fast_parameter_setters_accept_documented_ranges(audio_engine: AudioEngine) -> None:
    audio_engine.set_volume(VOLUME_MIN)
    audio_engine.set_volume(VOLUME_MAX)
    audio_engine.set_speed(SPEED_MIN)
    audio_engine.set_speed(SPEED_MAX)
    audio_engine.set_master_bpm(1.0)
    audio_engine.set_pad_bpm(0, None)
    audio_engine.set_pad_bpm(0, 120.0)
    audio_engine.set_pad_gain(0, PAD_GAIN_DB_MIN)
    audio_engine.set_pad_gain(0, PAD_GAIN_DB_MAX)
    audio_engine.set_pad_eq(0, PAD_EQ_DB_MIN, 0.0, PAD_EQ_DB_MAX)


def test_fast_parameter_setters_reject_invalid_values(audio_engine: AudioEngine) -> None:
    with pytest.raises(ValueError, match=r"volume out of range"):
        audio_engine.set_volume(float("nan"))

    with pytest.raises(ValueError, match=r"volume out of range"):
        audio_engine.set_volume(VOLUME_MAX + 0.01)

    with pytest.raises(ValueError, match=r"speed out of range"):
        audio_engine.set_speed(float("inf"))

    with pytest.raises(ValueError, match=r"speed out of range"):
        audio_engine.set_speed(SPEED_MIN - 0.01)

    with pytest.raises(ValueError, match=r"bpm out of range"):
        audio_engine.set_master_bpm(0.0)

    with pytest.raises(ValueError, match=r"bpm out of range"):
        audio_engine.set_pad_bpm(0, float("nan"))

    with pytest.raises(ValueError, match=r"gain out of range"):
        audio_engine.set_pad_gain(0, PAD_GAIN_DB_MAX + 0.01)

    with pytest.raises(ValueError, match=r"eq gain out of range"):
        audio_engine.set_pad_eq(0, PAD_EQ_DB_MIN - 0.01, 0.0, 0.0)

    with pytest.raises(ValueError, match=r"eq gain out of range"):
        audio_engine.set_pad_eq(0, 0.0, float("nan"), 0.0)


def test_fast_parameter_setters_require_initialized_engine() -> None:
    engine = AudioEngine()

    with pytest.raises(RuntimeError, match=r"Audio engine not initialized"):
        engine.set_volume(1.0)

    with pytest.raises(RuntimeError, match=r"Audio engine not initialized"):
        engine.set_speed(1.0)

    with pytest.raises(RuntimeError, match=r"Audio engine not initialized"):
        engine.set_master_bpm(120.0)

    with pytest.raises(RuntimeError, match=r"Audio engine not initialized"):
        engine.set_pad_bpm(0, 120.0)

    with pytest.raises(RuntimeError, match=r"Audio engine not initialized"):
        engine.set_pad_gain(0, 0.0)

    with pytest.raises(RuntimeError, match=r"Audio engine not initialized"):
        engine.set_pad_eq(0, 0.0, 0.0, 0.0)


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
        "1/64",
        "1_64",
        "grid_1_32",
        "1/16",
        "1_16",
    )

    for mode in modes:
        audio_engine.set_trigger_quantization(mode)


def test_set_trigger_quantization_rejects_invalid_mode(audio_engine: AudioEngine) -> None:
    with pytest.raises(ValueError, match=r"trigger quantization mode"):
        audio_engine.set_trigger_quantization("half_note")

    with pytest.raises(ValueError, match=r"trigger quantization mode"):
        audio_engine.set_trigger_quantization("1_bar")


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
    assert event["value"] == 100
    assert event["direct"] is False
    assert event["dispatched"] is True
    assert isinstance(event["received_at_ns"], int)

    assert audio_engine.inject_midi_input_for_test([0x90, 60, 0]) is False
    assert audio_engine.poll_input_events() is None


def test_injected_mapped_midi_is_capture_only_while_learn_active(
    audio_engine: AudioEngine,
) -> None:
    audio_engine.set_input_mapping_enabled(True)
    audio_engine.set_input_mapping_snapshot([("midi:note:1:60", "pad.trigger:0")])
    multi_loop = False
    audio_engine.set_input_runtime_state(
        multi_loop,
        [True] * NUM_SAMPLES,
        [0.0] * NUM_SAMPLES,
        [None] * NUM_SAMPLES,
    )
    learn_active = True
    audio_engine.set_input_learn_active(learn_active)

    assert audio_engine.inject_midi_input_for_test([0x90, 60, 100]) is True
    event = _wait_for_input_event(audio_engine)

    assert event["source"] == "midi"
    assert event["binding_key"] == "midi:note:1:60"
    assert "action_key" not in event
    assert event["direct"] is False
    assert event["dispatched"] is False


def test_injected_direct_midi_reports_failed_dispatch_for_fallback(
    audio_engine: AudioEngine,
) -> None:
    audio_engine.set_input_mapping_enabled(True)
    audio_engine.set_input_mapping_snapshot([("midi:note:1:60", "pad.trigger:0")])
    multi_loop = False
    audio_engine.set_input_runtime_state(
        multi_loop,
        [False] * NUM_SAMPLES,
        [0.0] * NUM_SAMPLES,
        [None] * NUM_SAMPLES,
    )

    assert audio_engine.inject_midi_input_for_test([0x90, 60, 100]) is True
    event = _wait_for_input_event(audio_engine)

    assert event["source"] == "midi"
    assert event["binding_key"] == "midi:note:1:60"
    assert event["action_key"] == "pad.trigger:0"
    assert event["direct"] is True
    assert event["dispatched"] is False


def test_injected_nrpn_increment_reports_stable_mapping_event(
    audio_engine: AudioEngine,
) -> None:
    audio_engine.set_input_mapping_enabled(True)
    audio_engine.set_input_mapping_snapshot([("midi:nrpn:1:0", "global.volume.delta")])

    assert audio_engine.inject_midi_input_for_test([0xB0, 99, 0]) is False
    assert audio_engine.inject_midi_input_for_test([0xB0, 98, 0]) is False
    assert audio_engine.inject_midi_input_for_test([0xB0, 96, 1]) is True
    event = _wait_for_input_event(audio_engine)

    assert event["source"] == "midi"
    assert event["binding_key"] == "midi:nrpn:1:0"
    assert event["action_key"] == "global.volume.delta"
    assert event["value"] == 65
    assert event["direct"] is False
    assert event["dispatched"] is True

    assert audio_engine.inject_midi_input_for_test([0xB0, 97, 1]) is True
    event = _wait_for_input_event(audio_engine)
    assert event["binding_key"] == "midi:nrpn:1:0"
    assert event["value"] == 63


def test_injected_standalone_inc_dec_reports_distinct_relative_cc_events(
    audio_engine: AudioEngine,
) -> None:
    audio_engine.set_input_mapping_enabled(True)
    audio_engine.set_input_mapping_snapshot([
        ("midi:cc:1:1", "pad.eq.selected.delta:low"),
        ("midi:cc:1:2", "pad.eq.selected.delta:mid"),
    ])

    assert audio_engine.inject_midi_input_for_test([0xB0, 96, 1]) is True
    event = _wait_for_input_event(audio_engine)

    assert event["source"] == "midi"
    assert event["binding_key"] == "midi:cc:1:1"
    assert event["action_key"] == "pad.eq.selected.delta:low"
    assert event["value"] == 65
    assert event["direct"] is False
    assert event["dispatched"] is True

    assert audio_engine.inject_midi_input_for_test([0xB0, 96, 2]) is True
    event = _wait_for_input_event(audio_engine)
    assert event["binding_key"] == "midi:cc:1:2"
    assert event["action_key"] == "pad.eq.selected.delta:mid"
    assert event["value"] == 65

    assert audio_engine.inject_midi_input_for_test([0xB0, 97, 2]) is True
    event = _wait_for_input_event(audio_engine)
    assert event["binding_key"] == "midi:cc:1:2"
    assert event["value"] == 63

    assert audio_engine.inject_midi_input_for_test([0xB0, 99, 0]) is False
    assert audio_engine.inject_midi_input_for_test([0xB0, 98, 0]) is False
    assert audio_engine.inject_midi_input_for_test([0xB0, 96, 2]) is True
    event = _wait_for_input_event(audio_engine)
    assert event["binding_key"] == "midi:cc:1:2"
    assert event["action_key"] == "pad.eq.selected.delta:mid"
    assert event["value"] == 65


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
    request_id = audio_engine.load_sample_async(0, "file-does-not-exist.wav", run_analysis=True)

    assert isinstance(request_id, int)
    assert request_id > 0

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
    assert seen["started"].get("request_id") == request_id

    assert "error" in seen
    assert seen["error"].get("id") == 0
    assert seen["error"].get("request_id") == request_id
    assert isinstance(seen["error"].get("msg"), str)


def test_poll_loader_events_returns_none_when_empty(audio_engine: AudioEngine) -> None:
    # Ensure the queue is drained.
    deadline = time.monotonic() + 0.2
    while time.monotonic() < deadline and audio_engine.poll_loader_events() is not None:
        pass

    assert audio_engine.poll_loader_events() is None
