"""One original whole-bank intent through Native and durable settlement."""

import json
import threading
from dataclasses import dataclass, field
from pathlib import Path
from queue import Empty, SimpleQueue
from typing import TYPE_CHECKING, cast
from uuid import uuid4

from flitzis_looper.bank_snapshot import capture_bank_snapshot, project_with_bank_snapshot
from flitzis_looper.constants import NUM_PADS
from flitzis_looper.controller.bank_arrangement_journal import (
    BankArrangementJournal,
    BankJournalOwner,
    bank_before_project,
    bank_journal_snapshots,
    project_with_journal_bank,
)
from flitzis_looper.controller.bank_arrangement_mirror import (
    prepare_bank_arrangement_mirror,
    validate_bank_mirror_admission,
)
from flitzis_looper.controller.bank_arrangement_startup import BankArrangementStartup
from flitzis_looper.controller.copy_arrangement_custody import (
    CopyJournalCustody,
    journal_project_images,
)

if TYPE_CHECKING:
    from flitzis_looper.bank_snapshot import BankSnapshot
    from flitzis_looper.controller.app import AppController
    from flitzis_looper.controller.asset_lifecycle import (
        AssetRetirementReservation,
        AssignmentPublicationPlan,
    )
    from flitzis_looper.controller.bank_arrangement_mirror import PreparedBankMirror
    from flitzis_looper.controller.bank_arrangement_startup import OriginalBankRecoveryHandoff
    from flitzis_looper.controller.pad_arrangement import AssignmentImage
    from flitzis_looper.controller.persistence import ConfigCommitReceipt
    from flitzis_looper.models import ProjectState
    from flitzis_looper_audio import (
        BankMutationAcknowledgement,
        BankMutationTicket,
        BankParticipants,
        CapturedBankMutation,
    )


@dataclass(frozen=True, slots=True)
class BankRequest:
    """The complete gesture retained through confirmation without a writer lease."""

    bank: BankSnapshot
    participants: BankParticipants


@dataclass(frozen=True, slots=True)
class _PreparedBank:
    owner: str
    ticket: BankMutationTicket | None
    error: str | None


@dataclass(slots=True)
class _Bank:
    owner: str
    request: BankRequest
    initial: BankArrangementJournal
    target_ids: frozenset[int]
    participant_ids: frozenset[int]
    inherited_key_lock: bool
    retirement: AssetRetirementReservation | None = None
    custody: CopyJournalCustody = field(default_factory=CopyJournalCustody)
    journal: BankJournalOwner | None = None
    journal_open_attempted: bool = False
    captured: CapturedBankMutation | None = None
    ticket: BankMutationTicket | None = None
    acknowledgement: BankMutationAcknowledgement | None = None
    worker_done: bool = True
    cancel_requested: bool = False
    cancellation_proven: bool = False
    published: bool = False
    mirror: PreparedBankMirror | None = None
    images: list[AssignmentImage] = field(default_factory=list)
    initial_image: AssignmentImage | None = None
    assignment: AssignmentImage | None = None
    publication: AssignmentPublicationPlan | None = None
    candidate: ProjectState | None = None
    candidate_revision: int | None = None
    receipt: ConfigCommitReceipt | None = None
    native_finished: bool = False
    writer_released: bool = False
    failure: str | None = None
    recovery_handoff: OriginalBankRecoveryHandoff | None = None


class BankArrangementController:
    """Retain one parent owner; historical data never supplies a Native ACK.

    Startup recovery transfers into this owner. Existing target
    preparation and disk-selection settlements must be terminal before admission.
    """

    def __init__(self, app: AppController) -> None:
        self._app = app
        self._thread = threading.get_ident()
        self._request: BankRequest | None = None
        self._bank: _Bank | None = None
        self._results: SimpleQueue[_PreparedBank] = SimpleQueue()
        self._closing = False
        self._status = "idle"
        self.error: str | None = None
        self._startup: BankArrangementStartup | None = None

    def bind_startup(self) -> None:
        """Retain durable readers and writer ownership before ordinary restore."""
        self._require_thread()
        if self._startup is not None:
            message = "Bank startup ownership is already bound"
            raise RuntimeError(message)
        self._startup = BankArrangementStartup(self._app)

    @property
    def status(self) -> str:
        """Report the actual live or fresh-process settlement stage."""
        if self._bank is None and self._startup is not None and self._startup.active:
            return self._startup.status
        return self._status

    def _require_thread(self) -> None:
        if threading.get_ident() != self._thread:
            message = "Bank settlement belongs to its original Controller thread"
            raise RuntimeError(message)

    @property
    def active(self) -> bool:
        """Whether a real transaction retains any live or durable obligation."""
        return self._bank is not None or (self._startup is not None and self._startup.active)

    @property
    def fenced_sample_ids(self) -> frozenset[int]:
        """All original Native participants remain reserved through settlement."""
        if self._bank is not None:
            return self._bank.participant_ids
        return frozenset() if self._startup is None else self._startup.fenced_sample_ids

    @property
    def observer_fenced_sample_ids(self) -> frozenset[int]:
        """Participant observers retain their owners while the bank is unsettled."""
        if self._bank is not None:
            return self.fenced_sample_ids
        return frozenset() if self._startup is None else self._startup.observer_fenced_sample_ids

    def ensure_global_key_lock_edit_allowed(self) -> None:
        """Preserve the exact inherited pitch intent until the parent finishes."""
        if self._startup is not None:
            self._startup.ensure_global_key_lock_edit_allowed()
        if self._bank is not None and self._bank.inherited_key_lock:
            message = "Key Lock inheritance is reserved by a pending Bank"
            raise RuntimeError(message)

    def _another_arrangement(self) -> bool:
        return self._app.arrangement.active or self._app.pairs.active

    def capture_request(self, source_bank: int | None, target_bank: int) -> BankRequest:
        """Capture actual Native originals before the logical confirmation image."""
        self._require_thread()
        if self._closing or self.active or self._another_arrangement():
            message = "Another complete arrangement owns admission"
            raise RuntimeError(message)
        participants = self._app._audio.capture_bank_participants(source_bank, target_bank)
        snapshot = capture_bank_snapshot(self._app.project, source_bank, target_bank)
        request = BankRequest(snapshot, participants)
        self._request = request
        return request

    def discard_request(self, request: BankRequest) -> None:
        """Cancel only this still-unreserved confirmation gesture."""
        self._require_thread()
        if self._request is request:
            self._request = None

    def begin(self, request: BankRequest) -> bool:
        """Consume the original request once and validate the detached writer image."""
        self._require_thread()
        if (
            self._request is not request
            or self._closing
            or self.active
            or self._another_arrangement()
        ):
            return False
        self._request = None
        persistence = self._app.persistence
        if persistence.load_error is not None:
            self.error = persistence.load_error
            return False
        owner = uuid4().hex
        revision, captured, digest = persistence.capture_migration(owner)
        try:
            project_with_bank_snapshot(captured, request.bank)
            data = request.bank.projected()
            initial = BankArrangementJournal(
                transaction_id=owner,
                config_reference=persistence.config_reference,
                captured_revision=revision,
                intent_revision=revision,
                captured_config_sha256=digest,
                bank=request.bank,
                captured_project_image_json=captured.model_dump_json(),
                project_image_json=captured.model_dump_json(),
                image_role="before",
                phase="captured",
            )
            validate_bank_mirror_admission(self._app, initial)
        except OSError, RuntimeError, TypeError, ValueError:
            persistence.release_migration(owner)
            raise
        targets = frozenset(row.target_id for row in data.slots)
        participants = targets | frozenset(
            row.source_id for row in data.slots if row.source_id is not None
        )
        self._bank = _Bank(
            owner,
            request,
            initial,
            targets,
            participants,
            any(
                image.source_identity is not None and image.key_lock is None
                for row in data.slots
                for image in (row.before, row.after)
            ),
        )
        self._app._sync_arrangement_observers()
        self.error = None
        self._status = "captured"
        self.poll()
        return self.active or self._status == "completed"

    def poll(self) -> None:
        """Retry the same owners and exact uncertain append, never another capture."""
        self._require_thread()
        self._collect()
        if self._bank is None and self._startup is not None and self._startup.active:
            handoff = self._startup.poll()
            self.error = self._startup.error
            if handoff is not None:
                self._adopt_handoff(handoff)
        bank = self._bank
        if bank is None:
            return
        try:
            self._advance(bank)
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            bank.failure = self.error = str(error)[:2048]
            self._status = "settlement_pending"
            if bank.acknowledgement is None and bank.receipt is None:
                bank.cancel_requested = True

    def _adopt_handoff(self, handoff: OriginalBankRecoveryHandoff) -> None:
        initial = handoff.initial
        data = initial.bank.projected()
        targets = frozenset(row.target_id for row in data.slots)
        self._bank = _Bank(
            initial.transaction_id,
            BankRequest(initial.bank, handoff.participants),
            initial,
            targets,
            targets | frozenset(row.source_id for row in data.slots if row.source_id is not None),
            any(
                image.source_identity is not None and image.key_lock is None
                for row in data.slots
                for image in (row.before, row.after)
            ),
            retirement=handoff.retirement,
            custody=handoff.custody,
            journal=handoff.journal,
            journal_open_attempted=True,
            recovery_handoff=handoff,
        )
        self._status = "captured"
        self._app._sync_arrangement_observers()

    def _collect(self) -> None:
        while True:
            try:
                result = self._results.get_nowait()
            except Empty:
                return
            bank = self._bank
            if bank is None or result.owner != bank.owner:
                message = "Bank preparation returned without its original owner"
                raise RuntimeError(message)
            bank.worker_done = True
            bank.ticket = result.ticket
            if result.error is not None:
                bank.failure = self.error = result.error[:2048]
                bank.cancel_requested = True

    def _ensure_journal(self, bank: _Bank) -> BankJournalOwner:
        if bank.retirement is None:
            # Original, map and legacy-stem retirement units for every target.
            bank.retirement = self._app._assets.reserve(11 * NUM_PADS)
        bank.custody.acquire(
            self._app._assets,
            Path.cwd(),
            bank_journal_snapshots(bank.initial),
            project_images=(
                bank.initial.captured_project_image_json,
                bank.initial.project_image_json,
                *journal_project_images(bank.journal.encoded if bank.journal else ()),
            ),
        )
        if bank.journal is None:
            bank.journal_open_attempted = True
            bank.journal = BankJournalOwner(Path.cwd() / "samples", bank.initial)
        journal = bank.journal
        if journal.intended is not None:
            journal.append(
                BankArrangementJournal.model_validate_json(journal.intended, strict=True)
            )
        if journal.journal is None:
            journal.append(bank.initial)
        return journal

    def _before_record(self, bank: _Bank, phase: str) -> BankArrangementJournal:
        image = bank_before_project(self._app.project, bank.initial)
        bank.custody.acquire(
            self._app._assets, Path.cwd(), (), project_images=(image.model_dump_json(),)
        )
        return bank.initial.changed(
            phase=phase,
            intent_revision=self._app.persistence.revision,
            project_image_json=image.model_dump_json(),
            error=bank.failure,
        )

    def _advance(self, bank: _Bank) -> None:
        if bank.cancel_requested and not bank.journal_open_attempted and bank.captured is None:
            # No append or Native admission was attempted; partial reader acquisition is ours.
            self._settle(bank, "cancelled")
            return
        journal = self._ensure_journal(bank)
        if self._settle_terminal(bank, journal):
            return
        if bank.cancel_requested:
            self._cancel(bank)
            if self._bank is None or not bank.worker_done:
                return
        if not bank.cancel_requested:
            self._start_preparation(bank)
        if bank.worker_done and bank.ticket is not None:
            self._advance_ticket(bank, journal)

    def _settle_terminal(self, bank: _Bank, journal: BankJournalOwner) -> bool:
        if journal.journal is not None and journal.journal.phase in {"settled", "cancelled"}:
            self._settle(bank, "completed" if journal.journal.phase == "settled" else "cancelled")
            return True
        if bank.native_finished:
            assert bank.receipt is not None
            journal.append(bank.initial.committed(bank.receipt).changed(phase="settled"))
            self._settle(bank, "completed")
            return True
        return False

    def _advance_ticket(self, bank: _Bank, journal: BankJournalOwner) -> None:
        assert bank.ticket is not None
        phase = bank.ticket.publication_status()
        if phase == "rejected":
            if not bank.cancel_requested and bank.failure is None and self.error is None:
                bank.failure = self.error = (
                    "Native Bank mutation was rejected before acceptance; retry the Bank action."
                )
            bank.cancel_requested = True
            self._cancel(bank)
            return
        if phase not in {"prepared", "pending", "adopting", "accepted", "cancelling"}:
            message = f"Unsupported Native Bank publication status: {phase}"
            raise RuntimeError(message)
        if phase == "prepared" and not bank.cancel_requested:
            self._prepare_initial_image(bank)
            journal.append(self._before_record(bank, "publication_pending"))
            self._app._audio.publish_bank_mutation(bank.ticket)
            bank.published = True
            self._status = "publication_pending"
        elif phase in {"pending", "adopting", "accepted"}:
            bank.published = True
        if not self._observe_acknowledgement(bank):
            return
        if journal.journal is not None and journal.journal.phase == "publication_pending":
            journal.append(self._before_record(bank, "native_ack_observed"))
        self._commit(bank)

    def _start_preparation(self, bank: _Bank) -> None:
        if bank.captured is not None:
            return
        validate_bank_mirror_admission(self._app, bank.initial)
        data = bank.request.bank.projected()
        intents = [
            json.dumps(
                {
                    "schema_version": 1,
                    "snapshot": row.source.model_dump(mode="json"),
                    "resolved_key_lock": row.resolved_key_lock,
                },
                sort_keys=True,
                separators=(",", ":"),
                allow_nan=False,
            )
            if row.source is not None and row.source.source_identity is not None
            else None
            for row in data.slots
        ]
        bank.captured = self._app._audio.capture_bank_mutation(
            data.source_bank,
            data.target_bank,
            intents,
            participants=bank.request.participants,
        )
        captured, audio, results, owner = (
            bank.captured,
            self._app._audio,
            self._results,
            bank.owner,
        )
        bank.worker_done = False

        def prepare() -> None:
            try:
                result = _PreparedBank(owner, audio.prepare_bank_mutation(captured), None)
            except (OSError, RuntimeError, TypeError, ValueError) as error:
                result = _PreparedBank(owner, None, str(error))
            results.put(result)

        try:
            self._app.stems._stem_task_runner(prepare)
        except OSError, RuntimeError, TypeError, ValueError:
            bank.worker_done = True
            bank.cancel_requested = True
            raise

    def _prepare_initial_image(self, bank: _Bank) -> None:
        if bank.initial_image is None:
            image = self._app._assets.prepare_assignment_image(
                project_with_journal_bank(self._app.project, bank.initial)
            )
            bank.images.append(image)
            bank.initial_image = image

    def _observe_acknowledgement(self, bank: _Bank) -> bool:
        assert bank.ticket is not None
        if bank.acknowledgement is None:
            bank.acknowledgement = bank.ticket.acknowledgement()
            if bank.acknowledgement is None:
                return False
        return self._app._audio.reconcile_bank_mutation(bank.ticket, bank.acknowledgement)

    def _prepare_mirror(self, bank: _Bank) -> PreparedBankMirror:
        if bank.mirror is None:
            assert bank.ticket is not None
            ticket = bank.ticket

            def validate(ack: object) -> bool:
                return self._app._audio.validate_bank_mutation(
                    ticket, cast("BankMutationAcknowledgement", ack)
                )

            bank.mirror = prepare_bank_arrangement_mirror(
                self._app,
                bank.initial,
                ticket,
                validate_acknowledgement=validate,
                waveform_views=tuple(self._app._pair_waveform_views),
            )
        return bank.mirror

    def _prepare_assignment(self, bank: _Bank, revision: int) -> None:
        if bank.candidate is not None and bank.candidate_revision == revision:
            return
        candidate = self._prepare_mirror(bank).projected_project()
        bank.custody.acquire(
            self._app._assets, Path.cwd(), (), project_images=(candidate.model_dump_json(),)
        )
        assets = self._app._assets
        image = assets.prepare_assignment_image(candidate)
        try:
            publication = assets.prepare_assignment_publication(image)
        except OSError, RuntimeError, TypeError, ValueError:
            assets.release_assignment_image(image)
            raise
        previous, previous_plan = bank.assignment, bank.publication
        bank.images.append(image)
        bank.assignment, bank.publication = image, publication
        bank.candidate, bank.candidate_revision = candidate, revision
        if (
            previous is not None
            and previous_plan is not None
            and previous_plan.owns_unpublished_image
        ):
            assets.release_assignment_image(previous)
            bank.images[:] = [held for held in bank.images if held is not previous]

    def _apply_assignment(self, bank: _Bank) -> None:
        assert bank.publication is not None
        assert bank.retirement is not None
        try:
            with bank.retirement.activate():
                bank.publication.apply()
        finally:
            if not bank.publication.owns_unpublished_image:
                bank.images[:] = [held for held in bank.images if held is not bank.assignment]

    def _commit(self, bank: _Bank) -> None:
        journal, persistence = bank.journal, self._app.persistence
        assert journal is not None
        mirror = self._prepare_mirror(bank)
        if (
            bank.publication is not None
            and bank.publication.applied
            and not bank.publication.settled
        ):
            self._apply_assignment(bank)
        revision = persistence.revision
        if bank.receipt is None or bank.receipt.revision != revision:
            self._prepare_assignment(bank, revision)
            journal.append(self._before_record(bank, "native_ack_observed"))
            assert bank.candidate is not None
            bank.receipt = persistence.commit_migration_receipt(
                bank.owner, revision, bank.candidate
            )
        journal.append(bank.initial.committed(bank.receipt))
        if persistence.revision != bank.receipt.revision:
            return
        mirror.apply(bank.acknowledgement)
        self._apply_assignment(bank)
        if not mirror.settle_retired():
            return
        self._release_images(bank)
        if persistence.revision != bank.receipt.revision:
            return
        assert bank.ticket is not None
        assert bank.acknowledgement is not None
        self._app._audio.finish_bank_mutation(bank.ticket, bank.acknowledgement)
        bank.native_finished = True
        journal.append(bank.initial.committed(bank.receipt).changed(phase="settled"))
        self._settle(bank, "completed")

    def _cancel(self, bank: _Bank) -> None:
        if bank.acknowledgement is not None or bank.receipt is not None:
            return
        if not bank.cancellation_proven:
            if bank.captured is not None and not self._app._audio.cancel_bank_mutation(
                bank.captured
            ):
                self._status = "cancelling"
                return
            bank.cancellation_proven = True
        if not bank.worker_done:
            return
        assert bank.journal is not None
        bank.journal.append(self._before_record(bank, "cancelled"))
        self._settle(bank, "cancelled")

    def _release_images(self, bank: _Bank) -> None:
        while bank.images:
            self._app._assets.release_assignment_image(bank.images[-1])
            bank.images.pop()

    def _settle(self, bank: _Bank, status: str) -> None:
        self._release_images(bank)
        if bank.recovery_handoff is not None:
            assert self._startup is not None
            self._startup.complete_original(bank.recovery_handoff)
        bank.custody.release()
        if not bank.writer_released:
            self._app.persistence.release_migration(bank.owner)
            bank.writer_released = True
        if bank.retirement is not None:
            bank.retirement.close()
        self._bank = None
        self._app._sync_arrangement_observers()
        self._status = status
        if status == "completed":
            self.error = None

    def cancel(self) -> bool:
        """Request cancellation while keeping actual worker and claim ownership."""
        self._require_thread()
        if self._bank is not None:
            self._bank.cancel_requested = True
        self.poll()
        return not self.active

    def prepare_shutdown(self) -> bool:
        """Close admission and settle the same original owner through normal polling."""
        self._require_thread()
        self._closing = True
        self._request = None
        return self.cancel()
