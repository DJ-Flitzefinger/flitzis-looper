"""Detached all-slot deletion scope; metadata never grants retirement authority.

The controller supplies already loaded metadata from its existing project, intent,
Pair and settlement owners. Capture performs no I/O and takes no lease or permit.
Actual deletion must still use the ordinary guarded per-PAD action.
"""

from dataclasses import dataclass
from typing import TYPE_CHECKING, Literal

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.controller.stem_batch_inventory import StemBatchSetIdentity
from flitzis_looper.controller.stem_job import StemGenerationInterest
from flitzis_looper.models import STEM_KINDS

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.stem_batch_inventory import StemBatchSourceIdentity
    from flitzis_looper.controller.stem_intent import StemGenerationIntent
    from flitzis_looper.models import PadContentIdentity, StemCacheEntry

type StemDeleteCurrentness = Literal["current", "stale", "deferred"]
type StemDeleteUnavailableReason = Literal["Loaded source identity unavailable"]


@dataclass(frozen=True, slots=True)
class StemDeleteRequestIdentity:
    """Existing monotone admission identity, independent of worker phase or binding.

    Binding may change an intent's material revision. Its admission sequence and
    material key remain stable through the original request, Pair and settlement.
    Neither object addresses nor processing fingerprints identify an admission.
    """

    material_key: str
    admission_sequence: int

    def __post_init__(self) -> None:
        """Reject unknown identities rather than equating unrelated requests."""
        if (
            not isinstance(self.material_key, str)
            or not self.material_key
            or type(self.admission_sequence) is not int
            or self.admission_sequence <= 0
        ):
            message = "stem delete request identity requires an existing positive admission"
            raise ValueError(message)


@dataclass(frozen=True, slots=True)
class StemDeletePadMetadata:
    """Explicit controller projection, inspected only while capturing or validating.

    Entries may belong to the current project, retained control selections, Pair
    work or disk settlement. They are metadata inputs, never borrowed ownership.
    Include surviving completed-admission metadata when its existing owner can
    still identify it. Do not infer request continuity from a matching fingerprint.
    """

    pad_id: int
    path: str
    content: PadContentIdentity | None
    source_identity: StemBatchSourceIdentity | None
    current_entry: StemCacheEntry | None = None
    retained_entries: tuple[StemCacheEntry, ...] = ()
    requests: tuple[StemDeleteRequestIdentity, ...] = ()
    logical_pending: tuple[StemGenerationInterest, ...] = ()
    """Exact existing unadmitted caller tokens; these frozen tokens own no readers."""


@dataclass(frozen=True, slots=True)
class StemDeleteItem:
    """One original loaded assignment and its exact detached known set image."""

    pad_id: int
    path: str
    content: PadContentIdentity | None
    source_identity: StemBatchSourceIdentity | None
    current_set: StemBatchSetIdentity | None
    retained_sets: frozenset[StemBatchSetIdentity]
    requests: frozenset[StemDeleteRequestIdentity]
    logical_pending: frozenset[StemGenerationInterest]
    unavailable_reason: StemDeleteUnavailableReason | None

    @property
    def set_identities(self) -> frozenset[StemBatchSetIdentity]:
        """Return exact unique versions without collapsing independent PAD rows."""
        if self.current_set is None:
            return self.retained_sets
        return self.retained_sets | {self.current_set}


@dataclass(frozen=True, slots=True)
class StemDeleteInventory:
    """Exactly all 216 original slots, including explicit unavailable assignments."""

    items: tuple[StemDeleteItem | None, ...]

    def __post_init__(self) -> None:
        """Reject narrowed scope or rows assigned to a different original slot."""
        if (
            type(self.items) is not tuple
            or len(self.items) != NUM_SAMPLES
            or any(
                item is not None
                and (
                    not isinstance(item, StemDeleteItem)
                    or type(item.pad_id) is not int
                    or item.pad_id != pad_id
                )
                for pad_id, item in enumerate(self.items)
            )
        ):
            message = f"stem delete inventory must retain exactly {NUM_SAMPLES} original slots"
            raise ValueError(message)

    @property
    def set_identities(self) -> frozenset[StemBatchSetIdentity]:
        """Count shared exact versions once without granting filesystem authority."""
        return frozenset(
            identity for item in self.items if item is not None for identity in item.set_identities
        )


def stem_delete_request_identity(intent: StemGenerationIntent) -> StemDeleteRequestIdentity:
    """Project the existing admission token without retaining its mutable holder."""
    return StemDeleteRequestIdentity(intent.material_key, intent.admission_sequence)


def capture_stem_delete_inventory(
    metadata: tuple[StemDeletePadMetadata | None, ...],
) -> StemDeleteInventory:
    """Detach the controller's no-I/O image of all original loaded assignments.

    Args:
        metadata: Exactly 216 control-owned metadata projections in slot order.
            Empty slots are None; occupied slots lacking source identity remain
            explicit unavailable rows. Settings and transient playback state are
            deliberately absent. The caller must capture outside image fences.

    Returns:
        Immutable warning scope with no references to mutable cache entries.
    """
    if type(metadata) is not tuple or len(metadata) != NUM_SAMPLES:
        message = f"stem delete metadata must cover exactly {NUM_SAMPLES} original slots"
        raise ValueError(message)
    items = tuple(_capture_item(row) if row is not None else None for row in metadata)
    return StemDeleteInventory(items)


def stem_delete_item_currentness(
    item: StemDeleteItem,
    *,
    read_metadata: Callable[[int], StemDeletePadMetadata | None],
    deferred: bool = False,
) -> StemDeleteCurrentness:
    """Validate the original target after its actual arrangement fence opens.

    A fence returns deferred before invoking the reader, so no half-published
    image is inspected. Missing source identity fails closed. New request tokens
    are stale even for equal source/Settings; captured tokens may disappear on
    completion or cancellation without that disappearance alone being a change.

    Current and retained exact set images remain strict. In particular, a known
    Pair/settlement set becoming the durable current set conservatively skips the
    old warning. An output unknown at capture can never become a deletion target.
    Request phases, availability and Settings revision alone do not invalidate.
    Unadmitted logical caller tokens use their existing frozen identity, not their
    label or an object address. This retains exact withdrawal scope for the caller
    without admitting work or granting authority over a newer batch's token.

    Args:
        item: The original warning-confirmed row, never a replacement capture.
        read_metadata: A no-I/O projection from existing controller owners.
        deferred: Whether the original PAD currently has an arrangement fence.

    Returns:
        A metadata comparison only, never Native or retirement admission.
    """
    if deferred:
        return "deferred"
    if item.unavailable_reason is not None or item.source_identity is None:
        return "stale"
    try:
        metadata = read_metadata(item.pad_id)
        current = _capture_item(metadata) if metadata is not None else None
    except RuntimeError, TypeError, ValueError:
        return "stale"
    if current is None:
        return "stale"
    unchanged = (
        current.pad_id == item.pad_id
        and current.path == item.path
        and current.content == item.content
        and current.source_identity == item.source_identity
        and current.unavailable_reason is None
        and current.current_set == item.current_set
        and current.retained_sets == item.retained_sets
        and current.requests <= item.requests
        and current.logical_pending <= item.logical_pending
    )
    return "current" if unchanged else "stale"


def _capture_item(metadata: StemDeletePadMetadata) -> StemDeleteItem:
    if (
        not isinstance(metadata, StemDeletePadMetadata)
        or type(metadata.pad_id) is not int
        or not 0 <= metadata.pad_id < NUM_SAMPLES
        or not isinstance(metadata.path, str)
        or not metadata.path
    ):
        message = "stem delete metadata must retain its original PAD, path and request identities"
        raise ValueError(message)
    if any(
        not isinstance(request, StemDeleteRequestIdentity) for request in metadata.requests
    ) or any(not isinstance(token, StemGenerationInterest) for token in metadata.logical_pending):
        message = "stem delete metadata must retain its original request identities"
        raise ValueError(message)
    source = _source_identity(metadata.source_identity)
    current = _set_identity(metadata.current_entry) if metadata.current_entry is not None else None
    retained = frozenset(_set_identity(entry) for entry in metadata.retained_entries)
    if current is not None:
        retained -= {current}
    return StemDeleteItem(
        pad_id=metadata.pad_id,
        path=metadata.path,
        content=metadata.content.model_copy(deep=True) if metadata.content is not None else None,
        source_identity=source,
        current_set=current,
        retained_sets=retained,
        requests=frozenset(metadata.requests),
        logical_pending=frozenset(metadata.logical_pending),
        unavailable_reason="Loaded source identity unavailable" if source is None else None,
    )


def _set_identity(entry: StemCacheEntry) -> StemBatchSetIdentity:
    return StemBatchSetIdentity(
        source_version=entry.source_version,
        cache_dir=entry.cache_dir,
        stems=tuple((kind, entry.stems.path_for(kind)) for kind in sorted(STEM_KINDS)),
        fingerprint_schema=entry.fingerprint_schema,
        generation_fingerprint=entry.generation_fingerprint,
        producer_identity=entry.producer_identity,
        pair_json=entry.pair.model_dump_json() if entry.pair is not None else None,
    )


def _source_identity(identity: StemBatchSourceIdentity | None) -> StemBatchSourceIdentity | None:
    if not isinstance(identity, tuple) or len(identity) != 4:
        return None
    generation, digest, frames, rate = identity
    if (
        type(generation) is not int
        or generation < 0
        or not isinstance(digest, str)
        or not digest
        or any(type(value) is not int or value <= 0 for value in (frames, rate))
    ):
        return None
    return generation, digest, frames, rate
