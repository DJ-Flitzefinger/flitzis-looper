"""Prepare blind private B2 drafts and seal strictly validated human input receipts.

Run ``python -m flitzis_looper.analysis.reference_inputs --help``. No command
imports the native engine, runs inference, scores or adopts a result. References
are sealed without reading predictions; subsequent correction validation hashes
the actual candidate/edit artifacts without parsing them.
"""

import argparse
import hashlib
import json
import sys
from pathlib import Path
from typing import TYPE_CHECKING

from pydantic import TypeAdapter, ValidationError

from flitzis_looper.analysis.reference_inputs_models import (
    BACKENDS,
    HELD_OUT,
    MUSICAL_CLASSES,
    CorrectionBundle,
    CorrectionOrder,
    OrderSeal,
    ReferenceBundle,
    ReferenceSeal,
)
from flitzis_looper.analysis.reference_inputs_validation import (
    LOADED_IDENTITIES_SHA256,
    MANIFEST_SHA256,
    SCORING_SHA256,
    frozen_corpus,
    loaded_identities,
    order_seal,
    private_path,
    read_json_bytes,
    read_source_aliases,
    reference_seal,
    utc_now,
    validate_corrections,
    validate_identities,
    validate_order,
    validate_reference,
    write_output,
)

if TYPE_CHECKING:
    from flitzis_looper.analysis.reference_inputs_models import LoadedIdentity
    from flitzis_looper.analysis.reference_source_aliases import SourcePathAlias


def _aliases(workspace: Path, path: str | None) -> tuple[SourcePathAlias, ...]:
    return () if path is None else read_source_aliases(workspace, path)


def _json(data: object) -> bytes:
    return (json.dumps(data, indent=2, ensure_ascii=False, allow_nan=False) + "\n").encode("utf-8")


def _track_draft(identity: LoadedIdentity) -> dict[str, object]:
    return {
        "identity": identity.model_dump(mode="json"),
        "split": "development" if identity.track_id in {"T01", "T02"} else "held_out",
        "annotator": None,
        "revision": None,
        "predictions_seen": None,
        "independent_listening": None,
        "listening_provenance": None,
        "time_domain": "native_loaded_frame_zero_seconds",
        "extent_start_seconds": 0.0,
        "extent_end_seconds": identity.pcm.frame_count / identity.pcm.sample_rate_hz,
        "assessor": None,
        "certification_provenance": None,
        "recording_groups": [],
        "classes": [
            {"musical_class": name, "status": None, "provenance": None} for name in MUSICAL_CLASSES
        ],
        "regions": [],
        "beats": [],
        "bars": [],
        "gap_transitions": [],
        "critical_features": [
            {"feature": name, "bar_ids": [], "absent_reason": None, "provenance": None}
            for name in ("first_unambiguous", "post_break", "late_track")
        ],
    }


def draft_reference(workspace: Path, identities_path: str, output: str) -> Path:
    """Create five empty, explicitly invalid drafts from immutable native identities."""
    identities = loaded_identities(workspace, identities_path)
    validate_identities(identities, frozen_corpus(workspace))
    data = {
        "schema_version": 1,
        "status": "draft",
        "manifest_sha256": MANIFEST_SHA256,
        "scoring_sha256": SCORING_SHA256,
        "tracks": [_track_draft(item) for item in identities.tracks],
        "loaded_identities_path": identities_path,
        "loaded_identities_sha256": LOADED_IDENTITIES_SHA256,
    }
    return write_output(workspace, output, _json(data))


def seal_reference(
    workspace: Path, input_path: str, output: str, source_aliases_path: str | None = None
) -> Path:
    """Seal complete independently certified reference input; acceptance stays pending."""
    path = private_path(workspace, input_path)
    raw = read_json_bytes(path)
    bundle = TypeAdapter(ReferenceBundle).validate_json(raw, strict=True)
    coverage, absent = validate_reference(
        workspace,
        bundle,
        frozen_corpus(workspace),
        source_aliases=_aliases(workspace, source_aliases_path),
    )
    seal = ReferenceSeal(
        schema_version=1,
        status="sealed_reference_inputs",
        sealed_at_utc=utc_now(),
        bundle_path=path.relative_to(workspace.resolve()).as_posix(),
        bundle_sha256=hashlib.sha256(raw).hexdigest(),
        manifest_sha256=MANIFEST_SHA256,
        scoring_sha256=SCORING_SHA256,
        coverage=coverage,
        absent_classes=absent,
        corrections="pending",
        musical_acceptance="pending",
        default_adoption="blocked",
    )
    return write_output(workspace, output, seal.model_dump_json(indent=2).encode("utf-8"))


def draft_order(
    workspace: Path, seal_path: str, output: str, source_aliases_path: str | None = None
) -> Path:
    """Prepare a balanced draft; humans confirm the schedule before sealing it."""
    _, _, reference_hash = reference_seal(
        workspace, seal_path, source_aliases=_aliases(workspace, source_aliases_path)
    )
    slots = [
        {"session_id": f"{track}-{position + 1}", "track_id": track, "backend": backend}
        for index, track in enumerate(HELD_OUT)
        for position, backend in enumerate(
            BACKENDS if index % 2 == 0 else tuple(reversed(BACKENDS))
        )
    ]
    data = {
        "schema_version": 1,
        "status": "draft",
        "reference_seal_sha256": reference_hash,
        "annotator": None,
        "tool": None,
        "workflow": None,
        "endpoint": None,
        "provenance": None,
        "slots": slots,
    }
    return write_output(workspace, output, _json(data))


def seal_order(
    workspace: Path,
    input_path: str,
    seal_path: str,
    output: str,
    source_aliases_path: str | None = None,
) -> Path:
    """Freeze the exact paired schedule before any actual correction session."""
    reference, _, reference_hash = reference_seal(
        workspace, seal_path, source_aliases=_aliases(workspace, source_aliases_path)
    )
    path = private_path(workspace, input_path)
    raw = read_json_bytes(path)
    order = TypeAdapter(CorrectionOrder).validate_json(raw, strict=True)
    validate_order(order, reference_hash)
    timestamp = utc_now()
    if timestamp < reference.sealed_at_utc:
        msg = "order_must_follow_reference_seal"
        raise ValueError(msg)
    receipt = OrderSeal(
        schema_version=1,
        status="sealed_correction_order",
        sealed_at_utc=timestamp,
        order_path=path.relative_to(workspace.resolve()).as_posix(),
        order_sha256=hashlib.sha256(raw).hexdigest(),
        reference_seal_sha256=reference_hash,
        musical_acceptance="pending",
    )
    return write_output(workspace, output, receipt.model_dump_json(indent=2).encode("utf-8"))


def draft_corrections(
    workspace: Path,
    reference_path: str,
    order_path: str,
    output: str,
    source_aliases_path: str | None = None,
) -> Path:
    """Create empty paired session fields without inventing operations or timings."""
    reference_receipt, reference, reference_hash = reference_seal(
        workspace, reference_path, source_aliases=_aliases(workspace, source_aliases_path)
    )
    order_receipt, order, order_hash = order_seal(workspace, order_path, reference_hash)
    if order_receipt.sealed_at_utc < reference_receipt.sealed_at_utc:
        msg = "order_must_follow_reference_seal"
        raise ValueError(msg)
    tracks = {track.identity.track_id: track for track in reference.tracks}
    sessions = [
        {
            **slot.model_dump(mode="json"),
            "producer": None,
            "source_sha256": tracks[slot.track_id].identity.source_sha256,
            "reference_revision": tracks[slot.track_id].revision,
            "reference_seal_sha256": reference_hash,
            "annotator": order.annotator,
            "tool": order.tool,
            "workflow": order.workflow,
            "endpoint": order.endpoint,
            "human_measured": None,
            "provenance": None,
            "zero_active_time_reason": None,
            "initial_prediction_sha256": None,
            "initial_prediction_path": None,
            "corrected_result_sha256": None,
            "corrected_result_path": None,
            "start_utc": None,
            "end_utc": None,
            "phases": [],
            "critical_outcomes": [],
        }
        for slot in order.slots
    ]
    data = {
        "schema_version": 1,
        "status": "draft",
        "reference_seal_sha256": reference_hash,
        "order_seal_sha256": order_hash,
        "sessions": sessions,
    }
    return write_output(workspace, output, _json(data))


def validate_measured_corrections(
    workspace: Path,
    input_path: str,
    reference_path: str,
    order_path: str,
    output: str,
    source_aliases_path: str | None = None,
) -> Path:
    """Issue a validated-input receipt, never comparative or musical acceptance."""
    reference_receipt, reference, reference_hash = reference_seal(
        workspace, reference_path, source_aliases=_aliases(workspace, source_aliases_path)
    )
    order_receipt, order, order_hash = order_seal(workspace, order_path, reference_hash)
    if order_receipt.sealed_at_utc < reference_receipt.sealed_at_utc:
        msg = "order_must_follow_reference_seal"
        raise ValueError(msg)
    raw = read_json_bytes(private_path(workspace, input_path))
    bundle = TypeAdapter(CorrectionBundle).validate_json(raw, strict=True)
    measurements = validate_corrections(
        workspace,
        bundle,
        reference,
        order,
        (reference_hash, order_hash),
        order_receipt.sealed_at_utc,
    )
    data = {
        "schema_version": 1,
        "status": "validated_measured_correction_inputs",
        "validated_at_utc": utc_now().isoformat(),
        "reference_seal_sha256": reference_hash,
        "order_seal_sha256": order_hash,
        "corrections_sha256": hashlib.sha256(raw).hexdigest(),
        "measurements": [
            {"session_id": session, "total_operations": operations, "active_seconds": seconds}
            for session, operations, seconds in measurements
        ],
        "musical_acceptance": "pending",
        "comparative_improvement": "pending",
        "default_adoption": "blocked",
    }
    return write_output(workspace, output, _json(data))


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command", required=True)
    commands = (
        "draft",
        "seal-reference",
        "draft-order",
        "seal-order",
        "draft-corrections",
        "validate-corrections",
    )
    for command in commands:
        sub = subcommands.add_parser(command)
        sub.add_argument("--workspace", type=Path, required=True)
        sub.add_argument("--output", required=True)
        sub.add_argument("--source-aliases")
        if command == "draft":
            sub.add_argument("--identities", required=True)
        if command in {"seal-reference", "seal-order", "validate-corrections"}:
            sub.add_argument("--input", required=True)
        if command in {"draft-order", "seal-order", "draft-corrections", "validate-corrections"}:
            sub.add_argument("--reference-seal", required=True)
        if command in {"draft-corrections", "validate-corrections"}:
            sub.add_argument("--order-seal", required=True)
    return parser


def _execute(args: argparse.Namespace) -> Path:
    workspace = args.workspace
    if args.command == "draft":
        return draft_reference(workspace, args.identities, args.output)
    if args.command == "seal-reference":
        return seal_reference(workspace, args.input, args.output, args.source_aliases)
    if args.command == "draft-order":
        return draft_order(workspace, args.reference_seal, args.output, args.source_aliases)
    if args.command == "seal-order":
        return seal_order(
            workspace, args.input, args.reference_seal, args.output, args.source_aliases
        )
    if args.command == "draft-corrections":
        return draft_corrections(
            workspace, args.reference_seal, args.order_seal, args.output, args.source_aliases
        )
    return validate_measured_corrections(
        workspace,
        args.input,
        args.reference_seal,
        args.order_seal,
        args.output,
        args.source_aliases,
    )


def main() -> int:
    """Run one bounded private input command with explicit rejection on failure."""
    args = _parser().parse_args()
    try:
        result = _execute(args)
    except (OSError, ValueError, ValidationError) as error:
        sys.stderr.write(f"reference inputs rejected: {error}\n")
        return 1
    sys.stdout.write(f"created {result}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
