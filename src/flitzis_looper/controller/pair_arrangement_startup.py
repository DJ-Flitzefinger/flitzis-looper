"""Own fresh-process Pair recovery before restore, without replaying an after layout."""

import hashlib
import threading
from dataclasses import dataclass
from itertools import pairwise
from pathlib import Path
from typing import TYPE_CHECKING
from uuid import uuid4

from flitzis_looper.controller.material_migration_recovery import (
    config_reference_matches,
    inspect_migration_recovery,
    resolve_config_reference,
)
from flitzis_looper.controller.pair_arrangement_journal import (
    PairArrangementAfterData,
    PairArrangementAfterImage,
    PairArrangementJournal,
    PairJournalOwner,
    validated_pair_history,
    validated_project_image,
    written_pair_image_matches,
    written_project_intent,
)
from flitzis_looper.controller.pair_arrangement_recovery import (
    PairRecoveryDecision,
    inspect_pair_recovery,
)
from flitzis_looper.controller.persistence import PersistenceFenceError
from flitzis_looper.copy_snapshot import canonical_snapshot_json
from flitzis_looper.models import ProjectState
from flitzis_looper.pair_snapshot import capture_pair_snapshot, project_with_pair_snapshot

if TYPE_CHECKING:
    from flitzis_looper.controller.app import AppController
    from flitzis_looper.controller.asset_lifecycle import AssetRetirementReservation
    from flitzis_looper.controller.persistence import ConfigCommitReceipt
    from flitzis_looper.pair_snapshot import PairSnapshot
    from flitzis_looper_audio import RecoveredPairAcceptance


@dataclass(slots=True)
class OriginalPairRecoveryHandoff:
    """One transferred writer/retirement/journal owner for the existing live coordinator."""

    owner: str
    snapshot: PairSnapshot
    initial: PairArrangementJournal
    journal: PairJournalOwner
    retirement: AssetRetirementReservation
    predecessor: PairJournalOwner
    ancestors: tuple[PairJournalOwner, ...] = ()
    writer_transferred: bool = False
    delivered: bool = False
    completed: bool = False


@dataclass(slots=True)
class _AfterRecovery:
    journal: PairJournalOwner
    decision: PairRecoveryDecision
    ancestors: tuple[PairJournalOwner, ...] = ()
    terminal: bool = False
    acceptance: RecoveredPairAcceptance | None = None
    receipt: ConfigCommitReceipt | None = None
    acceptance_released: bool = False


def _before_image(snapshot: PairSnapshot) -> PairArrangementAfterImage:
    original = snapshot.projected()
    data = PairArrangementAfterData(
        source_id=original.source_id,
        target_id=original.target_id,
        source=original.source,
        target=original.target,
    )
    return PairArrangementAfterImage(
        payload_json=canonical_snapshot_json(data.model_dump(mode="json"))
    )


class PairArrangementStartup:
    """Retain selected exact histories and a Config fence through genuine fresh restore.

    Construct before ``loader.restore_samples_from_project_state``. ``poll`` runs
    on the original Controller thread after the ordinary restore observers. Input
    fences are immediate; restore observers remain reachable until fresh Native
    custody is captured. Original pending intent is handed to the existing Pair
    coordinator, while a recognized after image never publishes another Pair.
    """

    def __init__(self, app: AppController) -> None:
        self._app = app
        self._thread = threading.get_ident()
        self.samples = Path.cwd() / "samples"
        self.pending_records: tuple[PairArrangementJournal, ...] = ()
        self.histories: list[PairJournalOwner] = []
        self.error: str | None = None
        self.status = "idle"
        self._fence: str | None = None
        self._retirement: AssetRetirementReservation | None = None
        self._original: PairJournalOwner | None = None
        self._cancelled_tip: PairJournalOwner | None = None
        self._after: _AfterRecovery | None = None
        self._handoff: OriginalPairRecoveryHandoff | None = None
        self._unknown = False
        self._completed = False
        self._compacted_ancestors: set[str] = set()
        try:
            self._initialize()
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self.error = str(error)
            self.status = "recovery_pending"

    @property
    def active(self) -> bool:
        """Whether any recognized recovery or unresolved evidence still owns an obligation."""
        return not self._completed and bool(self.pending_records or self._unknown or self.error)

    @property
    def fenced_sample_ids(self) -> frozenset[int]:
        """Fence gestures at participant slots, or every slot for unknown evidence."""
        if not self.active:
            return frozenset()
        if self._unknown:
            return frozenset(range(len(self._app.project.sample_paths)))
        return frozenset(
            sample_id
            for record in self.pending_records
            for sample_id in (
                record.before.projected().source_id,
                record.before.projected().target_id,
            )
        )

    @property
    def observer_fenced_sample_ids(self) -> frozenset[int]:
        """Defer participant observers only once actual Native owners can retain them."""
        if self._after is not None and self._after.acceptance is not None:
            return self.fenced_sample_ids
        if self._handoff is not None and self._handoff.delivered:
            return self.fenced_sample_ids
        return frozenset()

    def _require_thread(self) -> None:
        if threading.get_ident() != self._thread:
            message = "Pair startup recovery must retain its original Controller thread"
            raise RuntimeError(message)

    def _config_bytes(self) -> bytes | None:
        config = self._app.persistence.config_path
        try:
            return config.read_bytes()
        except FileNotFoundError:
            return None

    def _initialize(self) -> None:
        persistence = self._app.persistence
        recovery = inspect_migration_recovery(self.samples, persistence.config_path)
        self.pending_records = tuple(
            record
            for record in recovery.pair_journals
            if record.phase not in {"settled", "cancelled"}
        )
        self._unknown = bool(recovery.errors)
        if not self.pending_records and not self._unknown:
            self._completed = True
            return
        if self._unknown:
            self._capture_writer(uuid4().hex)
            self.error = "; ".join(recovery.errors)
            self.status = "unresolved"
            return
        if self._open_selected_lineage(recovery.pair_journals):
            self._initialize_tip()

    def _open_selected_lineage(self, records: list[PairArrangementJournal]) -> bool:
        # Terminal descendants still supersede an unfinished original. Keep all
        # complete histories available until the one relevant lineage is selected.
        for record in records:
            self.histories.append(PairJournalOwner.open(self.samples, record.transaction_id))
        try:
            self.histories = self._selected_lineage(self.histories)
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self._unknown = True
            self._capture_writer(uuid4().hex)
            self.error = str(error)
            self.status = "unresolved"
            return False
        self.pending_records = tuple(
            owner.journal for owner in self.histories if owner.journal is not None
        )
        return True

    def _initialize_tip(self) -> None:
        persistence = self._app.persistence
        root = self.histories[0]
        assert root.journal is not None
        self._capture_writer(root.journal.transaction_id)
        selected = self.histories[-1]
        assert selected.journal is not None
        actual_reference = resolve_config_reference(self.samples, persistence.config_reference)
        if actual_reference != persistence.config_reference:
            message = "Selected Config custody changed location; Pair evidence remains fenced"
            raise PersistenceFenceError(message)
        decision = self._tip_decision(selected.journal)
        if decision.disposition == "conflict":
            self.error, self.status = decision.error, "conflict"
            return
        self._retirement = self._app._assets.reserve(64)
        assert self._fence is not None
        if self._fence != selected.journal.transaction_id:
            persistence.transfer_migration(self._fence, selected.journal.transaction_id)
            self._fence = selected.journal.transaction_id
        if decision.disposition in {"original_pending", "cancelled"}:
            self._restore_before_intent(selected, cancelled=decision.disposition == "cancelled")
        elif decision.disposition == "recognized_after":
            self._after = _AfterRecovery(selected, decision, tuple(self.histories[:-1]))
            self.status = "restore_after"
        elif decision.disposition == "settled":
            self._after = _AfterRecovery(
                selected, decision, tuple(self.histories[:-1]), terminal=True
            )
            self.status = "restore_after"

    def _capture_writer(self, owner: str) -> None:
        # A Legacy/unknown recovery may already own the writer. Never steal or open
        # its fence; this startup owner keeps selected Pair evidence visible.
        self._app.persistence.capture_migration(owner)
        self._fence = owner

    def _selected_lineage(self, owners: list[PairJournalOwner]) -> list[PairJournalOwner]:
        by_id = {owner.initial.transaction_id: owner for owner in owners}
        unfinished = {record.transaction_id for record in self.pending_records}
        children: dict[str, list[str]] = {}
        for owner in owners:
            parent = owner.initial.resume_of
            if parent is not None:
                children.setdefault(parent, []).append(owner.initial.transaction_id)
        root_id = self._lineage_root(next(iter(unfinished)), by_id)
        chain = self._lineage_chain(root_id, by_id, children)
        visited = {owner.initial.transaction_id for owner in chain}
        if not unfinished <= visited:
            message = "Multiple interrupted Pair roots remain unresolved; preserve every history"
            raise ValueError(message)
        self._validate_lineage(chain)
        return chain

    @staticmethod
    def _lineage_root(start: str, by_id: dict[str, PairJournalOwner]) -> str:
        root_id = start
        seen: set[str] = set()
        while by_id[root_id].initial.resume_of is not None:
            if root_id in seen:
                message = "Cyclic Pair recovery lineage remains unresolved"
                raise ValueError(message)
            seen.add(root_id)
            parent = by_id[root_id].initial.resume_of
            if parent not in by_id:
                message = "Pair recovery predecessor is missing; preserve remaining history"
                raise ValueError(message)
            root_id = parent
        return root_id

    @staticmethod
    def _lineage_chain(
        root_id: str,
        by_id: dict[str, PairJournalOwner],
        children: dict[str, list[str]],
    ) -> list[PairJournalOwner]:
        chain = []
        current_id = root_id
        visited: set[str] = set()
        while current_id not in visited:
            visited.add(current_id)
            chain.append(by_id[current_id])
            descendants = children.get(current_id, [])
            if len(descendants) > 1:
                message = "Forked Pair recovery lineage remains unresolved"
                raise ValueError(message)
            if not descendants:
                break
            current_id = descendants[0]
        if current_id in visited and children.get(current_id):
            message = "Cyclic Pair recovery lineage remains unresolved"
            raise ValueError(message)
        return chain

    def _validate_lineage(self, owners: list[PairJournalOwner]) -> None:
        for predecessor, successor in pairwise(owners):
            parent = predecessor.journal
            child = successor.initial
            assert parent is not None
            self._validate_lineage_edge(parent, child)

    def _validate_lineage_edge(
        self,
        parent: PairArrangementJournal,
        child: PairArrangementJournal,
    ) -> None:
        before, resumed = parent.before.projected(), child.before.projected()
        if parent.phase in {"settled", "cancelled"}:
            message = "Pair recovery cannot resume a terminal predecessor"
            raise ValueError(message)
        if (
            child.resume_of != parent.transaction_id
            or child.captured_config_sha256 != parent.captured_config_sha256
            or child.captured_revision < parent.intent_revision
            or not config_reference_matches(
                self.samples, child.config_reference, parent.config_reference
            )
        ):
            message = "Pair recovery lineage changes its Config capture or revision binding"
            raise ValueError(message)
        if (before.operation, before.source_id, before.target_id) != (
            resumed.operation,
            resumed.source_id,
            resumed.target_id,
        ):
            message = "Pair recovery lineage changes original operation or participant slots"
            raise ValueError(message)
        project = validated_project_image(child.project_image_json)
        if not written_pair_image_matches(project, _before_image(parent.before)):
            message = "Pair recovery lineage changes original content, musical intent or lineage"
            raise ValueError(message)
        projected = project_with_pair_snapshot(project, child.before)
        if not written_pair_image_matches(projected, parent.after):
            message = "Pair recovery lineage changes its complete original after image"
            raise ValueError(message)

    def _tip_decision(self, record: PairArrangementJournal) -> PairRecoveryDecision:
        decision = inspect_pair_recovery(record, self._config_bytes())
        if decision.disposition == "settled":
            self._validate_terminal_after(record)
        elif decision.disposition == "cancelled":
            current = self._config_bytes()
            digest = hashlib.sha256(current).hexdigest() if current is not None else None
            if digest != record.captured_config_sha256:
                return PairRecoveryDecision(
                    "conflict", error="Cancelled Pair Config changed; preserve current intent"
                )
        return decision

    def _restore_before_intent(self, selected: PairJournalOwner, *, cancelled: bool) -> None:
        persistence = self._app.persistence
        record = selected.journal
        assert record is not None
        assert self._fence is not None
        assert self._retirement is not None
        current = self._config_bytes()
        disk = ProjectState.model_validate_json(current) if current is not None else ProjectState()
        if persistence.revision != self._app.project.config_revision or written_project_intent(
            self._app.project
        ) != written_project_intent(disk):
            self.error = (
                "Newer unsaved current intent remains untouched; Pair recovery stays fenced"
            )
            self.status = "conflict"
            return
        intent = validated_project_image(record.project_image_json)
        intent.config_revision = max(intent.config_revision, record.intent_revision)
        if cancelled:
            self._cancelled_tip = selected
        else:
            self._original = selected
        persistence.recover_migration_intent(self._fence, intent)
        with self._retirement.activate():
            self._app._assets.sync_assignments()
        self.status = "restore_cancelled" if cancelled else "restore_original"

    def _restore_pending(self) -> bool:
        session = self._app.session
        ids = self.fenced_sample_ids
        return bool(ids & (session.loading_sample_ids | session.analyzing_sample_ids))

    def poll(self) -> OriginalPairRecoveryHandoff | None:
        """Advance one retained recovery, or hand one fresh live Pair owner to the coordinator."""
        self._require_thread()
        if not self.active or self._unknown or self.status in {"conflict", "unresolved"}:
            return None
        try:
            if self._fence is None or self._retirement is None or self._restore_pending():
                return None
            if self._original is not None:
                return self._prepare_handoff()
            if self._cancelled_tip is not None:
                self._complete_cancelled_tip()
            if self._after is not None:
                self._advance_after()
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self.error = str(error)
            self.status = "recovery_pending"
        return None

    def _prepare_handoff(self) -> OriginalPairRecoveryHandoff | None:
        assert self._original is not None
        assert self._original.journal is not None
        assert self._fence is not None
        assert self._retirement is not None
        predecessor = self._original.journal
        if self._handoff is None:
            if not written_pair_image_matches(self._app.project, _before_image(predecessor.before)):
                message = (
                    "Restored original Pair musical intent changed before fresh Native admission"
                )
                raise PersistenceFenceError(message)
            original = predecessor.before.projected()
            snapshot = capture_pair_snapshot(
                self._app.project,
                original.source_id,
                original.target_id,
                operation=original.operation,
            )
            initial = PairArrangementJournal(
                transaction_id=uuid4().hex,
                resume_of=predecessor.transaction_id,
                config_reference=self._app.persistence.config_reference,
                captured_revision=self._app.persistence.revision,
                intent_revision=self._app.persistence.revision,
                captured_config_sha256=predecessor.captured_config_sha256,
                before=snapshot,
                after=PairArrangementAfterImage.from_before(snapshot),
                project_image_json=self._app.project.model_dump_json(),
                image_role="before",
                phase="captured",
            )
            successor = PairJournalOwner(self.samples, initial)
            self._handoff = OriginalPairRecoveryHandoff(
                initial.transaction_id,
                snapshot,
                initial,
                successor,
                self._retirement,
                self._original,
                ancestors=tuple(self.histories),
            )
        handoff = self._handoff
        if handoff.delivered:
            return None
        handoff.journal.append(handoff.initial)
        if not handoff.writer_transferred:
            self._app.persistence.transfer_migration(self._fence, handoff.owner)
            self._fence = handoff.owner
            handoff.writer_transferred = True
        handoff.delivered = True
        self.status = "live_pair_handoff"
        self.error = None
        return handoff

    def complete_original(self, handoff: OriginalPairRecoveryHandoff) -> None:
        """Compact exact predecessor after the already transferred successor's durable finish.

        Call BEFORE the existing coordinator releases its writer/retirement.
        No second transfer, Native capture, Swap or historical mirror is performed.
        Failure retains both complete histories and the coordinator's same owners.
        """
        self._require_thread()
        if handoff is not self._handoff or not handoff.writer_transferred or not handoff.delivered:
            message = "Foreign or untransferred Pair startup handoff"
            raise ValueError(message)
        if handoff.completed:
            return
        child, _ = self._validated_original_records(handoff)
        self._validate_original_config(child)
        ancestors = handoff.ancestors or (handoff.predecessor,)
        self._compact_ancestors(ancestors, handoff.journal)
        handoff.completed = True
        self._completed = True
        self.status, self.error = "completed", None

    def _validated_original_records(
        self,
        handoff: OriginalPairRecoveryHandoff,
    ) -> tuple[PairArrangementJournal, PairArrangementJournal]:
        child = handoff.journal.journal
        parent = handoff.predecessor.journal
        persistence = self._app.persistence
        message = "Pair startup successor lacks its actual complete durable finish"
        if child is None or parent is None:
            raise PersistenceFenceError(message)
        if (
            child.phase not in {"settled", "cancelled"}
            or handoff.journal.intended is not None
            or handoff.predecessor.intended is not None
        ):
            raise PersistenceFenceError(message)
        if (
            child.resume_of != parent.transaction_id
            or persistence._migration_owner != handoff.owner
            or not config_reference_matches(
                self.samples, child.config_reference, parent.config_reference
            )
        ):
            raise PersistenceFenceError(message)
        if (
            child.before != handoff.initial.before
            or child.after != handoff.initial.after
            or not written_pair_image_matches(
                validated_project_image(handoff.initial.project_image_json),
                _before_image(parent.before),
            )
        ):
            raise PersistenceFenceError(message)
        return child, parent

    def _validate_original_config(self, child: PairArrangementJournal) -> None:
        persistence = self._app.persistence
        if child.phase == "settled":
            if (
                persistence.revision != child.committed_revision
                or self._config_bytes() != child.project_image_json.encode("utf-8")
            ):
                message = "Pair startup settled successor no longer matches actual Config"
                raise PersistenceFenceError(message)
        else:
            config_bytes = self._config_bytes()
            digest = hashlib.sha256(config_bytes).hexdigest() if config_bytes is not None else None
            before = validated_project_image(child.project_image_json)
            current_revision = self._app.project.config_revision
            known_revisions = {before.config_revision, child.intent_revision}
            before.config_revision = current_revision
            if (
                digest != child.captured_config_sha256
                or persistence.revision != child.intent_revision
                or current_revision not in known_revisions
                or written_project_intent(self._app.project) != written_project_intent(before)
            ):
                message = (
                    "Pair startup cancelled successor changed original Config or current intent"
                )
                raise PersistenceFenceError(message)

    def _advance_after(self) -> None:
        assert self._after is not None
        assert self._retirement is not None
        assert self._fence is not None
        recovery, app = self._after, self._app
        record = recovery.journal.journal
        assert record is not None
        if recovery.terminal:
            self._advance_terminal_after(recovery, record)
            return
        if not written_pair_image_matches(app.project, record.after):
            message = (
                "Recognized after intent changed during restore; original evidence remains fenced"
            )
            raise PersistenceFenceError(message)
        original = record.before.projected()
        if recovery.acceptance is None:
            recovery.acceptance = app._audio.capture_recovered_pad_pair(
                original.source_id, original.target_id
            )
        if not recovery.acceptance_released:
            if not app._audio.validate_recovered_pad_pair(recovery.acceptance):
                message = "Fresh Pair recovery source/timing acceptance is no longer current"
                raise PersistenceFenceError(message)
            if recovery.receipt is None or recovery.receipt.revision != app.persistence.revision:
                # Current restored rows already contain the after layout. Save only
                # that current independent image with genuinely fresh Native timing.
                recovery.receipt = app.persistence.commit_migration_receipt(
                    self._fence,
                    app.persistence.revision,
                    app.project.model_copy(deep=True),
                )
            recovery.journal.append(record.committed(recovery.receipt))
            if app.persistence.revision != recovery.receipt.revision:
                return
            if not app._audio.validate_recovered_pad_pair(recovery.acceptance):
                message = "Fresh Pair recovery acceptance changed after actual Config commit"
                raise PersistenceFenceError(message)
            if not app._audio.release_recovered_pad_pair(recovery.acceptance):
                return
            recovery.acceptance_released = True
        committed = recovery.journal.journal
        assert committed is not None
        recovery.journal.append(committed.changed(phase="settled"))
        latest = recovery.journal.journal
        assert latest is not None
        self._validate_terminal_after(latest)
        self._compact_ancestors(recovery.ancestors, recovery.journal)
        self._finish_startup()

    def _validate_terminal_after(self, record: PairArrangementJournal) -> None:
        persistence = self._app.persistence
        if (
            persistence.revision != record.committed_revision
            or self._config_bytes() != record.project_image_json.encode("utf-8")
        ):
            message = (
                "Terminal Pair Config or revision changed; "
                "preserve newer intent without reopening history"
            )
            raise PersistenceFenceError(message)
        if written_project_intent(self._app.project) != written_project_intent(
            validated_project_image(record.project_image_json),
        ):
            message = (
                "Terminal Pair current intent changed during restore; "
                "preserve fence without a forged receipt"
            )
            raise PersistenceFenceError(message)

    def _advance_terminal_after(
        self,
        recovery: _AfterRecovery,
        record: PairArrangementJournal,
    ) -> None:
        self._validate_terminal_after(record)
        original = record.before.projected()
        audio = self._app._audio
        if recovery.acceptance is None:
            recovery.acceptance = audio.capture_recovered_pad_pair(
                original.source_id, original.target_id
            )
        if not recovery.acceptance_released:
            if not audio.validate_recovered_pad_pair(recovery.acceptance):
                message = "Terminal Pair fresh source/timing acceptance is no longer current"
                raise PersistenceFenceError(message)
            self._compact_ancestors(recovery.ancestors, recovery.journal)
            if not audio.release_recovered_pad_pair(recovery.acceptance):
                return
            recovery.acceptance_released = True
        self._finish_startup()

    def _complete_cancelled_tip(self) -> None:
        assert self._cancelled_tip is not None
        record = self._cancelled_tip.journal
        assert record is not None
        self._validate_original_config(record)
        self._compact_ancestors(tuple(self.histories[:-1]), self._cancelled_tip)
        self._finish_startup()

    def _finish_startup(self) -> None:
        assert self._fence is not None
        assert self._retirement is not None
        self._app.persistence.release_migration(self._fence)
        self._retirement.close()
        self._completed = True
        self.status, self.error = "completed", None

    def _compact_ancestors(
        self,
        ancestors: tuple[PairJournalOwner, ...],
        successor: PairJournalOwner,
    ) -> None:
        child = successor.journal
        assert child is not None
        if self._app.persistence._migration_owner != child.transaction_id:
            message = "Pair lineage lost its already transferred Config writer"
            raise PersistenceFenceError(message)
        remaining = [
            owner
            for owner in ancestors
            if owner.initial.transaction_id not in self._compacted_ancestors
        ]
        chain = [*remaining, successor]
        self._validate_lineage(chain)
        for owner in chain:
            current = owner.store.history()
            if owner.intended is not None or current != owner.encoded:
                message = "Pair startup history changed before exact predecessor compaction"
                raise ValueError(message)
            validated_pair_history(current, owner.initial.transaction_id)
        for owner in remaining:
            owner.store.compact(owner.encoded)
            self._compacted_ancestors.add(owner.initial.transaction_id)
