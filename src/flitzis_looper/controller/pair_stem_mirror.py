"""Prepared surviving Stem control placements, without Native mutation authority.

The future Pair coordinator supplies its own opaque acknowledged Native receipt.
This helper only exchanges existing control owners after that trusted validator
accepts both captured participants. It neither moves audio nor completes a Pair.
"""

import threading
from contextlib import ExitStack
from dataclasses import dataclass, fields, is_dataclass
from typing import TYPE_CHECKING, cast

from pydantic import BaseModel

from flitzis_looper.constants import NUM_SAMPLES

if TYPE_CHECKING:
    from collections.abc import Callable
    from pathlib import Path
    from typing import Protocol

    from flitzis_looper.controller.stem_job import (
        StemGenerationJob,
        StemSubscriber,
        _PreparedStemSubscribers,
    )
    from flitzis_looper.controller.stems import StemController
    from flitzis_looper.models import StemCacheEntry

    class _PlacedStemRow(Protocol):
        sample_id: int


_MISSING = object()
_STEM_TABLES = (
    "_generation_intents",
    "_verified_sets",
    "_verified_candidates",
    "_generation_sources",
    "_disk_selection_settlements",
    "_pending_stem_publications",
    "_restored_stem_candidates",
    "_residency_errors",
    "_demands",
    "_resident_pairs",
    "_pair_release_ordered",
    "_fenced_pair_ids",
    "_pending_full_mix_releases",
)
_SESSION_TABLES = (
    "stem_generating_sample_ids",
    "stem_generation_source_versions",
    "stem_generation_progress",
    "stem_generation_stage",
    "stem_generation_errors",
    "stem_generation_diagnostics",
)


@dataclass(frozen=True, slots=True)
class _Participant:
    sample_id: int
    content_id: str | None
    source_path: str | None
    source_signature: object
    entry: StemCacheEntry | None


@dataclass(frozen=True, slots=True)
class _RowGuard:
    """Exact controller-owned row and field owners, never receipt metadata equality."""

    owner: object
    captured: tuple[tuple[str, object], ...]
    cancellation: tuple[threading.Event, bool] | None

    @classmethod
    def capture(cls, owner: object, *, waiting_worker: bool = False) -> _RowGuard:
        if not is_dataclass(owner) or isinstance(owner, type):
            message = "Stem pair row is not a supported controller dataclass"
            raise TypeError(message)
        ignored = {"prepared", "error", "compatibility_rejected"} if waiting_worker else set()
        captured = tuple(
            (field.name, getattr(owner, field.name))
            for field in fields(owner)
            if field.name not in ignored
        )
        cancelled = getattr(owner, "cancelled", None)
        cancellation = (
            (cancelled, cancelled.is_set()) if isinstance(cancelled, threading.Event) else None
        )
        return cls(owner, captured, cancellation)

    def validate(self, own_placement: tuple[int, int] | None = None) -> None:
        if any(
            getattr(self.owner, name) is not value
            and not (
                name == "sample_id"
                and own_placement is not None
                and value == own_placement[0]
                and getattr(self.owner, name) == own_placement[1]
            )
            for name, value in self.captured
        ):
            message = "Stem pair control row changed after mirror preparation"
            raise RuntimeError(message)
        if self.cancellation is not None:
            event, cancelled = self.cancellation
            if event.is_set() != cancelled:
                message = "Stem pair preparation cancellation changed after mirror capture"
                raise RuntimeError(message)


@dataclass(frozen=True, slots=True)
class _Table:
    owner: object
    name: str
    source: _Participant
    target: _Participant
    source_row: object
    target_row: object

    @classmethod
    def capture(
        cls, owner: object, name: str, source: _Participant, target: _Participant
    ) -> _Table:
        current = getattr(owner, name)
        if isinstance(current, dict):
            return cls(
                owner,
                name,
                source,
                target,
                current.get(source.sample_id, _MISSING),
                current.get(target.sample_id, _MISSING),
            )
        if isinstance(current, set):
            return cls(
                owner,
                name,
                source,
                target,
                source.sample_id in current,
                target.sample_id in current,
            )
        message = f"Stem pair table {name} is not a supported control container"
        raise TypeError(message)

    def merged(self, *, allow_own_after: bool = False) -> object:
        """Allocate the final image from latest unrelated rows before any exchange."""
        current = getattr(self.owner, self.name)
        if isinstance(current, dict):
            return self._merged_dict(current, allow_own_after=allow_own_after)
        if isinstance(current, set):
            return self._merged_set(current, allow_own_after=allow_own_after)
        message = f"Stem pair table {self.name} changed its control container type"
        raise RuntimeError(message)

    def _merged_dict(
        self, current: dict[int, object], *, allow_own_after: bool
    ) -> dict[int, object]:
        source_id, target_id = self.source.sample_id, self.target.sample_id
        before = (
            current.get(source_id, _MISSING) is self.source_row
            and current.get(target_id, _MISSING) is self.target_row
        )
        after = (
            current.get(source_id, _MISSING)
            is (self.target_row if self.target.content_id is not None else _MISSING)
            and current.get(target_id, _MISSING) is self.source_row
        )
        if not before and not (allow_own_after and after):
            message = f"Stem pair participant table {self.name} changed after preparation"
            raise RuntimeError(message)
        merged = current.copy()
        merged.pop(source_id, None)
        merged.pop(target_id, None)
        if self.source_row is not _MISSING:
            merged[target_id] = self.source_row
        if self.target.content_id is not None and self.target_row is not _MISSING:
            merged[source_id] = self.target_row
        return merged

    def _merged_set(self, current: set[int], *, allow_own_after: bool) -> set[int]:
        source_id, target_id = self.source.sample_id, self.target.sample_id
        before = (source_id in current) == self.source_row and (
            target_id in current
        ) == self.target_row
        after = (source_id in current) == (
            self.target.content_id is not None and self.target_row
        ) and (target_id in current) == self.source_row
        if not before and not (allow_own_after and after):
            message = f"Stem pair participant table {self.name} changed after preparation"
            raise RuntimeError(message)
        merged = current - {source_id, target_id}
        if self.source_row:
            merged.add(target_id)
        if self.target.content_id is not None and self.target_row:
            merged.add(source_id)
        return merged


def _store_pair_image(owner: object, name: str, image: object) -> None:
    """Exchange a prepared owner image without validation or participant callbacks."""
    if isinstance(owner, BaseModel):
        owner.__dict__[name] = image
    else:
        setattr(owner, name, image)


def _subscriber_image(
    plan: _PreparedStemSubscribers, *, allow_own_after: bool
) -> dict[int, StemSubscriber]:
    """Merge exact before/own-after subscribers while all actual job locks are held."""
    job = plan.job
    current = job._subscribers
    before = (
        current.get(plan.source_id) is plan.source and current.get(plan.target_id) is plan.target
    )
    after = (
        current.get(plan.source_id) is plan.moved_target
        and current.get(plan.target_id) is plan.moved_source
    )
    if job._cancelled or job._disposed or not (before or (allow_own_after and after)):
        message = "Stem pair subscriber ownership changed after mirror preparation"
        raise RuntimeError(message)
    merged = current.copy()
    merged.pop(plan.source_id, None)
    merged.pop(plan.target_id, None)
    if plan.moved_source is not None:
        merged[plan.target_id] = plan.moved_source
    if plan.moved_target is not None:
        merged[plan.source_id] = plan.moved_target
    return merged


def _strict_slot(sample_id: int) -> None:
    if type(sample_id) is not int or not 0 <= sample_id < NUM_SAMPLES:
        message = "Stem pair participant must name a valid integer pad slot"
        raise ValueError(message)


def _participant(
    controller: StemController,
    sample_id: int,
    content_id: str | None,
    source_path: str | None,
    source_signature: object,
) -> _Participant:
    _strict_slot(sample_id)
    identity = controller._project.pad_content[sample_id]
    if (
        (identity.instance_id if identity is not None else None) != content_id
        or controller._project.sample_paths[sample_id] != source_path
        or (content_id is None) != (source_path is None)
    ):
        message = "Stem pair capture differs from the actual controller participant"
        raise RuntimeError(message)
    return _Participant(
        sample_id,
        content_id,
        source_path,
        source_signature,
        controller._project.stem_cache[sample_id],
    )


def _validate_row_content(row: object, participant: _Participant) -> None:
    if hasattr(row, "content_id") and row.content_id != participant.content_id:
        message = "Stem pair control owner belongs to another content instance"
        raise RuntimeError(message)
    if hasattr(row, "source_path") and row.source_path != participant.source_path:
        message = "Stem pair control owner belongs to another captured source"
        raise RuntimeError(message)


class PreparedPairStemMirror:
    """Own an idempotent surviving-control projection, never a physical Pair permission.

    Preparation and application run on the same control thread. The enclosing
    coordinator must retain its layout/config fence and supply a validator of
    the real opaque own Native ACK, both captured Native keys and their committed
    placement. JSON fields, equal PCM and equal numeric generations are insufficient.
    """

    def __init__(
        self,
        controller: StemController,
        source: _Participant,
        target: _Participant,
        validate_acknowledgement: Callable[[object], bool],
    ) -> None:
        source_content_id = source.content_id
        if source_content_id is None:
            message = "Stem pair source must retain a surviving content identity"
            raise ValueError(message)
        self._controller = controller
        self._owners = (
            controller._project,
            controller._session,
            controller._pair_preparations,
            controller._publication_retries,
            controller._committed_residency,
        )
        self._source = source
        self._target = target
        self._validate_acknowledgement = validate_acknowledgement
        self._control_thread = threading.get_ident()
        self._applied = False
        self._acknowledgement: object | None = None
        self._tables = (
            tuple(_Table.capture(controller, name, source, target) for name in _STEM_TABLES)
            + tuple(
                _Table.capture(controller._session, name, source, target)
                for name in _SESSION_TABLES
            )
            + (
                _Table.capture(controller._pair_preparations, "pending", source, target),
                _Table.capture(controller._publication_retries, "pending", source, target),
                _Table.capture(controller._committed_residency, "_selections", source, target),
                _Table.capture(controller._committed_residency, "_live_selections", source, target),
            )
        )
        self._rows: list[_RowGuard] = []
        self._placements: list[tuple[_PlacedStemRow, int, int]] = []
        self._capture_control_rows()
        self._jobs: tuple[tuple[Path, _PreparedStemSubscribers], ...] = tuple(
            (
                path,
                job.prepare_pair_subscribers(
                    source.sample_id, target.sample_id, source_content_id, target.content_id
                ),
            )
            for path, job in controller._jobs.items()
            if any(
                subscriber.sample_id in {source.sample_id, target.sample_id}
                for subscriber in job.subscribers()
            )
        )
        for _, plan in self._jobs:
            for subscriber in (plan.source, plan.target):
                if subscriber is not None and (
                    controller._generation_sources.get(subscriber.sample_id)
                    is not subscriber.source
                    or controller._generation_intents.get(subscriber.sample_id)
                    is not subscriber.intent
                ):
                    message = "Stem pair subscriber has no matching current generation owner"
                    raise RuntimeError(message)

    @property
    def applied(self) -> bool:
        """Whether this exact prepared mirror was exchanged once after its own ACK."""
        return self._applied

    @property
    def source_signature(self) -> object:
        """Return the captured opaque source owner for the coordinator's ACK validator."""
        return self._source.source_signature

    @property
    def target_signature(self) -> object:
        """Return the captured target owner, including its actual empty SlotSnapshot."""
        return self._target.source_signature

    @property
    def projected_entries(self) -> tuple[tuple[int, StemCacheEntry | None], ...]:
        """Retain current metadata identities needed by existing owner identity guards.

        These entries alone claim no availability. The enclosing coordinator
        binds them into its separately validated complete project candidate only
        with the matching Native owners; this helper never writes project intent.
        """
        return (
            (self._source.sample_id, self._target.entry if self._target.content_id else None),
            (self._target.sample_id, self._source.entry),
        )

    def _watch_row(
        self,
        row: object,
        participant: _Participant,
        *,
        placement: bool = False,
        waiting_worker: bool = False,
    ) -> None:
        _validate_row_content(row, participant)
        if any(guard.owner is row for guard in self._rows):
            return
        self._rows.append(_RowGuard.capture(row, waiting_worker=waiting_worker))
        if placement:
            placed_row = cast("_PlacedStemRow", row)
            sample_id = placed_row.sample_id
            if sample_id != participant.sample_id:
                message = "Stem pair current owner has a conflicting placement"
                raise RuntimeError(message)
            destination = (
                self._target.sample_id if participant is self._source else self._source.sample_id
            )
            self._placements.append((placed_row, sample_id, destination))

    def _capture_control_rows(self) -> None:
        controller = self._controller
        for participant in (self._source, self._target):
            self._capture_participant_rows(participant)
            for row in controller._committed_residency._pair_control_rows(participant.sample_id):
                if participant.content_id is None:
                    message = "Empty Move target still owns a committed stem selection"
                    raise RuntimeError(message)
                self._watch_row(row, participant)

    def _capture_participant_rows(self, participant: _Participant) -> None:
        controller = self._controller
        sample_id = participant.sample_id
        owned_rows = (
            controller._generation_sources,
            controller._pair_preparations.pending,
            controller._disk_selection_settlements,
            controller._publication_retries.pending,
            controller._session.stem_generating_sample_ids,
            controller._resident_pairs,
        )
        if participant.content_id is None and any(sample_id in rows for rows in owned_rows):
            message = "Empty Move target still owns current stem work or publication"
            raise RuntimeError(message)
        for name in ("_demands", "_verified_sets", "_pending_stem_publications"):
            row = getattr(controller, name).get(sample_id)
            if row is not None:
                if participant.content_id is None:
                    message = "Empty Move target still owns a current stem participant"
                    raise RuntimeError(message)
                self._watch_row(row, participant, placement=name == "_pending_stem_publications")
        self._capture_pending_rows(participant)

    def _capture_pending_rows(self, participant: _Participant) -> None:
        controller = self._controller
        sample_id = participant.sample_id
        preparation = controller._pair_preparations.pending.get(sample_id)
        if preparation is not None:
            # The worker keeps its historical capture ID. Only the pending
            # delivery key moves; the normal collector updates sample_id
            # after this exact worker has actually returned.
            self._watch_row(preparation, participant, waiting_worker=True)
        settlement = controller._disk_selection_settlements.get(sample_id)
        if settlement is not None:
            self._watch_row(settlement, participant)
            self._watch_row(settlement.request, participant, placement=True)
        retry = controller._publication_retries.pending.get(sample_id)
        if retry is not None:
            self._watch_row(retry, participant)
            self._watch_row(retry.pending, participant, placement=True)

    def _validate_project(self) -> None:
        project = self._controller._project
        before = (
            (self._source.content_id, self._source.source_path, self._source.entry),
            (self._target.content_id, self._target.source_path, self._target.entry),
        )
        after = (
            before[1] if self._target.content_id is not None else (None, None, None),
            before[0],
        )
        current = tuple(
            (
                identity.instance_id if identity is not None else None,
                project.sample_paths[participant.sample_id],
                project.stem_cache[participant.sample_id],
            )
            for participant in (self._source, self._target)
            for identity in (project.pad_content[participant.sample_id],)
        )
        if not any(
            all(
                actual[:2] == expected[:2] and actual[2] is expected[2]
                for actual, expected in zip(current, image, strict=True)
            )
            for image in (before, after)
        ):
            message = "Stem pair project participants changed outside this prepared mirror"
            raise RuntimeError(message)

    def _validate_owners(self) -> None:
        controller = self._controller
        current = (
            controller._project,
            controller._session,
            controller._pair_preparations,
            controller._publication_retries,
            controller._committed_residency,
        )
        if controller._shutting_down or any(
            actual is not expected for actual, expected in zip(current, self._owners, strict=True)
        ):
            message = "Stem pair controller ownership changed or shutdown began"
            raise RuntimeError(message)

    def _job_images_locked(self) -> tuple[tuple[StemGenerationJob, dict[int, StemSubscriber]], ...]:
        controller = self._controller
        captured = {id(plan.job) for _, plan in self._jobs}
        for job in controller._jobs.values():
            if id(job) not in captured and (
                self._source.sample_id in job._subscribers
                or self._target.sample_id in job._subscribers
            ):
                message = "A newer stem job owns a captured pair participant"
                raise RuntimeError(message)
        images: list[tuple[StemGenerationJob, dict[int, StemSubscriber]]] = []
        for path, plan in self._jobs:
            if controller._jobs.get(path) is not plan.job:
                message = "Stem pair physical job owner changed after preparation"
                raise RuntimeError(message)
            images.append((
                plan.job,
                _subscriber_image(plan, allow_own_after=self._acknowledgement is not None),
            ))
        return tuple(images)

    def apply(self, acknowledgement: object) -> None:
        """Exchange all prepared rows once, retaining every living reader and receipt.

        Every expected rejection, Native validation and allocation precedes the
        first exchange. All current job locks stay held through validation, final
        unrelated-row merge and every subscriber/controller table exchange.
        """
        if threading.get_ident() != self._control_thread:
            message = "Stem pair mirror must apply on its preparation control thread"
            raise RuntimeError(message)
        if self._acknowledgement is not None and acknowledgement is not self._acknowledgement:
            message = "Applied stem pair mirror cannot accept another transaction receipt"
            raise RuntimeError(message)
        if self._applied:
            return
        controller = self._controller
        jobs = {id(job): job for job in controller._jobs.values()}
        jobs.update((id(plan.job), plan.job) for _, plan in self._jobs)
        with ExitStack() as locks:
            for _, job in sorted(jobs.items()):
                locks.enter_context(job._lock)
            self._apply_locked(acknowledgement, jobs)

    def _apply_locked(
        self, acknowledgement: object, locked_jobs: dict[int, StemGenerationJob]
    ) -> None:
        if acknowledgement is None or self._validate_acknowledgement(acknowledgement) is not True:
            message = "Stem pair mirror has no matching opaque own Native acknowledgement"
            raise RuntimeError(message)
        if any(id(job) not in locked_jobs for job in self._controller._jobs.values()):
            message = "Stem job registry changed during Native pair receipt validation"
            raise RuntimeError(message)
        self._validate_owners()
        self._validate_project()
        own_placements = (
            {id(row): (before, after) for row, before, after in self._placements}
            if self._acknowledgement is not None
            else {}
        )
        for row_guard in self._rows:
            row_guard.validate(own_placements.get(id(row_guard.owner)))
        table_images = tuple(
            (table, table.merged(allow_own_after=self._acknowledgement is not None))
            for table in self._tables
        )
        job_images = self._job_images_locked()
        # No admission, cancellation, re-registration, owner release or
        # Native call is permitted after the first prepared store below.
        self._acknowledgement = acknowledgement
        for job, job_image in job_images:
            _store_pair_image(job, "_subscribers", job_image)
        for placed_row, _, destination in self._placements:
            _store_pair_image(placed_row, "sample_id", destination)
        for table, table_image in table_images:
            _store_pair_image(table.owner, table.name, table_image)
        self._applied = True


def prepare_pair_stem_mirror(
    controller: StemController,
    source_id: int,
    target_id: int,
    *,
    source_content_id: str,
    target_content_id: str | None,
    source_path: str,
    target_path: str | None,
    source_signature: object,
    target_signature: object,
    validate_acknowledgement: Callable[[object], bool],
) -> PreparedPairStemMirror:
    """Prepare only the Stem lane of a separately authorized Native Move or Swap.

    The two signatures retain actual Native capture objects for the trusted
    coordinator validator. Their metadata is never interpreted as authority here.
    The enclosing transaction continues to own project/config/custody, transport,
    timing, loader, HoldRelease and feedback settlement.
    """
    source = _participant(controller, source_id, source_content_id, source_path, source_signature)
    target = _participant(controller, target_id, target_content_id, target_path, target_signature)
    if (
        source_id == target_id
        or source_content_id == target_content_id
        or not source_content_id
        or source_signature is None
        or target_signature is None
    ):
        message = "Stem pair must name two distinct placements and surviving content identities"
        raise ValueError(message)
    if controller._shutting_down:
        message = "Stem pair mirror cannot prepare during controller shutdown"
        raise RuntimeError(message)
    return PreparedPairStemMirror(controller, source, target, validate_acknowledgement)
