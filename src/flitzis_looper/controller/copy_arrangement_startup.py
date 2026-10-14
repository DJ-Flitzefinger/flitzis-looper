"""Own occupied Copy restart evidence before ordinary restore and cleanup."""

import hashlib
import threading
from dataclasses import dataclass
from itertools import pairwise
from pathlib import Path
from typing import TYPE_CHECKING
from uuid import uuid4

from flitzis_looper.controller.copy_arrangement_custody import CopyJournalCustody
from flitzis_looper.controller.copy_arrangement_journal import (
    CopyArrangementJournal,
    CopyJournalOwner,
    validated_copy_history,
    written_copy_image_matches,
)
from flitzis_looper.controller.copy_arrangement_recovery import (
    copy_before_project,
    inspect_copy_recovery,
)
from flitzis_looper.controller.material_migration_recovery import (
    config_reference_matches,
    inspect_migration_recovery,
    resolve_config_reference,
)
from flitzis_looper.controller.pair_arrangement_journal import (
    validated_project_image,
    written_project_intent,
)
from flitzis_looper.controller.persistence import PersistenceFenceError
from flitzis_looper.models import ProjectState

if TYPE_CHECKING:
    from flitzis_looper.controller.app import AppController
    from flitzis_looper.controller.asset_lifecycle import AssetRetirementReservation
    from flitzis_looper.controller.persistence import ConfigCommitReceipt
    from flitzis_looper_audio import InputRuntimePadBinding, RecoveredCopyAcceptance


@dataclass(slots=True)
class OriginalCopyRecoveryHandoff:
    """Transfer one existing writer to the same live Copy path with its fixed C."""

    initial: CopyArrangementJournal
    journal: CopyJournalOwner
    retirement: AssetRetirementReservation
    ancestors: tuple[CopyJournalOwner, ...]
    custody: CopyJournalCustody
    source_binding: InputRuntimePadBinding | None = None
    target_binding: InputRuntimePadBinding | None = None
    transferred: bool = False
    delivered: bool = False
    completed: bool = False


@dataclass(slots=True)
class _RestoredAfter:
    owner: CopyJournalOwner
    terminal: bool
    acceptance: RecoveredCopyAcceptance | None = None
    receipt: ConfigCommitReceipt | None = None
    released: bool = False


class CopyArrangementStartup:
    """Keep gesture fences separate from loader observers until genuine fresh ACK."""

    def __init__(self, app: AppController) -> None:
        self._app = app
        self._thread = threading.get_ident()
        self.samples = Path.cwd() / "samples"
        self.histories: list[CopyJournalOwner] = []
        self.pending_records: tuple[CopyArrangementJournal, ...] = ()
        self.status = "idle"
        self.error: str | None = None
        self._unknown = False
        self._completed = False
        self._fence: str | None = None
        self._retirement: AssetRetirementReservation | None = None
        self._custody = CopyJournalCustody()
        self._original: CopyJournalOwner | None = None
        self._cancelled: CopyJournalOwner | None = None
        self._after: _RestoredAfter | None = None
        self._handoff: OriginalCopyRecoveryHandoff | None = None
        self._compacted: set[str] = set()
        self._finishing = False
        self._before_recovery = False
        self._selected_ready = False
        self._recovered_intent: str | None = None
        try:
            self._initialize()
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self.error, self.status = str(error), "recovery_pending"

    @property
    def active(self) -> bool:
        """Whether recognized or unresolved evidence still owns recovery."""
        return not self._completed and bool(self.pending_records or self._unknown or self.error)

    @property
    def fenced_sample_ids(self) -> frozenset[int]:
        """Fence original A/B before capture and only fixed C during after recovery."""
        if not self.active:
            return frozenset()
        if self._unknown:
            return frozenset(range(len(self._app.project.sample_paths)))
        ids = {record.target_id for record in self.pending_records}
        if self._before_recovery and (self._handoff is None or not self._handoff.delivered):
            ids.update(record.source_id for record in self.pending_records)
        return frozenset(ids)

    @property
    def observer_fenced_sample_ids(self) -> frozenset[int]:
        """Keep fresh loader/timing/stem completion reachable before owning Native C."""
        if self._after is not None and self._after.acceptance is not None:
            return self.fenced_sample_ids
        if self._handoff is not None and self._handoff.delivered:
            return frozenset({self._handoff.initial.target_id})
        return frozenset()

    def ensure_global_key_lock_edit_allowed(self) -> None:
        """Preserve inherited target resolution during unresolved Copy recovery."""
        if self.active and (
            self._unknown
            or any(record.source.projected().key_lock is None for record in self.pending_records)
        ):
            message = "Key Lock inheritance is reserved by occupied Copy recovery"
            raise PersistenceFenceError(message)

    def _bytes(self) -> bytes | None:
        try:
            return self._app.persistence.config_path.read_bytes()
        except FileNotFoundError:
            return None

    def _initialize(self) -> None:
        persistence = self._app.persistence
        recovery = inspect_migration_recovery(self.samples, persistence.config_path)
        self.pending_records = tuple(
            record
            for record in recovery.copy_journals
            if record.phase not in {"settled", "cancelled"}
        )
        self._unknown = bool(recovery.errors)
        if not self.pending_records and not self._unknown:
            self._completed = True
            return
        if self._unknown:
            self._capture(uuid4().hex)
            self.error, self.status = "; ".join(recovery.errors), "unresolved"
            return
        try:
            owners = [
                CopyJournalOwner.open(self.samples, record.transaction_id)
                for record in recovery.copy_journals
            ]
            self.histories = self._select(owners)
        except OSError, RuntimeError, TypeError, ValueError:
            self._unknown = True
            self._capture(uuid4().hex)
            raise
        self.pending_records = tuple(
            owner.journal for owner in self.histories if owner.journal is not None
        )
        selected = self.histories[-1]
        record = selected.journal
        assert record is not None
        self._capture(record.transaction_id)
        if (
            resolve_config_reference(self.samples, persistence.config_reference)
            != persistence.config_reference
        ):
            message = "Copy Config custody changed location; evidence remains fenced"
            raise PersistenceFenceError(message)
        self._initialize_selected(selected)

    def _initialize_selected(self, selected: CopyJournalOwner) -> None:
        record = selected.journal
        assert record is not None
        decision = inspect_copy_recovery(
            record,
            self._bytes(),
            history=validated_copy_history(selected.encoded, record.transaction_id),
        )
        self._before_recovery = decision.disposition in {"original_pending", "cancelled"}
        if self._retirement is None:
            self._retirement = self._app._assets.reserve(64)
        self._custody.acquire(
            self._app._assets,
            self.samples.parent,
            (record.source, record.predecessor, record.after),
        )
        if decision.disposition == "conflict":
            self.error, self.status = decision.error, "conflict"
            return
        if decision.disposition in {"original_pending", "cancelled"}:
            self._restore_before(selected, cancelled=decision.disposition == "cancelled")
        else:
            self._restore_recognized_intent(decision.project_image_json)
            self._validate_current_after(record, terminal=decision.disposition == "settled")
            self._after = _RestoredAfter(selected, decision.disposition == "settled")
            self.status = "restore_after"
        self._selected_ready = True
        self.error = None

    def _restore_recognized_intent(self, image_json: str | None) -> None:
        if image_json is None:
            return
        if self._recovered_intent is not None:
            self._sync_recovered_assignments()
            return
        image = validated_project_image(image_json)
        if written_project_intent(image) == written_project_intent(self._app.project):
            return
        current_bytes = self._bytes()
        disk = (
            ProjectState.model_validate_json(current_bytes)
            if current_bytes is not None
            else ProjectState()
        )
        if not self._current_matches_disk_or_recovered(disk):
            message = "Unsaved current intent remains untouched during recognized Copy recovery"
            raise PersistenceFenceError(message)
        assert self._fence is not None
        assert self._retirement is not None
        self._app.persistence.recover_migration_intent(self._fence, image)
        self._recovered_intent = image.model_dump_json()
        self._sync_recovered_assignments()

    def _sync_recovered_assignments(self) -> None:
        assert self._retirement is not None
        with self._retirement.activate():
            self._app._assets.sync_assignments()

    def _current_matches_disk_or_recovered(self, disk: ProjectState) -> bool:
        current = written_project_intent(self._app.project)
        return current == written_project_intent(disk) or (
            self._recovered_intent is not None
            and current == written_project_intent(validated_project_image(self._recovered_intent))
        )

    def _capture(self, owner: str) -> None:
        # Mixed/Legacy owners may already fence Config. A failed capture preserves
        # their owner and this startup's visible unresolved obligation.
        self._app.persistence.capture_migration(owner)
        self._fence = owner

    def _select(self, owners: list[CopyJournalOwner]) -> list[CopyJournalOwner]:
        by_id = {owner.initial.transaction_id: owner for owner in owners}
        pending = {record.transaction_id for record in self.pending_records}
        children: dict[str, list[str]] = {}
        for owner in owners:
            if owner.initial.resume_of is not None:
                children.setdefault(owner.initial.resume_of, []).append(
                    owner.initial.transaction_id
                )
        current = self._root_id(next(iter(pending)), by_id)
        chain = self._chain(current, by_id, children)
        visited = {owner.initial.transaction_id for owner in chain}
        if not pending <= visited:
            message = "Multiple interrupted Copy roots remain unresolved"
            raise ValueError(message)
        self._validate_edges(chain)
        return chain

    @staticmethod
    def _root_id(current: str, by_id: dict[str, CopyJournalOwner]) -> str:
        visited: set[str] = set()
        while by_id[current].initial.resume_of is not None:
            if current in visited:
                message = "Cyclic Copy recovery lineage"
                raise ValueError(message)
            visited.add(current)
            parent = by_id[current].initial.resume_of
            if parent not in by_id:
                message = "Copy recovery predecessor is missing"
                raise ValueError(message)
            current = parent
        return current

    @staticmethod
    def _chain(
        current: str, by_id: dict[str, CopyJournalOwner], children: dict[str, list[str]]
    ) -> list[CopyJournalOwner]:
        chain: list[CopyJournalOwner] = []
        visited: set[str] = set()
        while current not in visited:
            visited.add(current)
            chain.append(by_id[current])
            descendants = children.get(current, [])
            if len(descendants) > 1:
                message = "Forked Copy recovery lineage"
                raise ValueError(message)
            if not descendants:
                break
            current = descendants[0]
        if children.get(current) and current in visited:
            message = "Cyclic Copy recovery lineage"
            raise ValueError(message)
        return chain

    def _validate_edges(self, owners: list[CopyJournalOwner]) -> None:
        for previous, successor in pairwise(owners):
            parent, child = previous.journal, successor.initial
            assert parent is not None
            if parent.phase in {"settled", "cancelled"} or (
                child.resume_of != parent.transaction_id
                or child.captured_revision < parent.intent_revision
                or not config_reference_matches(
                    self.samples, child.config_reference, parent.config_reference
                )
            ):
                message = "Copy lineage changes Config, revision or terminal predecessor"
                raise ValueError(message)
            for name in (
                "captured_config_sha256",
                "source_id",
                "target_id",
                "source",
                "predecessor",
                "after",
                "resolved_key_lock",
                "captured_project_image_json",
            ):
                if getattr(parent, name) != getattr(child, name):
                    message = "Copy lineage changes immutable A/B/C capture"
                    raise ValueError(message)

    def _restore_before(self, selected: CopyJournalOwner, *, cancelled: bool) -> None:
        record = selected.journal
        assert record is not None
        assert self._fence is not None
        assert self._retirement is not None
        persistence = self._app.persistence
        current_bytes = self._bytes()
        disk = (
            ProjectState.model_validate_json(current_bytes)
            if current_bytes is not None
            else ProjectState()
        )
        if self._recovered_intent is None and not self._current_matches_disk_or_recovered(disk):
            message = "Unsaved current intent remains untouched; Copy recovery fenced"
            raise PersistenceFenceError(message)
        intent = validated_project_image(record.project_image_json)
        intent.config_revision = max(intent.config_revision, record.intent_revision)
        if not cancelled and not (
            written_copy_image_matches(intent, record.source, record.source_id)
            and written_copy_image_matches(intent, record.predecessor, record.target_id)
        ):
            message = "Original Copy A/B changed; recovery must not copy newer A"
            raise PersistenceFenceError(message)
        if self._recovered_intent is None:
            persistence.recover_migration_intent(self._fence, intent)
            self._recovered_intent = intent.model_dump_json()
        self._sync_recovered_assignments()
        self._cancelled = selected if cancelled else None
        self._original = None if cancelled else selected
        self.status = "restore_cancelled" if cancelled else "restore_original"

    def poll(self) -> OriginalCopyRecoveryHandoff | None:
        """Admit original capture or settle only the actual restored target acceptance."""
        if threading.get_ident() != self._thread:
            message = "Copy startup recovery must retain its Controller thread"
            raise RuntimeError(message)
        if not self.active or self._unknown or self.status in {"conflict", "unresolved"}:
            return None
        try:
            if not self._prepare_poll():
                return None
            if self._original is not None:
                return self._original_handoff()
            if self._cancelled is not None:
                self._validate_cancelled(self._cancelled)
                self._compact(self._cancelled)
                self._finish()
            if self._after is not None:
                self._advance_after()
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self.error, self.status = str(error), "recovery_pending"
        return None

    def _prepare_poll(self) -> bool:
        if self._finishing:
            self._finish()
            return False
        if self._fence is not None and self.histories and not self._selected_ready:
            self._initialize_selected(self.histories[-1])
        if self._fence is None or self._retirement is None or not self._custody.acquired:
            return False
        pending = self._app.session.loading_sample_ids | self._app.session.analyzing_sample_ids
        return not bool(self.fenced_sample_ids & pending)

    def _original_handoff(self) -> OriginalCopyRecoveryHandoff | None:
        assert self._original is not None
        assert self._original.journal is not None
        assert self._retirement is not None
        assert self._fence is not None
        record = self._original.journal
        project = self._app.project
        if self._handoff is None:
            if not (
                written_copy_image_matches(project, record.source, record.source_id)
                and written_copy_image_matches(project, record.predecessor, record.target_id)
            ):
                message = "Restored Copy A/B changed before fresh admission"
                raise PersistenceFenceError(message)
            source = self._app._audio.current_input_runtime_pad_binding(record.source_id)
            target = self._app._audio.current_input_runtime_pad_binding(record.target_id)
            if source is None or target is None:
                message = "Original Copy A/B await fresh restored Native bindings"
                raise PersistenceFenceError(message)
            initial = record.changed(
                transaction_id=uuid4().hex,
                resume_of=record.transaction_id,
                captured_revision=self._app.persistence.revision,
                intent_revision=self._app.persistence.revision,
                project_image_json=project.model_dump_json(),
                image_role="before",
                phase="captured",
                committed_revision=None,
                committed_config_sha256=None,
                error=None,
            )
            owner = CopyJournalOwner(self.samples, initial)
            self._handoff = OriginalCopyRecoveryHandoff(
                initial,
                owner,
                self._retirement,
                tuple(self.histories),
                self._custody,
                source_binding=source,
                target_binding=target,
            )
        handoff = self._handoff
        if handoff.delivered:
            return None
        handoff.journal.append(handoff.initial)
        if not handoff.transferred:
            self._app.persistence.transfer_migration(self._fence, handoff.initial.transaction_id)
            self._fence = handoff.initial.transaction_id
            handoff.transferred = True
        handoff.delivered = True
        self.status, self.error = "live_copy_handoff", None
        return handoff

    def _validate_current_after(self, record: CopyArrangementJournal, *, terminal: bool) -> None:
        if not written_copy_image_matches(self._app.project, record.after, record.target_id):
            message = "Fixed Copy C changed during restore"
            raise PersistenceFenceError(message)
        data = record.after.projected()
        resolved = self._app.project.key_lock if data.key_lock is None else data.key_lock
        if resolved != record.resolved_key_lock:
            message = "Restored Copy C Key Lock inheritance changed"
            raise PersistenceFenceError(message)
        if terminal and (
            self._bytes() != record.project_image_json.encode("utf-8")
            or self._app.persistence.revision != record.committed_revision
            or written_project_intent(self._app.project)
            != written_project_intent(validated_project_image(record.project_image_json))
        ):
            message = "Terminal Copy receipt or current intent changed"
            raise PersistenceFenceError(message)

    def _advance_after(self) -> None:
        recovery = self._after
        assert recovery is not None
        assert self._fence is not None
        record = recovery.owner.journal
        assert record is not None
        if recovery.owner.intended is not None:
            recovery.owner.append(
                CopyArrangementJournal.model_validate_json(recovery.owner.intended, strict=True)
            )
            record = recovery.owner.journal
            assert record is not None
        self._validate_current_after(record, terminal=recovery.terminal)
        audio = self._app._audio
        if recovery.acceptance is None:
            if not self._app.stems.recovered_copy_selection_ready(
                record.target_id, record.after.projected().stem_cache
            ):
                self.status = "restore_selection"
                return
            binding = audio.current_input_runtime_pad_binding(record.target_id)
            if binding is None:
                message = "Restored Copy C has no current source binding"
                raise PersistenceFenceError(message)
            recovery.acceptance = audio.capture_recovered_pad_copy(binding)
        if not self._app.stems.recovered_copy_selection_ready(
            record.target_id, record.after.projected().stem_cache
        ):
            message = "Restored Copy C selection changed across its Native capture"
            raise PersistenceFenceError(message)
        if not recovery.released:
            if not self._commit_and_release_after(recovery, record):
                return
            recovery.released = True
        if not recovery.terminal:
            committed = recovery.owner.journal
            assert committed is not None
            recovery.owner.append(committed.changed(phase="settled"))
        settled = recovery.owner.journal
        assert settled is not None
        self._validate_current_after(settled, terminal=True)
        self._compact(recovery.owner)
        self._finish()

    def _commit_and_release_after(
        self, recovery: _RestoredAfter, record: CopyArrangementJournal
    ) -> bool:
        app, audio = self._app, self._app._audio
        assert recovery.acceptance is not None
        assert self._fence is not None
        if not audio.validate_recovered_pad_copy(recovery.acceptance):
            message = "Fresh Copy C source/timing acceptance changed"
            raise PersistenceFenceError(message)
        if not recovery.terminal:
            if recovery.receipt is None or recovery.receipt.revision != app.persistence.revision:
                if record.phase == "native_ack_observed" or (
                    record.phase == "config_committed"
                    and app.persistence.revision > record.intent_revision
                ):
                    before = copy_before_project(app.project, record)
                    recovery.owner.append(
                        record.changed(
                            phase="native_ack_observed",
                            image_role="before",
                            project_image_json=before.model_dump_json(),
                            intent_revision=app.persistence.revision,
                            committed_revision=None,
                            committed_config_sha256=None,
                        )
                    )
                else:
                    recovery.owner.append(record)
                recovery.receipt = app.persistence.commit_migration_receipt(
                    self._fence, app.persistence.revision, app.project.model_copy(deep=True)
                )
            recovery.owner.append(record.committed(recovery.receipt))
            if app.persistence.revision != recovery.receipt.revision:
                return False
        if not audio.validate_recovered_pad_copy(recovery.acceptance):
            message = "Fresh Copy C acceptance changed after Config commit"
            raise PersistenceFenceError(message)
        return audio.release_recovered_pad_copy(recovery.acceptance)

    def complete_original(self, handoff: OriginalCopyRecoveryHandoff) -> None:
        """Validate terminal successor before the live owner releases writer and readers."""
        if handoff is not self._handoff or not handoff.transferred or not handoff.delivered:
            message = "Foreign Copy recovery handoff"
            raise PersistenceFenceError(message)
        if handoff.completed:
            return
        record = handoff.journal.journal
        if record is None or record.phase not in {"settled", "cancelled"}:
            message = "Copy recovery successor lacks durable terminal finish"
            raise PersistenceFenceError(message)
        if self._app.persistence._migration_owner != handoff.initial.transaction_id:
            message = "Copy recovery successor lost transferred writer"
            raise PersistenceFenceError(message)
        if record.phase == "settled":
            self._validate_current_after(record, terminal=True)
        else:
            self._validate_cancelled(handoff.journal)
        self._compact(handoff.journal)
        handoff.completed = self._completed = True
        self.status, self.error = "completed", None

    def _validate_cancelled(self, owner: CopyJournalOwner) -> None:
        record = owner.journal
        assert record is not None
        current = self._bytes()
        digest = hashlib.sha256(current).hexdigest() if current is not None else None
        before = validated_project_image(record.project_image_json)
        before.config_revision = self._app.project.config_revision
        if digest != record.captured_config_sha256 or (
            written_project_intent(self._app.project) != written_project_intent(before)
        ):
            message = "Cancelled Copy Config or current intent changed"
            raise PersistenceFenceError(message)

    def _compact(self, successor: CopyJournalOwner) -> None:
        remaining = [
            owner
            for owner in self.histories
            if owner is not successor and owner.initial.transaction_id not in self._compacted
        ]
        chain = [*remaining, successor]
        self._validate_edges(chain)
        for owner in chain:
            if owner.intended is not None or owner.store.history() != owner.encoded:
                message = "Copy history changed before exact ancestor compaction"
                raise ValueError(message)
            validated_copy_history(owner.encoded, owner.initial.transaction_id)
        for owner in remaining:
            owner.store.compact(owner.encoded)
            self._compacted.add(owner.initial.transaction_id)

    def _finish(self) -> None:
        assert self._fence is not None
        assert self._retirement is not None
        self._finishing = True
        self._custody.release()
        self._retirement.close()
        self._app.persistence.release_migration(self._fence)
        self._completed = True
        self.status, self.error = "completed", None
