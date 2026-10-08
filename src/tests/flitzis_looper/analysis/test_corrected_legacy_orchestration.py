"""Genuine synthetic seals precede every comparator read; fixtures certify no music."""

import json
from dataclasses import replace
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis import beat_scoring_cli as cli
from flitzis_looper.analysis import reference_inputs_validation as validation
from flitzis_looper.analysis.corrected_legacy_models import (
    CorrectedLegacyCandidate,
    decode_corrected_legacy,
)
from tests.flitzis_looper.analysis import test_beat_scoring_cli as fixtures
from tests.flitzis_looper.analysis.test_corrected_legacy_models import encoded, legacy_fixture

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.analysis.reference_inputs_models import (
        ReferenceBundle,
        ReferenceSeal,
        ReferenceTrack,
    )
    from flitzis_looper.analysis.reference_source_aliases import SourcePathAlias
    from tests.flitzis_looper.analysis.test_beat_scoring_cli import Sealed

prepared = fixtures.prepared
sealed = fixtures.sealed


def _setup(sealed: Sealed, monkeypatch: pytest.MonkeyPatch, *, selected: bool = True) -> Path:
    plan = fixtures._plan(sealed, None if selected else [])
    data = json.loads(plan.read_bytes())
    data["comparators"] = [{"track_id": "T01", "profile_id": "synthetic-qm-T01"}]
    plan.write_text(json.dumps(data), encoding="utf8")
    monkeypatch.setattr(cli, "available_corrected_legacy_profiles", lambda: data["comparators"])
    return plan


def _unexpected(*args: object, **kwargs: object) -> CorrectedLegacyCandidate:
    pytest.fail("comparator read before its actual validated prerequisites")


def test_missing_reference_blocks_selected_and_comparator_reads(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch
) -> None:
    _setup(sealed, monkeypatch)
    monkeypatch.setattr(cli, "load_corrected_legacy", _unexpected)
    fixtures._reject_candidate_read(monkeypatch)
    sealed.seal_path.unlink()
    report = json.loads(fixtures._score(sealed).read_bytes())
    assert report["candidate_artifacts_read"] is False
    assert report["temporal_scores"] == []


def test_changed_sealed_reference_rejects_before_comparator_access(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch
) -> None:
    _setup(sealed, monkeypatch)
    monkeypatch.setattr(cli, "load_corrected_legacy", _unexpected)
    fixtures._reject_candidate_read(monkeypatch)
    with (sealed.workspace / "reference.json").open("ab") as handle:
        handle.write(b" ")
    with pytest.raises(ValueError, match="reference"):
        fixtures._score(sealed)


@pytest.mark.parametrize(
    "change", ["duplicate", "wrong_track", "unsupported", "candidate_as_comparator"]
)
def test_all_comparator_selections_validate_before_any_artifact_read(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch, change: str
) -> None:
    plan = _setup(sealed, monkeypatch)
    # Freeze the supported list independently of subsequent caller mutations.
    monkeypatch.setattr(
        cli,
        "available_corrected_legacy_profiles",
        lambda: [{"track_id": "T01", "profile_id": "synthetic-qm-T01"}],
    )
    data = json.loads(plan.read_bytes())
    if change == "duplicate":
        data["comparators"].append(data["comparators"][0])
    else:
        data["comparators"][0]["profile_id"] = (
            "synthetic-T01" if change == "candidate_as_comparator" else "unsupported"
        )
        if change == "wrong_track":
            data["comparators"][0] = {"track_id": "T02", "profile_id": "synthetic-qm-T01"}
    plan.write_text(json.dumps(data), encoding="utf8")
    monkeypatch.setattr(cli, "load_corrected_legacy", _unexpected)
    fixtures._reject_candidate_read(monkeypatch)
    with pytest.raises(ValueError, match=r"comparator|legacy"):
        fixtures._score(sealed)


def test_missing_selected_candidate_does_not_open_comparator(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch
) -> None:
    _setup(sealed, monkeypatch, selected=False)
    monkeypatch.setattr(cli, "load_corrected_legacy", _unexpected)
    report = json.loads(fixtures._score(sealed).read_bytes())
    assert {"track_id": "T01", "input": "selected_candidate_for_comparator_missing"} in report[
        "missing_inputs"
    ]
    assert report["temporal_scores"] == []


def test_missing_comparator_file_is_explicit_and_preserves_selected_scores(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch
) -> None:
    _setup(sealed, monkeypatch)
    monkeypatch.setattr(cli, "load_candidate", fixtures._candidate)

    def missing(*args: object) -> CorrectedLegacyCandidate:
        raise FileNotFoundError(2, "missing synthetic comparator", "missing-qm.json")

    monkeypatch.setattr(cli, "load_corrected_legacy", missing)
    report = json.loads(fixtures._score(sealed).read_bytes())
    assert len(report["temporal_scores"]) == 5
    assert report["status"] == "incomplete_temporal_diagnostics"
    assert report["missing_inputs"] == [
        {"track_id": "T01", "input": "corrected_legacy_artifact", "path": "missing-qm.json"}
    ]


def test_complete_comparator_uses_same_sealed_reference_and_full_raw_arrays(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch
) -> None:
    _setup(sealed, monkeypatch)
    order = []
    actual_seal = validation.reference_seal

    def seal(
        workspace: Path, path: str, *, source_aliases: tuple[SourcePathAlias, ...] = ()
    ) -> tuple[ReferenceSeal, ReferenceBundle, str]:
        result = actual_seal(workspace, path, source_aliases=source_aliases)
        order.append("validated_reference")
        return result

    monkeypatch.setattr(cli, "reference_seal", seal)
    monkeypatch.setattr(cli, "load_candidate", fixtures._candidate)

    def load(workspace: Path, track: ReferenceTrack, profile: str) -> CorrectedLegacyCandidate:
        assert order == ["validated_reference"]
        order.append("comparator")
        selected = fixtures._candidate(workspace, track, "synthetic-T01")
        # Simulate the strict reader's typed return. Separate reader tests verify
        # actual pinned source/PCM/native producer packets without monkeypatching.
        result = decode_corrected_legacy(encoded(legacy_fixture()))
        loaded = result.envelope.loaded.model_copy(
            update={
                "sample_rate_hz": track.identity.pcm.sample_rate_hz,
                "frame_count": track.identity.pcm.frame_count,
                "mono_sha256": track.identity.pcm.sha256,
            }
        )
        result = replace(result, envelope=result.envelope.model_copy(update={"loaded": loaded}))
        return CorrectedLegacyCandidate(
            profile, "T01", selected.source_sha256, selected.source_bytes, result, ()
        )

    monkeypatch.setattr(cli, "load_corrected_legacy", load)
    report = json.loads(fixtures._score(sealed).read_bytes())
    corrected = report["temporal_scores"][0]["corrected_legacy"]
    assert order == ["validated_reference", "comparator"]
    assert len(corrected["comparator"]["arrays"]["beat_frames"]) == 5
    assert corrected["temporal"]["beats"]["prediction_event_count"] == 5
    assert corrected["engineering"]["musical_scores"] == "not_run"
    assert report["musical_acceptance"] == "pending"
    assert report["default_adoption"] == "blocked"


def test_draft_comparator_opt_in_never_changes_selected_backend(
    sealed: Sealed, monkeypatch: pytest.MonkeyPatch
) -> None:
    profiles = [{"track_id": "T01", "profile_id": "synthetic-qm-T01"}]
    monkeypatch.setattr(cli, "available_corrected_legacy_profiles", lambda: profiles)
    monkeypatch.setattr(cli, "load_corrected_legacy", _unexpected)
    plain = json.loads(
        cli.draft_plan(sealed.workspace, sealed.seal_path.name, "plain.json", None).read_bytes()
    )
    opted = json.loads(
        cli.draft_plan(
            sealed.workspace,
            sealed.seal_path.name,
            "opted.json",
            None,
            include_corrected_legacy=True,
        ).read_bytes()
    )
    assert plain["comparators"] == []
    assert opted["comparators"] == profiles
    assert plain["candidates"] == opted["candidates"] == fixtures._selections()
