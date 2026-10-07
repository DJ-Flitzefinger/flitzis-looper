"""Offline loopback collection and comparison; no app/device/playback control.

``python -m flitzis_looper.analysis.loop_capture --help`` describes the private
artifact commands. Synthetic evidence and validated caller declarations never
complete actual device or human listening gates.
"""

import argparse
import sys
from datetime import UTC, datetime
from itertools import pairwise
from pathlib import Path
from typing import TYPE_CHECKING

from pydantic import TypeAdapter, ValidationError

from flitzis_looper.analysis.loop_capture_compare import (
    RunEnvelope,
    compare_receipt,
    validate_productive_run,
)
from flitzis_looper.analysis.loop_capture_io import (
    _json,
    artifact,
    verified_features,
    verify_artifact,
)
from flitzis_looper.analysis.loop_capture_models import (
    DetectorPolicy,
    FeatureEvidence,
    ListeningInput,
)
from flitzis_looper.analysis.loop_capture_wav import measure_wave
from flitzis_looper.analysis.reference_inputs_validation import (
    fail,
    private_path,
    read_input,
    read_json_bytes,
    write_output,
)

if TYPE_CHECKING:
    from flitzis_looper.analysis.loop_capture_models import EvidenceKind


def draft_detector(workspace: Path, output: str) -> Path:
    """Create a concrete detector draft without assigning human provenance."""
    return write_output(
        workspace,
        output,
        _json({
            "schema_version": 1,
            "method": "absolute_threshold_hysteresis_v1",
            "frozen_at_utc": None,
            "provenance": None,
            "channels": [
                {
                    "channel": 0,
                    "high": 0.1,
                    "low": 0.01,
                    "minimum_gap_capture_frames": 1,
                    "rearm_low_capture_frames": 96,
                }
            ],
        }),
    )


def collect_features(
    workspace: Path, capture_path: str, policy_path: str, output: str, evidence_kind: EvidenceKind
) -> Path:
    """Measure complete stable WAV bytes with a period-independent frozen policy."""
    policy = read_input(workspace, policy_path, TypeAdapter(DetectorPolicy))
    if policy.frozen_at_utc.utcoffset() is None:
        fail("detector_timestamp_requires_timezone")
    capture = artifact(workspace, capture_path)
    policy_identity = artifact(workspace, policy_path)
    info, channels = measure_wave(private_path(workspace, capture_path), policy)
    verify_artifact(workspace, capture)
    verify_artifact(workspace, policy_identity)
    report = FeatureEvidence(
        schema_version=1,
        evidence_kind=evidence_kind,
        capture=capture,
        policy=policy_identity,
        wave_format=info.format,
        capture_sample_rate_hz=info.rate,
        capture_channel_count=info.channels,
        capture_frame_count=info.frames,
        measured_channels=channels,
        scope="offline_features_only_no_device_or_listening_acceptance",
    )
    return write_output(workspace, output, report.model_dump_json(indent=2).encode() + b"\n")


def _optional_features(workspace: Path, feature_path: str | None) -> FeatureEvidence | None:
    if feature_path is None:
        return None
    return verified_features(workspace, artifact(workspace, feature_path))


def draft_comparison(
    workspace: Path, run_path: str | None, feature_path: str | None, output: str
) -> Path:
    """Bind measured bytes and leave all human/clock/cycle assertions unfilled."""
    features = _optional_features(workspace, feature_path)
    data: dict[str, object] = {
        "schema_version": 1,
        "evidence_kind": features.evidence_kind if features else "real_device_loopback",
        "productive_run": artifact(workspace, run_path).model_dump() if run_path else None,
        "features": artifact(workspace, feature_path).model_dump() if feature_path else None,
        "capture_started_at_utc": None,
        "recorder_identity": None,
        "capture_route": None,
        "clock": {
            "mode": "unknown",
            "output_clock_identity": None,
            "recording_clock_identity": None,
            "output_frames_per_capture_frame": None,
            "ratio_halfwidth": None,
            "valid_duration_seconds": None,
            "independent_of_candidate_period": None,
            "provenance": None,
            "evidence": None,
        },
        "channels": [
            {
                "channel": channel,
                "pad_id": None,
                "cycle_count_provenance": None,
                "independent_cycle_counts": None,
                "stationary_configuration_asserted": None,
                "feature_localization_halfwidth_capture_frames": 0.5,
                "seam_feature_offset_output_frames": None,
                "seam_feature_variation_halfwidth_output_frames": None,
                "seam_feature_provenance": None,
                "audible_dsp_alignment_output_frames": None,
                "audible_dsp_variation_halfwidth_output_frames": None,
                "audible_dsp_provenance": None,
                "observations": [],
            }
            for channel in (
                [item.channel for item in features.measured_channels] if features else [0]
            )
        ],
    }
    return write_output(workspace, output, _json(data))


def draft_listening(
    workspace: Path, run_path: str | None, feature_path: str | None, output: str
) -> Path:
    """Prepare a full 30-minute human form without asserting any listening took place."""
    features = _optional_features(workspace, feature_path)
    data: dict[str, object] = {
        "schema_version": 1,
        "evidence_kind": features.evidence_kind if features else "real_device_loopback",
        "productive_run": artifact(workspace, run_path).model_dump() if run_path else None,
        "productive_run_end": None,
        "features": artifact(workspace, feature_path).model_dump() if feature_path else None,
        "capture_start_frame": None,
        "capture_end_frame": None,
        "capture_interval_provenance": None,
        "observer": None,
        "listening_provenance": None,
        "started_at_utc": None,
        "ended_at_utc": None,
        "continuous_uninterrupted": None,
        "actual_productive_app_listened_to": None,
        "outcome": "inconclusive",
        "observations": [
            {
                "elapsed_seconds": float(seconds),
                "audible_drift": "unassessed",
                "seam_clicks": "unassessed",
                "missed_or_doubled_beats": "unassessed",
                "interpad_alignment": "unassessed",
                "keylock_stem_dsp": "unassessed",
                "note": None,
            }
            for seconds in range(0, 1801, 300)
        ],
    }
    return write_output(workspace, output, _json(data))


def _listening_runs(workspace: Path, declaration: ListeningInput) -> None:
    """Bind start/end current source/timing and actual versus synthetic run origin."""
    start_run_path = verify_artifact(workspace, declaration.productive_run)
    end_run_path = verify_artifact(workspace, declaration.productive_run_end)
    adapter = TypeAdapter(RunEnvelope)
    start_run = adapter.validate_json(read_json_bytes(start_run_path), strict=True)
    end_run = adapter.validate_json(read_json_bytes(end_run_path), strict=True)
    validate_productive_run(workspace, start_run)
    validate_productive_run(workspace, end_run)
    expected_kind = (
        "synthetic_fixture"
        if declaration.evidence_kind == "synthetic_fixture"
        else "real_productive_app_observation"
    )
    if start_run.evidence_kind != expected_kind or end_run.evidence_kind != expected_kind:
        fail("listening_requires_matching_actual_productive_run_kind")
    if start_run.capture_comparison != end_run.capture_comparison:
        fail("listening_start_end_productive_source_timing_configuration_mismatch")
    if start_run.pads is not None and end_run.pads is not None:
        fields = ("pad_id", "current_source_binding", "current_constant_timing_before_export")
        start_bindings = [{key: row.get(key) for key in fields} for row in start_run.pads]
        end_bindings = [{key: row.get(key) for key in fields} for row in end_run.pads]
        if start_bindings != end_bindings:
            fail("listening_start_end_current_source_generation_or_adoption_mismatch")
        if _processing_identities(start_run) != _processing_identities(end_run):
            fail("listening_start_end_effective_processing_configuration_mismatch")


def _processing_identities(run: RunEnvelope) -> list[dict[str, object]]:
    fields = (
        "key_lock_requested",
        "key_lock_native_active",
        "native_pitch_scale",
        "stem_all_requested",
        "stem_all_applied",
        "stem_source_version_hash",
        "stem_enabled_mask",
        "prepared_stems_current",
        "eq_applied_normalized",
        "eq_target_normalized",
        "applied_pad_gain_linear",
        "target_pad_gain_linear",
        "master_volume",
        "voice_volume",
        "bpm_lock",
        "master_period_seconds",
    )
    identities = []
    for row in run.pads or ():
        native = row.get("native_callback_snapshot")
        if not isinstance(native, dict) or any(key not in native for key in fields):
            fail("listening_effective_processing_identity_incomplete")
        assert isinstance(native, dict)
        identities.append({"pad_id": row.get("pad_id"), **{key: native[key] for key in fields}})
    return identities


def _listening_outcome(declaration: ListeningInput) -> None:
    """Require actual category declarations to support a claimed outcome."""
    statuses = [
        value
        for item in declaration.observations
        for key, value in item.model_dump().items()
        if key not in {"elapsed_seconds", "note"}
    ]
    if declaration.outcome == "pass" and any(value != "clear" for value in statuses):
        fail("listening_pass_requires_every_observation_assessed_clear")
    if declaration.outcome == "fail" and "failed" not in statuses:
        fail("listening_fail_requires_recorded_failure")


def _listening_timeline(
    declaration: ListeningInput, features: FeatureEvidence
) -> tuple[float, float]:
    """Validate sustained coverage and explicit observed outcomes without certifying truth."""
    if any(
        stamp.utcoffset() is None
        for stamp in (declaration.started_at_utc, declaration.ended_at_utc)
    ):
        fail("listening_timestamps_require_timezone")
    duration = (declaration.ended_at_utc - declaration.started_at_utc).total_seconds()
    times = [item.elapsed_seconds for item in declaration.observations]
    if duration < 1800 or not declaration.continuous_uninterrupted:
        fail("listening_requires_continuous_30_minutes")
    if not declaration.actual_productive_app_listened_to:
        fail("listening_requires_actual_productive_app_declaration")
    if times != sorted(set(times)) or times[0] != 0 or times[-1] < 1800 or times[-1] > duration:
        fail("listening_observations_must_cover_start_to_30_minutes_in_order")
    if any(right - left > 300 for left, right in pairwise(times)):
        fail("listening_observation_gap_exceeds_five_minutes")
    if (
        not 0
        <= declaration.capture_start_frame
        < declaration.capture_end_frame
        <= features.capture_frame_count
    ):
        fail("listening_capture_interval_outside_actual_wave")
    capture_duration = (
        declaration.capture_end_frame - declaration.capture_start_frame
    ) / features.capture_sample_rate_hz
    if capture_duration < 1800:
        fail("listening_capture_must_contain_at_least_30_nominal_minutes")
    if duration > capture_duration:
        fail("declared_listening_duration_exceeds_selected_capture_nominal_interval")
    _listening_outcome(declaration)
    return duration, capture_duration


def listening_receipt(workspace: Path, input_path: str, output: str) -> Path:
    """Validate completeness of a human declaration; truth and G3 acceptance stay open."""
    declaration = read_input(workspace, input_path, TypeAdapter(ListeningInput))
    features = verified_features(workspace, declaration.features)
    _listening_runs(workspace, declaration)
    if declaration.evidence_kind != features.evidence_kind:
        fail("listening_and_capture_evidence_kind_mismatch")
    duration, capture_duration = _listening_timeline(declaration, features)
    receipt = {
        "schema_version": 1,
        "status": "human_declaration_consistent_not_certified",
        "evidence_kind": declaration.evidence_kind,
        "input": artifact(workspace, input_path).model_dump(),
        "productive_run": declaration.productive_run.model_dump(),
        "productive_run_end": declaration.productive_run_end.model_dump(),
        "features": declaration.features.model_dump(),
        "capture": features.capture.model_dump(),
        "declared_outcome": declaration.outcome,
        "declared_listening_duration_seconds": duration,
        "capture_nominal_duration_seconds": capture_duration,
        "capture_start_frame": declaration.capture_start_frame,
        "capture_end_frame": declaration.capture_end_frame,
        "observer": declaration.observer,
        "created_at_utc": datetime.now(UTC).isoformat(),
        "g3_device_gate": "open_requires_review_of_actual_device_evidence",
        "g3_listening_gate": "open_requires_human_declaration_review",
        "limitations": [
            "Caller declarations are not independently certified by this tool.",
            "Synthetic fixtures cannot pass real device or human gates.",
            "Nominal WAV rate is not proof of physical recording-clock frequency.",
        ],
    }
    return write_output(workspace, output, _json(receipt))


def main(argv: list[str] | None = None) -> int:
    """Execute offline-only commands, preserving partial inputs on validation failure."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace", type=Path, required=True)
    commands = parser.add_subparsers(dest="command", required=True)
    detector = commands.add_parser("detector-draft")
    detector.add_argument("--output", required=True)
    features = commands.add_parser("features")
    features.add_argument("--capture", required=True)
    features.add_argument("--policy", required=True)
    features.add_argument("--output", required=True)
    features.add_argument(
        "--evidence-kind", choices=("real_device_loopback", "synthetic_fixture"), required=True
    )
    for name in ("comparison-draft", "listening-draft"):
        draft = commands.add_parser(name)
        draft.add_argument("--run")
        draft.add_argument("--features")
        draft.add_argument("--output", required=True)
    for name in ("compare", "listening-receipt"):
        operation = commands.add_parser(name)
        operation.add_argument("--input", required=True)
        operation.add_argument("--output", required=True)
    args = parser.parse_args(argv)
    try:
        if args.command == "detector-draft":
            result = draft_detector(args.workspace, args.output)
        elif args.command == "features":
            result = collect_features(
                args.workspace, args.capture, args.policy, args.output, args.evidence_kind
            )
        elif args.command == "comparison-draft":
            result = draft_comparison(args.workspace, args.run, args.features, args.output)
        elif args.command == "listening-draft":
            result = draft_listening(args.workspace, args.run, args.features, args.output)
        elif args.command == "compare":
            result = compare_receipt(args.workspace, args.input, args.output)
        else:
            result = listening_receipt(args.workspace, args.input, args.output)
    except (OSError, ValueError, ValidationError) as error:
        sys.stderr.write(f"loop_capture: {error}\n")
        return 2
    sys.stdout.write(f"{result}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
