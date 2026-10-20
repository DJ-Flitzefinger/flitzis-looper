"""Deletion warning scope stays exact without borrowing worker or Native authority."""

from dataclasses import FrozenInstanceError, replace
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.controller import stem_job, stem_provenance
from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.stem_delete_inventory import (
    StemDeleteInventory,
    StemDeleteItem,
    StemDeletePadMetadata,
    StemDeleteRequestIdentity,
    capture_stem_delete_inventory,
    stem_delete_item_currentness,
    stem_delete_request_identity,
)
from flitzis_looper.controller.stem_intent import StemGenerationIntent, StemIntentRegistry
from flitzis_looper.controller.stem_job import StemGenerationInterest
from flitzis_looper.models import PadContentIdentity, ProjectState, StemCacheEntry, StemFileSet
from flitzis_looper.stem_pair_selection import (
    STEM_FINGERPRINT_SCHEMA,
    StemGenerationProvenance,
    StemPairSelection,
)

if TYPE_CHECKING:
    from collections.abc import Callable

_SOURCE = (7, "a" * 64, 128, 44_100)


def _assign_field(target: object, name: str, value: object) -> None:
    """Exercise runtime immutability through a dynamic external consumer."""
    setattr(target, name, value)


def _content(pad_id: int = 0) -> PadContentIdentity:
    return PadContentIdentity(
        instance_id=f"{pad_id + 1:032x}",
        material_id="b" * 32,
    )


def _entry(generation: int = 1) -> StemCacheEntry:
    root = "samples/shared"
    token = f"{generation:032x}"
    wav = f"{root}/stems/.ready-{token}"
    provenance = StemGenerationProvenance(
        fingerprint_schema=STEM_FINGERPRINT_SCHEMA,
        generation_fingerprint="c" * 64,
        producer_identity="d" * 64,
    )
    return StemCacheEntry(
        source_version="source|sha256-v1:" + "a" * 64,
        cache_dir=wav,
        stems=StemFileSet(
            vocals=f"{wav}/vocals.wav",
            melody=f"{wav}/melody.wav",
            bass=f"{wav}/bass.wav",
            drums=f"{wav}/drums.wav",
            instrumental=f"{wav}/instrumental.wav",
        ),
        available=True,
        pair=StemPairSelection(
            descriptor_reference=f"{root}/.pcm-cache/stems/v1/.pairs/{token}.json",
            stem_set_identity=f"{generation:064x}",
            wav_generation=wav,
            pcm_generation=f"{root}/.pcm-cache/stems/v1/.ready-{token}",
            provenance=provenance,
        ),
        fingerprint_schema=STEM_FINGERPRINT_SCHEMA,
        generation_fingerprint="c" * 64,
        producer_identity="d" * 64,
    )


def _intent(admission_sequence: int = 1) -> StemGenerationIntent:
    return StemGenerationIntent(
        settings_revision=3,
        material_revision=2,
        admission_sequence=admission_sequence,
        material_key="a" * 64,
        signature=("captured request",),
    )


def _metadata(pad_id: int = 0) -> StemDeletePadMetadata:
    return StemDeletePadMetadata(
        pad_id=pad_id,
        path="samples/shared/original.wav",
        content=_content(pad_id),
        source_identity=_SOURCE,
        current_entry=_entry(),
    )


def _single(metadata: StemDeletePadMetadata) -> StemDeleteInventory:
    rows: list[StemDeletePadMetadata | None] = [None] * NUM_SAMPLES
    rows[metadata.pad_id] = metadata
    return capture_stem_delete_inventory(tuple(rows))


def _first(inventory: StemDeleteInventory) -> StemDeleteItem:
    item = inventory.items[0]
    assert item is not None
    return item


def _current(item: StemDeleteItem, metadata: StemDeletePadMetadata | None) -> str:
    return stem_delete_item_currentness(item, read_metadata=lambda _pad_id: metadata)


def test_all_216_rows_keep_independent_assignments_and_deduplicate_exact_shared_sets() -> None:
    rows = tuple(_metadata(pad_id) for pad_id in range(NUM_SAMPLES))

    inventory = capture_stem_delete_inventory(rows)

    assert len(inventory.items) == 216
    assert [item.pad_id for item in inventory.items if item is not None] == list(range(216))
    assert len(inventory.set_identities) == 1
    first = _first(inventory)
    last = inventory.items[215]
    assert last is not None
    assert first.path == last.path
    assert first.content != last.content
    assert first.current_set == last.current_set
    assert _current(first, rows[0]) == "current"
    assert _current(last, rows[215]) == "current"


def test_capture_is_detached_and_never_performs_io_or_acquires_custody(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    metadata = _metadata()
    entry = metadata.current_entry
    assert entry is not None
    forbidden = Mock(side_effect=AssertionError("deletion inventory must remain metadata only"))
    with monkeypatch.context() as no_io:
        no_io.setattr("builtins.open", forbidden)
        for name in ("open", "read_bytes", "resolve", "mkdir", "unlink", "exists"):
            no_io.setattr(Path, name, forbidden)
        no_io.setattr(ProjectAssetLifecycle, "admission", forbidden)
        no_io.setattr(ProjectAssetLifecycle, "acquire", forbidden)
        no_io.setattr(ProjectAssetLifecycle, "retire", forbidden)
        no_io.setattr(stem_provenance, "capture_processing_files", forbidden)
        no_io.setattr(stem_job, "StemGenerationJob", forbidden)
        no_io.setattr(StemIntentRegistry, "capture", forbidden)
        inventory = _single(metadata)
        item = _first(inventory)
        assert _current(item, metadata) == "current"
    forbidden.assert_not_called()

    captured = item.current_set
    assert captured is not None
    entry.cache_dir = "samples/newer/stems/.ready-newer"
    assert item.current_set == captured
    assert item.content == metadata.content
    assert item.content is not metadata.content
    assert _current(item, metadata) == "stale"
    with pytest.raises(FrozenInstanceError):
        _assign_field(item, "path", "samples/replacement.wav")


def test_settings_revision_alone_does_not_invalidate_existing_deletion_scope() -> None:
    project = ProjectState()
    metadata = _metadata()
    project.sample_paths[0] = metadata.path
    project.pad_content[0] = metadata.content
    project.stem_cache[0] = metadata.current_entry

    def read_metadata(pad_id: int) -> StemDeletePadMetadata:
        path = project.sample_paths[pad_id]
        assert path is not None
        return replace(
            metadata,
            path=path,
            content=project.pad_content[pad_id],
            current_entry=project.stem_cache[pad_id],
        )

    item = _first(_single(read_metadata(0)))
    before_revision = project.stem_processing_revision
    project.stem_separator = "bs-roformer:musdb18hq"
    project.demucs_shifts += 1
    project.demucs_overlap = 0.5
    assert project.stem_processing_revision > before_revision

    assert stem_delete_item_currentness(item, read_metadata=read_metadata) == "current"


@pytest.mark.parametrize(
    "alter",
    [
        pytest.param(lambda row: replace(row, pad_id=215), id="different-original-slot"),
        pytest.param(lambda row: replace(row, path="samples/other.wav"), id="path"),
        pytest.param(lambda row: replace(row, content=_content(8)), id="content-instance"),
        pytest.param(
            lambda row: replace(
                row, content=_content().model_copy(update={"material_id": "f" * 32})
            ),
            id="material-identity",
        ),
        pytest.param(
            lambda row: replace(row, source_identity=(8, "a" * 64, 128, 44_100)),
            id="source-generation",
        ),
        pytest.param(
            lambda row: replace(row, source_identity=(7, "f" * 64, 128, 44_100)),
            id="source-digest",
        ),
        pytest.param(
            lambda row: replace(row, source_identity=(7, "a" * 64, 256, 44_100)),
            id="source-frames",
        ),
        pytest.param(
            lambda row: replace(row, source_identity=(7, "a" * 64, 128, 48_000)),
            id="source-rate",
        ),
        pytest.param(lambda row: replace(row, current_entry=_entry(2)), id="new-exact-set"),
        pytest.param(
            lambda row: replace(
                row, current_entry=_entry().model_copy(update={"source_version": "other-source"})
            ),
            id="set-source-version",
        ),
        pytest.param(
            lambda row: replace(
                row, current_entry=_entry().model_copy(update={"producer_identity": "e" * 64})
            ),
            id="set-producer",
        ),
        pytest.param(
            lambda row: replace(
                row,
                current_entry=_entry().model_copy(
                    update={"stems": StemFileSet(vocals="samples/shared/changed.wav")}
                ),
            ),
            id="declared-stem-path",
        ),
        pytest.param(lambda row: replace(row, current_entry=None), id="current-set-removed"),
        pytest.param(
            lambda row: replace(row, retained_entries=(_entry(2),)), id="new-retained-set"
        ),
    ],
)
def test_changed_assignment_source_or_exact_set_is_stale(
    alter: Callable[[StemDeletePadMetadata], StemDeletePadMetadata],
) -> None:
    metadata = _metadata()
    item = _first(_single(metadata))

    assert _current(item, alter(metadata)) == "stale"


def test_retained_owner_order_duplicates_and_availability_do_not_change_scope() -> None:
    old, pending = _entry(2), _entry(3)
    metadata = replace(_metadata(), retained_entries=(old, pending, old, _entry()))
    item = _first(_single(metadata))
    assert len(item.retained_sets) == 2
    assert len(item.set_identities) == 3
    old.available = False
    pending.available = False
    current = replace(metadata, retained_entries=(pending, old))

    assert _current(item, current) == "current"
    assert _current(item, replace(current, retained_entries=(pending,))) == "stale"


def test_pending_request_binding_and_completion_keep_the_original_admission_identity() -> None:
    intent = _intent()
    original = stem_delete_request_identity(intent)
    metadata = replace(_metadata(), requests=(original,))
    item = _first(_single(metadata))
    bound = stem_delete_request_identity(replace(intent, material_revision=9))

    assert bound == original
    assert _current(item, replace(metadata, requests=(bound, bound))) == "current"
    assert _current(item, replace(metadata, requests=())) == "current"


def test_new_equal_configuration_admission_is_stale_even_without_a_new_output() -> None:
    original = stem_delete_request_identity(_intent())
    newer = stem_delete_request_identity(_intent(2))
    metadata = replace(_metadata(), requests=(original,))
    item = _first(_single(metadata))

    assert newer != original
    assert _current(item, replace(metadata, requests=(newer,))) == "stale"
    assert _current(item, replace(metadata, requests=(original, newer))) == "stale"


def test_logical_pending_scope_retains_only_the_original_frozen_batch_tokens() -> None:
    original = StemGenerationInterest("batch-pad-1")
    metadata = replace(_metadata(), logical_pending=(original, original))
    item = _first(_single(metadata))

    assert len(item.logical_pending) == 1
    assert next(iter(item.logical_pending)) is original
    assert _current(item, replace(metadata, logical_pending=(original,))) == "current"
    assert _current(item, replace(metadata, logical_pending=())) == "current"


def test_new_batch_token_with_equal_label_cannot_replace_original_logical_scope() -> None:
    original = StemGenerationInterest("batch-pad-1")
    replacement = StemGenerationInterest("batch-pad-1")
    metadata = replace(_metadata(), logical_pending=(original,))
    item = _first(_single(metadata))

    assert replacement.owner == original.owner
    assert replacement != original
    assert _current(item, replace(metadata, logical_pending=(replacement,))) == "stale"
    assert replacement not in item.logical_pending


@pytest.mark.parametrize("known_output", ["known-retained", "unknown-at-capture"])
def test_pending_completion_never_extends_confirmed_current_set_scope(known_output: str) -> None:
    output = _entry(2)
    request = stem_delete_request_identity(_intent())
    retained = (output,) if known_output == "known-retained" else ()
    metadata = replace(_metadata(), retained_entries=retained, requests=(request,))
    item = _first(_single(metadata))
    completed = replace(metadata, current_entry=output, retained_entries=(_entry(),), requests=())

    if known_output == "known-retained":
        # Even an unchanged total known-set image conservatively skips a changed
        # durable current selection; no request provenance is inferred from it.
        completed_item = _first(_single(completed))
        assert completed_item.set_identities == item.set_identities
    assert _current(item, completed) == "stale"


def test_arrangement_fence_defers_before_observing_any_intermediate_image() -> None:
    item = _first(_single(_metadata()))
    read = Mock(side_effect=AssertionError("the fenced image must not be observed"))

    assert stem_delete_item_currentness(item, read_metadata=read, deferred=True) == "deferred"
    read.assert_not_called()
    read.side_effect = None
    read.return_value = replace(_metadata(), content=_content(9))
    assert stem_delete_item_currentness(item, read_metadata=read) == "stale"
    read.assert_called_once_with(0)


def test_unavailable_snapshot_never_recaptures_a_later_source() -> None:
    item = _first(_single(replace(_metadata(), source_identity=None)))
    read = Mock(return_value=_metadata())

    assert item.unavailable_reason == "Loaded source identity unavailable"
    assert stem_delete_item_currentness(item, read_metadata=read) == "stale"
    read.assert_not_called()


@pytest.mark.parametrize("error_type", [RuntimeError, TypeError, ValueError])
def test_failed_current_metadata_reader_cannot_grant_deletion(error_type: type[Exception]) -> None:
    item = _first(_single(_metadata()))
    read = Mock(side_effect=error_type("loaded metadata unavailable"))

    assert stem_delete_item_currentness(item, read_metadata=read) == "stale"
    assert _current(item, None) == "stale"


def test_equal_processing_fingerprint_does_not_collapse_different_exact_generations() -> None:
    first, second = _entry(), _entry(2)
    assert first.generation_fingerprint == second.generation_fingerprint
    metadata = replace(_metadata(), current_entry=first, retained_entries=(second,))

    assert len(_single(metadata).set_identities) == 2


def test_empty_slots_and_malformed_scope_cannot_retarget_original_rows() -> None:
    inventory = _single(_metadata())
    assert inventory.items[1:] == (None,) * 215
    with pytest.raises(ValueError, match="exactly 216"):
        capture_stem_delete_inventory((None,) * 215)
    with pytest.raises(ValueError, match="exactly 216"):
        capture_stem_delete_inventory((replace(_metadata(), pad_id=1),) + (None,) * 215)


@pytest.mark.parametrize("sequence", [0, -1])
def test_unknown_request_sequence_cannot_identify_a_pending_generation(sequence: int) -> None:
    with pytest.raises(ValueError, match="positive admission"):
        StemDeleteRequestIdentity("a" * 64, sequence)
