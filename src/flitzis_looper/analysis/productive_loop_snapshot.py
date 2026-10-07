"""Validate native applied voice/current ACK coherence without app or device imports."""

import json
import struct


def comparison_pad(
    native: dict[str, object] | None,
    current_before: dict[str, object] | None,
    current_after: dict[str, object] | None,
    exported: str | None,
) -> dict[str, object] | None:
    """Project only an exact current-bound applied native voice into capture units."""
    if (
        native is None
        or current_before is None
        or current_before != current_after
        or exported is None
        or native.get("current_acknowledged") is not True
    ):
        return None
    if not _steady_native(native):
        return None
    binding = native.get("current_binding")
    if not isinstance(binding, dict) or binding.get("accepted_timing") != current_before:
        return None
    if not _native_current_matches(native, binding, current_before) or not _verified_export_matches(
        exported, current_before
    ):
        return None
    return {
        "pad_id": native["pad_id"],
        "loaded_sample_rate_hz": native["loaded_sample_rate_hz"],
        "musical_period_loaded_frames": native["musical_loop_period_frames"],
        "physical_start_loaded_frame": native["physical_loop_start_frame"],
        "physical_end_loaded_frame": native["physical_loop_end_frame"],
        "applied_source_rate": native["applied_source_rate"],
        "current_accepted_revision": native["effective_accepted_revision"],
        "current_acknowledged": True,
    }


def _steady_native(native: dict[str, object]) -> bool:
    required = {
        "pad_id",
        "loaded_sample_rate_hz",
        "musical_loop_period_frames",
        "physical_loop_start_frame",
        "physical_loop_end_frame",
        "applied_source_rate",
        "target_source_rate",
        "applied_pad_gain_linear",
        "target_pad_gain_linear",
        "eq_applied_normalized",
        "eq_target_normalized",
        "stem_all_requested",
        "stem_all_applied",
        "key_lock_requested",
        "key_lock_native_active",
    }
    if not required.issubset(native):
        return False
    if any(
        native.get(key) is not expected
        for key, expected in (
            ("fresh", True),
            ("paused", False),
            ("rate_settled", True),
            ("stem_transition_active", False),
        )
    ):
        return False
    if (
        native.get("status") != "available"
        or native.get("source_seek_mode") != 0
        or native.get("musical_loop_period_frames") is None
    ):
        return False
    pairs = (
        ("applied_source_rate", "target_source_rate"),
        ("applied_pad_gain_linear", "target_pad_gain_linear"),
        ("eq_applied_normalized", "eq_target_normalized"),
        ("stem_all_requested", "stem_all_applied"),
    )
    if any(native.get(first) != native.get(second) for first, second in pairs):
        return False
    if (
        native.get("stem_all_requested") is True
        and native.get("prepared_stems_current") is not True
    ):
        return False
    return not (
        native.get("key_lock_requested") is True
        and native.get("applied_source_rate") != 1.0
        and native.get("key_lock_native_active") is not True
    )


def _bits(value: object) -> str | None:
    return struct.pack("!d", value).hex() if type(value) is float else None


def _native_current_matches(
    native: dict[str, object], binding: dict[str, object], current: dict[str, object]
) -> bool:
    direct_pairs = (
        ("pad_id", "pad_id"),
        ("source_generation", "source_generation"),
        ("publication_epoch", "publication_epoch"),
        ("loaded_sample_rate_hz", "sample_rate_hz"),
        ("effective_accepted_revision", "revision"),
    )
    if any(native.get(first) != current.get(second) for first, second in direct_pairs):
        return False
    source_keys = (
        "pad_id",
        "source_id",
        "source_generation",
        "source_sha256",
        "sample_rate_hz",
        "frame_count",
    )
    if any(binding.get(key) != current.get(key) for key in source_keys):
        return False
    if native.get("authority_revision") != binding.get("authority_revision"):
        return False
    float_pairs = (
        ("effective_period_seconds_per_quarter", "period_seconds_per_quarter"),
        ("effective_origin_seconds", "origin_seconds"),
    )
    return all(
        _bits(native.get(first)) == _bits(current.get(second))
        and _bits(current.get(second)) is not None
        for first, second in float_pairs
    )


def _verified_export_matches(exported: str, current: dict[str, object]) -> bool:
    try:
        envelope = json.loads(exported)
    except json.JSONDecodeError:
        return False
    if not isinstance(envelope, dict) or envelope.get("schema_version") != 1:
        return False
    if envelope.get("encoding") != "accepted-constant-timing-qm-raw-v1":
        return False
    record = envelope.get("record")
    if not isinstance(record, dict) or record.get("accepted_revision") != current.get("revision"):
        return False
    evidence = record.get("evidence")
    if not isinstance(evidence, dict) or not isinstance(evidence.get("binding"), dict):
        return False
    binding = evidence["binding"]
    return _export_binding_matches(binding, current) and _export_scalars_match(
        record, binding, current
    )


def _export_binding_matches(binding: dict[str, object], current: dict[str, object]) -> bool:
    job = binding.get("job")
    if not isinstance(job, dict):
        return False
    if job.get("request_id") != current.get("accepted_request_id"):
        return False
    if any(
        job.get(key) != current.get(key) for key in ("pad_id", "source_id", "source_generation")
    ):
        return False
    keys = (
        "source_sha256",
        "source_provenance",
        "pcm_sha256",
        "sample_rate_hz",
        "frame_count",
        "mono_revision",
    )
    return all(binding.get(key) == current.get(key) for key in keys)


def _export_scalars_match(
    record: dict[str, object], binding: dict[str, object], current: dict[str, object]
) -> bool:
    origin, decision = record.get("origin"), record.get("decision")
    if not isinstance(origin, dict) or not isinstance(decision, dict):
        return False
    if any(
        _bits(current.get(key)) is None
        for key in (
            "period_seconds_per_quarter",
            "source_zero_seconds",
            "origin_seconds",
        )
    ):
        return False
    return all((
        record.get("period_bits") == _bits(current.get("period_seconds_per_quarter")),
        binding.get("source_zero_bits") == _bits(current.get("source_zero_seconds")),
        origin.get("seconds_bits") == _bits(current.get("origin_seconds")),
        origin.get("provenance") == current.get("origin_provenance"),
        decision.get("policy_version") == current.get("acceptance_policy_version"),
        decision.get("provenance") == current.get("acceptance_provenance"),
    ))
