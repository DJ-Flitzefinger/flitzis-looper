"""Own complete Bank restart evidence before ordinary restore and cleanup."""

import hashlib
import threading
from dataclasses import dataclass
from itertools import pairwise
from pathlib import Path
from typing import TYPE_CHECKING
from uuid import uuid4

from flitzis_looper.bank_snapshot import project_with_bank_snapshot
from flitzis_looper.constants import NUM_PADS
from flitzis_looper.controller.bank_arrangement_journal import (
    BankArrangementJournal,
    BankJournalOwner,
    bank_before_project,
    bank_journal_snapshots,
    validate_bank_successor,
    validated_bank_history,
    written_bank_image_matches,
)
from flitzis_looper.controller.bank_arrangement_recovery import inspect_bank_recovery
from flitzis_looper.controller.copy_arrangement_custody import (
    CopyJournalCustody,
    journal_project_images,
)
from flitzis_looper.controller.material_migration_recovery import (
    config_reference_matches,
    inspect_migration_recovery,
    resolve_config_reference,
)
from flitzis_looper.controller.pair_arrangement_journal import (
    strict_json_object,
    validated_project_image,
    written_project_intent,
)
from flitzis_looper.controller.persistence import PersistenceFenceError
from flitzis_looper.copy_snapshot import canonical_snapshot_json
from flitzis_looper.models import ProjectState

if TYPE_CHECKING:
    from flitzis_looper.controller.app import AppController
    from flitzis_looper.controller.asset_lifecycle import AssetRetirementReservation
    from flitzis_looper.controller.persistence import ConfigCommitReceipt
    from flitzis_looper_audio import BankParticipants, RecoveredBankAcceptance


@dataclass(slots=True)
class OriginalBankRecoveryHandoff:
    """Transfer the existing writer and immutable bank to the normal live owner."""

    initial: BankArrangementJournal
    journal: BankJournalOwner
    retirement: AssetRetirementReservation
    ancestors: tuple[BankJournalOwner, ...]
    custody: CopyJournalCustody
    participants: BankParticipants
    transferred: bool = False
    delivered: bool = False
    completed: bool = False


@dataclass(slots=True)
class _RestoredBank:
    owner: BankJournalOwner
    terminal: bool
    acceptance: RecoveredBankAcceptance | None = None
    receipt: ConfigCommitReceipt | None = None
    native_owner: int | None = None
    released: bool = False


class BankArrangementStartup:
    """Retain one lineage and fresh all36 acceptance under the existing Config writer.

    Construct before ordinary Loader restore. Gestures remain fenced immediately;
    restore observers remain reachable until the fresh Native probe owns targets,
    and become reachable again whenever that same owner awaits queued restore work.
    """

    def __init__(self, app: AppController) -> None:
        self._app = app
        self._thread = threading.get_ident()
        self.samples = Path.cwd() / "samples"
        self.histories: list[BankJournalOwner] = []
        self.pending_records: tuple[BankArrangementJournal, ...] = ()
        self.status = "idle"
        self.error: str | None = None
        self._unknown = False
        self._completed = False
        self._fence: str | None = None
        self._retirement: AssetRetirementReservation | None = None
        self._custody = CopyJournalCustody()
        self._original: BankJournalOwner | None = None
        self._cancelled: BankJournalOwner | None = None
        self._after: _RestoredBank | None = None
        self._handoff: OriginalBankRecoveryHandoff | None = None
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
        """Fence all original participants, or only the fixed recovered target bank."""
        if not self.active:
            return frozenset()
        if self._unknown:
            return frozenset(range(len(self._app.project.sample_paths)))
        ids = {
            row.target_id
            for record in self.pending_records
            for row in record.bank.projected().slots
        }
        if self._before_recovery and (self._handoff is None or not self._handoff.delivered):
            ids.update(
                row.source_id
                for record in self.pending_records
                for row in record.bank.projected().slots
                if row.source_id is not None
            )
        return frozenset(ids)

    @property
    def observer_fenced_sample_ids(self) -> frozenset[int]:
        """Permit restore completion while the same recovered owner awaits its probe."""
        recovery = self._after
        if (
            recovery is not None
            and recovery.acceptance is not None
            and self._acceptance_status(recovery) != "waiting_restore"
        ):
            return self.fenced_sample_ids
        if self._handoff is not None and self._handoff.delivered:
            return frozenset(row.target_id for row in self._handoff.initial.bank.projected().slots)
        return frozenset()

    def ensure_global_key_lock_edit_allowed(self) -> None:
        """Keep inherited resolutions fixed across restore, capture and settlement."""
        if self.active and (
            self._unknown
            or any(
                row.after.source_identity is not None and row.after.key_lock is None
                for record in self.pending_records
                for row in record.bank.projected().slots
            )
            or (
                self._before_recovery
                and any(self._original_inherits(record) for record in self.pending_records)
            )
        ):
            message = "Key Lock inheritance is reserved by pending Bank recovery"
            raise PersistenceFenceError(message)

    def _require_thread(self) -> None:
        if threading.get_ident() != self._thread:
            message = "Bank startup recovery must retain its original Controller thread"
            raise RuntimeError(message)

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
            for record in recovery.bank_journals
            if record.phase not in {"settled", "cancelled"}
        )
        self._unknown = bool(recovery.errors)
        if not self.pending_records and not self._unknown:
            # A standalone terminal history carries no current startup obligation.
            self._completed = True
            return
        if self._unknown:
            self._capture(uuid4().hex)
            self.error, self.status = "; ".join(recovery.errors), "unresolved"
            return
        try:
            owners = [self._open_owner(record) for record in recovery.bank_journals]
            self.histories = self._select(owners)
        except OSError, RuntimeError, TypeError, ValueError:
            self._unknown = True
            self._capture(uuid4().hex)
            raise
        self.pending_records = tuple(
            owner.journal for owner in self.histories if owner.journal is not None
        )
        selected = self.histories[-1]
        assert selected.journal is not None
        self._capture(selected.journal.transaction_id)
        if resolve_config_reference(self.samples, persistence.config_reference) != (
            persistence.config_reference
        ):
            message = "Bank Config custody changed location; evidence remains fenced"
            raise PersistenceFenceError(message)
        self._initialize_selected(selected)

    def _open_owner(self, scanned: BankArrangementJournal) -> BankJournalOwner:
        owner = BankJournalOwner.open(self.samples, scanned.transaction_id)
        if owner.journal != scanned or not config_reference_matches(
            self.samples, self._app.persistence.config_reference, scanned.config_reference
        ):
            message = "Bank history or Config binding changed after complete inventory scan"
            raise ValueError(message)
        return owner

    def _capture(self, owner: str) -> None:
        self._app.persistence.capture_migration(owner)
        self._fence = owner

    def _select(self, owners: list[BankJournalOwner]) -> list[BankJournalOwner]:
        by_id = {owner.initial.transaction_id: owner for owner in owners}
        if len(by_id) != len(owners):
            message = "Duplicate Bank recovery transaction identity"
            raise ValueError(message)
        pending = {record.transaction_id for record in self.pending_records}
        children: dict[str, list[str]] = {}
        for owner in owners:
            if owner.initial.resume_of is not None:
                children.setdefault(owner.initial.resume_of, []).append(
                    owner.initial.transaction_id
                )
        current = self._root_id(next(iter(pending)), by_id)
        chain = self._chain(current, by_id, children)
        if not pending <= {owner.initial.transaction_id for owner in chain}:
            message = "Multiple interrupted Bank roots remain unresolved"
            raise ValueError(message)
        self._validate_edges(chain)
        return chain

    @staticmethod
    def _root_id(current: str, by_id: dict[str, BankJournalOwner]) -> str:
        visited: set[str] = set()
        while by_id[current].initial.resume_of is not None:
            if current in visited:
                message = "Cyclic Bank recovery lineage"
                raise ValueError(message)
            visited.add(current)
            parent = by_id[current].initial.resume_of
            if parent not in by_id:
                message = "Bank recovery predecessor is missing"
                raise ValueError(message)
            current = parent
        return current

    @staticmethod
    def _chain(
        current: str, by_id: dict[str, BankJournalOwner], children: dict[str, list[str]]
    ) -> list[BankJournalOwner]:
        chain: list[BankJournalOwner] = []
        visited: set[str] = set()
        while current not in visited:
            visited.add(current)
            chain.append(by_id[current])
            descendants = children.get(current, [])
            if len(descendants) > 1:
                message = "Forked Bank recovery lineage"
                raise ValueError(message)
            if not descendants:
                return chain
            current = descendants[0]
        message = "Cyclic Bank recovery lineage"
        raise ValueError(message)

    def _validate_edges(self, owners: list[BankJournalOwner]) -> None:
        for previous, successor in pairwise(owners):
            parent, child = previous.journal, successor.initial
            assert parent is not None
            validate_bank_successor(parent, child)
            if not config_reference_matches(
                self.samples, child.config_reference, parent.config_reference
            ):
                message = "Bank lineage changes actual Config custody"
                raise ValueError(message)

    def _initialize_selected(self, selected: BankJournalOwner) -> None:
        record = selected.journal
        assert record is not None
        decision = inspect_bank_recovery(record, self._bytes(), history=selected.encoded)
        self._before_recovery = decision.disposition in {"original_pending", "cancelled"}
        if self._retirement is None:
            self._retirement = self._app._assets.reserve(11 * NUM_PADS)
        self._custody.acquire(
            self._app._assets,
            self.samples.parent,
            bank_journal_snapshots(record),
            project_images=journal_project_images(
                text for owner in self.histories for text in owner.encoded
            ),
        )
        if decision.disposition == "conflict":
            self.error, self.status = decision.error, "conflict"
            return
        if decision.disposition in {"original_pending", "cancelled"}:
            self._restore_before(selected, cancelled=decision.disposition == "cancelled")
        else:
            self._restore_recognized_intent(record, decision.project_image_json)
            terminal = decision.disposition == "settled"
            self._validate_current_after(record, terminal=terminal)
            self._after = _RestoredBank(selected, terminal)
            self.status = "restore_after"
        self._selected_ready = True
        self.error = None

    def _current_matches_disk_or_recovered(self, disk: ProjectState) -> bool:
        current = written_project_intent(self._app.project)
        return current == written_project_intent(disk) or (
            self._recovered_intent is not None
            and current == written_project_intent(validated_project_image(self._recovered_intent))
        )

    def _sync_recovered_assignments(self) -> None:
        assert self._retirement is not None
        with self._retirement.activate():
            self._app._assets.sync_assignments()

    def _restore_before(self, selected: BankJournalOwner, *, cancelled: bool) -> None:
        record = selected.journal
        assert record is not None
        assert self._fence is not None
        current_bytes = self._bytes()
        disk = ProjectState.model_validate_json(current_bytes) if current_bytes else ProjectState()
        if self._recovered_intent is None and not self._current_matches_disk_or_recovered(disk):
            message = "Unsaved current intent remains untouched; Bank recovery fenced"
            raise PersistenceFenceError(message)
        intent = validated_project_image(record.project_image_json)
        intent.config_revision = max(intent.config_revision, record.intent_revision)
        if not cancelled:
            self._validate_original_participants(intent, record)
        if self._recovered_intent is None:
            self._app.persistence.recover_migration_intent(self._fence, intent)
            self._recovered_intent = intent.model_dump_json()
        self._sync_recovered_assignments()
        self._cancelled = selected if cancelled else None
        self._original = None if cancelled else selected
        self.status = "restore_cancelled" if cancelled else "restore_original"

    @staticmethod
    def _original_inherits(record: BankArrangementJournal) -> bool:
        return any(
            image is not None and image.source_identity is not None and image.key_lock is None
            for row in record.bank.projected().slots
            for image in (row.source, row.before)
        )

    @staticmethod
    def _validate_original_participants(
        project: ProjectState, record: BankArrangementJournal
    ) -> None:
        if BankArrangementStartup._original_inherits(record) and project.key_lock != (
            validated_project_image(record.captured_project_image_json).key_lock
        ):
            message = "Original Bank source/before inherited Key Lock changed"
            raise PersistenceFenceError(message)
        try:
            project_with_bank_snapshot(project, record.bank)
        except (TypeError, ValueError) as error:
            message = (
                "Original Bank source/before or inherited intent changed; fixed capture retained"
            )
            raise PersistenceFenceError(message) from error

    def _restore_recognized_intent(
        self, record: BankArrangementJournal, image_json: str | None
    ) -> None:
        if image_json is None:
            return
        if self._recovered_intent is not None:
            self._sync_recovered_assignments()
            return
        image = validated_project_image(image_json)
        current = self._app.project
        if written_project_intent(image) == written_project_intent(current):
            return
        disk_bytes = self._bytes()
        disk = ProjectState.model_validate_json(disk_bytes) if disk_bytes else ProjectState()
        if written_project_intent(image) == written_project_intent(disk) and (
            written_bank_image_matches(current, record.bank, image_role="after")
        ):
            # Newer source/outside intent is independent of the fixed recovered bank.
            return
        if not self._current_matches_disk_or_recovered(disk):
            message = "Competing unsaved current intent remains untouched during Bank recovery"
            raise PersistenceFenceError(message)
        assert self._fence is not None
        self._app.persistence.recover_migration_intent(self._fence, image)
        self._recovered_intent = image.model_dump_json()
        self._sync_recovered_assignments()

    def poll(self) -> OriginalBankRecoveryHandoff | None:
        """Admit original fixed intent or verify the fresh complete restored target bank."""
        self._require_thread()
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
        session = self._app.session
        pending = session.loading_sample_ids | session.analyzing_sample_ids
        return not bool(self.fenced_sample_ids & (pending | session.pending_sample_paths.keys()))

    def _original_selection_ready(self, record: BankArrangementJournal) -> bool:
        ready = True
        for row in record.bank.projected().slots:
            ready &= self._app.stems.recovered_copy_selection_ready(
                row.target_id, row.before.stem_cache
            )
            if row.source_id is not None and row.source is not None:
                ready &= self._app.stems.recovered_copy_selection_ready(
                    row.source_id, row.source.stem_cache
                )
        return ready

    def _original_handoff(self) -> OriginalBankRecoveryHandoff | None:
        assert self._original is not None
        assert self._original.journal is not None
        assert self._retirement is not None
        assert self._fence is not None
        record = self._original.journal
        if self._handoff is None:
            self._validate_original_participants(self._app.project, record)
            if not self._original_selection_ready(record):
                self.status = "restore_selection"
                return None
            bank = record.bank.projected()
            participants = self._app._audio.capture_bank_participants(
                bank.source_bank, bank.target_bank
            )
            initial = record.changed(
                transaction_id=uuid4().hex,
                resume_of=record.transaction_id,
                captured_revision=self._app.persistence.revision,
                intent_revision=self._app.persistence.revision,
                project_image_json=self._app.project.model_dump_json(),
                image_role="before",
                phase="captured",
                committed_revision=None,
                committed_config_sha256=None,
                error=None,
            )
            validate_bank_successor(record, initial)
            owner = BankJournalOwner(self.samples, initial)
            self._handoff = OriginalBankRecoveryHandoff(
                initial, owner, self._retirement, tuple(self.histories), self._custody, participants
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
        self.status, self.error = "live_bank_handoff", None
        return handoff

    def _validate_current_after(
        self, record: BankArrangementJournal, *, terminal: bool, live: bool = False
    ) -> None:
        project = self._app.project
        if not written_bank_image_matches(project, record.bank, image_role="after"):
            message = "Fixed Bank after image changed during restore"
            raise PersistenceFenceError(message)
        for row in record.bank.projected().slots:
            if row.after.source_identity is not None:
                resolved = project.key_lock if row.after.key_lock is None else row.after.key_lock
                if resolved != row.resolved_key_lock:
                    message = "Restored Bank target Key Lock inheritance changed"
                    raise PersistenceFenceError(message)
        if terminal and self._bytes() != record.project_image_json.encode("utf-8"):
            message = "Terminal Bank disk receipt changed"
            raise PersistenceFenceError(message)
        if live and (
            self._app.persistence.revision != record.committed_revision
            or written_project_intent(project)
            != written_project_intent(validated_project_image(record.project_image_json))
        ):
            message = "Live Bank successor current intent changed after its Config receipt"
            raise PersistenceFenceError(message)

    def _after_selection_ready(self, record: BankArrangementJournal) -> bool:
        ready = True
        for row in record.bank.projected().slots:
            ready &= self._app.stems.recovered_copy_selection_ready(
                row.target_id, row.after.stem_cache
            )
        return ready

    @staticmethod
    def _after_intents(record: BankArrangementJournal) -> list[str | None]:
        return [
            canonical_snapshot_json({
                "schema_version": 1,
                "resolved_key_lock": row.resolved_key_lock,
                "snapshot": row.after.model_dump(mode="json", warnings="error"),
            })
            if row.after.source_identity is not None
            else None
            for row in record.bank.projected().slots
        ]

    @staticmethod
    def _acceptance_status(recovery: _RestoredBank) -> str:
        assert recovery.acceptance is not None
        data = strict_json_object(recovery.acceptance.metadata_json())
        owner, status = data.get("owner"), data.get("status")
        if (
            type(owner) is not int
            or owner <= 0
            or status
            not in {
                "captured",
                "pending",
                "checking",
                "accepted",
                "waiting_restore",
                "stale",
                "released",
            }
        ):
            message = "Unrecognized fresh Bank acceptance status"
            raise ValueError(message)
        if recovery.native_owner is None:
            recovery.native_owner = owner
        if owner != recovery.native_owner:
            message = "Fresh Bank acceptance owner changed"
            raise ValueError(message)
        return str(status)

    def _advance_after(self) -> None:
        recovery = self._after
        assert recovery is not None
        record = recovery.owner.journal
        assert record is not None
        if recovery.owner.intended is not None:
            recovery.owner.append(
                BankArrangementJournal.model_validate_json(recovery.owner.intended, strict=True)
            )
            record = recovery.owner.journal
            assert record is not None
        self._validate_current_after(record, terminal=recovery.terminal)
        if not recovery.released and not self._fresh_acceptance_ready(recovery, record):
            return
        if not recovery.released:
            if not self._commit_and_release_after(recovery):
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

    def _fresh_acceptance_ready(
        self, recovery: _RestoredBank, record: BankArrangementJournal
    ) -> bool:
        audio = self._app._audio
        if not self._after_selection_ready(record):
            self.status = "restore_selection"
            return False
        if recovery.acceptance is None:
            target = record.bank.projected().target_bank
            participants = audio.capture_bank_participants(None, target)
            recovery.acceptance = audio.capture_recovered_bank(
                participants, self._after_intents(record)
            )
        status = self._acceptance_status(recovery)
        if status == "waiting_restore":
            audio.retry_recovered_bank(recovery.acceptance)
            self.status, self.error = "restore_probe", None
            return False
        if status in {"captured", "pending", "checking"}:
            self.status, self.error = "restore_probe", None
            return False
        if status != "accepted" or not audio.validate_recovered_bank(recovery.acceptance):
            message = "Fresh Bank acceptance is stale or no longer current"
            raise PersistenceFenceError(message)
        return True

    def _commit_and_release_after(self, recovery: _RestoredBank) -> bool:
        app, audio = self._app, self._app._audio
        assert recovery.acceptance is not None
        assert self._fence is not None
        record = recovery.owner.journal
        assert record is not None
        if not audio.validate_recovered_bank(recovery.acceptance):
            message = "Fresh Bank acceptance changed before Config commit"
            raise PersistenceFenceError(message)
        if not recovery.terminal:
            if recovery.receipt is None or recovery.receipt.revision != app.persistence.revision:
                self._custody.acquire(
                    app._assets,
                    self.samples.parent,
                    (),
                    project_images=(app.project.model_dump_json(),),
                )
                if record.phase == "native_ack_observed" or (
                    record.phase == "config_committed"
                    and app.persistence.revision > record.intent_revision
                ):
                    before = bank_before_project(app.project, record)
                    self._custody.acquire(
                        app._assets,
                        self.samples.parent,
                        (),
                        project_images=(before.model_dump_json(),),
                    )
                    record = recovery.owner.append(
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
                self._validate_current_after(record, terminal=False)
                if not audio.validate_recovered_bank(recovery.acceptance):
                    message = "Fresh Bank acceptance changed across recorded Config intent"
                    raise PersistenceFenceError(message)
                assert self._retirement is not None
                recovery.receipt = app.persistence.commit_migration_receipt(
                    self._fence, app.persistence.revision, app.project.model_copy(deep=True)
                )
            recovery.owner.append(record.committed(recovery.receipt))
            if app.persistence.revision != recovery.receipt.revision:
                return False
        current = recovery.owner.journal
        assert current is not None
        self._validate_current_after(current, terminal=True)
        if not audio.validate_recovered_bank(recovery.acceptance):
            message = "Fresh Bank acceptance changed after Config commit"
            raise PersistenceFenceError(message)
        return audio.release_recovered_bank(recovery.acceptance)

    def complete_original(self, handoff: OriginalBankRecoveryHandoff) -> None:
        """Validate a durable live successor before its owner releases writer/readers."""
        self._require_thread()
        if handoff is not self._handoff or not handoff.transferred or not handoff.delivered:
            message = "Foreign Bank recovery handoff"
            raise PersistenceFenceError(message)
        if handoff.completed:
            return
        record = handoff.journal.journal
        if record is None or record.phase not in {"settled", "cancelled"}:
            message = "Bank recovery successor lacks durable terminal finish"
            raise PersistenceFenceError(message)
        if self._app.persistence._migration_owner != handoff.initial.transaction_id:
            message = "Bank recovery successor lost transferred writer"
            raise PersistenceFenceError(message)
        if record.phase == "settled":
            self._validate_current_after(record, terminal=True, live=True)
        else:
            self._validate_cancelled(handoff.journal)
        self._compact(handoff.journal)
        handoff.completed = self._completed = True
        self.status, self.error = "completed", None

    def _validate_cancelled(self, owner: BankJournalOwner) -> None:
        record = owner.journal
        assert record is not None
        current = self._bytes()
        digest = hashlib.sha256(current).hexdigest() if current is not None else None
        before = validated_project_image(record.project_image_json)
        if digest != record.captured_config_sha256 or (
            written_project_intent(self._app.project) != written_project_intent(before)
        ):
            message = "Cancelled Bank Config or current intent changed"
            raise PersistenceFenceError(message)

    def _compact(self, successor: BankJournalOwner) -> None:
        if self._app.persistence._migration_owner != successor.initial.transaction_id:
            message = "Bank lineage lost its existing Config writer before compaction"
            raise PersistenceFenceError(message)
        chain = [owner for owner in self.histories if owner is not successor] + [successor]
        self._validate_edges(chain)
        remaining = [
            owner for owner in chain if owner.initial.transaction_id not in self._compacted
        ]
        for owner in remaining:
            if owner.intended is not None or owner.store.history() != owner.encoded:
                message = "Bank history changed before exact ancestor compaction"
                raise ValueError(message)
            validated_bank_history(owner.encoded, owner.initial.transaction_id)
        for owner in remaining:
            if owner is not successor:
                owner.store.compact(owner.encoded)
                self._compacted.add(owner.initial.transaction_id)

    def _finish(self) -> None:
        assert self._fence is not None
        assert self._retirement is not None
        self._finishing = True
        self._custody.release()
        self._retirement.close()
        self._app.persistence.release_migration(self._fence)
        if self._after is not None:
            self._after.acceptance = None
        self._completed = True
        self.status, self.error = "completed", None
