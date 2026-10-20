"""One acknowledged Bank's prepared Python projection and retained old readers.

The coordinator owns the Native parent, Config writer, observer fences, journal
and assignment publication. This helper grants none of those permissions. It
replaces only the sealed target bank; source and outside owners stay in place.
"""

import json
import threading
from contextlib import ExitStack
from dataclasses import dataclass, replace
from typing import TYPE_CHECKING, cast

from flitzis_looper.bank_snapshot import _project_content_ids
from flitzis_looper.controller.bank_arrangement_journal import (
    BankArrangementJournal,
    _resolved_inheritance_current,
    project_with_journal_bank,
)
from flitzis_looper.controller.committed_stem_residency import (
    CommittedStemResidency,
    _CommittedSelection,
)
from flitzis_looper.controller.pair_control_mirror import (
    _LOADER_CONTAINERS,
    _RENDER_FIELDS,
    _RESIDENCY_CONTAINERS,
    _SESSION_CONTAINERS,
    _SESSION_PLACEMENTS,
    _TIMING_CONTAINERS,
    _Container,
    _references,
    _store,
)
from flitzis_looper.controller.pair_stem_mirror import _SESSION_TABLES, _STEM_TABLES, _RowGuard
from flitzis_looper.copy_snapshot import (
    CONTENT_FIELDS,
    canonical_snapshot_json,
    musical_content_data,
)
from flitzis_looper.models import ProjectState, SessionState

if TYPE_CHECKING:
    from collections.abc import Callable, Iterable
    from pathlib import Path

    from flitzis_looper.controller.app import AppController
    from flitzis_looper.controller.stem_job import PreparedStemSubscriberDetach, StemGenerationJob
    from flitzis_looper.models import StemCacheEntry
    from flitzis_looper.ui.context import WaveformEditorActions
    from flitzis_looper_audio import BankMutationTicket


def _validate_admission(
    app: AppController, journal: BankArrangementJournal, *, job_locks_held: bool
) -> None:
    """Reject unsettled target collectors before Native reservation or preparation.

    These collectors still settle by original slot and have no aggregate retired-
    owner handoff. The coordinator calls this on its control thread before
    reserving Native participants, and keeps target admission/observers fenced.
    The mirror repeats it under all job locks before its first store. Shared
    inference jobs have an exact subscriber handoff and remain supported.
    """
    bank = BankArrangementJournal.model_validate(journal).bank.projected()
    targets = frozenset(row.target_id for row in bank.slots)
    participants = targets | frozenset(
        row.source_id for row in bank.slots if row.source_id is not None
    )
    if any(
        slot in app._session.loading_sample_ids or slot in app._session.pending_sample_paths
        for slot in participants
    ):
        message = "Bank participants still have an incoming source assignment"
        raise RuntimeError(message)
    if any(
        slot in app.stems._pair_preparations.pending
        or slot in app.stems._disk_selection_settlements
        or slot in app.loader._incoming_loads
        for slot in targets
    ) or any(slot in targets for slot, _ in app.loader._load_retirements):
        message = "Bank target still owns an unsettled preparation, load or durable selection"
        raise RuntimeError(message)
    original = {
        row.target_id: (
            row.before.source_identity.instance_id if row.before.source_identity else None
        )
        for row in bank.slots
    }
    for job in app.stems._jobs.values():
        with ExitStack() as lock:
            if not job_locks_held:
                lock.enter_context(job._lock)
            for slot, subscriber in job._subscribers.items():
                if slot in targets and (
                    original[slot] is None
                    or subscriber.sample_id != slot
                    or subscriber.content_id != original[slot]
                ):
                    message = "Bank stem interest belongs to another original target content"
                    raise RuntimeError(message)


def validate_bank_mirror_admission(app: AppController, journal: BankArrangementJournal) -> None:
    """Check original collectors and job interests before any Native reservation."""
    _validate_admission(app, journal, job_locks_held=False)


@dataclass(frozen=True, slots=True)
class _Table:
    owner: object
    name: str
    targets: frozenset[int]
    before: object
    after: object
    compound: bool = False

    def contains(self, key: object) -> bool:
        slot = key[0] if self.compound and isinstance(key, tuple) else key
        return type(slot) is int and slot in self.targets

    def selected(self, current: object) -> object:
        if isinstance(current, dict):
            return {key: row for key, row in current.items() if self.contains(key)}
        if isinstance(current, set):
            return current.intersection(self.targets)
        if isinstance(current, list):
            return tuple(current[slot] for slot in sorted(self.targets))
        message = f"Bank control table {self.name} has an unsupported container"
        raise TypeError(message)

    def merged(self, *, allow_own_after: bool) -> object:
        current = getattr(self.owner, self.name)
        selected = self.selected(current)
        if not _Container._matches(selected, self.before) and not (
            allow_own_after and _Container._matches(selected, self.after)
        ):
            message = f"Bank control participants in {self.name} changed outside the owning mirror"
            raise RuntimeError(message)
        if isinstance(current, dict) and isinstance(self.after, dict):
            merged = {key: row for key, row in current.items() if not self.contains(key)}
            return merged | self.after
        if isinstance(current, set) and isinstance(self.after, set):
            return (current - self.targets) | self.after
        if isinstance(current, list) and isinstance(self.after, tuple):
            image = current.copy()
            for slot, value in zip(sorted(self.targets), self.after, strict=True):
                image[slot] = value
            return image
        message = f"Bank control table {self.name} changed its container type"
        raise TypeError(message)


@dataclass(slots=True)
class _Cleanup:
    action: Callable[[], object]
    complete: bool = False

    def settle(self) -> None:
        if not self.complete:
            self.action()
            self.complete = True


class PreparedBankMirror:
    """Exchange all36 targets after one own ACK, retaining interrupted old-owner cleanup."""

    def __init__(
        self,
        app: AppController,
        journal: BankArrangementJournal,
        ticket: BankMutationTicket,
        validate_acknowledgement: Callable[[object], bool],
        waveform_views: tuple[WaveformEditorActions, ...],
    ) -> None:
        self._app = app
        self._journal = BankArrangementJournal.model_validate(journal)
        self._bank = self._journal.bank.projected()
        self._targets = frozenset(row.target_id for row in self._bank.slots)
        self._ticket = ticket
        self._validator = validate_acknowledgement
        self._thread = threading.get_ident()
        self._acknowledgement: object | None = None
        self._applied = False
        self._cleanup_cursor = 0
        self._cleanup: list[_Cleanup] = []
        self._tables: list[_Table] = []
        self._views = waveform_views
        self._references = (
            *_references(app, waveform_views),
            (app, "stems", app.stems),
            (app.stems, "_project", app._project),
            (app.stems, "_session", app._session),
            (app.stems, "_audio", app._audio),
            (app.stems, "_jobs", app.stems._jobs),
            (app.stems, "_committed_residency", app.stems._committed_residency),
            (app.stems, "_pair_preparations", app.stems._pair_preparations),
            (app.stems, "_publication_retries", app.stems._publication_retries),
        )
        validate_bank_mirror_admission(app, self._journal)
        candidate = project_with_journal_bank(app._project, self._journal)
        self._components = self._read_components()
        registrations = ticket.committed_stem_tickets()
        if len(registrations) != len(self._bank.slots):
            message = "Bank receipt must retain exactly36 copied registrations"
            raise ValueError(message)
        self._selections: dict[int, _CommittedSelection] = {}
        for row, available, registration in zip(
            self._bank.slots, self._components, registrations, strict=True
        ):
            entry = candidate.stem_cache[row.target_id]
            if entry is None:
                if available or registration is not None:
                    message = "Empty Bank image cannot inherit Native components or registration"
                    raise ValueError(message)
                continue
            if (entry.pair is None) != (registration is None):
                message = "Bank copied stem metadata differs from its own registration"
                raise ValueError(message)
            entry = type(entry).model_validate(entry.model_dump() | {"available": available})
            candidate.stem_cache[row.target_id] = entry
            if registration is not None:
                identity = candidate.pad_content[row.target_id]
                self._selections[row.target_id] = CommittedStemResidency.prepare_copied_selection(
                    entry,
                    identity.instance_id if identity is not None else None,
                    candidate.sample_paths[row.target_id],
                    registration,
                )
        self._prepare_tables(candidate)
        self._prepare_retirement()
        self._row_guards = tuple(
            _RowGuard.capture(row)
            for slot in self._targets
            for row in (
                *app.stems._committed_residency._pair_control_rows(slot),
                app.stems._pending_stem_publications.get(slot),
                app.stems._publication_retries.pending.get(slot),
            )
            if row is not None
        )
        target_ids = tuple(
            (
                row.target_id,
                row.before.source_identity.instance_id if row.before.source_identity else None,
            )
            for row in self._bank.slots
        )
        self._jobs: tuple[tuple[Path, PreparedStemSubscriberDetach], ...] = tuple(
            (path, job.prepare_subscriber_detach(target_ids))
            for path, job in app.stems._jobs.items()
            if any(subscriber.sample_id in self._targets for subscriber in job.subscribers())
        )
        self._cleanup.extend(_Cleanup(plan.settle) for _, plan in self._jobs)

    @property
    def applied(self) -> bool:
        """Whether this plan completed all of its acknowledged Python stores."""
        return self._applied

    @property
    def settled(self) -> bool:
        """Whether every retained retired handle was settled or handed to its existing reader."""
        return self._applied and self._cleanup_cursor == len(self._cleanup)

    @property
    def projected_entries(self) -> tuple[tuple[int, StemCacheEntry | None], ...]:
        """Expose actual copied availability, independently from registration and residency."""
        table = next(
            table
            for table in self._tables
            if table.owner is self._app._project and table.name == "stem_cache"
        )
        values = cast("tuple[StemCacheEntry | None, ...]", table.after)
        return tuple(zip(sorted(self._targets), values, strict=True))

    def _require_owner(self) -> None:
        if threading.get_ident() != self._thread or self._app.stems._shutting_down:
            message = "Bank mirror lost its original control thread or entered shutdown"
            raise RuntimeError(message)
        if any(getattr(owner, name) is not value for owner, name, value in self._references):
            message = "Bank mirror component ownership changed after preparation"
            raise RuntimeError(message)

    def _read_components(self) -> tuple[bool, ...]:
        data = json.loads(self._ticket.metadata_json())
        message = "Bank mirror has no complete acknowledged Native target image"
        if (
            not isinstance(data, dict)
            or not {"status", "control_mirrored", "source_bank", "target_bank", "targets"}
            <= data.keys()
        ):
            raise ValueError(message)
        if data.get("status") != "accepted" or data.get("control_mirrored") is not True:
            raise ValueError(message)
        if (
            data.get("source_bank") != self._bank.source_bank
            or data.get("target_bank") != self._bank.target_bank
            or type(data.get("target_bank")) is not int
            or (self._bank.source_bank is not None and type(data.get("source_bank")) is not int)
        ):
            raise ValueError(message)
        targets = data.get("targets")
        if not isinstance(targets, list) or len(targets) != len(self._bank.slots):
            message = "Bank Native target observation is not the complete ordered bank"
            raise ValueError(message)
        components: list[bool] = []
        for row, observed in zip(self._bank.slots, targets, strict=True):
            if (
                not isinstance(observed, dict)
                or type(observed.get("slot")) is not int
                or observed["slot"] != row.target_id
                or type(observed.get("components_available")) is not bool
                or any(
                    type(observed.get(name)) is not int
                    or observed[name] < 0
                    or (observed[name] == 0) != (row.after.source_identity is None)
                    for name in ("after_lifetime", "source_generation")
                )
            ):
                message = "Bank target observation lacks its own identity or explicit availability"
                raise ValueError(message)
            components.append(observed["components_available"])
        return tuple(components)

    def _add(self, owner: object, names: Iterable[str], *, defaults: object | None = None) -> None:
        for name in names:
            current = getattr(owner, name)
            if isinstance(current, dict):
                after: object = {}
            elif isinstance(current, set):
                after = set()
            elif isinstance(current, list) and defaults is not None:
                after = tuple(getattr(defaults, name)[slot] for slot in sorted(self._targets))
            else:
                message = f"Bank control table {name} has no prepared empty image"
                raise TypeError(message)
            table = _Table(owner, name, self._targets, None, after)
            self._tables.append(replace(table, before=table.selected(current)))

    def _prepare_tables(self, candidate: ProjectState) -> None:
        app = self._app
        self._add(app._project, (name for _, name in CONTENT_FIELDS), defaults=candidate)
        self._add(app._session, _SESSION_CONTAINERS, defaults=SessionState())
        self._add(app.transport.residency, _RESIDENCY_CONTAINERS)
        self._add(app.accepted_timing, _TIMING_CONTAINERS)
        self._add(app.loader, _LOADER_CONTAINERS)
        self._add(app.loader._accepted_restore, ("_pending",))
        self._add(app.loader._key_analysis, ("_requests", "_errors"))
        self._add(app.transport.waveform, ("_readiness",))
        self._add(app.metering, ("_active_peak_sample_ids",))
        self._add(
            app.stems, (name for name in _STEM_TABLES if name != "_disk_selection_settlements")
        )
        self._add(app._session, _SESSION_TABLES)
        self._add(app.stems._publication_retries, ("pending",))
        residency = app.stems._committed_residency
        self._add(residency, ("_selections", "_live_selections"))
        self._tables[-2] = replace(self._tables[-2], after=self._selections)
        for view in self._views:
            self._add(view, ("_pad_view_ranges", "_pad_view_sources"))

    def _remember(self, action: Callable[[], object]) -> None:
        self._cleanup.append(_Cleanup(action))

    def _prepare_retirement(self) -> None:
        remembered: set[tuple[int, str]] = set()

        def remember(owner: object | None, name: str) -> None:
            if owner is not None and (id(owner), name) not in remembered:
                remembered.add((id(owner), name))
                self._remember(getattr(owner, name))

        for slot in self._targets:
            self._prepare_slot_retirement(slot, remember)

    def _prepare_slot_retirement(
        self, slot: int, remember: Callable[[object | None, str], None]
    ) -> None:
        app = self._app
        residency = app.transport.residency
        remember(residency._launched.get(slot), "cancel_launch")
        pending = residency._pending.get(slot)
        if pending is not None:
            remember(pending.ticket, "cancel")
            remember(pending.waiting_ticket, "cancel")
        remember(app.accepted_timing._preparations.get(slot), "cancel")
        publication = app.accepted_timing._publications.get(slot)
        if publication is not None:
            remember(publication.future, "cancel")
            remember(publication.original_future, "cancel")
        restore = app.loader._accepted_restore._pending.get(slot)
        if restore is not None:
            remember(restore.future, "cancel")
        for row in app.stems._committed_residency._pair_control_rows(slot):
            if isinstance(row, _CommittedSelection):
                remember(row.receipt, "cancel")
                remember(row.preload_receipt, "cancel")
                remember(row.ticket, "cancel")
        stem_pending = app.stems._pending_stem_publications.get(slot)
        retry = app.stems._publication_retries.pending.get(slot)
        for owned in (stem_pending, retry.pending if retry is not None else None):
            if owned is not None:
                remember(owned.prepared_pair, "discard")
                remember(owned.previous_lease, "release")
                remember(owned.retirement, "close")

    def projected_project(self) -> ProjectState:
        """Build the fixed complete after image, recognizing only this plan's own stores."""
        self._require_owner()
        _project_content_ids(self._app._project)
        _resolved_inheritance_current(self._app._project, self._bank)
        current_rows = tuple(
            musical_content_data(self._app._project, row.target_id) for row in self._bank.slots
        )
        before_rows = tuple(row.before.model_dump(mode="python") for row in self._bank.slots)
        after_rows = tuple(row.after.model_dump(mode="python") for row in self._bank.slots)
        for name, _ in CONTENT_FIELDS:
            current = tuple(row[name] for row in current_rows)
            if current != tuple(row[name] for row in before_rows) and not (
                self._acknowledgement is not None
                and current == tuple(row[name] for row in after_rows)
            ):
                message = "Bank musical table changed outside its sealed before or own after image"
                raise RuntimeError(message)
        detached = self._app._project.model_copy(deep=True)
        for table in self._tables:
            if table.owner is self._app._project:
                detached.__dict__[table.name] = table.merged(
                    allow_own_after=self._acknowledgement is not None
                )
        data = detached.model_dump(mode="json", exclude_computed_fields=True, warnings="error")
        candidate = ProjectState.model_validate_json(canonical_snapshot_json(data), strict=True)
        _project_content_ids(candidate)
        return candidate

    def _attributes(self) -> tuple[tuple[object, str, object], ...]:
        app = self._app
        images: list[tuple[object, str, object]] = []
        for name in _SESSION_PLACEMENTS:
            value = getattr(app._session, name)
            if type(value) is int and value in self._targets:
                images.append((app._session, name, None))
                if name == "tap_bpm_pad_id":
                    images.append((app._session, "tap_bpm_timestamps", []))
                if name == "waveform_editor_pad_id":
                    images.append((app._session, "waveform_editor_open", False))
        global_playback = app.transport.playback._global_playback
        pending = global_playback._pending
        if pending is not None:
            images.append((
                global_playback,
                "_pending",
                replace(
                    pending,
                    restore_ids=pending.restore_ids - self._targets,
                    start_ids=pending.start_ids - self._targets,
                ),
            ))
        deferred = global_playback._deferred
        if deferred is not None:
            images.append((
                global_playback,
                "_deferred",
                replace(deferred, sample_ids=deferred.sample_ids - self._targets),
            ))
        for view in self._views:
            if view._last_pad_id in self._targets:
                images.extend((view, name, None) for name in _RENDER_FIELDS)
        return tuple(images)

    def apply(self, acknowledgement: object) -> None:
        """Lock and validate every lane before the first prepared Python store."""
        self._require_owner()
        if self._acknowledgement is not None and acknowledgement is not self._acknowledgement:
            message = "Bank mirror cannot accept another transaction acknowledgement"
            raise RuntimeError(message)
        if self._applied:
            return
        jobs = {id(job): job for job in self._app.stems._jobs.values()}
        jobs.update((id(plan.job), plan.job) for _, plan in self._jobs)
        with ExitStack() as locks:
            for _, job in sorted(jobs.items()):
                locks.enter_context(job._lock)
            self._apply_locked(acknowledgement, jobs)

    def _apply_locked(
        self, acknowledgement: object, locked_jobs: dict[int, StemGenerationJob]
    ) -> None:
        if acknowledgement is None or self._validator(acknowledgement) is not True:
            message = "Bank mirror has no matching opaque actual own Native ACK"
            raise RuntimeError(message)
        if any(id(job) not in locked_jobs for job in self._app.stems._jobs.values()):
            message = "Stem job registry changed during Bank receipt validation"
            raise RuntimeError(message)
        self._require_owner()
        _validate_admission(self._app, self._journal, job_locks_held=True)
        if self._read_components() != self._components:
            message = "Bank copied component observation changed after mirror preparation"
            raise RuntimeError(message)
        for selection in self._selections.values():
            CommittedStemResidency.validate_copied_selection(selection)
        for guard in self._row_guards:
            guard.validate()
        self.projected_project()
        stores = self._prepare_stores_locked()
        # All locks, Native checks, whole-project checks and merged allocations
        # precede this point. Retain the same ACK before any interrupted store.
        self._acknowledgement = acknowledgement
        for owner, name, value in stores:
            _store(owner, name, value)
        self._applied = True

    def _prepare_stores_locked(self) -> list[tuple[object, str, object]]:
        captured = {id(plan.job) for _, plan in self._jobs}
        for job in self._app.stems._jobs.values():
            if id(job) not in captured and any(slot in self._targets for slot in job._subscribers):
                message = "A newer stem job owns a captured Bank target"
                raise RuntimeError(message)
        stores: list[tuple[object, str, object]] = []
        for path, plan in self._jobs:
            if self._app.stems._jobs.get(path) is not plan.job:
                message = "Bank physical stem job owner changed after preparation"
                raise RuntimeError(message)
            job_image = plan.merged_locked(allow_own_after=self._acknowledgement is not None)
            stores.append((plan.job, "_subscribers", job_image))
            if not job_image:
                stores.append((plan.job, "_cancelled", True))
        stores.extend(
            (
                table.owner,
                table.name,
                table.merged(allow_own_after=self._acknowledgement is not None),
            )
            for table in self._tables
        )
        stores.extend(self._attributes())
        return stores

    def settle_retired(self) -> bool:
        """Retry captured-handle cleanup without looking up any successor by slot."""
        self._require_owner()
        if not self._applied:
            return False
        while self._cleanup_cursor < len(self._cleanup):
            self._cleanup[self._cleanup_cursor].settle()
            self._cleanup_cursor += 1
        return True


def prepare_bank_arrangement_mirror(
    app: AppController,
    journal: BankArrangementJournal,
    ticket: BankMutationTicket,
    *,
    validate_acknowledgement: Callable[[object], bool],
    waveform_views: Iterable[WaveformEditorActions] = (),
) -> PreparedBankMirror:
    """Prepare one fixed Bank image; the coordinator retains every external owner."""
    views = tuple(waveform_views)
    if len({id(view) for view in views}) != len(views):
        message = "Bank waveform view registration contains duplicate owners"
        raise ValueError(message)
    return PreparedBankMirror(app, journal, ticket, validate_acknowledgement, views)
