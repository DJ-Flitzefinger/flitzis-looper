"""Control ownership wiring; actual file identities/hash/last-reader proofs are Native tests."""

import json
from pathlib import Path
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.copy_arrangement_custody import (
    CopyJournalCustody,
    journal_project_images,
    prepare_recovery_map_publication,
)
from flitzis_looper.models import ProjectState
from flitzis_looper.source_beatmap_intent import (
    SourceBeatmapReference,
    next_source_beatmap_intent,
)
from tests.flitzis_looper.conftest import FakeProjectAssetLease
from tests.flitzis_looper.test_source_beatmap_intent import _reference

if TYPE_CHECKING:
    from flitzis_looper_audio import AudioEngine


class _MapLease(FakeProjectAssetLease):
    def __init__(self, path: str) -> None:
        super().__init__(path)
        self.retirement_error: RuntimeError | None = None
        self.acknowledgement_error: RuntimeError | None = None

    def bind_source_beatmap(self, receipt_json: str) -> None:
        if not Path(self.path).is_file():
            message = "historical map leaf is missing"
            raise RuntimeError(message)
        super().bind_source_beatmap(receipt_json)

    def retire_source_beatmap(self) -> None:
        super().retire_source_beatmap()
        if self.retirement_error is not None:
            raise self.retirement_error

    def acknowledge_source_beatmap(self) -> None:
        if self.acknowledgement_error is not None:
            raise self.acknowledgement_error
        super().acknowledge_source_beatmap()


def _environment(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> tuple[ProjectState, ProjectAssetLifecycle, Mock, list[_MapLease]]:
    monkeypatch.chdir(tmp_path)
    (tmp_path / "samples").mkdir()
    project = ProjectState()
    leases: list[_MapLease] = []

    def acquire(path: str) -> _MapLease:
        lease = _MapLease(path)
        leases.append(lease)
        return lease

    audio = Mock()
    audio.acquire_project_asset_lease.side_effect = acquire
    assets = ProjectAssetLifecycle(project, cast("AudioEngine", audio))
    # Drive the actual bounded retry method deterministically on this control fixture.
    monkeypatch.setattr(assets, "_start_retry_worker_locked", lambda: None)
    return project, assets, audio, leases


def _backing(tmp_path: Path, generation: str = "a") -> SourceBeatmapReference:
    original = _reference("samples")
    reference = SourceBeatmapReference.model_validate(
        original.model_dump() | {"reference": original.reference.replace("a" * 32, generation * 32)}
    )
    path = tmp_path / reference.reference
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(b"control fixture only; Native validates actual bytes separately")
    return reference


def _selected(reference: SourceBeatmapReference, sample_id: int = 0) -> ProjectState:
    project = ProjectState()
    project.pad_source_beatmap_intent[sample_id] = next_source_beatmap_intent(None, reference)
    return project


def test_candidate_receipt_is_bound_before_commit_and_acknowledged_only_when_published(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    project, assets, audio, leases = _environment(tmp_path, monkeypatch)
    reference = _backing(tmp_path)
    candidate = _selected(reference)
    image = assets.prepare_assignment_image(candidate)
    plan = assets.prepare_assignment_publication(image)
    assert project.pad_source_beatmap_intent[0] is None
    assert leases[0].source_beatmap_receipt == reference.model_dump()
    assert leases[0].source_beatmap_acknowledgements == 0
    assert not assets._assignments
    plan.apply()
    assert assets._assignments["source_beatmap", 0][1] is leases[0]
    assert leases[0].source_beatmap_acknowledgements == 1
    assert plan.settled
    audio.retire_project_asset.assert_not_called()


def test_transient_sync_and_history_pin_do_not_cancel_queued_proof(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    project, assets, _, leases = _environment(tmp_path, monkeypatch)
    reference = _backing(tmp_path)
    path = tmp_path / reference.reference
    queued = _MapLease(str(path))
    queued.bind_source_beatmap(reference.model_dump_json())
    assets._pending[path, False] = [queued]
    project.pad_source_beatmap_intent[0] = next_source_beatmap_intent(None, reference)
    assets.sync_assignments()
    history = assets._acquire_map(reference)
    assert (path, False) in assets._pending
    assert not queued.released
    assert all(lease.source_beatmap_acknowledgements == 0 for lease in leases)
    assets.publish_assignment_image(assets.prepare_assignment_image(project))
    assert (path, False) not in assets._pending
    assert queued.released
    assert leases[0].source_beatmap_acknowledgements == 1
    assert leases[1].source_beatmap_acknowledgements == 0
    history.release()


def test_published_plan_retries_current_ack_failure_but_skips_owner_removed_by_unload(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _, assets, _, leases = _environment(tmp_path, monkeypatch)
    candidate = _selected(_backing(tmp_path))
    plan = assets.prepare_assignment_publication(assets.prepare_assignment_image(candidate))
    owner = leases[0]
    owner.acknowledgement_error = RuntimeError("saved map acknowledgement unavailable")
    with pytest.raises(RuntimeError, match="acknowledgement unavailable"):
        plan.apply()
    assert plan.applied
    assert not plan.settled
    with pytest.raises(RuntimeError, match="acknowledgement unavailable"):
        plan.apply()
    assets.sync_assignments()
    assert owner.released
    assert not assets._assignments
    plan.apply()
    assert plan.settled
    assert owner.source_beatmap_acknowledgements == 0


def test_failed_candidate_releases_unpublished_readers_without_saved_ack(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _, assets, _, leases = _environment(tmp_path, monkeypatch)
    candidate = _selected(_backing(tmp_path))
    image = assets.prepare_assignment_image(candidate)
    assets.release_assignment_image(image)
    assert leases[0].released
    assert leases[0].source_beatmap_acknowledgements == 0
    assert not assets._assignments


def test_missing_historical_map_does_not_block_source_scalars_or_unrelated_publication(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    project, assets, audio, _ = _environment(tmp_path, monkeypatch)
    reference = _backing(tmp_path)
    (tmp_path / reference.reference).unlink()
    source = tmp_path / "samples" / "source.wav"
    source.write_bytes(b"source retention fixture")
    project.sample_paths[0] = "samples/source.wav"
    project.manual_bpm[0] = 123.456789
    project.pad_timing_intent[0] = "manual"
    project.pad_source_beatmap_intent[0] = next_source_beatmap_intent(None, reference)
    assets.sync_assignments()
    historical = assets._assignments["source_beatmap", 0][1]
    assert not historical.source_beatmap_bound
    assert not historical.released
    candidate = project.model_copy(deep=True)
    candidate.volume = 0.25
    plan = assets.prepare_assignment_publication(assets.prepare_assignment_image(candidate))
    plan.apply()
    assert not historical.released
    assert project.sample_paths[0] == "samples/source.wav"
    assert project.manual_bpm[0] == 123.456789
    assert project.pad_timing_intent[0] == "manual"
    assert cast("_MapLease", historical).source_beatmap_acknowledgements == 0
    project.sample_paths[0] = None
    project.pad_source_beatmap_intent[0] = None
    assets.sync_assignments()
    assert historical.released
    assert cast("_MapLease", historical).source_beatmap_retirements == 0
    # Only the former original is eligible for the existing generic retirement.
    assert all(
        call.args[0] != str(tmp_path / reference.reference)
        for call in audio.retire_project_asset.call_args_list
    )


def test_missing_new_selection_is_strict_even_when_another_historical_map_is_missing(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    project, assets, _, _ = _environment(tmp_path, monkeypatch)
    old, new = _backing(tmp_path), _backing(tmp_path, "b")
    (tmp_path / old.reference).unlink()
    (tmp_path / new.reference).unlink()
    project.pad_source_beatmap_intent[0] = next_source_beatmap_intent(None, old)
    assets.sync_assignments()
    before = assets._assignments.copy()
    candidate = project.model_copy(deep=True)
    candidate.pad_source_beatmap_intent[1] = next_source_beatmap_intent(None, new)
    with pytest.raises(RuntimeError, match="historical map leaf is missing"):
        assets.prepare_assignment_image(candidate)
    assert assets._assignments == before
    assert all(not lease.released for _, lease in before.values())


def test_returned_backing_upgrades_unbound_historical_owner_before_saved_ack(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    project, assets, _, _ = _environment(tmp_path, monkeypatch)
    reference = _backing(tmp_path)
    path = tmp_path / reference.reference
    path.unlink()
    project.pad_source_beatmap_intent[0] = next_source_beatmap_intent(None, reference)
    assets.sync_assignments()
    old = assets._assignments["source_beatmap", 0][1]
    assert not old.source_beatmap_bound
    path.write_bytes(b"returned backing control fixture")
    assets.sync_assignments()
    new = assets._assignments["source_beatmap", 0][1]
    assert new is not old
    assert new.source_beatmap_bound
    assert old.released
    assert cast("_MapLease", new).source_beatmap_acknowledgements == 0
    assets.publish_assignment_image(assets.prepare_assignment_image(project))
    assert cast("_MapLease", new).source_beatmap_acknowledgements == 1


def test_unopenable_historical_leaf_uses_safe_generation_retention_only(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    project, assets, _, _ = _environment(tmp_path, monkeypatch)
    reference = _backing(tmp_path)
    path = tmp_path / reference.reference
    acquire = assets.acquire

    def fail_leaf(target: Path) -> FakeProjectAssetLease:
        if target == path:
            message = "leaf metadata sharing unavailable"
            raise OSError(message)
        return cast("FakeProjectAssetLease", acquire(target))

    monkeypatch.setattr(assets, "acquire", fail_leaf)
    project.pad_source_beatmap_intent[0] = next_source_beatmap_intent(None, reference)
    assets.sync_assignments()
    retained = assets._assignments["source_beatmap", 0][1]
    assert cast("_MapLease", retained).path == str(path.parent)
    assert not retained.source_beatmap_bound
    assert not retained.released
    assets.publish_assignment_image(assets.prepare_assignment_image(project))
    assert cast("_MapLease", retained).source_beatmap_acknowledgements == 0


def test_unsafe_historical_ancestor_omits_only_unadmitted_map_owner_and_new_receipts_stay_strict(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    project, assets, audio, _ = _environment(tmp_path, monkeypatch)
    reference = _backing(tmp_path)
    project.pad_source_beatmap_intent[0] = next_source_beatmap_intent(None, reference)
    project.sample_paths[0] = "samples/source.wav"
    project.manual_bpm[0] = 123.456789
    (tmp_path / "samples" / "source.wav").write_bytes(b"source remains independent")
    acquire = assets.acquire

    def refuse_map_ancestor(target: Path) -> FakeProjectAssetLease:
        if target.name == "map.json" or target.name.startswith(".ready-"):
            message = "unsafe/reparse map ancestor refused"
            raise ValueError(message)
        return cast("FakeProjectAssetLease", acquire(target))

    monkeypatch.setattr(assets, "acquire", refuse_map_ancestor)
    assets.sync_assignments()
    assert ("original", 0) in assets._assignments
    assert ("source_beatmap", 0) not in assets._assignments
    assert project.pad_source_beatmap_intent[0].selected == reference
    assert project.sample_paths[0] == "samples/source.wav"
    assert project.manual_bpm[0] == 123.456789
    error = assets.retirement_errors[tmp_path / reference.reference]
    assert "Saved map retention unavailable" in error
    audio.retire_project_asset.assert_not_called()
    candidate = project.model_copy(deep=True)
    candidate.pad_source_beatmap_intent[1] = next_source_beatmap_intent(
        None, _backing(tmp_path, "b")
    )
    with pytest.raises(ValueError, match="unsafe/reparse map ancestor refused"):
        assets.prepare_assignment_image(candidate)


def test_bound_retirement_full_queue_retains_owner_until_admission_retry(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    project, assets, audio, leases = _environment(tmp_path, monkeypatch)
    reference = _backing(tmp_path)
    project.pad_source_beatmap_intent[0] = next_source_beatmap_intent(None, reference)
    assets.sync_assignments()
    owner = leases[0]
    owner.retirement_error = RuntimeError("source map retirement queue full")
    project.pad_source_beatmap_intent[0] = None
    assets.sync_assignments()
    assert not owner.released
    assert owner.source_beatmap_retirements == 1
    assert assets._pending
    owner.retirement_error = None
    assets.retry_retirements()
    assert owner.released
    assert owner.source_beatmap_retirements == 2
    assert not assets._pending
    audio.retire_project_asset.assert_not_called()


def test_unbound_generic_map_retirement_fails_closed_and_keeps_backing(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _, assets, audio, _ = _environment(tmp_path, monkeypatch)
    reference = _backing(tmp_path)
    path = tmp_path / reference.reference
    assets.retire(path)
    assert "exact bound artifact lease" in assets.retirement_errors[path]
    assert path.is_file()
    audio.retire_project_asset.assert_not_called()


def test_same_path_changed_receipt_cannot_replace_prepared_original_binding(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    project, assets, _, leases = _environment(tmp_path, monkeypatch)
    reference = _backing(tmp_path)
    project.pad_source_beatmap_intent[0] = next_source_beatmap_intent(None, reference)
    assets.sync_assignments()
    changed = SourceBeatmapReference.model_validate(reference.model_dump() | {"sha256": "e" * 64})
    candidate = _selected(changed)
    before = assets._assignments.copy()
    with pytest.raises(ValueError, match="different immutable receipt"):
        assets.prepare_assignment_image(candidate)
    assert assets._assignments == before
    assert leases[0].source_beatmap_receipt == reference.model_dump()
    assert not leases[0].released


def test_full_image_history_pins_unrelated_maps_incrementally_and_deduplicates_backing(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _, assets, _, leases = _environment(tmp_path, monkeypatch)
    first, second = _backing(tmp_path), _backing(tmp_path, "b")
    a, b = _selected(first, 100), _selected(second, 200)
    custody = CopyJournalCustody()
    custody.acquire(assets, tmp_path, (), project_images=(a.model_dump_json(),))
    assert custody.acquired
    assert len(custody.readers) == 1
    custody.acquire(assets, tmp_path, (), project_images=(a.model_dump_json(), b.model_dump_json()))
    assert custody.acquired
    assert len(custody.readers) == 2
    bound = [lease for lease in leases if lease.source_beatmap_receipt is not None]
    assert len(bound) == 2
    assert all(lease.source_beatmap_acknowledgements == 0 for lease in leases)
    assert all(not lease.released for lease in bound)
    custody.release()
    assert all(lease.released for lease in leases)


def test_history_conflict_or_partial_image_keeps_available_readers_and_acquisition_fenced(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _, assets, _, _ = _environment(tmp_path, monkeypatch)
    reference = _backing(tmp_path)
    custody = CopyJournalCustody()
    image = _selected(reference)
    custody.acquire(assets, tmp_path, (), project_images=(image.model_dump_json(),))
    changed = SourceBeatmapReference.model_validate(reference.model_dump() | {"sha256": "e" * 64})
    with pytest.raises(RuntimeError, match="contradictory immutable map receipts"):
        custody.acquire(
            assets, tmp_path, (), project_images=(_selected(changed).model_dump_json(),)
        )
    assert not custody.acquired
    assert len(custody.readers) == 1
    assert not custody.readers[0].released
    bad = image.model_dump(mode="json")
    bad.pop("pad_gain_db")
    with pytest.raises(RuntimeError, match="incomplete"):
        custody.acquire(assets, tmp_path, (), project_images=(json.dumps(bad),))
    assert not custody.acquired
    assert not custody.readers[0].released


def test_unavailable_leaf_keeps_transient_generation_history_pin_without_saved_qualification(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _, assets, _, leases = _environment(tmp_path, monkeypatch)
    reference = _backing(tmp_path)
    path = tmp_path / reference.reference
    queued = _MapLease(str(path))
    queued.bind_source_beatmap(reference.model_dump_json())
    assets._pending[path, False] = [queued]
    custody = CopyJournalCustody()
    acquire_map = assets._acquire_map

    def fail(_reference: SourceBeatmapReference) -> FakeProjectAssetLease:
        message = "leaf binding temporarily unavailable"
        raise RuntimeError(message)

    monkeypatch.setattr(assets, "_acquire_map", fail)
    image = _selected(reference).model_dump_json()
    custody.acquire(assets, tmp_path, (), project_images=(image,))
    assert custody.acquired
    assert len(custody.readers) == 1
    preservation = leases[0]
    assert preservation.path == str(path.parent)
    assert preservation.source_beatmap_receipt is None
    assert not preservation.released
    assert not queued.released
    assert (path, False) in assets._pending
    monkeypatch.setattr(assets, "_acquire_map", acquire_map)
    # Identical history remains pinned without retrying qualification or ACK.
    custody.acquire(assets, tmp_path, (), project_images=(image,))
    assert custody.acquired
    assert len(custody.readers) == 1
    assert not preservation.released
    assert not queued.released
    assert all(lease.source_beatmap_acknowledgements == 0 for lease in leases)
    assert (path, False) in assets._pending
    custody.release()
    assert preservation.released


def test_failed_actual_preservation_pin_leaves_history_acquisition_fenced(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _, assets, _, _ = _environment(tmp_path, monkeypatch)
    reference = _backing(tmp_path)
    custody = CopyJournalCustody()

    def fail(_target: Path) -> FakeProjectAssetLease:
        message = "generation preservation unavailable"
        raise RuntimeError(message)

    monkeypatch.setattr(assets, "acquire", fail)
    with pytest.raises(RuntimeError, match="generation preservation unavailable"):
        custody.acquire(
            assets, tmp_path, (), project_images=(_selected(reference).model_dump_json(),)
        )
    assert not custody.acquired
    assert not custody.readers


def test_recovery_prepares_owned_plan_before_commit_and_keeps_ack_for_apply(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    _, assets, _, leases = _environment(tmp_path, monkeypatch)
    candidate = _selected(_backing(tmp_path))
    prepared = prepare_recovery_map_publication(assets, candidate, None)
    assert prepared is not None
    assert leases[0].source_beatmap_acknowledgements == 0
    replacement = prepare_recovery_map_publication(assets, candidate, prepared)
    assert leases[0].released
    assert replacement is not None
    replacement[1].apply()
    assert leases[1].source_beatmap_acknowledgements == 1
    assert replacement[1].settled


def test_raw_ancestor_before_after_images_are_yielded_unchanged() -> None:
    before, after = ' {"historical":1} ', '{"historical":2}\n'
    encoded = json.dumps({
        "captured_project_image_json": before,
        "project_image_json": after,
    })
    assert list(journal_project_images((encoded,))) == [before, after]
