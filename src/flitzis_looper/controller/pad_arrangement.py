"""Control-thread orchestration of a complete stopped Native Copy."""

import json
import threading
from dataclasses import dataclass, field
from pathlib import Path
from queue import Empty, SimpleQueue
from typing import TYPE_CHECKING, Protocol
from uuid import uuid4

from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.controller.copy_arrangement_custody import (
    CopyJournalCustody,
    journal_project_images,
)
from flitzis_looper.controller.copy_arrangement_journal import (
    CopyArrangementJournal,
    CopyJournalOwner,
    copy_after_snapshot,
)
from flitzis_looper.controller.copy_arrangement_recovery import copy_before_project
from flitzis_looper.controller.copy_arrangement_startup import CopyArrangementStartup
from flitzis_looper.copy_snapshot import capture_copy_snapshot, project_with_copy_snapshot
from flitzis_looper.models import PadContentIdentity

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller.app import AppController
    from flitzis_looper.controller.asset_lifecycle import (
        AssetRetirementReservation,
        AssignmentPublicationPlan,
        ProjectAssetLifecycle,
    )
    from flitzis_looper.controller.copy_arrangement_startup import OriginalCopyRecoveryHandoff
    from flitzis_looper.controller.persistence import ConfigCommitReceipt, ProjectPersistence
    from flitzis_looper.controller.stems import StemTaskRunner
    from flitzis_looper.copy_snapshot import CopySnapshot
    from flitzis_looper.models import ProjectState, SessionState
    from flitzis_looper_audio import (
        CapturedPadCopy,
        InputRuntimePadBinding,
        PadCopyTicket,
        ProjectAssetLease,
    )

type AssignmentImage = tuple[
    dict[tuple[str, int], Path], dict[tuple[str, int], tuple[Path, ProjectAssetLease]]
]

_COPY_FIELDS = (
    "sample_paths",
    "pad_content",
    "sample_durations",
    "sample_analysis",
    "stem_cache",
    "pad_stem_mix_mode",
    "pad_stem_intent",
    "pad_key_lock",
    "manual_bpm",
    "pad_timing_intent",
    "pad_key_intent",
    "pad_source_beatmap_intent",
    "pad_gain_db",
    "pad_eq_low_db",
    "pad_eq_mid_db",
    "pad_eq_high_db",
    "pad_loop_start_s",
    "pad_loop_end_s",
    "pad_loop_auto",
    "pad_loop_bars",
    "pad_grid_offset_samples",
    "pad_grid_anchor_s",
)


class NativeAdapterProtocol(Protocol):
    """Actual engine Copy operations; enqueue alone is never completion."""

    def current_input_runtime_pad_binding(
        self, sample_id: int
    ) -> InputRuntimePadBinding | None: ...

    def capture_pad_copy(
        self,
        source_binding: InputRuntimePadBinding,
        target_id: int,
        intent_json: str,
        *,
        target_binding: InputRuntimePadBinding | None = None,
    ) -> CapturedPadCopy: ...

    def prepare_pad_copy(self, captured: CapturedPadCopy) -> PadCopyTicket: ...

    def publish_pad_copy(self, ticket: PadCopyTicket) -> None: ...

    def reconcile_pad_copy(self, ticket: PadCopyTicket) -> bool: ...

    def finish_pad_copy(self, ticket: PadCopyTicket) -> None: ...

    def cancel_pad_copy(self, captured: CapturedPadCopy) -> bool: ...


@dataclass(frozen=True, slots=True)
class _PreparedCopy:
    owner: str
    ticket: PadCopyTicket | None
    error: str | None


@dataclass(slots=True)
class _Copy:
    owner: str
    source_id: int
    target_id: int
    snapshot: CopySnapshot
    identity: PadContentIdentity
    retirement: AssetRetirementReservation
    resolved_key_lock: bool
    predecessor: CopySnapshot | None = None
    initial_journal: CopyArrangementJournal | None = None
    journal: CopyJournalOwner | None = None
    journal_custody: CopyJournalCustody = field(default_factory=CopyJournalCustody)
    receipt: ConfigCommitReceipt | None = None
    recovery_handoff: OriginalCopyRecoveryHandoff | None = None
    source_binding: InputRuntimePadBinding | None = None
    target_binding: InputRuntimePadBinding | None = None
    predecessor_retired: bool = False
    captured: CapturedPadCopy | None = None
    ticket: PadCopyTicket | None = None
    images: list[AssignmentImage] = field(default_factory=list)
    initial_image: AssignmentImage | None = None
    committed_image: AssignmentImage | None = None
    committed_revision: int | None = None
    candidate: ProjectState | None = None
    candidate_revision: int | None = None
    assignment_publication: AssignmentPublicationPlan | None = None
    worker_done: bool = True
    cancel_requested: bool = False
    published: bool = False
    acknowledged: bool = False
    config_committed: bool = False
    model_committed: bool = False
    assets_published: bool = False
    callback_done: bool = False
    native_finished: bool = False
    cancellation_proven: bool = False
    writer_released: bool = False
    failure: str | None = None


def _prepare_copy(
    audio: NativeAdapterProtocol,
    captured: CapturedPadCopy,
    owner: str,
    results: SimpleQueue[_PreparedCopy],
) -> None:
    """Return immutable worker output; the worker never touches controller models."""
    try:
        result = _PreparedCopy(owner, audio.prepare_pad_copy(captured), None)
    except (OSError, RuntimeError, TypeError, ValueError) as error:
        result = _PreparedCopy(owner, None, str(error))
    results.put(result)


def _valid_pad(sample_id: int) -> bool:
    return type(sample_id) is int and 0 <= sample_id < NUM_SAMPLES


def _empty_target(project: ProjectState, session: SessionState, sample_id: int) -> bool:
    return (
        project.sample_paths[sample_id] is None
        and project.pad_content[sample_id] is None
        and sample_id not in session.loading_sample_ids
        and sample_id not in session.pending_sample_paths
        and sample_id not in session.active_sample_ids
        and sample_id not in session.paused_sample_ids
        and sample_id not in session.analyzing_sample_ids
        and sample_id not in session.stem_generating_sample_ids
    )


def _occupied_target(project: ProjectState, session: SessionState, sample_id: int) -> bool:
    return (
        project.sample_paths[sample_id] is not None
        and project.pad_content[sample_id] is not None
        and sample_id not in session.loading_sample_ids
        and sample_id not in session.pending_sample_paths
    )


def _resolved_key_lock(project: ProjectState, copy: _Copy) -> bool:
    override = copy.snapshot.projected().key_lock
    return project.key_lock if override is None else override


def _candidate(project: ProjectState, copy: _Copy) -> ProjectState:
    _check_target(project, copy)
    target = project.pad_content[copy.target_id]
    base = project
    if target is not None and target.instance_id == copy.identity.instance_id:
        # A retry may already have this transaction's durable target projection.
        base = project.model_copy(deep=True)
        base.pad_content[copy.target_id] = None
        base.sample_paths[copy.target_id] = None
    return project_with_copy_snapshot(
        base, copy.target_id, copy.snapshot, target_identity=copy.identity
    )


def _check_target(project: ProjectState, copy: _Copy) -> None:
    target = project.pad_content[copy.target_id]
    path = project.sample_paths[copy.target_id]
    empty = (
        copy.predecessor is None and target is None and path is None and not copy.model_committed
    )
    own = (
        target is not None
        and target.instance_id == copy.identity.instance_id
        and path == copy.snapshot.projected().source_path
    )
    original = (
        copy.predecessor is not None
        and target is not None
        and path is not None
        and not copy.model_committed
        and capture_copy_snapshot(project, copy.target_id) == copy.predecessor
    )
    if not empty and not own and not original:
        message = "Copy target changed while its original assignment was reserved"
        raise RuntimeError(message)
    if _resolved_key_lock(project, copy) != copy.resolved_key_lock:
        message = "Copy inherited Key Lock changed before complete publication"
        raise RuntimeError(message)


def _publish_project(project: ProjectState, candidate: ProjectState, target_id: int) -> None:
    for name in _COPY_FIELDS:
        getattr(project, name)[target_id] = getattr(candidate, name)[target_id]


def _release_images(assets: ProjectAssetLifecycle, copy: _Copy) -> None:
    while copy.images:
        image = copy.images[-1]
        assets.release_assignment_image(image)
        copy.images.pop()


def _discard_obsolete_images(assets: ProjectAssetLifecycle, copy: _Copy) -> None:
    for image in tuple(copy.images):
        if image is copy.initial_image or image is copy.committed_image:
            continue
        assets.release_assignment_image(image)
        copy.images[:] = [held for held in copy.images if held is not image]


class PadArrangementController:
    """Own one stopped Copy until Native, durable config and custody agree."""

    def __init__(
        self,
        persistence: ProjectPersistence,
        session: SessionState,
        audio: NativeAdapterProtocol,
        assets: ProjectAssetLifecycle,
        *,
        stem_task_runner: StemTaskRunner,
        on_copy_committed: Callable[[int, PadCopyTicket], None] | None = None,
        on_target_removed: Callable[[int, CopySnapshot], None] | None = None,
        can_admit: Callable[[], bool] | None = None,
    ) -> None:
        self._persistence = persistence
        self._project = persistence.project
        self._session = session
        self._audio = audio
        self._assets = assets
        self._runner = stem_task_runner
        self._on_copy_committed = on_copy_committed
        self._on_target_removed = on_target_removed
        self._can_admit = can_admit or (lambda: True)
        self._results: SimpleQueue[_PreparedCopy] = SimpleQueue()
        self._copy: _Copy | None = None
        self._control_thread = threading.get_ident()
        self._closing = False
        self._status = "idle"
        self.error: str | None = None
        self._startup: CopyArrangementStartup | None = None

    def bind_startup(self, app: AppController) -> None:
        """Inspect and pin durable evidence before ordinary Loader restore/cleanup."""
        self._startup = CopyArrangementStartup(app)

    @property
    def active(self) -> bool:
        """Whether this transaction still owns reservations and the writer fence."""
        return self._copy is not None or (self._startup is not None and self._startup.active)

    @property
    def pending(self) -> bool:
        """Expose pending ownership, including claimed uncertainty and commit retries."""
        return self.active

    @property
    def status(self) -> str:
        """Current control phase or the most recent settled outcome."""
        if self._copy is None and self._startup is not None and self._startup.active:
            return self._startup.status
        return self._status

    @property
    def target_id(self) -> int | None:
        """The reserved target, until actual settlement releases it."""
        return self._copy.target_id if self._copy is not None else None

    @property
    def fenced_sample_ids(self) -> frozenset[int]:
        """Targets whose assignment and musical edits must wait for settlement."""
        if self._copy is not None:
            return frozenset({self._copy.target_id})
        return frozenset() if self._startup is None else self._startup.fenced_sample_ids

    @property
    def observer_fenced_sample_ids(self) -> frozenset[int]:
        """Keep fresh restore observers reachable before startup owns actual C."""
        if self._copy is not None:
            return self.fenced_sample_ids
        return frozenset() if self._startup is None else self._startup.observer_fenced_sample_ids

    def ensure_global_key_lock_edit_allowed(self) -> None:
        """Fence inheritance changes while a captured Copy uses the global default."""
        if self._startup is not None:
            self._startup.ensure_global_key_lock_edit_allowed()
        if self._copy is not None and self._copy.snapshot.projected().key_lock is None:
            message = "Key Lock inheritance is reserved by a pending Copy"
            raise RuntimeError(message)

    def copy_to_empty(
        self,
        source_id: int,
        target_id: int,
        *,
        source_binding: InputRuntimePadBinding | None = None,
    ) -> bool:
        """Copy to an empty target using the original source binding when supplied."""
        self._require_control_thread()
        if self._closing or not self._can_admit():
            return False
        if (
            self.active
            or not all(_valid_pad(value) for value in (source_id, target_id))
            or source_id == target_id
            or self._project.sample_paths[source_id] is None
            or not _empty_target(self._project, self._session, target_id)
        ):
            return False
        try:
            self._ensure_config_writable()
            binding = (
                source_binding
                if source_binding is not None
                else self._audio.current_input_runtime_pad_binding(source_id)
            )
            if (
                binding is None
                or (source_binding is not None and binding.metadata().get("pad_id") != source_id)
                or self._audio.current_input_runtime_pad_binding(target_id) is not None
            ):
                return False
            return self._begin(source_id, target_id, binding)
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self.error = str(error)
            self._session.sample_load_errors[target_id] = f"Copy failed: {error}"
            return False

    def copy_overwrite(
        self,
        source_id: int,
        target_id: int,
        *,
        source_binding: InputRuntimePadBinding | None = None,
    ) -> bool:
        """Copy the optional original source over the target captured at this admission."""
        self._require_control_thread()
        if self._closing or not self._can_admit():
            return False
        if (
            self.active
            or not all(_valid_pad(value) for value in (source_id, target_id))
            or source_id == target_id
            or self._project.sample_paths[source_id] is None
            or not _occupied_target(self._project, self._session, target_id)
        ):
            return False
        try:
            self._ensure_config_writable()
            source = (
                source_binding
                if source_binding is not None
                else self._audio.current_input_runtime_pad_binding(source_id)
            )
            target = self._audio.current_input_runtime_pad_binding(target_id)
            if (
                source is None
                or target is None
                or (source_binding is not None and source.metadata().get("pad_id") != source_id)
            ):
                return False
            return self._begin(source_id, target_id, source, target_binding=target)
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self.error = str(error)
            self._session.sample_load_errors[target_id] = f"Copy failed: {error}"
            return False

    def _ensure_config_writable(self) -> None:
        if self._persistence.load_error is not None:
            message = f"Copy config is fenced: {self._persistence.load_error}"
            raise RuntimeError(message)

    def _begin(
        self,
        source_id: int,
        target_id: int,
        binding: InputRuntimePadBinding,
        *,
        target_binding: InputRuntimePadBinding | None = None,
    ) -> bool:
        snapshot = capture_copy_snapshot(self._project, source_id)
        predecessor = (
            capture_copy_snapshot(self._project, target_id) if target_binding is not None else None
        )
        source = snapshot.projected().source_identity
        identity = PadContentIdentity(
            instance_id=uuid4().hex,
            material_id=source.material_id,
            material_creation=source.material_creation,
            copied_from=source.instance_id,
        )
        owner = uuid4().hex
        revision, project_image, digest = self._persistence.capture_migration(owner)
        try:
            retirement = self._assets.reserve()
        except OSError, RuntimeError, TypeError, ValueError:
            self._persistence.release_migration(owner)
            raise
        copy = _Copy(
            owner,
            source_id,
            target_id,
            snapshot,
            identity,
            retirement,
            resolved_key_lock=False,
            predecessor=predecessor,
            source_binding=binding,
            target_binding=target_binding,
        )
        copy.resolved_key_lock = _resolved_key_lock(self._project, copy)
        if predecessor is not None:
            try:
                copy.initial_journal = CopyArrangementJournal(
                    transaction_id=owner,
                    config_reference=self._persistence.config_reference,
                    captured_revision=revision,
                    intent_revision=revision,
                    captured_config_sha256=digest,
                    source_id=source_id,
                    target_id=target_id,
                    source=snapshot,
                    predecessor=predecessor,
                    after=copy_after_snapshot(snapshot, identity),
                    resolved_key_lock=copy.resolved_key_lock,
                    captured_project_image_json=project_image.model_dump_json(),
                    project_image_json=project_image.model_dump_json(),
                    image_role="before",
                    phase="captured",
                )
            except OSError, RuntimeError, TypeError, ValueError:
                retirement.close()
                self._persistence.release_migration(owner)
                raise
        self._copy = copy
        self.error = None
        self._session.sample_load_errors.pop(target_id, None)
        self._session.loading_sample_ids.add(target_id)
        self._session.sample_load_stage[target_id] = "Copy preparing"
        self._status = "preparing"
        try:
            self._ensure_journal(copy)
            self._start_preparation(copy)
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            # The bounded runner raises before admitting a worker. Journal uncertainty
            # still retains its exact intended bytes, readers and Config owner.
            copy.cancel_requested = True
            self._report(copy, str(error))
            self._cancel_unclaimed(copy)
            return False
        return True

    def _ensure_journal(self, copy: _Copy) -> CopyJournalOwner | None:
        initial = copy.initial_journal
        if initial is None:
            intent = copy.snapshot.projected().source_beatmap_intent
            if intent is not None and intent.selected is not None:
                copy.journal_custody.acquire(self._assets, Path.cwd(), (copy.snapshot,))
            return None
        if (
            copy.journal is not None
            and copy.journal.journal is not None
            and (
                copy.journal.journal.phase in {"settled", "cancelled"}
                and copy.journal.intended is None
            )
        ):
            return copy.journal
        copy.journal_custody.acquire(
            self._assets,
            Path.cwd(),
            (initial.source, initial.predecessor, initial.after),
            project_images=(
                initial.captured_project_image_json,
                initial.project_image_json,
                *journal_project_images(copy.journal.encoded if copy.journal else ()),
            ),
        )
        if copy.journal is None:
            copy.journal = CopyJournalOwner(Path.cwd() / "samples", initial)
        journal = copy.journal
        if journal.intended is not None:
            journal.append(
                CopyArrangementJournal.model_validate_json(journal.intended, strict=True)
            )
        if journal.journal is None:
            journal.append(initial)
        return journal

    def _start_preparation(self, copy: _Copy) -> None:
        if copy.captured is not None:
            return
        binding, target_binding = copy.source_binding, copy.target_binding
        if binding is None:
            message = "Copy lost its original source binding before preparation"
            raise RuntimeError(message)
        envelope = json.dumps(
            {
                "schema_version": 1,
                "snapshot": json.loads(copy.snapshot.payload_json),
                "resolved_key_lock": copy.resolved_key_lock,
            },
            sort_keys=True,
            separators=(",", ":"),
            allow_nan=False,
        )
        copy.captured = (
            self._audio.capture_pad_copy(
                binding, copy.target_id, envelope, target_binding=target_binding
            )
            if target_binding is not None
            else self._audio.capture_pad_copy(binding, copy.target_id, envelope)
        )
        captured, audio, results, owner = copy.captured, self._audio, self._results, copy.owner
        copy.worker_done = False
        try:
            self._runner(lambda: _prepare_copy(audio, captured, owner, results))
        except OSError, RuntimeError, TypeError, ValueError:
            copy.worker_done = True
            copy.cancel_requested = True
            raise

    def poll(self) -> None:
        """Collect worker completion and reconcile only genuine complete-target ACKs."""
        self._require_control_thread()
        self._collect()
        if self._copy is None and self._startup is not None and self._startup.active:
            handoff = self._startup.poll()
            self.error = self._startup.error
            if handoff is not None:
                self._adopt_handoff(handoff)
        copy = self._copy
        if copy is None:
            return
        try:
            self._advance(copy)
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self._report(copy, str(error))
            if not copy.acknowledged and not copy.config_committed:
                copy.cancel_requested = True
                self._cancel_unclaimed(copy)

    def _adopt_handoff(self, handoff: OriginalCopyRecoveryHandoff) -> None:
        initial = handoff.initial
        self._copy = _Copy(
            initial.transaction_id,
            initial.source_id,
            initial.target_id,
            initial.source,
            initial.target_identity,
            handoff.retirement,
            initial.resolved_key_lock,
            predecessor=initial.predecessor,
            initial_journal=initial,
            journal=handoff.journal,
            journal_custody=handoff.custody,
            recovery_handoff=handoff,
        )
        copy = self._copy
        copy.source_binding = handoff.source_binding
        copy.target_binding = handoff.target_binding
        self._session.loading_sample_ids.add(initial.target_id)
        self._status = "preparing"

    def cancel(self) -> bool:
        """Cancel only after Native proves no claim and the worker has returned."""
        self._require_control_thread()
        copy = self._copy
        if copy is None:
            return False
        copy.cancel_requested = True
        self._status = "cancel_pending"
        try:
            return self._cancel_unclaimed(copy)
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self._report(copy, str(error))
            return False

    def prepare_shutdown(self) -> bool:
        """Close admission and advance settlement without waiting or releasing live workers."""
        self._require_control_thread()
        self._closing = True
        copy = self._copy
        if copy is None:
            self.poll()
            return not self.active
        copy.cancel_requested = True
        self._status = "cancel_pending"
        try:
            # Native cancellation may prevent a queued worker from preparing anything.
            # Its readers and our custody remain owned until its actual result arrives.
            self._prove_cancellation(copy)
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self._report(copy, str(error))
        try:
            self.poll()
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self._report(copy, str(error))
        return not self.active

    def _require_control_thread(self) -> None:
        if threading.get_ident() != self._control_thread:
            message = "Copy controller mutation must run on its control thread"
            raise RuntimeError(message)

    def _report(self, copy: _Copy, message: str) -> None:
        self.error = message
        copy.failure = message
        self._session.sample_load_errors[copy.target_id] = f"Copy pending/error: {message}"

    def _collect(self) -> None:
        while True:
            try:
                result = self._results.get_nowait()
            except Empty:
                return
            copy = self._copy
            if copy is None or result.owner != copy.owner:
                message = "Copy worker returned without its retained transaction owner"
                raise RuntimeError(message)
            copy.worker_done = True
            copy.ticket = result.ticket
            if result.error is not None and not copy.cancellation_proven:
                copy.cancel_requested = True
                self._report(copy, result.error)

    def _advance(self, copy: _Copy) -> None:
        journal = self._ensure_journal(copy)
        if copy.cancel_requested and self._cancel_unclaimed(copy):
            return
        if not copy.cancel_requested:
            self._start_preparation(copy)
        if not copy.worker_done or copy.ticket is None:
            return
        phase = copy.ticket.publication_status()
        if phase == "rejected":
            self._reject(copy)
            return
        if phase not in {"captured", "preparing", "pending", "adopting", "accepted"}:
            message = f"Native Copy returned an unsupported publication status: {phase}"
            raise RuntimeError(message)
        if not copy.published and phase in {"captured", "pending"} and not copy.cancel_requested:
            self._publish(copy)
        if not copy.published and phase in {"captured", "preparing", "pending"}:
            return
        if not self._observe_ack(copy, phase):
            return
        if (
            journal is not None
            and journal.journal is not None
            and journal.journal.phase == "publication_pending"
        ):
            journal.append(self._before_record(copy, "native_ack_observed"))
        self._retire_predecessor(copy)
        self._status = "commit_pending"
        self._commit(copy)

    def _reject(self, copy: _Copy) -> None:
        if copy.acknowledged or copy.config_committed:
            message = "Previously acknowledged Native Copy was rejected; owners remain fenced"
            raise RuntimeError(message)
        self._report(copy, "Native Copy was rejected without target adoption")
        copy.cancel_requested = True
        self._cancel_unclaimed(copy)

    def _observe_ack(self, copy: _Copy, phase: str) -> bool:
        if not copy.acknowledged:
            assert copy.ticket is not None
            if not self._audio.reconcile_pad_copy(copy.ticket):
                self._status = "adopting" if phase in {"adopting", "accepted"} else "pending"
                return False
            copy.acknowledged = True
        return True

    def _retire_predecessor(self, copy: _Copy) -> None:
        if copy.predecessor is None or copy.predecessor_retired:
            return
        if self._on_target_removed is not None:
            self._on_target_removed(copy.target_id, copy.predecessor)
        copy.predecessor_retired = True

    def _publish(self, copy: _Copy) -> None:
        if copy.ticket is None:
            message = "Copy publication has no prepared Native ticket"
            raise RuntimeError(message)
        candidate = _candidate(self._project, copy)
        image = self._assets.prepare_assignment_image(candidate)
        copy.images.append(image)
        copy.initial_image = image
        if copy.journal is not None:
            copy.journal.append(self._before_record(copy, "publication_pending"))
        self._audio.publish_pad_copy(copy.ticket)
        copy.published = True
        self._status = "pending"
        self._session.sample_load_stage[copy.target_id] = "Copy awaiting complete Native ACK"

    def _commit(self, copy: _Copy) -> None:
        _check_target(self._project, copy)
        if copy.predecessor is not None:
            self._commit_occupied(copy)
            return
        if (
            copy.config_committed
            and self._persistence.revision != copy.committed_revision
            and not copy.native_finished
        ):
            # Preserve newer unrelated intent even during postcommit publication retries.
            copy.config_committed = False
            copy.assets_published = False
        if not copy.config_committed:
            _discard_obsolete_images(self._assets, copy)
            candidate = _candidate(self._project, copy)
            image = self._assets.prepare_assignment_image(candidate)
            copy.images.append(image)
            copy.committed_image = image
            copy.candidate = candidate
            _discard_obsolete_images(self._assets, copy)
            revision = self._persistence.revision
            self._persistence.commit_migration(copy.owner, revision, candidate)
            if self._persistence.revision != revision:
                message = "Newer project intent arrived during the atomic Copy config write"
                raise RuntimeError(message)
            copy.committed_revision = revision
            copy.config_committed = True
        if copy.candidate is None or copy.committed_image is None:
            message = "Durable Copy image disappeared before model and custody publication"
            raise RuntimeError(message)
        if not copy.model_committed:
            _publish_project(self._project, copy.candidate, copy.target_id)
            copy.model_committed = True
        if not copy.assets_published:
            with copy.retirement.activate():
                self._assets.publish_assignment_image(copy.committed_image)
            copy.images[:] = [held for held in copy.images if held is not copy.committed_image]
            copy.assets_published = True
        _release_images(self._assets, copy)
        self._finish(copy)

    def _prepare_occupied_assignment(self, copy: _Copy, revision: int) -> None:
        if copy.candidate is not None and copy.candidate_revision == revision:
            return
        candidate = _candidate(self._project, copy)
        copy.journal_custody.acquire(
            self._assets, Path.cwd(), (), project_images=(candidate.model_dump_json(),)
        )
        image = self._assets.prepare_assignment_image(candidate)
        try:
            publication = self._assets.prepare_assignment_publication(image)
        except OSError, RuntimeError, TypeError, ValueError:
            self._assets.release_assignment_image(image)
            raise
        previous, previous_plan = copy.committed_image, copy.assignment_publication
        copy.images.append(image)
        copy.committed_image = image
        copy.assignment_publication = publication
        copy.candidate = candidate
        copy.candidate_revision = revision
        if (
            previous is not None
            and previous_plan is not None
            and previous_plan.owns_unpublished_image
        ):
            self._assets.release_assignment_image(previous)
            copy.images[:] = [held for held in copy.images if held is not previous]

    def _apply_occupied_assignment(self, copy: _Copy) -> None:
        assert copy.assignment_publication is not None
        try:
            with copy.retirement.activate():
                copy.assignment_publication.apply()
        finally:
            if not copy.assignment_publication.owns_unpublished_image:
                copy.images[:] = [held for held in copy.images if held is not copy.committed_image]

    def _commit_occupied(self, copy: _Copy) -> None:
        if (
            copy.assignment_publication is not None
            and copy.assignment_publication.applied
            and not copy.assignment_publication.settled
        ):
            self._apply_occupied_assignment(copy)
        revision = self._persistence.revision
        if not copy.config_committed or copy.committed_revision != revision:
            self._prepare_occupied_assignment(copy, revision)
            assert copy.candidate is not None
            assert copy.journal is not None
            assert copy.initial_journal is not None
            if copy.receipt is None or copy.receipt.revision != revision:
                copy.journal.append(self._before_record(copy, "native_ack_observed"))
                copy.receipt = self._persistence.commit_migration_receipt(
                    copy.owner, revision, copy.candidate
                )
            copy.journal.append(copy.initial_journal.committed(copy.receipt))
            if self._persistence.revision != revision:
                message = (
                    "Newer project intent arrived during the atomic occupied Copy config write"
                )
                raise RuntimeError(message)
            copy.committed_revision = revision
            copy.config_committed = True
            copy.assets_published = False
        assert copy.candidate is not None
        if not copy.model_committed:
            _publish_project(self._project, copy.candidate, copy.target_id)
            copy.model_committed = True
        if not copy.assets_published:
            self._apply_occupied_assignment(copy)
            copy.assets_published = True
        _release_images(self._assets, copy)
        self._finish(copy)

    def _finish(self, copy: _Copy) -> None:
        if copy.ticket is None:
            message = "Durable Copy lost its complete Native acknowledgement ticket"
            raise RuntimeError(message)
        self._status = "finishing"
        if not copy.callback_done:
            if self._on_copy_committed is not None:
                self._on_copy_committed(copy.target_id, copy.ticket)
            copy.callback_done = True
        if not copy.native_finished:
            self._audio.finish_pad_copy(copy.ticket)
            copy.native_finished = True
        if copy.journal is not None:
            assert copy.receipt is not None
            assert copy.initial_journal is not None
            copy.journal.append(
                copy.initial_journal.committed(copy.receipt).changed(phase="settled")
            )
        self._settle(copy, "completed")

    def _before_record(self, copy: _Copy, phase: str) -> CopyArrangementJournal:
        assert copy.initial_journal is not None
        initial = copy.initial_journal
        image = copy_before_project(self._project, initial)
        copy.journal_custody.acquire(
            self._assets, Path.cwd(), (), project_images=(image.model_dump_json(),)
        )
        return initial.changed(
            project_image_json=image.model_dump_json(),
            intent_revision=self._persistence.revision,
            image_role="before",
            phase=phase,
            committed_revision=None,
            committed_config_sha256=None,
            error=copy.failure,
        )

    def _cancel_unclaimed(self, copy: _Copy) -> bool:
        if not copy.worker_done or copy.acknowledged or copy.config_committed:
            return False
        if not self._prove_cancellation(copy):
            return False
        if copy.initial_journal is not None:
            journal = self._ensure_journal(copy)
            assert journal is not None
            journal.append(self._before_record(copy, "cancelled"))
        self._settle(copy, "failed" if copy.failure is not None else "cancelled")
        return True

    def _prove_cancellation(self, copy: _Copy) -> bool:
        if copy.acknowledged or copy.config_committed:
            return False
        if not copy.cancellation_proven:
            try:
                if copy.captured is not None and not self._audio.cancel_pad_copy(copy.captured):
                    return False
            except (OSError, RuntimeError, TypeError, ValueError) as error:
                self._report(copy, str(error))
                return False
            copy.cancellation_proven = True
        return True

    def _settle(self, copy: _Copy, status: str) -> None:
        _release_images(self._assets, copy)
        if copy.recovery_handoff is not None:
            assert self._startup is not None
            self._startup.complete_original(copy.recovery_handoff)
        copy.journal_custody.release()
        if not copy.writer_released:
            self._persistence.release_migration(copy.owner)
            copy.writer_released = True
        copy.retirement.close()
        self._session.loading_sample_ids.discard(copy.target_id)
        self._session.sample_load_stage.pop(copy.target_id, None)
        if status == "completed":
            self._session.sample_load_errors.pop(copy.target_id, None)
            self.error = None
        self._copy = None
        self._status = status
