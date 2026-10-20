"""Metadata-only confirmation scope, identity fences and detached immutable state."""

from dataclasses import FrozenInstanceError, replace
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import Mock, call

import pytest
from pydantic import ValidationError

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.controller import stem_cache, stem_job, stem_provenance
from flitzis_looper.controller.stem_batch_inventory import (
    StemBatchInventory,
    StemBatchItem,
    capture_stem_batch_inventory,
    stem_batch_item_is_current,
)
from flitzis_looper.controller.stem_generation import AudioShape
from flitzis_looper.controller.stem_intent import StemIntentRegistry
from flitzis_looper.material_migration_model import MaterialCreationReceipt, MigrationFileEvidence
from flitzis_looper.models import (
    PadContentIdentity,
    ProjectState,
    StemCacheEntry,
    StemFileSet,
)
from flitzis_looper.stem_pair_selection import (
    STEM_FINGERPRINT_SCHEMA,
    StemGenerationProvenance,
    StemPairSelection,
)

if TYPE_CHECKING:
    from collections.abc import Callable

_SOURCE = (7, "a" * 64, 128, 44_100)
_SHAPE = AudioShape(44_100, 2, 128)


def _content(pad_id: int) -> PadContentIdentity:
    root = f"samples/material-{pad_id}"
    return PadContentIdentity(
        instance_id=f"{pad_id + 1:032x}",
        material_id=f"{pad_id + 1000:032x}",
        material_creation=MaterialCreationReceipt(
            root_reference=root,
            root_identity=(1, 2, pad_id + 3),
            binding=MigrationFileEvidence(
                name=".material.json",
                identity=(1, 2, pad_id + 1000),
                bytes=32,
                sha256="b" * 64,
            ),
        ),
    )


def _entry(pad_id: int) -> StemCacheEntry:
    root = f"samples/material-{pad_id}"
    generation = f"{pad_id + 1:032x}"
    provenance = StemGenerationProvenance(
        fingerprint_schema=STEM_FINGERPRINT_SCHEMA,
        generation_fingerprint="c" * 64,
        producer_identity="d" * 64,
    )
    return StemCacheEntry(
        source_version=f"source-{pad_id}",
        cache_dir=f"{root}/stems/.ready-{generation}",
        stems=StemFileSet(
            vocals=f"{root}/stems/vocals.wav",
            melody=f"{root}/stems/melody.wav",
            bass=f"{root}/stems/bass.wav",
            drums=f"{root}/stems/drums.wav",
            instrumental=f"{root}/stems/instrumental.wav",
        ),
        available=True,
        pair=StemPairSelection(
            descriptor_reference=f"{root}/.pcm-cache/stems/v1/.pairs/{generation}.json",
            stem_set_identity="e" * 64,
            wav_generation=f"{root}/stems/.ready-{generation}",
            pcm_generation=f"{root}/.pcm-cache/stems/v1/.ready-{generation}",
            provenance=provenance,
        ),
        fingerprint_schema=STEM_FINGERPRINT_SCHEMA,
        generation_fingerprint="c" * 64,
        producer_identity="d" * 64,
    )


class _InventoryCase:
    def __init__(self) -> None:
        self.project = ProjectState()
        self.project.sample_paths[0] = "samples/material-0/original.wav"
        self.project.pad_content[0] = _content(0)
        self.project.stem_cache[0] = _entry(0)
        self.source = Mock(return_value=_SOURCE)
        self.shape = Mock(return_value=_SHAPE)

    def capture(self) -> StemBatchInventory:
        return capture_stem_batch_inventory(
            self.project, source_identity=self.source, target_shape=self.shape
        )

    def current(self, item: StemBatchItem) -> bool:
        return stem_batch_item_is_current(
            self.project, item, source_identity=self.source, target_shape=self.shape
        )


@pytest.fixture
def inventory_case() -> _InventoryCase:
    return _InventoryCase()


def _first(inventory: StemBatchInventory) -> StemBatchItem:
    item = inventory.items[0]
    assert item is not None
    return item


def test_capture_covers_all_216_slots_and_preserves_duplicate_assignments() -> None:
    case = _InventoryCase()
    for pad_id in range(NUM_SAMPLES):
        case.project.sample_paths[pad_id] = f"samples/material-{pad_id}/original.wav"
        case.project.pad_content[pad_id] = _content(pad_id)
        case.project.stem_cache[pad_id] = _entry(pad_id)
    case.project.sample_paths[1] = case.project.sample_paths[0]
    before = case.project.model_dump()

    inventory = case.capture()

    assert len(inventory.items) == 216
    assert [item.pad_id for item in inventory.items if item is not None] == list(range(216))
    first, duplicate, *remaining = inventory.items
    assert first is not None
    assert duplicate is not None
    assert first.path == duplicate.path
    assert first.source_identity == duplicate.source_identity
    assert first.content != duplicate.content
    assert first.stem_set != duplicate.stem_set
    assert len(remaining) == 214
    assert all(item is not None and item.unavailable_reason is None for item in inventory.items)
    assert case.source.call_args_list == [call(pad_id) for pad_id in range(216)]
    assert case.shape.call_args_list == [call(pad_id) for pad_id in range(216)]
    assert case.project.model_dump() == before


def test_empty_slots_never_query_loaded_metadata(inventory_case: _InventoryCase) -> None:
    case = inventory_case
    case.project.sample_paths[215] = "samples/last/original.wav"
    inventory = case.capture()

    assert len(inventory.items) == 216
    assert inventory.items[0] is not None
    assert inventory.items[215] is not None
    assert inventory.items[1:215] == (None,) * 214
    assert case.source.call_args_list == [call(0), call(215)]
    assert case.shape.call_args_list == [call(0), call(215)]


def test_capture_and_currentness_have_no_io_or_custody_side_effects(
    inventory_case: _InventoryCase, monkeypatch: pytest.MonkeyPatch
) -> None:
    case = inventory_case
    forbidden = Mock(side_effect=AssertionError("metadata capture must not prepare or do I/O"))
    before = case.project.model_dump()
    with monkeypatch.context() as no_io:
        no_io.setattr("builtins.open", forbidden)
        no_io.setattr(Path, "open", forbidden)
        no_io.setattr(Path, "mkdir", forbidden)
        no_io.setattr(Path, "read_bytes", forbidden)
        no_io.setattr(Path, "resolve", forbidden)
        no_io.setattr(stem_cache, "source_version_for_sample_path", forbidden)
        no_io.setattr(stem_provenance, "capture_processing_files", forbidden)
        no_io.setattr(stem_job, "StemGenerationJob", forbidden)
        no_io.setattr(StemIntentRegistry, "capture", forbidden)
        inventory = case.capture()
        item = _first(inventory)
        assert case.current(item)
        # Rendering/dismissing the immutable value needs no controller operation.
        assert item.path == "samples/material-0/original.wav"
        assert inventory.items[1] is None
        del inventory
    forbidden.assert_not_called()
    assert case.project.model_dump() == before


def test_snapshot_is_detached_and_nested_content_is_frozen(inventory_case: _InventoryCase) -> None:
    case = inventory_case
    inventory = case.capture()
    item = _first(inventory)
    original = case.project.pad_content[0]
    entry = case.project.stem_cache[0]
    assert original is not None
    assert original.material_creation is not None
    assert item.content is not None
    assert item.content.material_creation is not None
    assert entry is not None
    assert entry.pair is not None
    assert item.stem_set is not None
    assert item.content is not original
    assert item.content.material_creation is not original.material_creation
    assert item.content.material_creation.binding is not original.material_creation.binding
    assert item.target_shape is not _SHAPE
    assert item.target_shape is not None
    assert item.stem_set.stems == tuple(sorted(item.stem_set.stems))
    pair_json = item.stem_set.pair_json

    with pytest.raises(FrozenInstanceError):
        inventory.processing_revision = 1  # type: ignore[misc]
    with pytest.raises(FrozenInstanceError):
        item.path = "samples/replacement.wav"  # type: ignore[misc]
    with pytest.raises(ValidationError):
        item.content.material_creation.binding.bytes = 64  # type: ignore[misc]
    with pytest.raises(FrozenInstanceError):
        item.stem_set.cache_dir = "samples/replacement/stems"  # type: ignore[misc]
    with pytest.raises(FrozenInstanceError):
        item.target_shape.frame_count = 256  # type: ignore[misc]

    entry.stems.vocals = "samples/replaced/vocals.wav"
    entry.pair = None
    assert item.stem_set.pair_json == pair_json
    assert dict(item.stem_set.stems)["vocals"] == "samples/material-0/stems/vocals.wav"
    assert not case.current(item)


@pytest.mark.parametrize(
    "identity",
    [
        None,
        (True, "a" * 64, 128, 44_100),
        (-1, "a" * 64, 128, 44_100),
        (7, None, 128, 44_100),
        (7, "a" * 64, False, 44_100),
        (7, "a" * 64, 128, 0),
        (7, "a" * 64, 128.0, 44_100),
        [7, "a" * 64, 128, 44_100],
        (7, "a" * 64, 128),
    ],
)
def test_invalid_or_unloaded_source_stays_explicitly_unavailable(
    inventory_case: _InventoryCase, identity: object
) -> None:
    case = inventory_case
    case.source.return_value = identity
    item = _first(case.capture())

    assert item.source_identity is None
    assert item.unavailable_reason == "Loaded source identity unavailable"
    case.source.return_value = _SOURCE
    assert not case.current(item)


@pytest.mark.parametrize(
    "shape",
    [
        None,
        (44_100, 2, 128),
        AudioShape(sample_rate_hz=True, channels=2, frame_count=128),
        AudioShape(44_100, 0, 128),
    ],
)
def test_missing_or_invalid_shape_never_substitutes_a_later_loaded_target(
    inventory_case: _InventoryCase, shape: object
) -> None:
    case = inventory_case
    case.shape.return_value = shape
    item = _first(case.capture())

    assert item.target_shape is None
    assert item.unavailable_reason == "Loaded target shape unavailable"
    case.shape.return_value = _SHAPE
    assert not case.current(item)


@pytest.mark.parametrize("reader", ["source", "shape"])
@pytest.mark.parametrize("error_type", [RuntimeError, TypeError, ValueError])
def test_loading_native_reader_failures_fail_closed_at_capture_and_check(
    inventory_case: _InventoryCase, reader: str, error_type: type[Exception]
) -> None:
    case = inventory_case
    admitted = _first(case.capture())
    callback = case.source if reader == "source" else case.shape
    callback.side_effect = error_type("source loading or disappeared")

    assert not case.current(admitted)
    unavailable = _first(case.capture())
    assert unavailable.unavailable_reason is not None
    callback.side_effect = None
    assert not case.current(unavailable)
    assert case.current(admitted)


@pytest.mark.parametrize(
    "identity",
    [
        (8, "a" * 64, 128, 44_100),
        (7, "b" * 64, 128, 44_100),
        (7, "a" * 64, 129, 44_100),
        (7, "a" * 64, 128, 48_000),
    ],
)
def test_source_fence_detects_generation_digest_frames_and_rate_changes(
    inventory_case: _InventoryCase, identity: tuple[int, str, int, int]
) -> None:
    case = inventory_case
    item = _first(case.capture())
    case.source.return_value = identity
    assert not case.current(item)


def test_source_aba_same_bytes_and_path_with_new_generation_is_stale(
    inventory_case: _InventoryCase,
) -> None:
    case = inventory_case
    item = _first(case.capture())
    case.source.return_value = (8, "b" * 64, 128, 44_100)
    assert not case.current(item)
    case.source.return_value = (9, "a" * 64, 128, 44_100)
    assert not case.current(item)
    assert item.source_identity == _SOURCE


@pytest.mark.parametrize(
    "change",
    ["path", "content", "material", "creation", "lineage", "shape", "empty"],
)
def test_replaced_original_target_is_stale(inventory_case: _InventoryCase, change: str) -> None:
    case = inventory_case
    item = _first(case.capture())
    content = case.project.pad_content[0]
    assert content is not None
    assert content.material_creation is not None
    if change == "path":
        case.project.sample_paths[0] = "samples/other/original.wav"
    elif change == "content":
        case.project.pad_content[0] = content.model_copy(update={"instance_id": "f" * 32})
    elif change == "material":
        case.project.pad_content[0] = content.model_copy(update={"material_id": "f" * 32})
    elif change == "creation":
        creation = content.material_creation.model_copy(update={"root_identity": (1, 2, 99)})
        case.project.pad_content[0] = content.model_copy(update={"material_creation": creation})
    elif change == "lineage":
        case.project.pad_content[0] = content.model_copy(update={"copied_from": "f" * 32})
    elif change == "shape":
        case.shape.return_value = AudioShape(48_000, 1, 256)
    else:
        case.project.sample_paths[0] = None
    assert not case.current(item)
    assert item.path == "samples/material-0/original.wav"
    assert item.content == _content(0)


@pytest.mark.parametrize(
    "change",
    ["source_version", "cache_dir", "stems", "schema", "fingerprint", "producer", "pair", "none"],
)
def test_changed_exact_stem_set_metadata_is_stale(
    inventory_case: _InventoryCase, change: str
) -> None:
    case = inventory_case
    item = _first(case.capture())
    entry = case.project.stem_cache[0]
    assert entry is not None
    assert entry.pair is not None
    updates: dict[str, object] = {
        "source_version": "new-source-version",
        "cache_dir": "samples/new/stems",
        "stems": entry.stems.with_kind("drums", "samples/new/drums.wav"),
        "schema": None,
        "fingerprint": "f" * 64,
        "producer": "f" * 64,
        "pair": entry.pair.model_copy(update={"stem_set_identity": "f" * 64}),
    }
    if change == "none":
        case.project.stem_cache[0] = None
    else:
        field = {
            "schema": "fingerprint_schema",
            "fingerprint": "generation_fingerprint",
            "producer": "producer_identity",
        }.get(change, change)
        case.project.stem_cache[0] = entry.model_copy(update={field: updates[change]})
    assert not case.current(item)


def test_exact_old_set_returning_is_same_metadata_target_without_new_rights(
    inventory_case: _InventoryCase,
) -> None:
    case = inventory_case
    item = _first(case.capture())
    original = case.project.stem_cache[0]
    case.project.stem_cache[0] = _entry(1)
    assert not case.current(item)
    case.project.stem_cache[0] = original
    # This predicate grants no source/publication lease. A transient equal-value
    # metadata replacement has no mutation epoch; Native re-admits actual work.
    assert case.current(item)


@pytest.mark.parametrize("setting", ["stem_separator", "demucs_shifts", "demucs_overlap"])
def test_processing_settings_aba_is_stale_even_after_value_restoration(
    inventory_case: _InventoryCase, setting: str
) -> None:
    case = inventory_case
    item = _first(case.capture())
    original = getattr(case.project, setting)
    replacement = {
        "stem_separator": "bs-roformer:musdb18hq",
        "demucs_shifts": 2,
        "demucs_overlap": 0.75,
    }[setting]
    setattr(case.project, setting, replacement)
    assert not case.current(item)
    setattr(case.project, setting, original)
    assert case.project.stem_processing_revision == item.processing_revision + 2
    assert not case.current(item)


def test_inactive_demucs_settings_do_not_change_roformer_target(
    inventory_case: _InventoryCase,
) -> None:
    case = inventory_case
    case.project.stem_separator = "bs-roformer:musdb18hq"
    inventory = case.capture()
    item = _first(inventory)
    assert inventory.processing_signature == ("bs-roformer:musdb18hq", None, None)
    case.project.demucs_shifts = 2
    case.project.demucs_overlap = 0.75
    assert case.current(item)


def test_unrelated_selection_tempo_pitch_and_availability_leave_target_current(
    inventory_case: _InventoryCase,
) -> None:
    case = inventory_case
    item = _first(case.capture())
    case.project.selected_bank = 5
    case.project.selected_pad = 215
    case.project.speed = 1.2
    case.project.key_lock = True
    case.project.pad_key_lock[0] = False
    case.project.manual_bpm[0] = 130.0
    case.project.pad_loop_start_s[0] = 0.25
    entry = case.project.stem_cache[0]
    assert entry is not None
    entry.available = False
    assert case.current(item)


@pytest.mark.parametrize(
    "alter",
    [
        lambda inventory: replace(inventory, items=()),
        lambda inventory: replace(inventory, items=inventory.items[:-1]),
        lambda inventory: replace(inventory, processing_revision=True),
        lambda inventory: replace(inventory, processing_revision=-1),
        lambda inventory: replace(inventory, processing_revision=1),
        lambda inventory: replace(inventory, items=tuple(reversed(inventory.items))),
    ],
)
def test_inventory_rejects_wrong_scope_or_processing_context(
    inventory_case: _InventoryCase, alter: Callable[[StemBatchInventory], StemBatchInventory]
) -> None:
    with pytest.raises(ValueError, match="stem batch"):
        alter(inventory_case.capture())
