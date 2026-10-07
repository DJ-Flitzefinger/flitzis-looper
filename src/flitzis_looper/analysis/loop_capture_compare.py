"""Independent unwrapped capture recurrence comparison with explicit uncertainty."""

import json
import math
from typing import TYPE_CHECKING, Annotated, Literal

from pydantic import BaseModel, ConfigDict, Field, TypeAdapter

from flitzis_looper.analysis.loop_capture_io import (
    _json,
    artifact,
    verified_features,
    verify_artifact,
)
from flitzis_looper.analysis.loop_capture_models import (
    Artifact,
    ChannelComparison,
    ComparisonInput,
    DetectorPolicy,
    FeatureEvidence,
    Frame,
    Positive,
    RecordingClock,
)
from flitzis_looper.analysis.productive_loop_snapshot import comparison_pad
from flitzis_looper.analysis.reference_inputs_models import StrictInput, Text
from flitzis_looper.analysis.reference_inputs_validation import (
    fail,
    read_input,
    read_json_bytes,
    write_output,
)

if TYPE_CHECKING:
    from pathlib import Path


class OutputReference(StrictInput):
    """Actual productive output timebase, distinct from WAV recording rate."""

    sample_rate_hz: Annotated[int, Field(strict=True, ge=1, le=768000)]
    clock_identity: Text


class PadReference(StrictInput):
    """Actual acknowledged effective voice values; intent cannot supply them."""

    pad_id: Annotated[int, Field(strict=True, ge=0, le=215)]
    loaded_sample_rate_hz: Annotated[int, Field(strict=True, ge=1, le=768000)]
    musical_period_loaded_frames: Positive
    physical_start_loaded_frame: Frame
    physical_end_loaded_frame: Frame
    applied_source_rate: Positive
    current_accepted_revision: Annotated[
        str, Field(strict=True, pattern=r"^accepted-constant-timing-v1:[0-9a-f]{64}$")
    ]
    current_acknowledged: Literal[True]


class ProductiveReference(StrictInput):
    output: OutputReference
    pads: Annotated[tuple[PadReference, ...], Field(min_length=1, max_length=216)]


class RunEnvelope(BaseModel):
    """Read only authority fields while the artifact binds the complete native packet."""

    model_config = ConfigDict(extra="ignore", strict=True)
    schema_version: Literal[1]
    evidence_kind: Literal["real_productive_app_observation", "synthetic_fixture"]
    capture_comparison: ProductiveReference
    pads: tuple[dict[str, object], ...] | None = None
    comparison_blockers: tuple[str, ...] = ()


def _object(value: object) -> dict[str, object] | None:
    if not isinstance(value, dict):
        return None
    return {str(key): item for key, item in value.items()}


def validate_productive_run(workspace: Path, run: RunEnvelope) -> None:
    """Recompute real summaries through the runner's shared native/current guard."""
    if run.evidence_kind == "synthetic_fixture":
        return
    if run.pads is None or run.comparison_blockers:
        fail("real_productive_run_requires_complete_native_snapshot_records")
    assert run.pads is not None
    projected = []
    for row in run.pads:
        exported = _object(row.get("verified_current_timing_export"))
        native = _object(row.get("native_callback_snapshot"))
        before = _object(row.get("current_constant_timing_before_export"))
        after = _object(row.get("current_constant_timing_after_export"))
        current_source = _object(row.get("current_source_binding"))
        summary = comparison_pad(native, before, after, json.dumps(exported) if exported else None)
        if summary is None or current_source is None:
            fail("real_productive_snapshot_current_source_or_voice_unavailable")
        assert current_source is not None
        assert native is not None
        if current_source != _object(native.get("current_binding")):
            fail("real_productive_source_binding_differs_from_callback_current_binding")
        source_path, digest = row.get("actual_source_path"), current_source.get("source_sha256")
        if not isinstance(source_path, str) or not isinstance(digest, str):
            fail("real_productive_run_original_source_identity_missing")
        assert isinstance(source_path, str)
        assert isinstance(digest, str)
        verify_artifact(workspace, Artifact(path=source_path, sha256=digest))
        projected.append(summary)
    if projected != [pad.model_dump() for pad in run.capture_comparison.pads]:
        fail("real_productive_compact_summary_differs_from_native_current_snapshot")


def _clock_reasons(workspace: Path, clock: RecordingClock, duration: float) -> list[str]:
    if clock.mode == "unknown":
        return ["recording_to_output_clock_relation_unknown"]
    if clock.independent_of_candidate_period is not True:
        return ["clock_relation_not_independently_declared"]
    if (
        clock.output_frames_per_capture_frame is None
        or clock.ratio_halfwidth is None
        or clock.valid_duration_seconds is None
        or clock.provenance is None
        or clock.evidence is None
    ):
        return ["clock_calibration_or_shared_digital_clock_evidence_incomplete"]
    verify_artifact(workspace, clock.evidence)
    if clock.output_frames_per_capture_frame <= clock.ratio_halfwidth:
        fail("clock_ratio_uncertainty_includes_nonpositive_frequency")
    if duration > clock.valid_duration_seconds:
        return ["capture_exceeds_declared_clock_calibration_valid_duration"]
    return []


def _mapped_frames(channel: ChannelComparison, features: FeatureEvidence) -> dict[int, int]:
    measured = [item for item in features.measured_channels if item.channel == channel.channel]
    if len(measured) != 1:
        fail("comparison_channel_missing_or_ambiguous")
    edges = {item.event_id: item.capture_frame for item in measured[0].edges}
    cycles = [item.cycle for item in channel.observations]
    event_ids = [item.event_id for item in channel.observations]
    if cycles != sorted(set(cycles)) or event_ids != sorted(set(event_ids)) or cycles[0] != 0:
        fail("cycle_observations_require_ordered_unique_cycles_events_and_cycle_zero_anchor")
    if any(event_id not in edges for event_id in event_ids):
        fail("cycle_observation_references_unmeasured_feature")
    return {item.cycle: edges[item.event_id] for item in channel.observations}


def _channel_reasons(channel: ChannelComparison, features: FeatureEvidence) -> list[str]:
    reasons = []
    if not channel.independent_cycle_counts:
        reasons.append("cycle_counts_not_independently_declared")
    if not channel.stationary_configuration_asserted:
        reasons.append("configuration_not_declared_stationary_use_separate_segments")
    measured = next(item for item in features.measured_channels if item.channel == channel.channel)
    if measured.rejected_by_minimum_gap:
        reasons.append("detector_rejected_edges_need_independent_cycle_adjudication")
    if measured.samples_at_or_above_full_scale:
        reasons.append("capture_contains_full_scale_samples_review_signal_integrity")
    if not {75, 1000}.issubset(item.cycle for item in channel.observations):
        reasons.append("required_75_and_1000_cycle_observations_missing")
    return reasons


def _point(
    cycle: int,
    delta_capture: int,
    reference: PadReference,
    channel: ChannelComparison,
    clock: RecordingClock,
) -> dict[str, object]:
    assert clock.output_frames_per_capture_frame is not None
    assert clock.ratio_halfwidth is not None
    ratio = clock.output_frames_per_capture_frame
    rate = reference.applied_source_rate
    expected_output = cycle * reference.musical_period_loaded_frames / rate
    measured_output = delta_capture * ratio
    error = rate * (measured_output - expected_output)
    clock_uncertainty = abs(delta_capture) * clock.ratio_halfwidth
    sampling = (
        0.0 if cycle == 0 else 2 * channel.feature_localization_halfwidth_capture_frames * ratio
    )
    variation = (
        0.0
        if cycle == 0
        else 2
        * (
            channel.seam_feature_variation_halfwidth_output_frames
            + channel.audible_dsp_variation_halfwidth_output_frames
        )
    )
    uncertainty = rate * (clock_uncertainty + sampling + variation)
    lower, upper = max(0.0, abs(error) - uncertainty), abs(error) + uncertainty
    if not all(
        math.isfinite(value) for value in (expected_output, error, uncertainty, lower, upper)
    ):
        fail("capture_comparison_numeric_overflow")
    status = (
        "consistent_with_one_loaded_frame"
        if upper <= 1.0
        else ("fails_one_loaded_frame" if lower > 1.0 else "inconclusive_due_to_uncertainty")
    )
    return {
        "cycle": cycle,
        "capture_displacement_frames": delta_capture,
        "measured_output_displacement_frames": measured_output,
        "musical_expected_output_displacement_frames": expected_output,
        "physical_expected_output_displacement_frames": (
            cycle
            * (reference.physical_end_loaded_frame - reference.physical_start_loaded_frame)
            / rate
        ),
        "continuous_period_error_loaded_frames_estimate": error,
        "uncertainty_loaded_frames": uncertainty,
        "absolute_error_interval_loaded_frames": [lower, upper],
        "clock_ratio_uncertainty_output_frames": clock_uncertainty,
        "integer_feature_localization_uncertainty_output_frames": sampling,
        "seam_and_dsp_feature_variation_uncertainty_output_frames": variation,
        "status": status,
    }


def _alignment(
    channels: tuple[ChannelComparison, ...],
    mapped: dict[int, dict[int, int]],
    clock: RecordingClock,
) -> list[dict[str, object]]:
    assert clock.output_frames_per_capture_frame is not None
    anchor = channels[0]
    results = []
    for channel in channels[1:]:
        raw = (
            mapped[channel.channel][0] - mapped[anchor.channel][0]
        ) * clock.output_frames_per_capture_frame
        seam = channel.seam_feature_offset_output_frames - anchor.seam_feature_offset_output_frames
        dsp = (
            channel.audible_dsp_alignment_output_frames - anchor.audible_dsp_alignment_output_frames
        )
        results.append({
            "reference_channel": anchor.channel,
            "channel": channel.channel,
            "observed_anchor_feature_delta_output_frames": raw,
            "declared_seam_feature_offset_delta_output_frames": seam,
            "declared_audible_dsp_alignment_delta_output_frames": dsp,
            "corrected_anchor_delta_output_frames": raw - seam - dsp,
            "status": "alignment_observation_only_no_b5_or_audible_sync_acceptance",
        })
    return results


def _validate_reference(
    workspace: Path, packet: ComparisonInput, features: FeatureEvidence
) -> tuple[ProductiveReference, list[str]]:
    run_path = verify_artifact(workspace, packet.productive_run)
    run = TypeAdapter(RunEnvelope).validate_json(read_json_bytes(run_path), strict=True)
    validate_productive_run(workspace, run)
    reference = run.capture_comparison
    expected_kind = (
        "synthetic_fixture"
        if packet.evidence_kind == "synthetic_fixture"
        else "real_productive_app_observation"
    )
    if features.evidence_kind != packet.evidence_kind or run.evidence_kind != expected_kind:
        fail("capture_and_productive_reference_evidence_kind_mismatch")
    if packet.capture_started_at_utc.utcoffset() is None:
        fail("capture_timestamp_requires_timezone")
    policy = read_input(workspace, features.policy.path, TypeAdapter(DetectorPolicy))
    if policy.frozen_at_utc > packet.capture_started_at_utc:
        fail("detector_policy_was_not_declared_frozen_before_capture")
    if packet.clock.output_clock_identity != reference.output.clock_identity:
        fail("capture_clock_does_not_bind_productive_output_clock")
    channel_ids = [item.channel for item in packet.channels]
    pad_ids = [item.pad_id for item in reference.pads]
    if len(set(channel_ids)) != len(channel_ids) or len(set(pad_ids)) != len(pad_ids):
        fail("duplicate_comparison_channel_or_productive_pad")
    duration = features.capture_frame_count / features.capture_sample_rate_hz
    clock_reasons = _clock_reasons(workspace, packet.clock, duration)
    return reference, clock_reasons


def _channel_result(
    channel: ChannelComparison,
    reference: ProductiveReference,
    features: FeatureEvidence,
    mapped: dict[int, dict[int, int]],
    clock: RecordingClock,
    clock_reasons: list[str],
) -> dict[str, object]:
    pad = next((item for item in reference.pads if item.pad_id == channel.pad_id), None)
    if pad is None or pad.physical_end_loaded_frame <= pad.physical_start_loaded_frame:
        fail("comparison_requires_productive_pad_and_nonempty_physical_loop")
    reasons = [*clock_reasons, *_channel_reasons(channel, features)]
    assert pad is not None
    anchor = mapped[channel.channel][0]
    points = (
        []
        if clock_reasons
        else [
            _point(cycle, frame - anchor, pad, channel, clock)
            for cycle, frame in mapped[channel.channel].items()
        ]
    )
    return {
        "channel": channel.channel,
        "pad_id": channel.pad_id,
        "status": _comparison_status(reasons, points),
        "productive_reference": pad.model_dump(),
        "blocked_reasons": reasons,
        "fixed_offsets": {
            "seam_feature_offset_output_frames": channel.seam_feature_offset_output_frames,
            "audible_dsp_alignment_output_frames": channel.audible_dsp_alignment_output_frames,
            "meaning": "Fixed offsets cancel in recurrence; their variation does not.",
        },
        "observations": points,
    }


def _comparison_status(reasons: list[str], points: list[dict[str, object]]) -> str:
    if reasons:
        return "blocked_evidence_not_suitable"
    statuses = [point["status"] for point in points]
    if "fails_one_loaded_frame" in statuses:
        return "measured_recurrence_fails_one_loaded_frame"
    if "inconclusive_due_to_uncertainty" in statuses:
        return "inconclusive_due_to_uncertainty"
    return "recurrence_consistent_in_declared_bounds_no_device_acceptance"


def compare_receipt(workspace: Path, input_path: str, output: str) -> Path:
    """Compare independent measured recurrence while leaving G3 gates explicitly open."""
    packet = read_input(workspace, input_path, TypeAdapter(ComparisonInput))
    features = verified_features(workspace, packet.features)
    reference, clock_reasons = _validate_reference(workspace, packet, features)
    mapped = {channel.channel: _mapped_frames(channel, features) for channel in packet.channels}
    results = [
        _channel_result(channel, reference, features, mapped, packet.clock, clock_reasons)
        for channel in packet.channels
    ]
    data = {
        "schema_version": 1,
        "evidence_kind": packet.evidence_kind,
        "input": artifact(workspace, input_path).model_dump(),
        "productive_run": packet.productive_run.model_dump(),
        "features": packet.features.model_dump(),
        "capture": features.capture.model_dump(),
        "capture_sample_rate_hz": features.capture_sample_rate_hz,
        "output_sample_rate_hz": reference.output.sample_rate_hz,
        "clock": packet.clock.model_dump(mode="json"),
        "channels": results,
        "interpad_anchor_alignment": []
        if clock_reasons
        else _alignment(packet.channels, mapped, packet.clock),
        "numerical_limit_loaded_frames": 1.0,
        "g3_device_gate": "open_requires_review_of_actual_productive_device_evidence",
        "g3_listening_gate": "open_requires_actual_sustained_human_listening",
        "limitations": [
            "A feature recurrence is not a direct continuous trajectory measurement.",
            "Cycle labels, clock provenance and stationary/feature bounds are caller assertions.",
            "Nominal WAV/output rates alone never establish equal clocks.",
            "Fixed seam/DSP offsets are not cumulative period drift; B5 remains separate.",
            "Synthetic fixtures and missing/blocked evidence cannot pass G3.",
        ],
    }
    return write_output(workspace, output, _json(data))
