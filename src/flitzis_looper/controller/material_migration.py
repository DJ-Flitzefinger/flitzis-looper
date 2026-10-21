"""One current-project material transaction; runtime ACKs never come from journal JSON."""

import json
from contextlib import suppress
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING
from uuid import uuid4

from flitzis_looper.controller.material_migration_cleanup import (
    ArtifactRequest,
    MigrationArtifactReconciler,
    MigrationCleanupQueue,
)
from flitzis_looper.controller.material_migration_grouping import (
    current_candidates,
    original_requests,
    retain_content_group,
)
from flitzis_looper.controller.material_migration_history import (
    check_alias_capacity,
    merge_artifacts,
    rollback_artifacts,
    successor_aliases,
)
from flitzis_looper.controller.material_migration_pair import prepare_migrated_pair
from flitzis_looper.controller.material_migration_recovery import inspect_migration_recovery
from flitzis_looper.controller.stem_cache import expected_stem_files
from flitzis_looper.material_migration_model import (
    MaterialCreationReceipt,
    MaterialMigrationAlias,
    MaterialMigrationJournal,
    MigrationAssignment,
)
from flitzis_looper.models import (
    PadContentIdentity,
    ProjectState,
    StemCacheEntry,
    validate_sample_id,
)
from flitzis_looper.project_materials import original_asset, resolve_asset
from flitzis_looper.stem_pair_selection import StemPairSelection
from flitzis_looper_audio import MaterialMigrationJournalStore, ProjectAssetLease

if TYPE_CHECKING:
    from collections.abc import Callable
    from concurrent.futures import Future

    from flitzis_looper.controller.asset_lifecycle import (
        AssetRetirementReservation,
        ProjectAssetLifecycle,
    )
    from flitzis_looper.controller.loader import LoaderController
    from flitzis_looper.controller.material_migration_cleanup import ArtifactCapture
    from flitzis_looper.controller.material_migration_recovery import MigrationRecovery
    from flitzis_looper.controller.persistence import ProjectPersistence
    from flitzis_looper.controller.stems import StemTaskRunner
    from flitzis_looper.models import SessionState
    from flitzis_looper_audio import (
        AudioEngine,
        ConstantTimingTicket,
        MaterialMigrationHold,
        MaterialMigrationPreparation,
        MaterialMigrationSourceTicket,
        MaterialMigrationStemPreparation,
        PreparedSourceTicket,
        PreparedStemPair,
    )


@dataclass
class _Subscriber:
    capture: MigrationAssignment
    source: MaterialMigrationSourceTicket | None = None
    terminal: bool = False
    terminal_error: bool = False
    original_owner: tuple[Path, ProjectAssetLease] | None = None
    timing_future: Future[ConstantTimingTicket] | None = None
    timing_ticket: ConstantTimingTicket | None = None
    timing_ready: bool = False
    stem_ticket: PreparedSourceTicket | None = None
    pair_future: Future[PreparedStemPair] | None = None
    pair_prepared: PreparedStemPair | None = None
    pair_selection: StemPairSelection | None = None
    pair_release_ordered: bool = False
    timing_revision: str | None = None


class MaterialMigrationController:
    """Prepare once, obtain fresh per-pad authority, then persist the related image."""

    def __init__(
        self,
        persistence: ProjectPersistence,
        session: SessionState,
        audio: AudioEngine,
        assets: ProjectAssetLifecycle,
        loader: LoaderController,
        *,
        stem_task_runner: StemTaskRunner | None = None,
    ) -> None:
        self._persistence = persistence
        self._project = persistence.project
        self._session = session
        self._audio = audio
        self._assets = assets
        self._loader = loader
        self._stem_task_runner = stem_task_runner
        self._journal: MaterialMigrationJournal | None = None
        self._store: MaterialMigrationJournalStore | None = None
        self._capture_logged = False
        self._unresolved_phases: tuple[tuple[int, str, bool], ...] | None = None
        self._preparation: MaterialMigrationPreparation | None = None
        self._hold: MaterialMigrationHold | None = None
        self._retirement: AssetRetirementReservation | None = None
        self._owners: list[ProjectAssetLease] = []
        self._subscribers: dict[int, _Subscriber] = {}
        self._old_stems: dict[int, StemCacheEntry] = {}
        self._stem_work: dict[str, MaterialMigrationStemPreparation] = {}
        self._new_stems: dict[str, str] = {}
        self.error: str | None = None
        self._preclaim_abort: str | None = None
        self._recovery_owner: str | None = None
        self._fence_owner: str | None = None
        self._startup_scheduled = False
        self._startup_candidates: tuple[tuple[int, str], ...] = ()
        self._recovery_pending: list[MaterialMigrationJournal] = []
        self._recovery_errors: list[str] = []
        self._recovery_active: MaterialMigrationJournal | None = None
        self._recovery_hold: MaterialMigrationHold | None = None
        self._recovery_pins: list[ProjectAssetLease] = []
        self._recovery_fence_id: str | None = None
        self._arrangement_recovery_deferred = False
        self._arrangement_recovery_allowed: Callable[[], bool] = lambda: True
        self.cleanup_error: str | None = None
        self._ledger_enabled: bool = (
            getattr(self._audio, "migration_artifact_ledger_supported", lambda: False)() is True
        )
        self._init_material_custody()
        self._loader._set_migration_event_handler(self.handle_loader_event)
        self._recover_interrupted()
        if self._ledger_enabled and not self._arrangement_recovery_deferred:
            try:
                self._ensure_artifacts()
            except (OSError, RuntimeError, ValueError) as error:
                self.cleanup_error = str(error)

    def _init_material_custody(self) -> None:
        self._artifact_reconciler: MigrationArtifactReconciler | None = None
        self._artifact_future: Future[ArtifactCapture] | None = None
        self._history_future: Future[MaterialMigrationJournal] | None = None
        self._artifact_captures: list[ArtifactCapture] = []
        self._cleanup = MigrationCleanupQueue(Path.cwd() / "samples")
        self._preparation_source: str | None = None
        self._stem_targets_captured = False
        self._group_candidates: tuple[MigrationAssignment, ...] = ()
        self._grouping_originals = False
        self._group_witness: tuple[str, int] | None = None
        self._creation_restores: dict[int, PadContentIdentity] = {}
        self._creation_restore_future: Future[None] | None = None
        binder = getattr(self._loader, "bind_material_creation_restore", None)
        if callable(binder):
            binder(self._queue_material_creation_restore)

    def schedule_after_restore(self) -> None:
        """Capture a bounded related-reference inventory for serial reconciliation."""
        self._startup_candidates = tuple(
            (sample_id, reference)
            for sample_id, reference in enumerate(self._project.sample_paths)
            if reference is not None
        )
        self._startup_scheduled = True

    def _poll_startup(self) -> None:
        if (
            self._arrangement_recovery_deferred
            or not self._startup_scheduled
            or self.status not in {"idle", "config_committed"}
        ):
            return
        if self._cleanup.preparing:
            return
        if self._history_future is not None:
            return
        if self._session.loading_sample_ids or self._session.stem_generating_sample_ids:
            return
        while self._startup_candidates:
            (sample_id, reference), *remaining = self._startup_candidates
            self._startup_candidates = tuple(remaining)
            if self._project.sample_paths[sample_id] != reference:
                continue
            try:
                asset = original_asset(reference)
                if asset.layout in {"legacy", "material_id"}:
                    self.begin(sample_id)
                    return
            except (OSError, RuntimeError, ValueError) as error:
                self.error = str(error)
                self._session.sample_load_errors[sample_id] = str(error)
                self._startup_scheduled = False
                return
        self._startup_scheduled = False

    def _recover_interrupted(self, recovery: MigrationRecovery | None = None) -> None:
        if recovery is None:
            recovery = inspect_migration_recovery(
                Path.cwd() / "samples", self._persistence.config_path
            )
        for item in recovery.settled:
            self._cleanup.add(item)
        if self._persistence.load_error is not None:
            recovery.errors.append(self._persistence.load_error)
        if recovery.unfinished_arrangement_kinds:
            self._defer_arrangement_recovery(recovery)
            return
        if not recovery.pending and not recovery.errors:
            return
        owner = uuid4().hex
        self._persistence.capture_migration(owner)
        self._recovery_owner = owner
        self._recovery_fence_id = owner
        self._recovery_pending = recovery.pending
        self._recovery_errors = recovery.errors
        if recovery.intent is not None:
            self._persistence.recover_migration_intent(owner, recovery.intent)
            self._assets.sync_assignments()
        ids = {item.sample_id for journal in recovery.pending for item in journal.assignments}
        if recovery.errors:
            ids.update(range(len(self._project.sample_paths)))
        self.error = (
            "; ".join(recovery.errors) or "Interrupted material migration requires fresh recovery"
        )
        try:
            self._recovery_hold = self._audio.hold_material_migration(sorted(ids))
            self._pin_recovery_references(recovery.pending)
        except (OSError, RuntimeError, ValueError) as error:
            self.error = f"{self.error}; {error}"
            self._recovery_errors.append(str(error))
        for sample_id in ids:
            self._session.sample_load_errors[sample_id] = self.error

    def _defer_arrangement_recovery(self, recovery: MigrationRecovery) -> None:
        """Leave the arrangement writer and pins to its actual startup owner."""
        self._arrangement_recovery_deferred = True
        self._recovery_pending = recovery.pending
        self._recovery_errors = list(recovery.errors)
        self.error = "; ".join(recovery.errors) or (
            "Arrangement recovery retains material migration and cleanup admission"
        )

    def bind_arrangement_recovery_guard(self, allowed: Callable[[], bool]) -> None:
        """Use the App's actual arrangement owners for automatic and explicit retries."""
        self._arrangement_recovery_allowed = allowed

    def refresh_arrangement_recovery(self) -> bool:
        """Rescan after ALL startup owners become inactive, before admitting Legacy work.

        The App calls this at the actual startup active-to-inactive transition,
        never on every frame. Saved material-birth restore remains independently
        reachable while the arrangement startup owns its writer and source pins.

        Returns:
            Whether arrangement deferral cleared; Legacy recovery may still own its fence.
        """
        if not self._arrangement_recovery_allowed():
            self._arrangement_recovery_deferred = True
            self.error = "Arrangement recovery retains its actual owners and cleanup admission"
            return False
        recovery = inspect_migration_recovery(Path.cwd() / "samples", self._persistence.config_path)
        if self._persistence.load_error is not None:
            recovery.errors.append(self._persistence.load_error)
        if recovery.unfinished_arrangement_kinds or recovery.errors:
            self._defer_arrangement_recovery(recovery)
            return False
        if self._recovery_active is not None or self._fence_owner is not None:
            message = "active material recovery retains its current writer and custody"
            raise RuntimeError(message)
        if self._recovery_owner is not None:
            self._pin_recovery_references(recovery.pending)
            self._recovery_pending = recovery.pending
            for item in recovery.settled:
                self._cleanup.add(item)
            self._recovery_errors.clear()
            self.error = None
        else:
            self._recover_interrupted(recovery)
            if not recovery.pending:
                self._recovery_errors.clear()
                self.error = None
        # A successful rescan and custody refresh, rather than a startup flag
        # alone, grants admission. Failure above retains cached deferral.
        self._arrangement_recovery_deferred = False
        return True

    def _pin_recovery_references(self, journals: list[MaterialMigrationJournal]) -> None:
        references = {item.old_reference for journal in journals for item in journal.assignments}
        references.update(
            journal.alias.new_reference for journal in journals if journal.alias is not None
        )
        for reference in references:
            path = original_asset(reference).path
            if path.exists():
                self._recovery_pins.append(self._assets.acquire(path))

    def retry_recovery(self) -> None:
        """Reinspect recognized journals while preserving current intent and existing fences."""
        if self._arrangement_recovery_deferred:
            if self.refresh_arrangement_recovery():
                return
            message = self.error or "Arrangement recovery retains material migration admission"
            raise ValueError(message)
        if self._recovery_owner is None or self._recovery_active is not None:
            message = "there is no settled interrupted transaction to retry"
            raise ValueError(message)
        recovery = inspect_migration_recovery(Path.cwd() / "samples", self._persistence.config_path)
        if recovery.unfinished_arrangement_kinds:
            self._defer_arrangement_recovery(recovery)
            message = self.error or "Arrangement recovery retains material migration admission"
            raise ValueError(message)
        if recovery.errors:
            self._recovery_errors = recovery.errors
            message = "; ".join(recovery.errors)
            self.error = message
            raise ValueError(message)
        self._pin_recovery_references(recovery.pending)
        self._recovery_pending = recovery.pending
        self._recovery_errors.clear()
        self.error = None

    @property
    def status(self) -> str:
        if self._arrangement_recovery_deferred or self._recovery_owner is not None:
            return "unresolved"
        if (
            self.error is not None
            and self._journal is not None
            and self._journal.phase not in {"failed", "config_committed"}
        ):
            return "unresolved"
        return self._journal.phase if self._journal is not None else "idle"

    def begin(self, sample_id: int) -> str:
        """Migrate one distinct existing original and its current subscribers in all banks."""
        validate_sample_id(sample_id)
        if self._arrangement_recovery_deferred:
            message = "arrangement recovery retains material migration and cleanup admission"
            raise RuntimeError(message)
        if self._recovery_owner is not None:
            message = "interrupted journal retains the writer and launch fences for recovery"
            raise RuntimeError(message)
        if self._journal is not None and self.status not in {"failed", "config_committed"}:
            message = "a material migration is already active or unresolved"
            raise RuntimeError(message)
        source = self._project.sample_paths[sample_id]
        if source is None:
            message = "material migration requires an assigned original"
            raise ValueError(message)
        old = original_asset(source).path
        assignments: list[MigrationAssignment] = []
        for index, path in enumerate(self._project.sample_paths):
            if path is None or original_asset(path).path != old:
                continue
            content = self._project.pad_content[index]
            if content is None or index in self._session.loading_sample_ids:
                message = "wait for the original assignment to finish restoring"
                raise RuntimeError(message)
            assignments.append(
                MigrationAssignment(
                    sample_id=index, instance_id=content.instance_id, old_reference=path
                )
            )
        return self._start_transaction(source, assignments)

    def _start_transaction(
        self,
        source: str,
        assignments: list[MigrationAssignment],
        *,
        previous_owner: str | None = None,
        resume_of: str | None = None,
    ) -> str:
        self._reset_transaction()
        transaction_id = uuid4().hex
        candidates = tuple(
            {
                item.sample_id: item
                for item in (*current_candidates(self._project, self._session), *assignments)
            }.values()
        )
        retirement = self._assets.reserve(max(len(candidates), len(assignments)) * 8 + 16)
        self._retirement = retirement
        try:
            revision, snapshot, config_sha = (
                self._persistence.capture_migration(transaction_id)
                if previous_owner is None
                else self._persistence.transfer_migration(previous_owner, transaction_id)
            )
            self._fence_owner = transaction_id
            self._preparation_source = source
            self._group_candidates = candidates
            parent = self._recovery_active
            check_alias_capacity(snapshot, parent)
            self._journal = MaterialMigrationJournal(
                transaction_id=transaction_id,
                config_reference=self._persistence.config_reference,
                captured_revision=revision,
                intent_revision=revision,
                config_sha256=config_sha,
                snapshot_json=snapshot.model_dump_json(),
                assignments=tuple(assignments),
                resume_of=resume_of,
                artifacts=rollback_artifacts(parent) if parent is not None else (),
            )
            self._store = MaterialMigrationJournalStore(str(Path.cwd() / "samples"), transaction_id)
            self._store.append(self._journal.model_dump_json())
            self._capture_logged = True
            self._owners = [self._assets.acquire(original_asset(source).path)]
            self._subscribers = {item.sample_id: _Subscriber(item) for item in assignments}
            self._old_stems = {}
            self._stem_work = {}
            self._new_stems = {}
            self.error = None
            self._preclaim_abort = None
            self._grouping_originals = True
            if not self._creation_restores and self._creation_restore_future is None:
                self._artifact_future = self._ensure_artifacts().capture(
                    original_requests(source, candidates), include_legacy_pcm=False
                )
        except (OSError, RuntimeError, ValueError) as error:
            if self._fence_owner == transaction_id:
                self._fail_before_admission(str(error))
            else:
                retirement.close()
                self._retirement = None
            raise
        return transaction_id

    def _ensure_artifacts(self) -> MigrationArtifactReconciler:
        if self._artifact_reconciler is None:
            samples = Path.cwd() / "samples"
            samples.mkdir(exist_ok=True)
            self._artifact_reconciler = MigrationArtifactReconciler(
                samples, self._persistence.config_path, self._audio
            )
        return self._artifact_reconciler

    def _queue_material_creation_restore(self, sample_id: int, content: PadContentIdentity) -> None:
        self._creation_restores[sample_id] = content

    def _poll_material_creation_restore(self) -> None:
        future = self._creation_restore_future
        if future is not None:
            if not future.done():
                return
            try:
                future.result()
            except (OSError, RuntimeError, ValueError) as error:
                self.cleanup_error = f"Material birth ownership preserved: {error}"
            self._creation_restore_future = None
        if not self._creation_restores or not self._ledger_enabled:
            return
        try:
            reconciler = self._ensure_artifacts()
            if not reconciler.available:
                return
            sample_id = next(iter(self._creation_restores))
            content = self._creation_restores.pop(sample_id)
            if self._project.pad_content[sample_id] != content or content.material_creation is None:
                return
            journal = self._saved_creation_journal(sample_id, content)
            self._creation_restore_future = reconciler.restore_material_creation(
                sample_id, content.instance_id, content.material_creation, journal
            )
        except (OSError, RuntimeError, ValueError) as error:
            self.cleanup_error = f"Material birth ownership preserved: {error}"

    def _saved_creation_journal(
        self, sample_id: int, content: PadContentIdentity
    ) -> MaterialMigrationJournal | None:
        for alias in self._project.material_migrations.values():
            if (
                alias.material_creation != content.material_creation
                or alias.new_reference != self._project.sample_paths[sample_id]
            ):
                continue
            store = MaterialMigrationJournalStore.open(
                str(Path.cwd() / "samples"), alias.transaction_id
            )
            records = store.history()
            if not records:
                message = "saved migration birth has no current durable journal"
                raise ValueError(message)
            candidate = MaterialMigrationJournal.model_validate_json(records[-1])
            if not any(
                item.sample_id == sample_id and item.instance_id == content.instance_id
                for item in candidate.assignments
            ):
                # A later ordinary importer carries the birth fact, but cannot
                # borrow the creating migration's captured assignment authority.
                continue
            if candidate.alias != alias or candidate.phase != "config_committed":
                message = "saved migration alias differs from its current durable journal"
                raise ValueError(message)
            return candidate
        return None

    def _start_old_artifact_capture(
        self, source: str, assignments: list[MigrationAssignment]
    ) -> None:
        references = dict.fromkeys([source, *(item.old_reference for item in assignments)])
        creations = {
            item.old_reference: content.material_creation
            for item in assignments
            if (content := self._project.pad_content[item.sample_id]) is not None
        }
        requests = [
            ArtifactRequest(reference, "rollback", material_creation=creations.get(reference))
            for reference in references
        ]
        seen = set(references)
        for item in assignments:
            entry = self._project.stem_cache[item.sample_id]
            if entry is not None and entry.cache_dir not in seen:
                requests.append(
                    ArtifactRequest(
                        entry.cache_dir,
                        "rollback",
                        required=entry.available,
                    )
                )
                seen.add(entry.cache_dir)
            if entry is not None and entry.pair is not None:
                for reference in (entry.pair.pcm_generation, entry.pair.descriptor_reference):
                    if reference not in seen:
                        # Capture exact old receipts first. Only the authoritative
                        # prepared material root can establish same-root reuse.
                        requests.append(
                            ArtifactRequest(reference, "rollback", required=entry.available)
                        )
                        seen.add(reference)
        self._artifact_future = self._ensure_artifacts().capture(requests)

    def _poll_artifact_capture(self) -> bool:
        future = self._artifact_future
        if future is None:
            return True
        if not future.done():
            return False
        capture = future.result()
        self._artifact_captures.append(capture)
        self._artifact_future = None
        if self._journal is None:
            message = "artifact proof has no migration journal"
            raise RuntimeError(message)
        if self._grouping_originals:
            return self._finish_content_group(capture)
        self._record(
            self._journal.phase, artifacts=merge_artifacts(self._journal.artifacts, capture.records)
        )
        return True

    def _finish_content_group(self, capture: ArtifactCapture) -> bool:
        source = self._preparation_source
        if source is None or self._journal is None:
            message = "material grouping lost its captured source"
            raise RuntimeError(message)
        assignments, self._group_witness = retain_content_group(
            source, capture, self._group_candidates
        )
        # Receipt verification can outlive source replacement, ABA or restoration.
        # Each retained pad keeps its own captured path and content-instance guard.
        subscribers = {item.sample_id: _Subscriber(item) for item in assignments}
        subscribers = {index: item for index, item in subscribers.items() if self._matches(item)}
        if not subscribers:
            message = "newer source assignments superseded material verification"
            raise ValueError(message)
        self._subscribers = subscribers
        self._grouping_originals = False
        self._group_candidates = ()
        if capture.errors:
            self.cleanup_error = "; ".join(capture.errors)
        self._record(
            "captured",
            assignments=tuple(item.capture for item in subscribers.values()),
            artifacts=merge_artifacts(self._journal.artifacts, capture.records),
        )
        self._owners.extend(
            self._assets.acquire(original_asset(reference).path)
            for reference in {item.capture.old_reference for item in subscribers.values()}
            if original_asset(reference).path != original_asset(source).path
        )
        self._hold = (
            self._recovery_hold.take(list(subscribers))
            if self._recovery_active is not None and self._recovery_hold is not None
            else self._audio.hold_material_migration(list(subscribers))
        )
        if self._ledger_enabled:
            self._start_old_artifact_capture(
                source, [item.capture for item in subscribers.values()]
            )
            return False
        return True

    def _poll_recovery(self) -> None:
        if (
            self._arrangement_recovery_deferred
            or self._recovery_owner is None
            or self._recovery_errors
            or self._history_future is not None
        ):
            return
        if self._session.loading_sample_ids or self._session.stem_generating_sample_ids:
            return
        if not self._recovery_pending:
            self._release_recovery_resources()
            return
        original = self._recovery_pending[0]
        references = {item.old_reference for item in original.assignments}
        if original.alias is not None:
            references.add(original.alias.new_reference)
        assignments = [
            MigrationAssignment(
                sample_id=index, instance_id=content.instance_id, old_reference=path
            )
            for index, (path, content) in enumerate(
                zip(self._project.sample_paths, self._project.pad_content, strict=True)
            )
            if path is not None and path in references and content is not None
        ]
        if not assignments:
            # No current reference remains; no old runtime authority is recreated.
            try:
                store = MaterialMigrationJournalStore.open(
                    str(Path.cwd() / "samples"), original.transaction_id
                )
                settled = original.changed(
                    phase="failed",
                    error="newer current assignments superseded interrupted migration",
                )
                store.append(settled.model_dump_json())
                self._recovery_pending.pop(0)
                if settled.artifacts:
                    self._cleanup.add(settled)
            except (OSError, RuntimeError, ValueError) as error:
                self._recovery_errors.append(str(error))
            return
        source = assignments[0].old_reference
        self._recovery_active = original
        previous = self._recovery_owner
        self._recovery_owner = None
        try:
            self._start_transaction(
                source, assignments, previous_owner=previous, resume_of=original.transaction_id
            )
        except (OSError, RuntimeError, ValueError) as error:
            self._recovery_errors.append(str(error))
            if self._fence_owner is None:
                self._recovery_owner = previous

    def _release_recovery_resources(self) -> None:
        if self._recovery_hold is not None:
            self._recovery_hold.release()
            self._recovery_hold = None
        for owner in self._recovery_pins:
            owner.release()
        self._recovery_pins.clear()
        if self._recovery_owner is not None:
            self._persistence.release_migration(self._recovery_owner)
            self._recovery_owner = None
        self.error = None

    def _reset_transaction(self) -> None:
        self._journal = None
        self._store = None
        self._capture_logged = False
        self._preparation = None
        self._hold = None
        self._owners = []
        self._subscribers = {}
        self._old_stems = {}
        self._stem_work = {}
        self._new_stems = {}
        self.error = None
        self._preclaim_abort = None
        self._artifact_future = None
        self._artifact_captures = []
        self._preparation_source = None
        self._stem_targets_captured = False
        self._group_candidates = ()
        self._grouping_originals = False
        self._group_witness = None

    def _record(self, phase: str, *, error: str | None = None, **updates: object) -> None:
        if self._journal is None or self._store is None:
            message = "material migration journal is unavailable"
            raise RuntimeError(message)
        journal = self._journal.changed(
            phase=phase,
            error=error,
            snapshot_json=self._project.model_dump_json(),
            intent_revision=self._persistence.revision,
            **updates,
        )
        self._store.append(journal.model_dump_json())
        self._journal = journal

    def _matches(self, subscriber: _Subscriber) -> bool:
        sample_id = subscriber.capture.sample_id
        request = self._loader._load_request_ids.get(sample_id)
        source = subscriber.source
        if request is not None and (source is None or request != source.request_id):
            return False
        content = self._project.pad_content[sample_id]
        path = self._project.sample_paths[sample_id]
        if content is None or content.instance_id != subscriber.capture.instance_id or path is None:
            return False
        try:
            return (
                original_asset(path).path == original_asset(subscriber.capture.old_reference).path
            )
        except OSError, ValueError:
            return False

    def _owns_feedback(self, subscriber: _Subscriber) -> bool:
        current = self._loader._load_request_ids.get(subscriber.capture.sample_id)
        source = subscriber.source
        return (
            self._matches(subscriber)
            and source is not None
            and current in {None, source.request_id}
        )

    def handle_loader_event(self, event: dict[str, object]) -> bool:
        """Consume only the actual request owned by this transaction."""
        sample_id, request_id = event.get("id"), event.get("request_id")
        if type(sample_id) is not int or type(request_id) is not int:
            return False
        subscriber = self._subscribers.get(sample_id)
        if (
            subscriber is None
            or subscriber.source is None
            or request_id != subscriber.source.request_id
        ):
            return False
        event_type = event.get("type")
        if event_type == "success":
            self._accept_success(sample_id, subscriber, event.get("original_lease"))
        elif event_type == "error":
            subscriber.terminal = True
            subscriber.terminal_error = True
            if self._owns_feedback(subscriber):
                self._session.loading_sample_ids.discard(sample_id)
                self._session.sample_load_errors[sample_id] = str(event.get("error"))
        elif event_type == "started" and self._owns_feedback(subscriber):
            self._session.loading_sample_ids.add(sample_id)
        elif event_type == "progress":
            stage = event.get("stage")
            if isinstance(stage, str) and self._owns_feedback(subscriber):
                self._session.sample_load_stage[sample_id] = stage
        return True

    def _accept_success(self, sample_id: int, subscriber: _Subscriber, owner: object) -> None:
        subscriber.terminal = True
        if self._owns_feedback(subscriber):
            self._session.loading_sample_ids.discard(sample_id)
        if not isinstance(owner, ProjectAssetLease):
            return
        if not self._owns_feedback(subscriber):
            owner.release()
            return
        try:
            subscriber.original_owner = self._assets.prepare_original(
                self._alias().new_reference, owner
            )
        except (OSError, RuntimeError, ValueError) as error:
            self._unresolved(str(error))

    def _alias(self) -> MaterialMigrationAlias:
        if self._journal is None or self._journal.alias is None:
            message = "material migration has no verified alias"
            raise RuntimeError(message)
        return self._journal.alias

    def poll(self) -> None:
        """Advance bounded control work; never synthesize source/timing acceptance."""
        self._poll_material_creation_restore()
        self._poll_cleanup()
        self._poll_history_transfer()
        self._poll_recovery()
        self._poll_startup()
        if self.status == "unresolved" and self._recovery_owner is None:
            self._poll_retained_phase()
        if self._preclaim_abort is not None:
            try:
                self._poll_preclaim_abort()
            except (OSError, RuntimeError, ValueError) as error:
                self._unresolved(str(error))
            return
        if self._journal is None or self.status in {"failed", "unresolved", "config_committed"}:
            return
        try:
            if self.status == "captured":
                self._poll_material()
            if self.status == "material_verified":
                self._poll_stem_copy()
            if self.status in {"references_prepared", "adoption_pending"}:
                self._poll_sources()
            if self.status == "ack_confirmed":
                self._commit_current_image()
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self._settle_poll_error(str(error))

    def _settle_poll_error(self, message: str) -> None:
        cancelled = [
            item.source.cancel_unclaimed()
            for item in self._subscribers.values()
            if item.source is not None
        ]
        if not all(cancelled):
            self._unresolved(message)
        else:
            self._fail_before_admission(message)

    def _phase_signature(self) -> tuple[tuple[int, str, bool], ...]:
        result = []
        for item in self._subscribers.values():
            if item.source is not None:
                phase = item.source.phase()
                result.append((
                    item.source.request_id,
                    phase,
                    phase == "acknowledged" and item.source.is_current(),
                ))
        return tuple(result)

    def _poll_retained_phase(self) -> None:
        """A genuinely later native terminal phase can reconcile a retained claim."""
        try:
            phases = self._phase_signature()
            if not phases or phases == self._unresolved_phases:
                return
            self._unresolved_phases = phases
            if all(phase == "rejected" for _, phase, _ in phases):
                self._fail_before_admission(self.error or "late native rejection")
            elif all(phase == "acknowledged" and current for _, phase, current in phases):
                self.error = None
                self._record("adoption_pending")
        except (OSError, RuntimeError, ValueError) as error:
            self._unresolved(str(error))

    def _start_material_grouping(self) -> bool:
        if self._grouping_originals and self._artifact_future is None:
            if self._creation_restores or self._creation_restore_future is not None:
                return False
            if self._preparation_source is None:
                message = "deferred material grouping has no captured source"
                raise RuntimeError(message)
            self._artifact_future = self._ensure_artifacts().capture(
                original_requests(self._preparation_source, self._group_candidates),
                include_legacy_pcm=False,
            )
        return True

    def _poll_material(self) -> None:
        if not self._start_material_grouping():
            return
        if not self._poll_artifact_capture():
            return
        if self._preparation is None and self._preparation_source is not None:
            self._preparation = self._audio.prepare_material_migration(self._preparation_source)
        if self._preparation is None or self._journal is None:
            message = "material preparation is unavailable"
            raise RuntimeError(message)
        status = self._preparation.status()
        if status == "preparing":
            return
        if status != "ready":
            raise RuntimeError(self._preparation.error() or "material preparation failed")
        data = json.loads(self._preparation.metadata_json())
        original = data["original"]
        old, new, digest = data["old_reference"], data["new_reference"], original["sha256"]
        if self._group_witness != (digest, original["bytes"]):
            message = "prepared material differs from its held complete grouping proof"
            raise ValueError(message)
        creation = data.get("material_creation")
        if (
            creation is None
            and self._recovery_active is not None
            and self._recovery_active.alias is not None
        ):
            previous = self._recovery_active.alias
            if (
                previous.new_reference == new
                and previous.original_sha256 == digest
                and previous.original_bytes == original["bytes"]
            ):
                creation = previous.material_creation
        alias = MaterialMigrationAlias(
            transaction_id=self._journal.transaction_id,
            material_id=data["material_id"],
            old_reference=old,
            new_reference=new,
            original_sha256=digest,
            original_bytes=original["bytes"],
            decoder_identity=data["decoder_identity"],
            playback_identity=data["playback_identity"],
            cache_path=data["cache_path"],
            old_source_version=f"{old}|sha256-v1:{digest}",
            new_source_version=f"{new}|sha256-v1:{digest}",
            resume_of=self._journal.resume_of,
            material_creation=(
                MaterialCreationReceipt.model_validate(creation) if creation is not None else None
            ),
        )
        self._verify_recovery_alias(alias)
        self._record_verified_material(alias, data)

    def _verify_recovery_alias(self, alias: MaterialMigrationAlias) -> None:
        if self._recovery_active is not None and self._recovery_active.alias is not None:
            previous = self._recovery_active.alias
            if (alias.original_sha256, alias.original_bytes) != (
                previous.original_sha256,
                previous.original_bytes,
            ):
                message = "recovery original differs from the verified interrupted transaction"
                raise ValueError(message)

    def _record_verified_material(
        self, alias: MaterialMigrationAlias, data: dict[str, object]
    ) -> None:
        if self._journal is None:
            message = "verified material has no retained journal"
            raise RuntimeError(message)
        target_root = original_asset(alias.new_reference).material_root
        artifacts = self._journal.artifacts
        if target_root is not None:
            artifacts = merge_artifacts(
                (),
                (
                    item.model_copy(update={"role": "target", "created": False})
                    if resolve_asset(item.evidence.reference).material_root == target_root
                    else item
                    for item in artifacts
                ),
            )
        self._record("material_verified", alias=alias, artifacts=artifacts)
        if self._ledger_enabled:
            created_original = data.get("created_original")
            created_cache = data.get("created_cache")
            if (
                data.get("artifact_ledger_schema") != 1
                or not isinstance(created_original, bool)
                or not isinstance(created_cache, bool)
            ):
                message = "material preparation has no checked artifact provenance"
                raise ValueError(message)
            self._artifact_future = self._ensure_artifacts().capture([
                ArtifactRequest(
                    alias.new_reference,
                    "target",
                    created_original,
                    material_creation=alias.material_creation,
                ),
                ArtifactRequest(alias.cache_path, "target", created_cache),
            ])

    def _poll_stem_copy(self) -> None:
        if self._ledger_enabled and not self._poll_artifact_capture():
            return
        preparation = self._require_preparation()
        if any(
            sample_id in self._session.stem_generating_sample_ids for sample_id in self._subscribers
        ):
            return
        alias = self._alias()
        admitted = 0
        for sample_id, subscriber in self._subscribers.items():
            if not self._matches(subscriber):
                continue
            entry = self._capture_stem_entry(sample_id)
            if entry is None or not self._should_copy_stems(entry):
                continue
            if admitted >= 8:
                return
            self._owners.append(self._assets.acquire(Path(entry.cache_dir)))
            self._stem_work[entry.cache_dir] = self._audio.prepare_material_migration_stems(
                preparation,
                entry.cache_dir,
                entry.source_version,
                alias.new_source_version,
                uuid4().hex,
            )
            admitted += 1
        if not self._collect_stem_copies():
            return
        if self._ledger_enabled and not self._stem_targets_captured:
            self._stem_targets_captured = True
            requests = [
                ArtifactRequest(work.cache_reference(), "target", work.created())
                for work in self._stem_work.values()
            ]
            if requests:
                self._artifact_future = self._ensure_artifacts().capture(requests)
                return
        self._record("references_prepared")

    def _should_copy_stems(self, entry: StemCacheEntry) -> bool:
        if entry.cache_dir in self._stem_work:
            return False
        if entry.available:
            return True
        return (
            self._ledger_enabled
            and self._journal is not None
            and any(
                record.evidence.reference.replace("\\", "/") == entry.cache_dir.replace("\\", "/")
                and record.evidence.kind == "stem_directory"
                for record in self._journal.artifacts
            )
        )

    def _collect_stem_copies(self) -> bool:
        for key, work in self._stem_work.items():
            status = work.status()
            if status == "preparing":
                return False
            if status != "ready":
                raise RuntimeError(work.error() or "complete stem copy failed")
            if key not in self._new_stems:
                target = work.cache_reference()
                self._owners.append(self._assets.acquire(Path(target)))
                self._new_stems[key] = target
        return True

    def _capture_stem_entry(self, sample_id: int) -> StemCacheEntry | None:
        entry = self._project.stem_cache[sample_id]
        if entry is None:
            return None
        previous = self._old_stems.get(sample_id)
        if previous is not None and previous != entry:
            message = "stem version changed during material migration"
            raise RuntimeError(message)
        self._old_stems[sample_id] = entry.model_copy(deep=True)
        return entry

    def _settle_removed_subscriber(self, subscriber: _Subscriber) -> None:
        source = subscriber.source
        if source is None:
            return
        if source.cancel_unclaimed():
            sample_id = subscriber.capture.sample_id
            if sample_id not in self._loader._load_request_ids:
                self._session.loading_sample_ids.discard(sample_id)
            return
        if source.phase() == "claimed":
            message = "replaced migration subscriber has an unresolved native claim"
            raise RuntimeError(message)
        if source.phase() != "acknowledged":
            message = "replaced migration subscriber has an unknown native phase"
            raise RuntimeError(message)
        # A completed old admission can remain a reader while newer native intent wins.
        # Its actual sample/lease pins stay in the ticket through transaction settlement.

    def _poll_sources(self) -> None:
        preparation = self._require_preparation()
        admissions = 0
        pending = False
        for sample_id, subscriber in self._subscribers.items():
            if not self._matches(subscriber):
                self._settle_removed_subscriber(subscriber)
                continue
            if subscriber.source is None:
                if admissions >= 8:
                    pending = True
                    continue
                self._loader._clear_analysis_task_state(sample_id)
                subscriber.source = self._audio.adopt_material_migration(sample_id, preparation)
                self._session.loading_sample_ids.add(sample_id)
                admissions += 1
            pending |= not self._poll_subscriber(sample_id, subscriber)
        if self.status == "references_prepared":
            self._record("adoption_pending")
        if not pending:
            self._record("ack_confirmed")

    def _poll_subscriber(self, sample_id: int, subscriber: _Subscriber) -> bool:
        source = subscriber.source
        if source is None:
            return False
        if source.phase() in {"pending", "claimed"}:
            if subscriber.terminal_error:
                message = "migration source claim remains unresolved after terminal feedback"
                raise RuntimeError(message)
            return False
        if source.phase() != "acknowledged":
            message = "migration source rejected; actual source phases require settlement"
            raise RuntimeError(message)
        if not source.is_current():
            message = "acknowledged migration source was superseded; both owners retained"
            raise RuntimeError(message)
        if subscriber.original_owner is None:
            # Actual held phase/current native source is authority; metadata feedback is not.
            subscriber.original_owner = self._assets.prepare_original(self._alias().new_reference)
        self._session.loading_sample_ids.discard(sample_id)
        if not self._poll_timing(sample_id, subscriber):
            return False
        return self._poll_stem_ack(sample_id, subscriber)

    def _poll_stem_ack(self, sample_id: int, subscriber: _Subscriber) -> bool:
        entry = self._old_stems.get(sample_id)
        if entry is None or (not entry.available and entry.cache_dir not in self._new_stems):
            return True
        if entry.pair is not None or getattr(self._audio, "prepare_stem_pair", None) is not None:
            return self._poll_pair_ack(sample_id, subscriber, entry)
        if subscriber.stem_ticket is None:
            ticket = self._audio.capture_prepared_source(
                sample_id, self._alias().new_source_version
            )
            self._audio.publish_prepared_stems(
                sample_id,
                self._alias().new_source_version,
                self._new_stems[entry.cache_dir],
                ticket,
            )
            subscriber.stem_ticket = ticket
        status = subscriber.stem_ticket.publication_status()
        if status == "pending":
            return False
        if status != "accepted":
            message = "fresh migrated stem publication was rejected"
            raise RuntimeError(message)
        return True

    def _poll_pair_ack(
        self, sample_id: int, subscriber: _Subscriber, entry: StemCacheEntry
    ) -> bool:
        if self._project.stem_cache[sample_id] != entry:
            message = "newer stem selection wins over captured migration pair"
            raise ValueError(message)
        if self._project.pad_stem_mix_mode[sample_id] == "full_mix":
            return self._settle_pair_full_mix(sample_id, subscriber, entry)
        if subscriber.pair_release_ordered:
            self._start_pair_work(sample_id, subscriber, entry, components=True)
        if subscriber.pair_future is None:
            self._start_pair_work(sample_id, subscriber, entry, components=True)
        prepared = self._returned_pair(subscriber, entry)
        if prepared is None:
            return False
        if not prepared.has_components():
            # Finish the real metadata worker before admitting a new live demand.
            prepared.select()
            self._start_pair_work(sample_id, subscriber, entry, components=True)
            return False
        return self._settle_prepared_pair_ack(subscriber, prepared)

    def _settle_prepared_pair_ack(
        self, subscriber: _Subscriber, prepared: PreparedStemPair
    ) -> bool:
        ticket = subscriber.stem_ticket
        if ticket is None:
            message = "Migrated pair has no independent source ticket"
            raise RuntimeError(message)
        if ticket.publication_status() == "captured":
            self._audio.publish_stem_pair(prepared, ticket)
        status = ticket.publication_status()
        if status in {"captured", "pending"}:
            return False
        if status != "accepted":
            message = "Fresh migrated pair publication was rejected"
            raise RuntimeError(message)
        prepared.select()
        return True

    def _start_pair_work(
        self,
        sample_id: int,
        subscriber: _Subscriber,
        entry: StemCacheEntry,
        *,
        components: bool,
    ) -> None:
        alias = self._alias()
        target = self._new_stems[entry.cache_dir]
        migrated = StemCacheEntry.model_validate(
            entry.model_dump()
            | {
                "source_version": alias.new_source_version,
                "cache_dir": target,
                "stems": expected_stem_files(target),
                "pair": entry.pair
                if entry.pair is not None
                and entry.pair.wav_generation == target
                and entry.source_version == alias.new_source_version
                else None,
            }
        )
        ticket = self._audio.capture_prepared_source(sample_id, alias.new_source_version)
        future = prepare_migrated_pair(
            self._audio,
            self._stem_task_runner,
            sample_id,
            alias.new_source_version,
            migrated,
            ticket,
            components=components,
        )
        subscriber.stem_ticket = ticket
        subscriber.pair_future = future
        subscriber.pair_prepared = None
        subscriber.pair_release_ordered = False

    def _returned_pair(
        self, subscriber: _Subscriber, entry: StemCacheEntry
    ) -> PreparedStemPair | None:
        future = subscriber.pair_future
        if future is None or not future.done():
            return None
        if subscriber.pair_prepared is None:
            prepared = future.result()
            selection = StemPairSelection.model_validate_json(prepared.selection_json())
            if selection.wav_generation != self._new_stems[entry.cache_dir]:
                message = "Prepared migrated pair differs from the verified WAV target"
                raise ValueError(message)
            # Keep even an invalid result owned by the Future until visible settlement.
            subscriber.pair_selection = selection
            subscriber.pair_prepared = prepared
        return subscriber.pair_prepared

    def _settle_pair_full_mix(
        self, sample_id: int, subscriber: _Subscriber, entry: StemCacheEntry
    ) -> bool:
        # Every changed WAV target needs a source-bound disk pair, including FULL
        # MIX. An unchanged complete pair remains reusable without live components.
        target = self._new_stems[entry.cache_dir]
        if subscriber.pair_future is None and (
            entry.pair is None
            or entry.pair.wav_generation != target
            or entry.source_version != self._alias().new_source_version
        ):
            self._start_pair_work(sample_id, subscriber, entry, components=False)
        if subscriber.pair_future is None:
            return True
        prepared = self._returned_pair(subscriber, entry)
        if prepared is None:
            return False
        prepared.select()
        if prepared.has_components() and not subscriber.pair_release_ordered:
            self._audio.set_stem_pair_full_mix(sample_id)
            subscriber.pair_release_ordered = True
        return True

    def _require_preparation(self) -> MaterialMigrationPreparation:
        preparation = self._preparation
        if preparation is None:
            message = "material migration preparation is unavailable"
            raise RuntimeError(message)
        return preparation

    def _poll_timing(self, sample_id: int, subscriber: _Subscriber) -> bool:
        if self._audio.pad_timing_intent(sample_id) != "automatic":
            return True
        if subscriber.timing_ready:
            current = self._audio.current_constant_timing(sample_id)
            if current is not None and current.get("revision") == subscriber.timing_revision:
                return True
            subscriber.timing_ready = False
            subscriber.timing_future = None
            subscriber.timing_ticket = None
        if subscriber.timing_future is None:
            self._prepare_timing(sample_id, subscriber)
            return False
        if not subscriber.timing_future.done():
            return False
        if subscriber.timing_ticket is None:
            subscriber.timing_ticket = subscriber.timing_future.result()
        return self._verify_timing_ack(sample_id, subscriber)

    def _prepare_timing(self, sample_id: int, subscriber: _Subscriber) -> None:
        analysis = self._project.sample_analysis[sample_id]
        if analysis is None or analysis.accepted_timing is None:
            message = "saved Automatic timing has no complete migration evidence"
            raise ValueError(message)
        try:
            subscriber.timing_future = self._loader._accepted_restore.prepare_for_migration(
                sample_id, analysis.accepted_timing, self._alias().new_reference
            )
        except ValueError as error:
            if "currently loading" not in str(error):
                raise

    def _verify_timing_ack(self, sample_id: int, subscriber: _Subscriber) -> bool:
        ticket = subscriber.timing_ticket
        if ticket is None:
            return False
        status = ticket.publication_status()
        if status == "pending":
            return False
        if status != "accepted":
            message = "fresh saved timing acknowledgement was rejected"
            raise ValueError(message)
        current, captured, accepted = (
            self._audio.current_constant_timing(sample_id),
            ticket.metadata(),
            ticket.accepted_metadata(),
        )
        if (
            current is None
            or accepted is None
            or current.get("revision") != accepted.get("revision")
            or current.get("accepted_request_id") != captured.get("request_id")
        ):
            message = "fresh migration timing no longer matches current intent"
            raise ValueError(message)
        subscriber.timing_ready = True
        revision = current.get("revision")
        subscriber.timing_revision = revision if isinstance(revision, str) else None
        return True

    def _candidate(self) -> ProjectState:
        alias = self._alias()
        candidate = self._project.model_copy(deep=True)
        for sample_id, subscriber in self._subscribers.items():
            if not self._matches(subscriber):
                continue
            if subscriber.source is None or not subscriber.source.is_current():
                message = "current reference has no matching actual migration source ACK"
                raise ValueError(message)
            if not self._poll_timing(sample_id, subscriber):
                message = (
                    "current Automatic intent requires a fresh matching timing acknowledgement"
                )
                raise ValueError(message)
            candidate.sample_paths[sample_id] = alias.new_reference
            content = candidate.pad_content[sample_id]
            if content is None:
                message = "migration content identity disappeared"
                raise ValueError(message)
            candidate.pad_content[sample_id] = PadContentIdentity(
                instance_id=content.instance_id,
                material_id=alias.material_id,
                material_creation=alias.material_creation,
                copied_from=content.copied_from,
            )
            self._candidate_stems(candidate, sample_id, subscriber)
        candidate.material_migrations = successor_aliases(candidate, alias, self._recovery_active)
        return candidate

    def _candidate_stems(
        self, candidate: ProjectState, sample_id: int, subscriber: _Subscriber
    ) -> None:
        old = self._old_stems.get(sample_id)
        current = candidate.stem_cache[sample_id]
        if old is not None:
            if current != old:
                message = "newer stem version wins over captured migration entry"
                raise ValueError(message)
            updates: dict[str, object] = {"source_version": self._alias().new_source_version}
            if old.cache_dir in self._new_stems:
                target = self._new_stems[old.cache_dir]
                selection = subscriber.pair_selection or old.pair
                if selection is not None and selection.wav_generation != target:
                    message = "migrated WAV lineage differs from the saved complete pair"
                    raise ValueError(message)
                updates |= {"cache_dir": target, "stems": expected_stem_files(target)}
            if subscriber.pair_selection is not None:
                updates["pair"] = subscriber.pair_selection
                updates["available"] = True
            candidate.stem_cache[sample_id] = StemCacheEntry.model_validate(
                old.model_dump() | updates
            )

    def _commit_current_image(self) -> None:
        if self._journal is None or self._retirement is None:
            message = "migration transaction resources disappeared"
            raise RuntimeError(message)
        revision = self._persistence.revision
        candidate = self._candidate()
        image = self._assets.prepare_assignment_image(candidate)
        try:
            committed_revision, digest = self._persistence.commit_migration(
                self._journal.transaction_id, revision, candidate
            )
        except OSError, RuntimeError, ValueError:
            self._assets.release_assignment_image(image)
            raise
        if self._persistence.revision != revision:
            # Durable config already changed; retain both complete images and retry
            # from current intent. Never reinterpret this as a preclaim rollback.
            self._owners.extend(lease for _, lease in image[1].values())
            self._unresolved("newer project intent arrived during the atomic migration write")
            return
        self._project.sample_paths = candidate.sample_paths
        self._project.pad_content = candidate.pad_content
        self._project.pad_source_beatmap_intent = candidate.pad_source_beatmap_intent
        self._project.pad_source_sync_intent = candidate.pad_source_sync_intent
        self._project.stem_cache = candidate.stem_cache
        self._project.material_migrations = candidate.material_migrations
        with self._retirement.activate():
            self._assets.publish_assignment_image(image, preserve_previous=True)
        self._record(
            "config_committed",
            committed_revision=committed_revision,
            committed_config_sha256=digest,
        )
        for sample_id in self._subscribers:
            content = self._project.pad_content[sample_id]
            if content is not None and content.material_creation is not None:
                self._queue_material_creation_restore(sample_id, content)
        self._release_settled_resources()

    def _unresolved(self, message: str) -> None:
        self.error = message
        try:
            self._unresolved_phases = self._phase_signature()
        except OSError, RuntimeError, ValueError:
            self._unresolved_phases = None
        for sample_id in self._subscribers:
            self._session.sample_load_errors[sample_id] = message
        if self._journal is not None and self._journal.phase != "unresolved":
            try:
                self._record("unresolved", error=message)
            except OSError, RuntimeError, ValueError:
                # Keep the live native owners and writer/launch fences even if
                # recording this failure fails; neither phase nor permission is invented.
                self._session.sample_load_errors.update(dict.fromkeys(self._subscribers, message))

    def _fail_before_admission(self, message: str) -> None:
        self.error = message
        self._preclaim_abort = message
        for work in self._stem_work.values():
            work.cancel()
        if self._preparation is not None:
            self._preparation.cancel()
        self._poll_preclaim_abort()

    def _poll_preclaim_abort(self) -> None:
        """Wait for actual readers before rolling back only owned unpublished artifacts."""
        if any(work.status() == "preparing" for work in self._stem_work.values()):
            return
        if self._preparation is not None and self._preparation.status() == "preparing":
            return
        if not self._settle_aborted_capture():
            return
        self._retire_created_stems()
        if self._preparation is not None:
            if any(item.source is not None for item in self._subscribers.values()):
                self._preparation.abort_rejected()
            else:
                self._preparation.abort_unpublished()
        if self._store is not None and self._capture_logged:
            self._record("failed", error=self._preclaim_abort)
        elif self._journal is not None:
            self._journal = self._journal.changed(phase="failed", error=self._preclaim_abort)
        self._preclaim_abort = None
        self._release_settled_resources()

    def _settle_aborted_capture(self) -> bool:
        if self._artifact_future is not None:
            if not self._artifact_future.done():
                return False
            # Incomplete receipts grant no cleanup permission; the worker returned.
            with suppress(OSError, RuntimeError, ValueError):
                self._artifact_captures.append(self._artifact_future.result())
            self._artifact_future = None
        return True

    def _retire_created_stems(self) -> None:
        if self._retirement is not None:
            with self._retirement.activate():
                for work in self._stem_work.values():
                    if work.status() == "ready" and work.created():
                        target = Path(work.cache_reference())
                        self._assets.retire(
                            target, recursive=True, lease=self._assets.acquire(target)
                        )

    def _release_settled_resources(self) -> None:
        if self._hold is not None:
            self._hold.release()
            self._hold = None
        for subscriber in self._subscribers.values():
            if subscriber.original_owner is not None:
                subscriber.original_owner[1].release()
                subscriber.original_owner = None
            subscriber.pair_prepared = None
            subscriber.pair_future = None
        for owner in self._owners:
            owner.release()
        self._owners.clear()
        for capture in self._artifact_captures:
            capture.release()
        self._artifact_captures.clear()
        if self._retirement is not None:
            self._retirement.close()
            self._retirement = None
        self._settle_writer()
        if (
            self._preparation is not None
            and self._journal is not None
            and self._journal.phase == "config_committed"
        ):
            self._preparation.release_preparation()
        if self._ledger_enabled and self._journal is not None and self._journal.artifacts:
            self._cleanup.add(self._journal)
        self._preparation = None

    def _settle_writer(self) -> None:
        if self._fence_owner is not None:
            if self._recovery_active is not None and self._recovery_fence_id is not None:
                self._persistence.transfer_migration(self._fence_owner, self._recovery_fence_id)
                self._recovery_owner = self._recovery_fence_id
                if self._journal is not None and self._journal.phase == "config_committed":
                    self._recovery_pending.remove(self._recovery_active)
                    if self._ledger_enabled:
                        self._history_future = self._ensure_artifacts().transfer_history(
                            self._recovery_active, self._journal, self._project
                        )
                else:
                    self._recovery_errors.append(self.error or "recovery remains unresolved")
                self._recovery_active = None
            else:
                self._persistence.release_migration(self._fence_owner)
            self._fence_owner = None

    def _poll_history_transfer(self) -> None:
        future = self._history_future
        if future is None or not future.done():
            return
        try:
            journal = future.result()
            if self._journal is not None and self._journal.transaction_id == journal.transaction_id:
                self._journal = journal
            self._cleanup.add(journal)
        except (OSError, RuntimeError, ValueError) as error:
            self._recovery_errors.append(str(error))
            self.cleanup_error = str(error)
        self._history_future = None

    def retry_commit(self) -> None:
        """Retry a retained ACK/config failure using current intent and actual source phases."""
        if self.status != "unresolved":
            message = "there is no unresolved material migration"
            raise ValueError(message)
        if any(
            subscriber.source is None
            or subscriber.source.phase() != "acknowledged"
            or not subscriber.source.is_current()
            for subscriber in self._subscribers.values()
            if self._matches(subscriber)
        ):
            message = "migration source still requires fresh native acknowledgement"
            raise ValueError(message)
        self.error = None
        self._record("adoption_pending")

    def _poll_cleanup(self) -> None:
        if not self._ledger_enabled or self._arrangement_recovery_deferred or self._recovery_errors:
            return
        try:
            self._cleanup.poll(
                self._ensure_artifacts(),
                self._project,
                admit=self._artifact_future is None
                and self._history_future is None
                and not self._startup_scheduled
                and not self._recovery_pending
                and not self._session.loading_sample_ids
                and self.status in {"idle", "failed", "config_committed"},
            )
        except (OSError, RuntimeError, ValueError) as error:
            # A failed pending-path probe preserves its real inventory lease.
            self.cleanup_error = str(error)
        else:
            self.cleanup_error = self._cleanup.error

    def retry_cleanup(self) -> None:
        """Retry a bounded inventory after references or physical readers have changed."""
        self._cleanup.retry()
        self.cleanup_error = None

    def shut_down(self) -> None:
        """Preserve an unresolved current-intent journal before native owners are drained."""
        if self._journal is not None and self.status not in {"failed", "config_committed"}:
            message = "material migration paused by shutdown; both artifact sets retained"
            self.error = message
            try:
                self._record("unresolved", error=message)
            except (OSError, RuntimeError, ValueError) as error:
                self._unresolved(str(error))
        if self._artifact_reconciler is not None:
            self._artifact_reconciler.shut_down()
