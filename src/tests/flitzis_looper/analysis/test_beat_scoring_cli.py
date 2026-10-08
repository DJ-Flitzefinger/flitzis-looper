"""Reference-first private orchestration with explicitly synthetic sealed fixtures."""

import errno
import hashlib
import json
import struct
import sys
from dataclasses import asdict, dataclass
from typing import TYPE_CHECKING

import pytest
from pydantic import TypeAdapter, ValidationError

from flitzis_looper.analysis import beat_scoring_cli as cli
from flitzis_looper.analysis import reference_inputs as reference_cli
from flitzis_looper.analysis import reference_inputs_validation as validation
from flitzis_looper.analysis.beat_candidate_models import ArtifactBinding, NativeCandidate
from flitzis_looper.analysis.beat_scoring_inputs import ScoringPlan
from flitzis_looper.analysis.contracts import (
    AnalysisIdentity,
    BeatModelIdentity,
    BeatPredictions,
    BeatWorkerRequest,
    MonoPcmInput,
)
from flitzis_looper.analysis.reference_inputs_models import TRACK_IDS
from tests.flitzis_looper.analysis import test_reference_inputs as reference_fixtures

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.analysis.reference_inputs_models import (
        ReferenceBundle,
        ReferenceSeal,
        ReferenceTrack,
    )
    from flitzis_looper.analysis.reference_source_aliases import SourcePathAlias
    from tests.flitzis_looper.analysis.test_reference_inputs import Prepared

prepared = reference_fixtures.prepared


def _selections() -> list[dict[str, str]]:
    return [{"track_id": track, "profile_id": f"synthetic-{track}"} for track in TRACK_IDS]


def test_draft_prefers_fresh_profiles_and_preserves_historical_tracks(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch
) -> None:
    """All eight supported attempts still produce one explicit selection per track."""
    supported = _selections() + [
        {"track_id": track, "profile_id": f"fresh-{track}"} for track in TRACK_IDS[:3]
    ]
    monkeypatch.setattr(cli, "available_candidate_profiles", lambda: supported)
    _reject_candidate_read(monkeypatch)
    path = cli.draft_plan(sealed.workspace, sealed.seal_path.name, "fresh-plan.json", None)
    plan = TypeAdapter(ScoringPlan).validate_json(path.read_bytes(), strict=True)
    assert [selection.track_id for selection in plan.candidates] == list(TRACK_IDS)
    assert [selection.profile_id for selection in plan.candidates] == [
        "fresh-T01",
        "fresh-T02",
        "fresh-T03",
        "synthetic-T04",
        "synthetic-T05",
    ]
    assert len(supported) == 8


@dataclass(frozen=True)
class Sealed:
    prepared: Prepared
    seal_path: Path
    digest: str

    @property
    def workspace(self) -> Path:
        return self.prepared.workspace


@pytest.fixture
def sealed(prepared: Prepared, monkeypatch: pytest.MonkeyPatch) -> Sealed:
    """Use real byte-bound validation; the declarations are synthetic test data only."""
    reference = prepared.workspace / "reference.json"
    reference.write_text(prepared.bundle.model_dump_json(), encoding="utf-8")
    receipt = reference_cli.seal_reference(prepared.workspace, reference.name, "seal.json")
    monkeypatch.setattr(cli, "available_candidate_profiles", _selections)
    return Sealed(prepared, receipt, hashlib.sha256(receipt.read_bytes()).hexdigest())


def _plan(sealed: Sealed, selections: list[dict[str, str]] | None = None) -> Path:
    path = sealed.workspace / "plan.json"
    path.write_text(
        json.dumps({
            "schema_version": 1,
            "status": "ready_for_temporal_scoring",
            "reference_seal_sha256": sealed.digest,
            "candidates": _selections() if selections is None else selections,
        }),
        encoding="utf-8",
    )
    return path


def _reject_candidate_read(monkeypatch: pytest.MonkeyPatch) -> None:
    def unexpected(workspace: Path, track: ReferenceTrack, profile_id: str) -> NativeCandidate:
        pytest.fail("orchestration read a candidate before rejecting its prerequisite")

    monkeypatch.setattr(cli, "load_candidate", unexpected)


def _reject_plan_read(monkeypatch: pytest.MonkeyPatch, path: Path) -> None:
    actual = validation.read_json_bytes

    def guarded(target: Path) -> bytes:
        assert target.resolve() != path.resolve(), "candidate plan read before reference rejection"
        return actual(target)

    monkeypatch.setattr(cli, "read_json_bytes", guarded)
    _reject_candidate_read(monkeypatch)


def _score(sealed: Sealed, output: str = "report.json") -> Path:
    return cli.score_private(sealed.workspace, sealed.seal_path.name, "plan.json", output)


@pytest.mark.parametrize(
    ("target", "reason"),
    [
        ("bundle", "sealed_reference_bundle_changed"),
        ("coverage", "reference_receipt_coverage_mismatch"),
        ("source", "original_source_hash_or_size_mismatch"),
        ("pcm_hash", "loaded_pcm_hash_mismatch"),
        ("pcm_extent", "complete_loaded_pcm_size_mismatch"),
        ("pcm_nonfinite", "pcm_must_be_finite_complete_float32_le"),
        ("inventory", "approved_loaded_identity_inventory_hash_mismatch"),
        ("manifest", "frozen_protocol_hash_mismatch"),
    ],
)
def test_mutated_reference_rejected_before_plan_or_candidate_read(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch, target: str, reason: str
) -> None:
    plan = _plan(sealed)
    _reject_plan_read(monkeypatch, plan)
    paths = {
        "bundle": "reference.json",
        "inventory": "identities.json",
        "manifest": "scratch/b2a/frozen-manifest.json",
        "source": "T01.source",
    }
    if target in paths:
        path = sealed.workspace / paths[target]
        path.write_bytes(path.read_bytes() + b" ")
    elif target == "coverage":
        data = json.loads(sealed.seal_path.read_bytes())
        data["coverage"][0]["reference_beats"] -= 1
        sealed.seal_path.write_text(json.dumps(data), encoding="utf-8")
    else:
        pcm = sealed.workspace / "T01.f32le"
        replacement = {
            "pcm_hash": struct.pack("<128f", *([0.5] * 128)),
            "pcm_extent": pcm.read_bytes()[:-4],
            "pcm_nonfinite": struct.pack("<128f", *([float("nan")] * 128)),
        }
        pcm.write_bytes(replacement[target])
    with pytest.raises(ValueError, match=reason):
        _score(sealed)
    assert not (sealed.workspace / "report.json").exists()


@pytest.mark.parametrize("missing", ["seal.json", "reference.json", "T01.source", "T01.f32le"])
def test_missing_reference_material_is_explicit_blocker_without_candidate_read(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch, missing: str
) -> None:
    plan = _plan(sealed)
    _reject_plan_read(monkeypatch, plan)
    (sealed.workspace / missing).unlink()
    report = json.loads(_score(sealed).read_bytes())
    assert report["status"] == "blocked_missing_reference_input"
    assert report["candidate_artifacts_read"] is False
    assert report["temporal_scores"] == []
    assert report["missing_inputs"][0]["path"] == str(sealed.workspace / missing)
    assert report["musical_acceptance"] == "pending"
    assert report["default_adoption"] == "blocked"


def test_missing_plan_is_separate_from_validated_reference(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch
) -> None:
    _reject_candidate_read(monkeypatch)
    report = json.loads(_score(sealed).read_bytes())
    assert report["status"] == "blocked_missing_candidate_plan"
    assert report["reference"]["seal_sha256"] == sealed.digest
    assert report["candidate_artifacts_read"] is False
    assert report["temporal_scores"] == []
    assert report["missing_inputs"] == [
        {
            "input": "candidate_plan",
            "path": str(sealed.workspace / "plan.json"),
        }
    ]


def test_empty_selection_preserves_all_five_missing_candidate_inputs(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch
) -> None:
    _plan(sealed, [])
    _reject_candidate_read(monkeypatch)
    report = json.loads(_score(sealed).read_bytes())
    assert report["status"] == "incomplete_temporal_diagnostics"
    assert report["candidate_artifacts_read"] is False
    assert report["temporal_scores"] == []
    assert report["missing_inputs"] == [
        {"track_id": track, "input": "native_candidate_not_selected"} for track in TRACK_IDS
    ]


@pytest.mark.parametrize(
    ("mutation", "reason"),
    [
        ("seal", "candidate_plan_reference_seal_mismatch"),
        ("duplicate", "duplicate_candidate_track"),
        ("unknown", "unsupported_candidate_lineage"),
        ("wrong_track", "candidate_profile_track_mismatch"),
    ],
)
def test_invalid_plan_bindings_reject_without_candidate_read(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch, mutation: str, reason: str
) -> None:
    path = _plan(sealed)
    data = json.loads(path.read_bytes())
    if mutation == "seal":
        data["reference_seal_sha256"] = "f" * 64
    elif mutation == "duplicate":
        data["candidates"] = [data["candidates"][0], data["candidates"][0]]
    elif mutation == "unknown":
        data["candidates"][0]["profile_id"] = "unsupported-lineage"
    else:
        data["candidates"][0]["profile_id"] = "synthetic-T02"
    path.write_text(json.dumps(data), encoding="utf-8")
    _reject_candidate_read(monkeypatch)
    with pytest.raises(ValueError, match=reason):
        _score(sealed)
    assert not (sealed.workspace / "report.json").exists()


@pytest.mark.parametrize("target", ["seal.json", "plan.json"])
def test_duplicate_json_keys_are_rejected_without_output(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch, target: str
) -> None:
    plan = _plan(sealed)
    path = sealed.workspace / target
    original = path.read_bytes()
    path.write_bytes(b'{"schema_version":1,' + original[1:])
    _reject_candidate_read(monkeypatch)
    if target == "seal.json":
        _reject_plan_read(monkeypatch, plan)
    with pytest.raises(ValueError, match="duplicate_json_key"):
        _score(sealed)
    assert not (sealed.workspace / "report.json").exists()


@pytest.mark.parametrize(
    ("field", "value"),
    [("schema_version", True), ("schema_version", "1"), ("untrusted_checksum", "a" * 64)],
)
def test_plan_strict_schema_rejects_coercion_and_extra_trust_fields(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch, field: str, value: object
) -> None:
    path = _plan(sealed)
    data = json.loads(path.read_bytes())
    data[field] = value
    path.write_text(json.dumps(data), encoding="utf-8")
    _reject_candidate_read(monkeypatch)
    with pytest.raises(ValidationError):
        _score(sealed)
    assert not (sealed.workspace / "report.json").exists()


def _candidate(workspace: Path, track: ReferenceTrack, profile_id: str) -> NativeCandidate:
    """Replace the reader only; use its real immutable DTO and complete raw arrays."""
    identity = track.identity
    times = tuple(beat.seconds + 0.003 for beat in track.beats)
    predictions = BeatPredictions(
        beat_seconds=(*times, 15.997),
        downbeat_seconds=(*times[::4], 15.997),
        beat_logits=tuple(index * 0.123 - 48.5 for index in range(800)),
        downbeat_logits=tuple(70.125 - index * 0.375 for index in range(800)),
    )
    request = BeatWorkerRequest(
        AnalysisIdentity(0, 1, f"synthetic-source-{identity.track_id}", 1),
        MonoPcmInput(
            workspace / identity.pcm.path,
            identity.pcm.sample_rate_hz,
            identity.pcm.frame_count,
        ),
        BeatModelIdentity(sha256="a" * 64, frontend_id="synthetic", environment_id="synthetic"),
    )
    bindings = []
    for role in ("raw_worker", "final_envelope", "excluded_failed_attempt"):
        path = workspace / f"{identity.track_id}-{role}.json"
        raw = json.dumps({"synthetic": role, "predictions": asdict(predictions)}).encode()
        path.write_bytes(raw)
        bindings.append(ArtifactBinding(path.name, role, hashlib.sha256(raw).hexdigest(), len(raw)))
    source = workspace / f"{identity.track_id}.source"
    return NativeCandidate(
        profile_id=profile_id,
        track_id=identity.track_id,
        lineage_kind="verified_historical_summary",
        historical_source_path=source.name,
        source_sha256=identity.source_sha256,
        source_bytes=source.stat().st_size,
        pcm_sha256=identity.pcm.sha256,
        native_channels=2,
        request=request,
        predictions=predictions,
        artifact_bindings=tuple(bindings[:2]),
        excluded_historical_attempts=tuple(bindings[2:]),
    )


def test_complete_scores_preserve_raw_arrays_and_all_acceptance_boundaries(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch
) -> None:
    plan = _plan(sealed)
    ordered: list[str] = []
    candidates: list[NativeCandidate] = []
    actual_reference = validation.reference_seal
    actual_read = validation.read_json_bytes

    def reference(
        workspace: Path, path: str, *, source_aliases: tuple[SourcePathAlias, ...] = ()
    ) -> tuple[ReferenceSeal, ReferenceBundle, str]:
        result = actual_reference(workspace, path, source_aliases=source_aliases)
        ordered.append("reference_validated")
        return result

    def read(path: Path) -> bytes:
        if path == plan:
            assert ordered == ["reference_validated"]
            ordered.append("plan_read")
        return actual_read(path)

    def load(workspace: Path, track: ReferenceTrack, profile: str) -> NativeCandidate:
        assert ordered[:2] == ["reference_validated", "plan_read"]
        ordered.append(track.identity.track_id)
        result = _candidate(workspace, track, profile)
        candidates.append(result)
        return result

    monkeypatch.setattr(cli, "reference_seal", reference)
    monkeypatch.setattr(cli, "read_json_bytes", read)
    monkeypatch.setattr(cli, "load_candidate", load)
    report = json.loads(_score(sealed).read_bytes())
    assert ordered == ["reference_validated", "plan_read", *TRACK_IDS]
    assert report["status"] == "complete_temporal_diagnostics"
    assert report["missing_inputs"] == []
    assert report["candidate_artifacts_read"] is True
    assert report["musical_acceptance"] == "pending"
    assert report["default_adoption"] == "blocked"
    assert report["human_declarations"] == "supplied_only_not_verified_as_truth"
    assert report["reference"]["seal_sha256"] == sealed.digest
    assert report["candidate_plan"]["sha256"] == hashlib.sha256(plan.read_bytes()).hexdigest()
    assert len(report["temporal_scores"]) == 5
    for candidate, scored in zip(candidates, report["temporal_scores"], strict=True):
        raw_candidate = scored["candidate"]
        assert raw_candidate == json.loads(json.dumps(candidate.report()))
        assert raw_candidate["predictions"] == json.loads(json.dumps(asdict(candidate.predictions)))
        temporal = scored["temporal"]
        assert temporal["input_certification"] == "unchecked_by_metric_core"
        assert temporal["musical_acceptance"] == "pending"
        assert temporal["default_adoption"] == "blocked"
        assert temporal["quarter_count_and_bar_identity"] == "pending"
        assert temporal["paired_correction_burden"] == "pending"
        assert temporal["beats"]["prediction_event_count"] == 33
        assert temporal["downbeats"]["prediction_event_count"] == 9
        assert temporal["beats"]["tolerances"][-1]["extra_prediction_indices"] == [32]


def test_partial_selection_and_missing_artifact_keep_distinct_input_gaps(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch
) -> None:
    _plan(sealed, _selections()[:2])
    loaded: list[str] = []

    def load(workspace: Path, track: ReferenceTrack, profile: str) -> NativeCandidate:
        loaded.append(track.identity.track_id)
        if track.identity.track_id == "T02":
            raise FileNotFoundError(errno.ENOENT, "missing synthetic worker", "T02-worker.json")
        return _candidate(workspace, track, profile)

    monkeypatch.setattr(cli, "load_candidate", load)
    report = json.loads(_score(sealed).read_bytes())
    assert loaded == ["T01", "T02"]
    assert report["status"] == "incomplete_temporal_diagnostics"
    assert len(report["temporal_scores"]) == 1
    assert report["missing_inputs"] == [
        {"track_id": "T02", "input": "native_candidate_artifact", "path": "T02-worker.json"},
        *[{"track_id": track, "input": "native_candidate_not_selected"} for track in TRACK_IDS[2:]],
    ]
    assert report["default_adoption"] == "blocked"


@pytest.mark.parametrize("output", ["../outside.json", "repo/private.json"])
def test_reports_cannot_leave_private_workspace(sealed: Sealed, output: str) -> None:
    _plan(sealed, [])
    with pytest.raises(ValueError, match="private_path_outside_workspace_or_inside_repo"):
        _score(sealed, output)


def test_report_never_overwrites_retained_evidence(sealed: Sealed) -> None:
    _plan(sealed, [])
    path = sealed.workspace / "report.json"
    original = b"original failed attempt must remain"
    path.write_bytes(original)
    with pytest.raises(FileExistsError):
        _score(sealed)
    assert path.read_bytes() == original


def _aliases(sealed: Sealed) -> Path:
    aliases = []
    for track, renamed in (("T01", "renamed-1.mp3"), ("T02", "renamed-2.wav")):
        original = f"{track}.source"
        (sealed.workspace / original).rename(sealed.workspace / renamed)
        aliases.append({
            "track_id": track,
            "original_source_relative": original,
            "actual_source_path": renamed,
        })
    path = sealed.workspace / "aliases.json"
    path.write_text(
        json.dumps({
            "schema_version": 1,
            "manifest_sha256": sealed.prepared.bundle.manifest_sha256,
            "aliases": aliases,
        }),
        encoding="utf-8",
    )
    return path


def test_explicit_content_aliases_preserve_original_seal_and_manifest(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch
) -> None:
    manifest = sealed.workspace / "scratch/b2a/frozen-manifest.json"
    original_manifest = manifest.read_bytes()
    original_bundle = (sealed.workspace / "reference.json").read_bytes()
    aliases = _aliases(sealed)
    _plan(sealed, [])
    _reject_candidate_read(monkeypatch)
    path = cli.score_private(
        sealed.workspace, sealed.seal_path.name, "plan.json", "aliased-report.json", aliases.name
    )
    report = json.loads(path.read_bytes())
    assert report["status"] == "incomplete_temporal_diagnostics"
    assert report["reference"]["seal_sha256"] == sealed.digest
    assert report["source_aliases"]["sha256"] == hashlib.sha256(aliases.read_bytes()).hexdigest()
    assert report["source_aliases"]["bytes"] == aliases.stat().st_size
    assert report["source_aliases"]["aliases"] == json.loads(aliases.read_bytes())["aliases"]
    assert manifest.read_bytes() == original_manifest
    assert (sealed.workspace / "reference.json").read_bytes() == original_bundle
    assert hashlib.sha256(sealed.seal_path.read_bytes()).hexdigest() == sealed.digest
    assert all(not (sealed.workspace / f"{track}.source").exists() for track in TRACK_IDS[:2])


def test_mutated_alias_material_rejects_before_plan_read(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch
) -> None:
    aliases = _aliases(sealed)
    plan = _plan(sealed)
    _reject_plan_read(monkeypatch, plan)
    (sealed.workspace / "renamed-1.mp3").write_bytes(b"different synthetic bytes")
    with pytest.raises(ValueError, match="source_alias_hash_or_size_mismatch"):
        cli.score_private(
            sealed.workspace, sealed.seal_path.name, "plan.json", "report.json", aliases.name
        )
    assert not (sealed.workspace / "report.json").exists()


@pytest.mark.parametrize("missing", ["aliases.json", "renamed-1.mp3"])
@pytest.mark.parametrize("command", ["score", "inventory"])
def test_missing_alias_file_or_target_reports_the_actual_gap_before_candidate_reads(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch, missing: str, command: str
) -> None:
    aliases = _aliases(sealed)
    plan = _plan(sealed)
    _reject_plan_read(monkeypatch, plan)
    (sealed.workspace / missing).unlink()
    if command == "score":
        path = cli.score_private(
            sealed.workspace, sealed.seal_path.name, "plan.json", "report.json", aliases.name
        )
        expected_status = "blocked_missing_reference_input"
        expected_input = "sealed_reference_material_or_alias"
    else:
        path = cli.inventory(sealed.workspace, "identities.json", "report.json", aliases.name)
        expected_status = "blocked_missing_inventory_input"
        expected_input = "protocol_inventory_source_or_alias"
    report = json.loads(path.read_bytes())
    assert report["status"] == expected_status
    assert report["missing_inputs"] == [
        {
            "input": expected_input,
            "path": str(sealed.workspace / missing),
        }
    ]
    assert report["candidate_artifacts_read"] is False
    assert report["temporal_scores"] == []
    assert report["musical_acceptance"] == "pending"
    assert report["default_adoption"] == "blocked"


def test_alias_report_binds_the_single_parsed_byte_snapshot(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch
) -> None:
    aliases = _aliases(sealed)
    original = aliases.read_bytes()
    replacement = json.loads(original)
    replacement["aliases"].reverse()
    changed = json.dumps(replacement, indent=2).encode()
    _plan(sealed, [])
    parsed: list[bytes] = []
    alias_reads: list[Path] = []

    def read(path: Path) -> bytes:
        if path == aliases:
            alias_reads.append(path)
        return validation.read_json_bytes(path)

    def parse(workspace: Path, raw: bytes) -> tuple[SourcePathAlias, ...]:
        parsed.append(raw)
        aliases.write_bytes(changed)
        return validation.parse_source_aliases(workspace, raw)

    _reject_candidate_read(monkeypatch)
    monkeypatch.setattr(cli, "read_json_bytes", read)
    monkeypatch.setattr(cli, "parse_source_aliases", parse)
    path = cli.score_private(
        sealed.workspace, sealed.seal_path.name, "plan.json", "report.json", aliases.name
    )
    report = json.loads(path.read_bytes())
    assert parsed == [original]
    assert alias_reads == [aliases]
    assert aliases.read_bytes() == changed != original
    assert report["source_aliases"] == {
        "path": aliases.name,
        "sha256": hashlib.sha256(original).hexdigest(),
        "bytes": len(original),
        "aliases": json.loads(original)["aliases"],
    }
    assert report["status"] == "incomplete_temporal_diagnostics"
    assert report["reference"]["seal_sha256"] == sealed.digest


def test_draft_revalidates_seal_and_creates_a_strict_plan(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch
) -> None:
    _reject_candidate_read(monkeypatch)
    output = cli.draft_plan(sealed.workspace, sealed.seal_path.name, "draft.json", None)
    plan = TypeAdapter(ScoringPlan).validate_json(output.read_bytes(), strict=True)
    assert plan.reference_seal_sha256 == sealed.digest
    assert [selection.model_dump() for selection in plan.candidates] == _selections()
    reference = sealed.workspace / "reference.json"
    reference.write_bytes(reference.read_bytes() + b" ")
    with pytest.raises(ValueError, match="sealed_reference_bundle_changed"):
        cli.draft_plan(sealed.workspace, sealed.seal_path.name, "invalid-draft.json", None)
    assert not (sealed.workspace / "invalid-draft.json").exists()


@pytest.mark.parametrize("missing", ["T01.source", "T01.f32le"])
def test_inventory_never_opens_candidates_and_keeps_missing_human_inputs(
    prepared: Prepared, monkeypatch: pytest.MonkeyPatch, missing: str
) -> None:
    _reject_candidate_read(monkeypatch)
    (prepared.workspace / missing).unlink()
    path = cli.inventory(prepared.workspace, "identities.json", "inventory-report.json", None)
    report = json.loads(path.read_bytes())
    assert report["status"] == "input_inventory_only"
    assert report["candidate_artifacts_read"] is False
    assert report["material"][0]["status"] == "missing"
    assert all(row["status"] == "verified" for row in report["material"][1:])
    assert report["missing_inputs"] == [
        {"track_id": track, "input": "independent_complete_sealed_reference_not_supplied"}
        for track in TRACK_IDS
    ] + [{"track_id": "T01", "input": "source_or_pcm", "path": str(prepared.workspace / missing)}]
    assert report["temporal_scores"] == []


def test_cli_errors_return_failure_without_output(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    _plan(sealed)
    (sealed.workspace / "T01.source").write_bytes(b"changed")
    _reject_candidate_read(monkeypatch)
    monkeypatch.setattr(
        sys,
        "argv",
        [
            "beat_scoring_cli",
            "score",
            "--workspace",
            str(sealed.workspace),
            "--reference-seal",
            sealed.seal_path.name,
            "--input",
            "plan.json",
            "--output",
            "report.json",
        ],
    )
    assert cli.main() == 1
    captured = capsys.readouterr()
    assert not captured.out
    assert "original_source_hash_or_size_mismatch" in captured.err
    assert not (sealed.workspace / "report.json").exists()
