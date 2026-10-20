"""Immutable metadata for a deliberate all-slot stem-generation confirmation.

This control-only snapshot owns no source leases, jobs, assets or Native permits.
The supplied readers must inspect already loaded metadata without performing I/O.
Actual generation admission and effective producer verification remain separate.
"""

from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.controller.stem_generation import AudioShape
from flitzis_looper.models import STEM_KINDS, PadContentIdentity, validate_sample_id

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.models import ProjectState, StemCacheEntry, StemKind, StemSeparator
    from flitzis_looper.stem_pair_selection import StemFingerprintSchema

type StemBatchSourceIdentity = tuple[int, str, int, int]
type StemBatchProcessingSignature = tuple[StemSeparator, int | None, float | None]
type StemBatchUnavailableReason = Literal[
    "Loaded source identity unavailable", "Loaded target shape unavailable"
]


@dataclass(frozen=True, slots=True)
class StemBatchSetIdentity:
    """Detached durable set metadata, excluding transient playback availability."""

    source_version: str
    cache_dir: str
    stems: tuple[tuple[StemKind, str | None], ...]
    fingerprint_schema: StemFingerprintSchema | None
    generation_fingerprint: str | None
    producer_identity: str | None
    pair_json: str | None


@dataclass(frozen=True, slots=True)
class StemBatchItem:
    """One original occupied slot; unavailable metadata never grants admission."""

    pad_id: int
    path: str
    content: PadContentIdentity | None
    source_identity: StemBatchSourceIdentity | None
    target_shape: AudioShape | None
    stem_set: StemBatchSetIdentity | None
    processing_revision: int
    processing_signature: StemBatchProcessingSignature
    unavailable_reason: StemBatchUnavailableReason | None


@dataclass(frozen=True, slots=True)
class StemBatchInventory:
    """Exactly all project slots in stable order, with one processing context."""

    processing_revision: int
    processing_signature: StemBatchProcessingSignature
    items: tuple[StemBatchItem | None, ...]

    def __post_init__(self) -> None:
        """Reject a malformed or differently scoped hand-constructed inventory."""
        if type(self.items) is not tuple or len(self.items) != NUM_SAMPLES:
            message = f"stem batch inventory must contain exactly {NUM_SAMPLES} slots"
            raise ValueError(message)
        if type(self.processing_revision) is not int or self.processing_revision < 0:
            message = "stem batch processing revision must be a nonnegative integer"
            raise ValueError(message)
        if any(
            item is not None
            and (
                item.pad_id != pad_id
                or item.processing_revision != self.processing_revision
                or item.processing_signature != self.processing_signature
            )
            for pad_id, item in enumerate(self.items)
        ):
            message = "stem batch items must retain their slot and processing context"
            raise ValueError(message)


def capture_stem_batch_inventory(
    project: ProjectState,
    *,
    source_identity: Callable[[int], StemBatchSourceIdentity | None],
    target_shape: Callable[[int], AudioShape | None],
) -> StemBatchInventory:
    """Capture all slots using only project state and no-I/O loaded metadata readers.

    Empty slots stay empty. An occupied slot with unavailable loaded metadata stays
    represented with an explicit reason and can never later target a replacement.
    No source version is hashed and no effective producer identity is asserted.

    Args:
        project: Current control-owned project state.
        source_identity: No-I/O reader of loaded generation, digest, frames and rate.
        target_shape: No-I/O reader of the existing loaded target audio shape.

    Returns:
        Detached immutable metadata for exactly all project slots.
    """
    revision = project.stem_processing_revision
    signature = _processing_signature(project)
    items = tuple(
        _capture_item(project, pad_id, revision, signature, source_identity, target_shape)
        for pad_id in range(NUM_SAMPLES)
    )
    return StemBatchInventory(revision, signature, items)


def capture_stem_batch_item(
    project: ProjectState,
    pad_id: int,
    *,
    source_identity: Callable[[int], StemBatchSourceIdentity | None],
    target_shape: Callable[[int], AudioShape | None],
) -> StemBatchItem | None:
    """Capture one direct request using the same no-I/O all-loaded target contract."""
    validate_sample_id(pad_id)
    return _capture_item(
        project,
        pad_id,
        project.stem_processing_revision,
        _processing_signature(project),
        source_identity,
        target_shape,
    )


def _capture_item(
    project: ProjectState,
    pad_id: int,
    revision: int,
    signature: StemBatchProcessingSignature,
    source_identity: Callable[[int], StemBatchSourceIdentity | None],
    target_shape: Callable[[int], AudioShape | None],
) -> StemBatchItem | None:
    path = project.sample_paths[pad_id]
    if path is None:
        return None
    identity = _read_source_identity(source_identity, pad_id)
    shape = _read_target_shape(target_shape, pad_id)
    content = project.pad_content[pad_id]
    reason: StemBatchUnavailableReason | None = None
    if identity is None:
        reason = "Loaded source identity unavailable"
    elif shape is None:
        reason = "Loaded target shape unavailable"
    return StemBatchItem(
        pad_id=pad_id,
        path=path,
        content=content.model_copy(deep=True) if content is not None else None,
        source_identity=identity,
        target_shape=shape,
        stem_set=_stem_set_identity(_entry_for_pad(project, pad_id)),
        processing_revision=revision,
        processing_signature=signature,
        unavailable_reason=reason,
    )


def stem_batch_item_is_current(
    project: ProjectState,
    item: StemBatchItem,
    *,
    source_identity: Callable[[int], StemBatchSourceIdentity | None],
    target_shape: Callable[[int], AudioShape | None],
) -> bool:
    """Check the original target without recapturing or granting Native authority.

    Missing, invalid or failed loaded-source readers fail closed. A captured
    unavailable slot remains unavailable even if another source loads there.

    Args:
        project: Current control-owned project state.
        item: The original occupied slot captured for confirmation.
        source_identity: The same no-I/O loaded-source reader used at capture.
        target_shape: The same no-I/O loaded target-shape reader used at capture.

    Returns:
        Whether the original metadata target and effective Settings are unchanged.
    """
    if (
        type(item.pad_id) is not int
        or not 0 <= item.pad_id < NUM_SAMPLES
        or item.unavailable_reason is not None
        or item.source_identity is None
        or item.target_shape is None
    ):
        return False
    if not _same_project_target(project, item):
        return False
    return (
        _read_source_identity(source_identity, item.pad_id) == item.source_identity
        and _read_target_shape(target_shape, item.pad_id) == item.target_shape
    )


def _same_project_target(project: ProjectState, item: StemBatchItem) -> bool:
    return (
        project.stem_processing_revision == item.processing_revision
        and _processing_signature(project) == item.processing_signature
        and project.sample_paths[item.pad_id] == item.path
        and project.pad_content[item.pad_id] == item.content
        and _stem_set_identity(_entry_for_pad(project, item.pad_id)) == item.stem_set
    )


def _processing_signature(project: ProjectState) -> StemBatchProcessingSignature:
    if project.stem_separator == "demucs:htdemucs":
        return project.stem_separator, project.demucs_shifts, project.demucs_overlap
    return project.stem_separator, None, None


def _entry_for_pad(project: ProjectState, pad_id: int) -> StemCacheEntry | None:
    return project.stem_cache[pad_id] if pad_id < len(project.stem_cache) else None


def _stem_set_identity(entry: StemCacheEntry | None) -> StemBatchSetIdentity | None:
    if entry is None:
        return None
    return StemBatchSetIdentity(
        source_version=entry.source_version,
        cache_dir=entry.cache_dir,
        stems=tuple((kind, entry.stems.path_for(kind)) for kind in sorted(STEM_KINDS)),
        fingerprint_schema=entry.fingerprint_schema,
        generation_fingerprint=entry.generation_fingerprint,
        producer_identity=entry.producer_identity,
        pair_json=entry.pair.model_dump_json() if entry.pair is not None else None,
    )


def _read_source_identity(
    reader: Callable[[int], StemBatchSourceIdentity | None], pad_id: int
) -> StemBatchSourceIdentity | None:
    try:
        identity = reader(pad_id)
    except RuntimeError, TypeError, ValueError:
        return None
    if not isinstance(identity, tuple) or len(identity) != 4:
        return None
    generation, digest, frames, rate = identity
    if (
        type(generation) is not int
        or generation < 0
        or not isinstance(digest, str)
        or any(type(value) is not int or value <= 0 for value in (frames, rate))
    ):
        return None
    return generation, digest, frames, rate


def _read_target_shape(
    reader: Callable[[int], AudioShape | None], pad_id: int
) -> AudioShape | None:
    try:
        shape = reader(pad_id)
    except RuntimeError, TypeError, ValueError:
        return None
    if not isinstance(shape, AudioShape) or any(
        type(value) is not int or value <= 0
        for value in (shape.sample_rate_hz, shape.channels, shape.frame_count)
    ):
        return None
    return AudioShape(shape.sample_rate_hz, shape.channels, shape.frame_count)
