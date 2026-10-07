"""Hardware-free WAV/capture uncertainty checks; none is device/listening acceptance."""

import hashlib
import json
import math
import struct
from copy import deepcopy
from datetime import UTC, datetime, timedelta
from typing import TYPE_CHECKING

import pytest
from pydantic import TypeAdapter, ValidationError

from flitzis_looper.analysis import loop_capture as cli
from flitzis_looper.analysis import loop_capture_compare as comparison
from flitzis_looper.analysis.loop_capture_compare import (
    RunEnvelope,
    compare_receipt,
    validate_productive_run,
)
from flitzis_looper.analysis.loop_capture_io import artifact
from flitzis_looper.analysis.loop_capture_models import (
    ComparisonInput,
    DetectorChannel,
    DetectorPolicy,
    ListeningInput,
)
from flitzis_looper.analysis.loop_capture_wav import measure_wave

if TYPE_CHECKING:
    from pathlib import Path

_FROZEN = datetime(2026, 10, 7, tzinfo=UTC)


def _json(path: Path, data: object) -> None:
    path.write_text(json.dumps(data, allow_nan=False), encoding="utf-8")


def _record(value: object) -> dict[str, object]:
    assert isinstance(value, dict)
    assert all(isinstance(key, str) for key in value)
    return value


def _items(value: object) -> list[object]:
    assert isinstance(value, list)
    return value


def _read_record(path: Path) -> dict[str, object]:
    value: object = json.loads(path.read_text(encoding="utf-8"))
    return _record(value)


def _wave(
    path: Path,
    events: tuple[tuple[int, ...], ...],
    *,
    width: int = 2,
    encoding: int = 1,
    frames: int = 11000,
    rate: int = 1000,
) -> None:
    channels = len(events)
    frame = [0.0] * channels * frames
    for channel, pulses in enumerate(events):
        for pulse in pulses:
            frame[pulse * channels + channel] = -0.5 if pulse % 2 else 0.5
    if encoding == 3:
        data = struct.pack(f"<{len(frame)}f", *frame)
    else:
        scale = 2 ** (width * 8 - 1)
        data = b"".join(
            int(value * scale).to_bytes(width, "little", signed=True) for value in frame
        )
    fmt = struct.pack(
        "<HHIIHH", encoding, channels, rate, rate * channels * width, channels * width, width * 8
    )
    body = (
        b"WAVEfmt "
        + struct.pack("<I", len(fmt))
        + fmt
        + b"data"
        + struct.pack("<I", len(data))
        + data
    )
    if len(data) % 2:
        body += b"\0"
    path.write_bytes(b"RIFF" + struct.pack("<I", len(body)) + body)


def _policy(channels: int = 1, *, release: int = 2) -> DetectorPolicy:
    return DetectorPolicy(
        schema_version=1,
        method="absolute_threshold_hysteresis_v1",
        frozen_at_utc=_FROZEN,
        provenance="synthetic predeclared detector policy, no candidate period windows",
        channels=tuple(
            DetectorChannel(
                channel=channel,
                high=0.1,
                low=0.01,
                minimum_gap_capture_frames=1,
                rearm_low_capture_frames=release,
            )
            for channel in range(channels)
        ),
    )


@pytest.mark.parametrize(
    ("width", "encoding", "name"),
    [(2, 1, "pcm16"), (3, 1, "pcm24"), (4, 1, "pcm32"), (4, 3, "float32")],
)
def test_complete_multichannel_scan_measures_signed_edges_across_block_boundary(
    tmp_path: Path, width: int, encoding: int, name: str
) -> None:
    events = ((1, 8191, 8200, 10999), (5, 8192, 10000))
    path = tmp_path / "synthetic.wav"
    _wave(path, events, width=width, encoding=encoding)
    info, measured = measure_wave(path, _policy(2))
    assert info.format == name
    assert info.frames == 11000
    for channel, expected in zip(measured, events, strict=True):
        assert tuple(item.capture_frame for item in channel.edges) == expected
        assert channel.rejected_by_minimum_gap == 0
        assert channel.peak_absolute == 0.5


def test_sustained_low_rearm_does_not_invent_oscillatory_pulse_edges(tmp_path: Path) -> None:
    path = tmp_path / "synthetic-oscillatory.wav"
    _wave(path, ((1, 3, 5, 7, 11),), frames=16)
    _, channels = measure_wave(path, _policy(release=3))
    assert tuple(item.capture_frame for item in channels[0].edges) == (1, 11)
    assert channels[0].rejected_by_minimum_gap == 0


@pytest.mark.parametrize("width", [2, 3, 4])
def test_integer_positive_and_negative_full_scale_are_both_reported(
    tmp_path: Path, width: int
) -> None:
    path = tmp_path / "synthetic-full-scale.wav"
    _wave(path, ((),), frames=4, width=width)
    raw = path.read_bytes()
    scale = 2 ** (width * 8 - 1)
    peaks = (scale - 1).to_bytes(width, "little", signed=True) + (-scale).to_bytes(
        width, "little", signed=True
    )
    path.write_bytes(raw[:44] + peaks + raw[44 + 2 * width :])
    _, measured = measure_wave(path, _policy())
    assert measured[0].samples_at_or_above_full_scale == 2


@pytest.mark.parametrize("corruption", ["extent", "partial", "nan_unselected"])
def test_partial_or_nonfinite_capture_is_rejected_without_measurement(
    tmp_path: Path, corruption: str
) -> None:
    path = tmp_path / "synthetic-corrupt.wav"
    _wave(path, ((1,), (2,)), encoding=3, width=4, frames=16)
    raw = path.read_bytes()
    if corruption == "extent":
        raw += b"unexpected tail"
    elif corruption == "partial":
        raw = raw[:-1]
    else:
        raw = raw[:48] + struct.pack("<f", math.nan) + raw[52:]
    path.write_bytes(raw)
    with pytest.raises(ValueError, match="wav_"):
        measure_wave(path, _policy())


def _prepared(
    tmp_path: Path,
    *,
    period: float = 10.0,
    rate: float = 1.0,
    delta: float = 0.0,
    channels: int = 1,
    duration: int = 12,
) -> dict[str, object]:
    events = tuple(
        tuple(round(index * period / rate + delta * index + channel * 3) for index in range(1001))
        for channel in range(channels)
    )
    _wave(tmp_path / "synthetic.wav", events, frames=duration * 1000)
    (tmp_path / "policy.json").write_text(_policy(channels).model_dump_json())
    cli.collect_features(
        tmp_path, "synthetic.wav", "policy.json", "features.json", "synthetic_fixture"
    )
    run = {
        "schema_version": 1,
        "evidence_kind": "synthetic_fixture",
        "capture_comparison": {
            "output": {"sample_rate_hz": 1000, "clock_identity": "synthetic-output-clock"},
            "pads": [
                {
                    "pad_id": channel,
                    "loaded_sample_rate_hz": 1000,
                    "musical_period_loaded_frames": period,
                    "physical_start_loaded_frame": 0,
                    "physical_end_loaded_frame": round(period),
                    "applied_source_rate": rate,
                    "current_accepted_revision": "accepted-constant-timing-v1:" + "a" * 64,
                    "current_acknowledged": True,
                }
                for channel in range(channels)
            ],
        },
        "fixture_notice": "Synthetic metadata; no native acceptance or device run",
    }
    _json(tmp_path / "run.json", run)
    _json(tmp_path / "clock.json", {"fixture": "synthetic shared clock assertion"})
    cli.draft_comparison(tmp_path, "run.json", "features.json", "input.json")
    packet = _read_record(tmp_path / "input.json")
    packet.update(
        capture_started_at_utc=(_FROZEN + timedelta(seconds=1)).isoformat(),
        recorder_identity="synthetic fixture generator",
        capture_route="synthetic file, no device",
    )
    _record(packet["clock"]).update(
        mode="shared_digital_clock",
        output_clock_identity="synthetic-output-clock",
        recording_clock_identity="synthetic-recording-clock",
        output_frames_per_capture_frame=1.0,
        ratio_halfwidth=0.0,
        valid_duration_seconds=float(duration),
        independent_of_candidate_period=True,
        provenance="synthetic fixture clock",
        evidence=artifact(tmp_path, "clock.json").model_dump(),
    )
    for item in _items(packet["channels"]):
        channel = _record(item)
        channel_id = channel["channel"]
        assert isinstance(channel_id, int)
        channel.update(
            pad_id=channel_id,
            cycle_count_provenance=(
                "Independent synthetic generator cycle counts; never observed edge indices"
            ),
            independent_cycle_counts=True,
            stationary_configuration_asserted=True,
            seam_feature_offset_output_frames=float(channel_id * 3),
            seam_feature_variation_halfwidth_output_frames=0.0,
            seam_feature_provenance="synthetic fixed source seam",
            audible_dsp_alignment_output_frames=0.0,
            audible_dsp_variation_halfwidth_output_frames=0.0,
            audible_dsp_provenance="synthetic no DSP fixture",
            observations=[{"event_id": index, "cycle": index} for index in (0, 75, 1000)],
        )
    return packet


def _compare(tmp_path: Path, packet: dict[str, object]) -> dict[str, object]:
    _json(tmp_path / "input-complete.json", packet)
    compare_receipt(tmp_path, "input-complete.json", "comparison.json")
    return _read_record(tmp_path / "comparison.json")


def test_known_clock_separates_multichannel_fixed_offset_from_continuous_drift(
    tmp_path: Path,
) -> None:
    report = _compare(tmp_path, _prepared(tmp_path, channels=2))
    assert report["numerical_limit_loaded_frames"] == 1.0
    assert str(report["g3_device_gate"]).startswith("open_")
    channels = [_record(item) for item in _items(report["channels"])]
    assert all(not item["blocked_reasons"] for item in channels)
    assert all(
        _record(_items(item["observations"])[-1])["status"] == "consistent_with_one_loaded_frame"
        for item in channels
    )
    alignment = _record(_items(report["interpad_anchor_alignment"])[0])
    assert alignment["observed_anchor_feature_delta_output_frames"] == 3.0
    assert alignment["corrected_anchor_delta_output_frames"] == 0.0


def test_endpoint_rounding_drift_fails_unwrapped_musical_limit_not_modulo(tmp_path: Path) -> None:
    report = _compare(tmp_path, _prepared(tmp_path, period=10.25, delta=-0.25))
    channel = _record(_items(report["channels"])[0])
    last = _record(_items(channel["observations"])[-1])
    assert last["continuous_period_error_loaded_frames_estimate"] == -250.0
    assert last["status"] == "fails_one_loaded_frame"


def test_fast_rate_integer_output_feature_uncertainty_is_inconclusive(tmp_path: Path) -> None:
    report = _compare(tmp_path, _prepared(tmp_path, rate=1.25))
    channel = _record(_items(report["channels"])[0])
    last = _record(_items(channel["observations"])[-1])
    assert last["continuous_period_error_loaded_frames_estimate"] == 0.0
    assert last["uncertainty_loaded_frames"] == 1.25
    assert last["status"] == "inconclusive_due_to_uncertainty"


def test_double_recording_rate_keeps_clock_ratio_and_output_sampling_separate(
    tmp_path: Path,
) -> None:
    packet = _prepared(tmp_path, rate=1.25)
    _wave(
        tmp_path / "synthetic-double.wav",
        (tuple(16 * index for index in range(1001)),),
        frames=24000,
        rate=2000,
    )
    cli.collect_features(
        tmp_path, "synthetic-double.wav", "policy.json", "double-features.json", "synthetic_fixture"
    )
    packet["features"] = artifact(tmp_path, "double-features.json").model_dump()
    _record(packet["clock"])["output_frames_per_capture_frame"] = 0.5
    report = _compare(tmp_path, packet)
    assert report["capture_sample_rate_hz"] == 2000
    assert report["output_sample_rate_hz"] == 1000
    channel = _record(_items(report["channels"])[0])
    last = _record(_items(channel["observations"])[-1])
    assert last["continuous_period_error_loaded_frames_estimate"] == 0.0
    assert last["uncertainty_loaded_frames"] == 0.625
    assert last["status"] == "consistent_with_one_loaded_frame"


def test_unknown_recording_clock_does_not_assume_equal_nominal_wav_rate(tmp_path: Path) -> None:
    packet = _prepared(tmp_path)
    _record(packet["clock"])["mode"] = "unknown"
    report = _compare(tmp_path, packet)
    channel = _record(_items(report["channels"])[0])
    assert channel["blocked_reasons"] == ["recording_to_output_clock_relation_unknown"]
    assert channel["observations"] == []


def test_clock_frequency_uncertainty_accumulates_separately_from_feature_sampling(
    tmp_path: Path,
) -> None:
    packet = _prepared(tmp_path)
    clock = _record(packet["clock"])
    clock["mode"] = "calibrated"
    clock["ratio_halfwidth"] = 0.0001
    report = _compare(tmp_path, packet)
    channel = _record(_items(report["channels"])[0])
    last = _record(_items(channel["observations"])[-1])
    assert last["clock_ratio_uncertainty_output_frames"] == 1.0
    assert last["integer_feature_localization_uncertainty_output_frames"] == 1.0
    assert last["uncertainty_loaded_frames"] == 2.0
    assert last["status"] == "inconclusive_due_to_uncertainty"


@pytest.mark.parametrize(
    "mutation",
    [
        "digest",
        "missing_cycle",
        "missing_event",
        "unordered",
        "late_policy",
        "evidence_kind",
        "clock_identity",
    ],
)
def test_identity_labels_and_clock_failures_are_preserved_or_blocked(
    tmp_path: Path, mutation: str
) -> None:
    packet = _prepared(tmp_path)
    channel = _record(_items(packet["channels"])[0])
    if mutation == "digest":
        (tmp_path / "synthetic.wav").write_bytes(b"changed")
    elif mutation == "missing_cycle":
        _items(channel["observations"]).pop()
        report = _compare(tmp_path, packet)
        reported_channel = _record(_items(report["channels"])[0])
        assert "required_75_and_1000_cycle_observations_missing" in _items(
            reported_channel["blocked_reasons"]
        )
        return
    elif mutation == "missing_event":
        _record(_items(channel["observations"])[-1])["event_id"] = 1001
    elif mutation == "unordered":
        _items(channel["observations"]).reverse()
    elif mutation == "late_policy":
        packet["capture_started_at_utc"] = (_FROZEN - timedelta(seconds=1)).isoformat()
    elif mutation == "evidence_kind":
        packet["evidence_kind"] = "real_device_loopback"
    else:
        _record(packet["clock"])["output_clock_identity"] = "different output clock"
    _json(tmp_path / "invalid.json", packet)
    with pytest.raises(ValueError, match=r"artifact_|cycle_|detector_|capture_"):
        compare_receipt(tmp_path, "invalid.json", "not-created.json")
    assert not (tmp_path / "not-created.json").exists()


def test_missing_native_acknowledgement_cannot_be_supplied_by_intent(tmp_path: Path) -> None:
    packet = _prepared(tmp_path)
    run = _read_record(tmp_path / "run.json")
    summary = _record(run["capture_comparison"])
    _record(_items(summary["pads"])[0])["current_acknowledged"] = False
    _json(tmp_path / "run.json", run)
    packet["productive_run"] = artifact(tmp_path, "run.json").model_dump()
    with pytest.raises(ValidationError):
        _compare(tmp_path, packet)


def test_real_productive_reference_requires_raw_native_records(tmp_path: Path) -> None:
    _prepared(tmp_path)
    data = _read_record(tmp_path / "run.json")
    data["evidence_kind"] = "real_productive_app_observation"
    run = TypeAdapter(RunEnvelope).validate_json(json.dumps(data), strict=True)
    with pytest.raises(ValueError, match="complete_native_snapshot"):
        validate_productive_run(tmp_path, run)


def test_real_summary_is_recomputed_by_shared_guard_and_source_bytes_reverified(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _prepared(tmp_path)
    data = _read_record(tmp_path / "run.json")
    summary = _record(_items(_record(data["capture_comparison"])["pads"])[0])
    binding = {"source_sha256": artifact(tmp_path, "synthetic.wav").sha256}
    # Only test guard composition here. Full native/current/export semantics have
    # their own hardware-free productive snapshot tests, not fake live evidence.
    monkeypatch.setattr(comparison, "comparison_pad", lambda *_args: summary)
    data.update(
        evidence_kind="real_productive_app_observation",
        pads=[
            {
                "native_callback_snapshot": {"current_binding": binding},
                "current_source_binding": binding,
                "current_constant_timing_before_export": {},
                "current_constant_timing_after_export": {},
                "verified_current_timing_export": {"fixture": "guard-composition-only"},
                "actual_source_path": "synthetic.wav",
            }
        ],
    )
    run = TypeAdapter(RunEnvelope).validate_json(json.dumps(data), strict=True)
    validate_productive_run(tmp_path, run)
    summary["musical_period_loaded_frames"] = 11.0
    with pytest.raises(ValueError, match="summary_differs"):
        validate_productive_run(tmp_path, run)
    summary["musical_period_loaded_frames"] = 10.0
    (tmp_path / "synthetic.wav").write_bytes(b"changed after snapshot")
    with pytest.raises(ValueError, match="sha256_mismatch"):
        validate_productive_run(tmp_path, run)


def test_exclusive_private_writes_reject_repo_and_external_paths(tmp_path: Path) -> None:
    cli.draft_detector(tmp_path, "detector.json")
    with pytest.raises(FileExistsError):
        cli.draft_detector(tmp_path, "detector.json")
    for value in ("repo/detector.json", "../outside.json"):
        with pytest.raises(ValueError, match="private_path"):
            cli.draft_detector(tmp_path, value)


def test_precapture_templates_remain_invalid_until_actual_human_evidence(tmp_path: Path) -> None:
    cli.draft_comparison(tmp_path, None, None, "comparison-template.json")
    cli.draft_listening(tmp_path, None, None, "listening-template.json")
    for name, model in (
        ("comparison-template.json", ComparisonInput),
        ("listening-template.json", ListeningInput),
    ):
        with pytest.raises(ValidationError):
            TypeAdapter(model).validate_json((tmp_path / name).read_bytes(), strict=True)
    form = _read_record(tmp_path / "listening-template.json")
    assert [_record(item)["elapsed_seconds"] for item in _items(form["observations"])] == list(
        range(0, 1801, 300)
    )
    assert form["observer"] is None


def _listening(tmp_path: Path) -> dict[str, object]:
    _prepared(tmp_path, duration=1801)
    cli.draft_listening(tmp_path, "run.json", "features.json", "listening.json")
    data = _read_record(tmp_path / "listening.json")
    data.update(
        productive_run_end=artifact(tmp_path, "run.json").model_dump(),
        capture_start_frame=0,
        capture_end_frame=1801000,
        capture_interval_provenance="synthetic complete selected interval",
        observer="synthetic fixture, not a human listener",
        listening_provenance="synthetic validator fixture only",
        started_at_utc=_FROZEN.isoformat(),
        ended_at_utc=(_FROZEN + timedelta(seconds=1800)).isoformat(),
        continuous_uninterrupted=True,
        actual_productive_app_listened_to=True,
        outcome="pass",
    )
    for item in _items(data["observations"]):
        observation = _record(item)
        observation.update(
            audible_drift="clear",
            seam_clicks="clear",
            missed_or_doubled_beats="clear",
            interpad_alignment="clear",
            keylock_stem_dsp="clear",
            note="Synthetic assertion, no human listening claimed",
        )
    return data


def test_synthetic_30minute_declaration_receipt_never_certifies_human_truth(tmp_path: Path) -> None:
    _json(tmp_path / "listening-complete.json", _listening(tmp_path))
    receipt = _read_record(
        cli.listening_receipt(tmp_path, "listening-complete.json", "receipt.json")
    )
    assert receipt["declared_listening_duration_seconds"] == 1800.0
    assert receipt["status"] == "human_declaration_consistent_not_certified"
    assert receipt["evidence_kind"] == "synthetic_fixture"
    gate = receipt["g3_listening_gate"]
    assert isinstance(gate, str)
    assert gate.startswith("open_")


def _paired_processing_runs(tmp_path: Path, data: dict[str, object]) -> dict[str, object]:
    run = _read_record(tmp_path / "run.json")
    # Explicit synthetic endpoint consistency fixture, not a native/device run.
    run["pads"] = [
        {
            "pad_id": 0,
            "current_source_binding": {"source_generation": 1},
            "current_constant_timing_before_export": {"revision": "synthetic"},
            "native_callback_snapshot": {
                "key_lock_requested": False,
                "key_lock_native_active": False,
                "native_pitch_scale": 1.0,
                "stem_all_requested": False,
                "stem_all_applied": False,
                "stem_source_version_hash": 0,
                "stem_enabled_mask": 0,
                "prepared_stems_current": False,
                "eq_applied_normalized": [0.5, 0.5, 0.5],
                "eq_target_normalized": [0.5, 0.5, 0.5],
                "applied_pad_gain_linear": 1.0,
                "target_pad_gain_linear": 1.0,
                "master_volume": 0.1,
                "voice_volume": 1.0,
                "bpm_lock": True,
                "master_period_seconds": 0.5,
                "source_frame": 0,
                "source_fraction": 0.0,
                "output_frame": 512,
                "native_input_fifo_frames": 0,
                "native_output_fifo_frames": 0,
            },
        }
    ]
    _json(tmp_path / "run-processing-start.json", run)
    data["productive_run"] = artifact(tmp_path, "run-processing-start.json").model_dump()
    return deepcopy(run)


@pytest.mark.parametrize(
    ("field", "changed"),
    [
        ("key_lock_requested", True),
        ("key_lock_native_active", True),
        ("native_pitch_scale", 1.25),
        ("stem_enabled_mask", 3),
        ("stem_source_version_hash", 1),
        ("prepared_stems_current", True),
        ("eq_applied_normalized", [0.2, 0.5, 0.5]),
        ("applied_pad_gain_linear", 0.5),
        ("master_volume", 0.2),
        ("master_period_seconds", 0.4999),
    ],
)
def test_listening_reference_processing_changes_are_not_hidden_by_matching_period(
    tmp_path: Path, field: str, changed: object
) -> None:
    data = _listening(tmp_path)
    end_run = _paired_processing_runs(tmp_path, data)
    end_pad = _record(_items(end_run["pads"])[0])
    _record(end_pad["native_callback_snapshot"])[field] = changed
    _json(tmp_path / "run-processing-end.json", end_run)
    data["productive_run_end"] = artifact(tmp_path, "run-processing-end.json").model_dump()
    _json(tmp_path / "listening-processing-input.json", data)
    with pytest.raises(ValueError, match="processing_configuration_mismatch"):
        cli.listening_receipt(tmp_path, "listening-processing-input.json", "not-created.json")
    assert not (tmp_path / "not-created.json").exists()


def test_listening_reference_phase_and_fifo_progress_do_not_change_processing_identity(
    tmp_path: Path,
) -> None:
    data = _listening(tmp_path)
    end_run = _paired_processing_runs(tmp_path, data)
    end_pad = _record(_items(end_run["pads"])[0])
    _record(end_pad["native_callback_snapshot"]).update(
        source_frame=5,
        source_fraction=0.25,
        output_frame=1800000,
        native_input_fifo_frames=27,
        native_output_fifo_frames=41,
    )
    _json(tmp_path / "run-processing-end.json", end_run)
    data["productive_run_end"] = artifact(tmp_path, "run-processing-end.json").model_dump()
    _json(tmp_path / "listening-processing-input.json", data)
    receipt = _read_record(
        cli.listening_receipt(tmp_path, "listening-processing-input.json", "receipt.json")
    )
    assert receipt["status"] == "human_declaration_consistent_not_certified"
    gate = receipt["g3_listening_gate"]
    assert isinstance(gate, str)
    assert gate.startswith("open_")


@pytest.mark.parametrize(
    "mutation", ["short", "break", "unassessed", "no_failure", "extent", "gap", "changed_run"]
)
def test_incomplete_or_inconsistent_listening_declarations_fail(
    tmp_path: Path, mutation: str
) -> None:
    data = _listening(tmp_path)
    if mutation == "short":
        data["ended_at_utc"] = (_FROZEN + timedelta(seconds=1799)).isoformat()
    elif mutation == "break":
        data["continuous_uninterrupted"] = False
    elif mutation == "unassessed":
        _record(_items(data["observations"])[0])["seam_clicks"] = "unassessed"
    elif mutation == "no_failure":
        data["outcome"] = "fail"
    elif mutation == "extent":
        data["capture_end_frame"] = 1801001
    elif mutation == "gap":
        _items(data["observations"]).pop(1)
    else:
        run = _read_record(tmp_path / "run.json")
        summary = _record(run["capture_comparison"])
        _record(_items(summary["pads"])[0])["musical_period_loaded_frames"] = 10.01
        _json(tmp_path / "run-end.json", run)
        data["productive_run_end"] = artifact(tmp_path, "run-end.json").model_dump()
    _json(tmp_path / "listening-invalid.json", data)
    with pytest.raises(ValueError, match=r"listening_|declared_"):
        cli.listening_receipt(tmp_path, "listening-invalid.json", "not-created.json")
    assert not (tmp_path / "not-created.json").exists()


def test_cli_failure_does_not_overwrite_input_or_create_success_receipt(tmp_path: Path) -> None:
    cli.draft_detector(tmp_path, "detector.json")
    original = hashlib.sha256((tmp_path / "detector.json").read_bytes()).hexdigest()
    assert (
        cli.main([
            "--workspace",
            str(tmp_path),
            "features",
            "--capture",
            "missing.wav",
            "--policy",
            "detector.json",
            "--evidence-kind",
            "real_device_loopback",
            "--output",
            "not-created.json",
        ])
        == 2
    )
    assert hashlib.sha256((tmp_path / "detector.json").read_bytes()).hexdigest() == original
    assert not (tmp_path / "not-created.json").exists()
