"""Private B2 temporal diagnostics with reference-first content and native lineage checks.

Run ``python -m flitzis_looper.analysis.beat_scoring_cli --help``. This workflow
reads retained artifacts only. It imports no native engine, starts no worker,
supplies no human labels and changes no application state or default routing.
"""

import argparse
import hashlib
import json
import sys
from dataclasses import asdict
from pathlib import Path
from typing import TYPE_CHECKING

from pydantic import TypeAdapter, ValidationError

from flitzis_looper.analysis.beat_candidates import available_historical_profiles, load_candidate
from flitzis_looper.analysis.beat_scoring import score_track_timing
from flitzis_looper.analysis.beat_scoring_inputs import ScoringPlan
from flitzis_looper.analysis.reference_inputs_models import TRACK_IDS
from flitzis_looper.analysis.reference_inputs_validation import (
    frozen_corpus,
    loaded_identities,
    parse_source_aliases,
    private_path,
    read_json_bytes,
    reference_seal,
    utc_now,
    validate_identities,
    validate_source_material,
    write_output,
)

if TYPE_CHECKING:
    from flitzis_looper.analysis.beat_candidate_models import NativeCandidate
    from flitzis_looper.analysis.reference_inputs_models import ReferenceBundle, ReferenceSeal
    from flitzis_looper.analysis.reference_source_aliases import SourcePathAlias


def _artifact(workspace: Path, path: str | Path, raw: bytes) -> dict[str, object]:
    return {
        "path": private_path(workspace, path).relative_to(workspace.resolve()).as_posix(),
        "sha256": hashlib.sha256(raw).hexdigest(),
        "bytes": len(raw),
    }


def _aliases(
    workspace: Path, path: str | None
) -> tuple[tuple[SourcePathAlias, ...], dict[str, object] | None]:
    if path is None:
        return (), None
    raw = read_json_bytes(private_path(workspace, path))
    aliases = parse_source_aliases(workspace, raw)
    return aliases, {
        **_artifact(workspace, path, raw),
        "aliases": [a.model_dump() for a in aliases],
    }


def _base() -> dict[str, object]:
    return {
        "schema_version": 1,
        "created_at_utc": utc_now().isoformat(),
        "scope": "private_seal_bound_native_temporal_diagnostics",
        "human_declarations": "supplied_only_not_verified_as_truth",
        "quarter_count_and_bar_identity": "pending",
        "paired_correction_burden": "pending",
        "musical_acceptance": "pending",
        "default_adoption": "blocked",
    }


def _write(workspace: Path, output: str, report: dict[str, object]) -> Path:
    raw = (json.dumps(report, indent=2, ensure_ascii=False, allow_nan=False) + "\n").encode()
    return write_output(workspace, output, raw)


def _missing_report(
    workspace: Path,
    output: str,
    report: dict[str, object],
    error: FileNotFoundError,
    status: str,
    input_kind: str,
) -> Path:
    return _write(
        workspace,
        output,
        {
            **report,
            "status": status,
            "missing_inputs": [{"input": input_kind, "path": error.filename}],
            "candidate_artifacts_read": False,
            "temporal_scores": [],
        },
    )


def inventory(workspace: Path, identities: str, output: str, aliases_path: str | None) -> Path:
    """Check actual source/listening material without opening any candidate predictions."""
    try:
        corpus = frozen_corpus(workspace)
        aliases, alias_binding = _aliases(workspace, aliases_path)
        loaded = loaded_identities(workspace, identities)
    except FileNotFoundError as error:
        return _missing_report(
            workspace,
            output,
            _base(),
            error,
            "blocked_missing_inventory_input",
            "protocol_inventory_source_or_alias",
        )
    validate_identities(loaded, corpus)
    sources = {source.id: source for source in corpus.tracks}
    material: list[dict[str, object]] = []
    for identity in loaded.tracks:
        try:
            validate_source_material(
                workspace, identity, sources[identity.track_id], source_aliases=aliases
            )
        except FileNotFoundError as error:
            material.append({
                "track_id": identity.track_id,
                "status": "missing",
                "path": error.filename,
            })
        else:
            material.append({
                "track_id": identity.track_id,
                "status": "verified",
                "identity": identity.model_dump(),
            })
    return _write(
        workspace,
        output,
        {
            **_base(),
            "status": "input_inventory_only",
            "source_aliases": alias_binding,
            "material": material,
            "candidate_artifacts_read": False,
            "missing_inputs": [
                {"track_id": track, "input": "independent_complete_sealed_reference_not_supplied"}
                for track in TRACK_IDS
            ]
            + [
                {"track_id": item["track_id"], "input": "source_or_pcm", "path": item["path"]}
                for item in material
                if item["status"] == "missing"
            ],
            "temporal_scores": [],
        },
    )


def draft_plan(workspace: Path, seal_path: str, output: str, aliases_path: str | None) -> Path:
    """Prepare explicit historical selections after revalidating the supplied reference."""
    aliases, _ = _aliases(workspace, aliases_path)
    _, _, digest = reference_seal(workspace, seal_path, source_aliases=aliases)
    data = {
        "schema_version": 1,
        "status": "ready_for_temporal_scoring",
        "reference_seal_sha256": digest,
        "candidates": available_historical_profiles(),
    }
    return _write(workspace, output, data)


def _reference_binding(
    workspace: Path, path: str, seal: ReferenceSeal, bundle: ReferenceBundle, digest: str
) -> dict[str, object]:
    return {
        "seal_path": private_path(workspace, path).relative_to(workspace.resolve()).as_posix(),
        "seal_sha256": digest,
        "seal": seal.model_dump(mode="json"),
        "loaded_identities_path": bundle.loaded_identities_path,
        "loaded_identities_sha256": bundle.loaded_identities_sha256,
        "reference_revisions": {track.identity.track_id: track.revision for track in bundle.tracks},
    }


def _candidate_report(candidate: NativeCandidate) -> dict[str, object]:
    # NativeCandidate contains immutable complete arrays; never project a prefix.
    return candidate.report()


def _validate_plan(plan: ScoringPlan, digest: str) -> None:
    if plan.reference_seal_sha256 != digest:
        msg = "candidate_plan_reference_seal_mismatch"
        raise ValueError(msg)
    if len({selection.track_id for selection in plan.candidates}) != len(plan.candidates):
        msg = "duplicate_candidate_track"
        raise ValueError(msg)
    supported = {item["profile_id"]: item["track_id"] for item in available_historical_profiles()}
    for selection in plan.candidates:
        if selection.profile_id not in supported:
            msg = "unsupported_candidate_lineage"
            raise ValueError(msg)
        if supported[selection.profile_id] != selection.track_id:
            msg = "candidate_profile_track_mismatch"
            raise ValueError(msg)


def _scores(
    workspace: Path, bundle: ReferenceBundle, plan: ScoringPlan
) -> tuple[list[dict[str, object]], list[dict[str, object]]]:
    selections = {selection.track_id: selection for selection in plan.candidates}
    missing: list[dict[str, object]] = []
    candidates: list[NativeCandidate] = []
    for track in bundle.tracks:
        selection = selections.get(track.identity.track_id)
        if selection is None:
            missing.append({
                "track_id": track.identity.track_id,
                "input": "native_candidate_not_selected",
            })
            continue
        try:
            candidate = load_candidate(workspace, track, selection.profile_id)
        except FileNotFoundError as error:
            missing.append({
                "track_id": track.identity.track_id,
                "input": "native_candidate_artifact",
                "path": error.filename,
            })
        else:
            candidates.append(candidate)
    references = {track.identity.track_id: track for track in bundle.tracks}
    scores: list[dict[str, object]] = [
        {
            "candidate": _candidate_report(candidate),
            "temporal": asdict(
                score_track_timing(references[candidate.track_id], candidate.predictions)
            ),
        }
        for candidate in candidates
    ]
    return scores, missing


def score_private(
    workspace: Path, seal_path: str, input_path: str, output: str, aliases_path: str | None = None
) -> Path:
    """Revalidate the complete reference before reading plans, candidates or scoring."""
    report: dict[str, object] = {
        **_base(),
        "source_aliases": None,
        "candidate_artifacts_read": False,
    }
    try:
        aliases, alias_binding = _aliases(workspace, aliases_path)
        report["source_aliases"] = alias_binding
        seal, bundle, digest = reference_seal(workspace, seal_path, source_aliases=aliases)
    except FileNotFoundError as error:
        return _missing_report(
            workspace,
            output,
            report,
            error,
            "blocked_missing_reference_input",
            "sealed_reference_material_or_alias",
        )
    report["reference"] = _reference_binding(workspace, seal_path, seal, bundle, digest)
    plan_path = private_path(workspace, input_path)
    try:
        raw = read_json_bytes(plan_path)
    except FileNotFoundError:
        return _write(
            workspace,
            output,
            {
                **report,
                "status": "blocked_missing_candidate_plan",
                "missing_inputs": [{"input": "candidate_plan", "path": str(plan_path)}],
                "temporal_scores": [],
            },
        )
    plan = TypeAdapter(ScoringPlan).validate_json(raw, strict=True)
    _validate_plan(plan, digest)
    report["candidate_plan"] = _artifact(workspace, input_path, raw)
    scores, missing = _scores(workspace, bundle, plan)
    return _write(
        workspace,
        output,
        {
            **report,
            "status": "incomplete_temporal_diagnostics"
            if missing
            else "complete_temporal_diagnostics",
            "input_binding": "reference_bytes_and_approved_native_lineage_revalidated",
            "candidate_artifacts_read": bool(plan.candidates),
            "missing_inputs": missing,
            "temporal_scores": scores,
        },
    )


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    for name in ("inventory", "draft", "score"):
        command = commands.add_parser(name)
        command.add_argument("--workspace", type=Path, required=True)
        command.add_argument("--output", required=True)
        command.add_argument("--source-aliases")
        if name == "inventory":
            command.add_argument("--identities", required=True)
        else:
            command.add_argument("--reference-seal", required=True)
        if name == "score":
            command.add_argument("--input", required=True)
    return parser


def main() -> int:
    """Run one read-only diagnostic command and create an exclusive private report."""
    args = _parser().parse_args()
    try:
        if args.command == "inventory":
            result = inventory(args.workspace, args.identities, args.output, args.source_aliases)
        elif args.command == "draft":
            result = draft_plan(
                args.workspace, args.reference_seal, args.output, args.source_aliases
            )
        else:
            result = score_private(
                args.workspace, args.reference_seal, args.input, args.output, args.source_aliases
            )
    except (OSError, ValueError, TypeError, ValidationError) as error:
        sys.stderr.write(f"temporal scoring inputs rejected: {error}\n")
        return 1
    sys.stdout.write(f"created {result}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
