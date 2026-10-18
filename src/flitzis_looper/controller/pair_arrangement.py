"""Own one complete live Pair through Native, mirrors, Config and journal settlement."""

import threading
from dataclasses import dataclass, field
from pathlib import Path
from queue import Empty, SimpleQueue
from typing import TYPE_CHECKING, cast
from uuid import uuid4

from flitzis_looper.controller.pair_arrangement_journal import (
    PairArrangementAfterImage,
    PairArrangementJournal,
    PairJournalOwner,
)
from flitzis_looper.controller.pair_arrangement_startup import PairArrangementStartup
from flitzis_looper.controller.pair_control_mirror import prepare_pair_control_mirror
from flitzis_looper.controller.pair_stem_mirror import prepare_pair_stem_mirror
from flitzis_looper.pair_snapshot import capture_pair_snapshot

if TYPE_CHECKING:
    from flitzis_looper.controller.app import AppController
    from flitzis_looper.controller.asset_lifecycle import (
        AssetRetirementReservation,
        AssignmentPublicationPlan,
    )
    from flitzis_looper.controller.pad_arrangement import AssignmentImage
    from flitzis_looper.controller.pair_arrangement_startup import OriginalPairRecoveryHandoff
    from flitzis_looper.controller.pair_control_mirror import PreparedPairControlMirror
    from flitzis_looper.controller.pair_stem_mirror import PreparedPairStemMirror
    from flitzis_looper.controller.persistence import ConfigCommitReceipt
    from flitzis_looper.models import ProjectState
    from flitzis_looper.pair_snapshot import PairOperation, PairSnapshot
    from flitzis_looper_audio import (
        CapturedPadPair,
        InputRuntimePadBinding,
        PadPairAcknowledgement,
        PadPairTicket,
    )


@dataclass(frozen=True, slots=True)
class _Prepared:
    owner: str
    ticket: PadPairTicket | None
    error: str | None


@dataclass(slots=True)
class _Pair:
    owner: str
    snapshot: PairSnapshot
    retirement: AssetRetirementReservation
    initial: PairArrangementJournal
    source_binding: InputRuntimePadBinding | None = None
    journal: PairJournalOwner | None = None
    captured: CapturedPadPair | None = None
    ticket: PadPairTicket | None = None
    acknowledgement: PadPairAcknowledgement | None = None
    worker_done: bool = True
    published: bool = False
    cancel_requested: bool = False
    cancelled: bool = False
    mirror: PreparedPairControlMirror | None = None
    stems: PreparedPairStemMirror | None = None
    images: list[AssignmentImage] = field(default_factory=list)
    assignment: AssignmentImage | None = None
    receipt: ConfigCommitReceipt | None = None
    assets_published: bool = False
    native_finished: bool = False
    writer_released: bool = False
    candidate: ProjectState | None = None
    candidate_revision: int | None = None
    assignment_publication: AssignmentPublicationPlan | None = None
    recovery_handoff: OriginalPairRecoveryHandoff | None = None


class PairArrangementController:
    """Retain original participants and exact retry images until every lane settles."""

    def __init__(self, app: AppController) -> None:
        self._app = app
        self._thread = threading.get_ident()
        self._pair: _Pair | None = None
        self._results: SimpleQueue[_Prepared] = SimpleQueue()
        self._closing = False
        self.status = "idle"
        self.error: str | None = None
        self._startup = PairArrangementStartup(app)

    @property
    def active(self) -> bool:
        """Whether this owner still holds any live or durable obligation."""
        return self._pair is not None or self._startup.active

    @property
    def fenced_sample_ids(self) -> frozenset[int]:
        """Participants remain fenced even when a partial mirror is recognized."""
        if self._pair is None:
            return self._startup.fenced_sample_ids
        data = self._pair.snapshot.projected()
        return frozenset((data.source_id, data.target_id))

    @property
    def observer_fenced_sample_ids(self) -> frozenset[int]:
        """Allow genuine fresh restore before startup captures its current Native owners."""
        if self._pair is not None:
            return self.fenced_sample_ids
        return self._startup.observer_fenced_sample_ids

    def ensure_global_key_lock_edit_allowed(self) -> None:
        """Retain inherited intent while an original Pair or recovery owns it."""
        if not self.active:
            return
        if self._pair is not None:
            data = self._pair.snapshot.projected()
            occupied = (slot for slot in (data.source, data.target) if slot.source_identity)
            if all(slot.key_lock is not None for slot in occupied):
                return
        message = "Key Lock inheritance is reserved by a pending Pair"
        raise RuntimeError(message)

    def begin(
        self,
        source_id: int,
        target_id: int,
        *,
        operation: PairOperation,
        source_binding: InputRuntimePadBinding | None = None,
    ) -> bool:
        """Admit one Pair, retaining an optional original gesture source through retries."""
        self._require_thread()
        if self.active or self._closing or self._app.arrangement.active:
            return False
        if source_binding is not None and (
            source_binding.metadata().get("pad_id") != source_id or not source_binding.is_current()
        ):
            return False
        persistence = self._app.persistence
        if persistence.load_error is not None:
            self.error = persistence.load_error
            return False
        snapshot = capture_pair_snapshot(
            self._app.project, source_id, target_id, operation=operation
        )
        owner = uuid4().hex
        revision, captured_project, digest = persistence.capture_migration(owner)
        try:
            retirement = self._app._assets.reserve(64)
        except OSError, RuntimeError, TypeError, ValueError:
            persistence.release_migration(owner)
            raise
        try:
            initial = PairArrangementJournal(
                transaction_id=owner,
                config_reference=persistence.config_reference,
                captured_revision=revision,
                intent_revision=revision,
                captured_config_sha256=digest,
                before=snapshot,
                after=PairArrangementAfterImage.from_before(snapshot),
                project_image_json=captured_project.model_dump_json(),
                image_role="before",
                phase="captured",
            )
        except OSError, RuntimeError, TypeError, ValueError:
            retirement.close()
            persistence.release_migration(owner)
            raise
        self._pair = _Pair(owner, snapshot, retirement, initial, source_binding=source_binding)
        self._set_observer_fence(self.fenced_sample_ids)
        self.error = None
        self.status = "captured"
        self.poll()
        return self.status != "cancelled"

    def _set_observer_fence(self, sample_ids: frozenset[int]) -> None:
        for name in ("transport", "loader", "accepted_timing", "stems"):
            component = getattr(self._app, name, None)
            if component is not None:
                component._pair_deferred_sample_ids = sample_ids

    def _require_thread(self) -> None:
        if threading.get_ident() != self._thread:
            message = "Pair settlement must run on its original Controller thread"
            raise RuntimeError(message)

    def poll(self) -> None:
        """Advance bounded work without replacing a failed or partial owning plan."""
        self._require_thread()
        self._collect()
        pair = self._pair
        if pair is None:
            handoff = self._startup.poll()
            self._set_observer_fence(self.observer_fenced_sample_ids)
            if handoff is None:
                if self._startup.active:
                    self.status, self.error = self._startup.status, self._startup.error
                return
            pair = _Pair(
                handoff.owner,
                handoff.snapshot,
                handoff.retirement,
                handoff.initial,
                journal=handoff.journal,
                recovery_handoff=handoff,
            )
            self._pair = pair
            self._set_observer_fence(self.observer_fenced_sample_ids)
        try:
            self._advance(pair)
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self.error = str(error)
            self.status = "settlement_pending"

    def _collect(self) -> None:
        while True:
            try:
                result = self._results.get_nowait()
            except Empty:
                return
            pair = self._pair
            if pair is None or result.owner != pair.owner:
                message = "Pair preparation returned without its original owner"
                raise RuntimeError(message)
            pair.worker_done = True
            pair.ticket = result.ticket
            if result.error is not None:
                self.error = result.error
                pair.cancel_requested = True

    def _before_record(self, pair: _Pair, phase: str) -> PairArrangementJournal:
        return pair.initial.changed(
            phase=phase,
            intent_revision=self._app.persistence.revision,
            project_image_json=self._app.project.model_dump_json(),
            error=None,
        )

    def _advance(self, pair: _Pair) -> None:
        journal = self._ensure_journal(pair)
        if journal.journal is not None and journal.journal.phase in {"settled", "cancelled"}:
            self._settle(pair, "completed" if journal.journal.phase == "settled" else "cancelled")
            return
        if pair.cancel_requested and pair.worker_done and pair.acknowledgement is None:
            self._cancel(pair)
            if self._pair is None:
                return
        self._start_preparation(pair)
        if not pair.worker_done or pair.ticket is None:
            return
        if pair.ticket.publication_status() == "rejected":
            pair.cancel_requested = True
            self._cancel(pair)
            return
        if not pair.published:
            journal.append(self._before_record(pair, "publication_pending"))
            self._app._audio._publish_pad_pair(pair.ticket)
            pair.published = True
            self.status = "publication_pending"
        if not self._observe_acknowledgement(pair):
            return
        if journal.journal is not None and journal.journal.phase == "publication_pending":
            journal.append(self._before_record(pair, "native_ack_observed"))
        self._commit(pair)

    def _ensure_journal(self, pair: _Pair) -> PairJournalOwner:
        if pair.journal is None:
            pair.journal = PairJournalOwner(Path.cwd() / "samples", pair.initial)
        journal = pair.journal
        if journal.intended is not None:
            journal.append(
                PairArrangementJournal.model_validate_json(journal.intended, strict=True)
            )
        if journal.journal is None:
            journal.append(pair.initial)
        return journal

    def _start_preparation(self, pair: _Pair) -> None:
        app = self._app
        if pair.captured is None:
            data = pair.snapshot.projected()
            try:
                pair.captured = (
                    app._audio._capture_pad_pair(
                        data.source_id, data.target_id, source_binding=pair.source_binding
                    )
                    if pair.source_binding is not None
                    else app._audio._capture_pad_pair(data.source_id, data.target_id)
                )
            except ValueError:
                if pair.source_binding is None:
                    raise
                pair.cancel_requested = True
                self._cancel(pair)
                return
            captured, results, audio, owner = pair.captured, self._results, app._audio, pair.owner
            pair.worker_done = False

            def prepare() -> None:
                try:
                    result = _Prepared(owner, audio._prepare_pad_pair(captured), None)
                except (OSError, RuntimeError, TypeError, ValueError) as error:
                    result = _Prepared(owner, None, str(error))
                results.put(result)

            try:
                app.stems._stem_task_runner(prepare)
            except OSError, RuntimeError, TypeError, ValueError:
                pair.worker_done = True
                pair.cancel_requested = True
                raise

    def _observe_acknowledgement(self, pair: _Pair) -> bool:
        if pair.acknowledgement is None:
            assert pair.ticket is not None
            if not self._app._audio._reconcile_pad_pair(pair.ticket):
                return False
            pair.acknowledgement = pair.ticket.own_acknowledgement()
            if pair.acknowledgement is None:
                message = "Reconciled Pair has no original Native acknowledgement"
                raise RuntimeError(message)
        return True

    def _prepare_mirrors(self, pair: _Pair) -> None:
        app, ticket = self._app, pair.ticket
        if ticket is None or pair.captured is None:
            message = "Pair mirrors lost original Native custody"
            raise RuntimeError(message)

        def validate(ack: object) -> bool:
            return app._audio._validate_pad_pair_acknowledgement(
                ticket, cast("PadPairAcknowledgement", ack)
            )

        if pair.mirror is None:
            pair.mirror = prepare_pair_control_mirror(
                app,
                pair.snapshot,
                validate_acknowledgement=validate,
                waveform_views=tuple(app._pair_waveform_views),
            )
        if pair.stems is None:
            data = pair.snapshot.projected()
            assert data.source.source_identity is not None
            assert data.source.source_path is not None
            pair.stems = prepare_pair_stem_mirror(
                app.stems,
                data.source_id,
                data.target_id,
                source_content_id=data.source.source_identity.instance_id,
                target_content_id=data.target.source_identity.instance_id
                if data.target.source_identity
                else None,
                source_path=data.source.source_path,
                target_path=data.target.source_path,
                source_signature=pair.captured,
                target_signature=pair.captured,
                validate_acknowledgement=validate,
            )

    def _candidate(self, pair: _Pair) -> ProjectState:
        if pair.mirror is None or pair.stems is None:
            message = "Pair candidate requires its original prepared mirrors"
            raise RuntimeError(message)
        candidate = pair.mirror.projected_project()
        for sample_id, entry in pair.stems.projected_entries:
            candidate.stem_cache[sample_id] = entry
        return candidate

    def _commit(self, pair: _Pair) -> None:
        self._prepare_mirrors(pair)
        app, journal = self._app, pair.journal
        assert journal is not None
        assert pair.mirror is not None
        assert pair.stems is not None
        if (
            pair.assignment_publication is not None
            and pair.assignment_publication.applied
            and not pair.assignment_publication.settled
        ):
            with pair.retirement.activate():
                pair.assignment_publication.apply()
        if pair.receipt is None or pair.receipt.revision != app.persistence.revision:
            revision = app.persistence.revision
            self._prepare_assignment(pair, revision)
            assert pair.candidate is not None
            pair.receipt = app.persistence.commit_migration_receipt(
                pair.owner, revision, pair.candidate
            )
            pair.assets_published = False
        journal.append(pair.initial.committed(pair.receipt))
        if app.persistence.revision != pair.receipt.revision:
            return
        pair.mirror.apply(pair.acknowledgement)
        pair.stems.apply(pair.acknowledgement)
        if not pair.assets_published:
            assert pair.assignment is not None
            assert pair.assignment_publication is not None
            try:
                with pair.retirement.activate():
                    pair.assignment_publication.apply()
            finally:
                if not pair.assignment_publication.owns_unpublished_image:
                    pair.images[:] = [
                        image for image in pair.images if image is not pair.assignment
                    ]
            pair.assets_published = True
        self._release_images(pair)
        if app.persistence.revision != pair.receipt.revision:
            return
        if not pair.native_finished:
            assert pair.ticket is not None
            assert pair.acknowledgement is not None
            app._audio._finish_pad_pair(pair.ticket, pair.acknowledgement)
            pair.native_finished = True
        journal.append(pair.initial.committed(pair.receipt).changed(phase="settled"))
        self._settle(pair, "completed")

    def _prepare_assignment(self, pair: _Pair, revision: int) -> None:
        if pair.candidate is not None and pair.candidate_revision == revision:
            return
        # Keep the exact failed candidate until a newer intent prepares its successor.
        assets = self._app._assets
        candidate = self._candidate(pair)
        image = assets.prepare_assignment_image(candidate)
        try:
            publication = assets.prepare_assignment_publication(image)
        except OSError, RuntimeError, TypeError, ValueError:
            assets.release_assignment_image(image)
            raise
        previous, previous_publication = pair.assignment, pair.assignment_publication
        pair.images.append(image)
        pair.assignment = image
        pair.assignment_publication = publication
        pair.candidate = candidate
        pair.candidate_revision = revision
        if (
            previous is not None
            and previous_publication is not None
            and previous_publication.owns_unpublished_image
        ):
            assets.release_assignment_image(previous)
            pair.images[:] = [held for held in pair.images if held is not previous]

    def _cancel(self, pair: _Pair) -> None:
        if pair.acknowledgement is not None or pair.receipt is not None:
            return
        if not pair.cancelled:
            if pair.captured is not None and not self._app._audio._cancel_pad_pair(pair.captured):
                return
            pair.cancelled = True
        assert pair.journal is not None
        pair.journal.append(self._before_record(pair, "cancelled"))
        self._release_images(pair)
        self._settle(pair, "cancelled")

    def _release_images(self, pair: _Pair) -> None:
        while pair.images:
            self._app._assets.release_assignment_image(pair.images[-1])
            pair.images.pop()

    def _settle(self, pair: _Pair, status: str) -> None:
        if pair.recovery_handoff is not None:
            self._startup.complete_original(pair.recovery_handoff)
        if not pair.writer_released:
            self._app.persistence.release_migration(pair.owner)
            pair.writer_released = True
        pair.retirement.close()
        self._pair = None
        self._set_observer_fence(frozenset())
        self.status = status
        self.error = None

    def prepare_shutdown(self) -> bool:
        """Request preclaim cancellation or retain the actual claimed settlement."""
        self._closing = True
        if self._pair is not None and self._pair.acknowledgement is None:
            self._pair.cancel_requested = True
        self.poll()
        return not self.active
