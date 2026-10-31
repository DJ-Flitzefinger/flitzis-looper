import hashlib
import re
import threading
import wave
from collections.abc import Callable, Iterator, Mapping
from contextlib import ExitStack
from dataclasses import dataclass, replace
from pathlib import Path
from queue import Empty, SimpleQueue
from typing import TYPE_CHECKING, Literal, cast
from uuid import uuid4

from flitzis_looper.analysis.process import offline_work_scope
from flitzis_looper.bs_input_provenance import bs_input_contract
from flitzis_looper.controller.asset_lifecycle import (
    AssetRetirementReservation,
    ProjectAssetLifecycle,
)
from flitzis_looper.controller.base import BaseController
from flitzis_looper.controller.bs_roformer_assets import MODEL_NAME as BS_ROFORMER_MODEL_NAME
from flitzis_looper.controller.committed_stem_residency import (
    CommittedStemResidency,
    StemResidencyStatus,
    resident_pcm_budget_pressure,
    resident_stem_admission_pressure,
)
from flitzis_looper.controller.key_metadata import loaded_source_identity
from flitzis_looper.controller.saved_residency import saved_resident_loop
from flitzis_looper.controller.stem_admission import (
    StemAdmissionArbiter,
    StemAdmissionCandidate,
    StemAdmissionResult,
)
from flitzis_looper.controller.stem_batch_inventory import (
    StemBatchInventory,
    StemBatchItem,
    capture_stem_batch_inventory,
    capture_stem_batch_item,
    stem_batch_item_is_current,
)
from flitzis_looper.controller.stem_cache import (
    cache_dir_for_sample_id,
    cache_dir_matches_sample_id,
    expected_stem_files,
    promote_generation_artifacts,
    source_version_for_sample_path,
    verified_stem_cache_available,
)
from flitzis_looper.controller.stem_delete import StemDeleteOutcome
from flitzis_looper.controller.stem_delete_inventory import (
    StemDeleteInventory,
    StemDeleteItem,
    StemDeletePadMetadata,
    capture_stem_delete_inventory,
    stem_delete_item_currentness,
    stem_delete_request_identity,
)
from flitzis_looper.controller.stem_generation import (
    AudioShape,
    StemGenerationBackend,
    StemGenerationRequest,
    StemGenerationResult,
    resolved_demucs_encoder,
)
from flitzis_looper.controller.stem_identity import request_fingerprint
from flitzis_looper.controller.stem_intent import StemGenerationIntent, StemIntentRegistry
from flitzis_looper.controller.stem_job import (
    DIRECT_STEM_INTEREST,
    StemGenerationInterest,
    StemGenerationInterests,
    StemGenerationJob,
    StemSubscriber,
)
from flitzis_looper.controller.stem_pair_preparation import (
    PairPreparation,
    StemPairIntentBinding,
    StemPairPreparationQueue,
    StemPairPreparationSpec,
)
from flitzis_looper.controller.stem_process import stem_command_scope
from flitzis_looper.controller.stem_provenance import (
    capture_processing_files,
    processing_source_roots,
)
from flitzis_looper.controller.stem_publication_retry import (
    StemPublicationRetries,
    StemPublicationRetry,
)
from flitzis_looper.controller.stem_resources import StemResourceGuard
from flitzis_looper.controller.stem_separators import (
    SelectedStemGenerationBackend,
    separator_model_cache_dir,
)
from flitzis_looper.controller.stem_workers import (
    STEM_QUEUED_JOBS,
    STEM_WORKERS,
    StemWorkerPool,
    StemWorkerReservation,
)
from flitzis_looper.encoder_provenance import encoder_contract
from flitzis_looper.models import (
    STEM_MIX_MODES,
    StemCacheEntry,
    StemGridIndicatorState,
    StemMaskDisplayMode,
    StemMixMode,
    validate_sample_id,
)
from flitzis_looper.project_materials import original_asset
from flitzis_looper.stem_execution import StemExecution, StemExecutionProgress
from flitzis_looper.stem_intent import transition_stem_intent
from flitzis_looper.stem_pair_selection import STEM_FINGERPRINT_SCHEMA

if TYPE_CHECKING:
    from flitzis_looper.control_diagnostics import ConsoleDiagnostics
    from flitzis_looper.models import ProjectState, SessionState
    from flitzis_looper_audio import (
        AudioEngine,
        CommittedStemSetTicket,
        OfflineStemSourceLease,
        PreparedSourceTicket,
        PreparedStemPair,
        ProjectAssetLease,
        SealedProcessingFiles,
    )

type StemTaskRunner = Callable[[Callable[[], None]], None]
type _StemBackendEventType = Literal["identity", "progress", "success", "error", "settled"]


def _generation_interest_allows_preparation(owner: StemGenerationInterests | None) -> bool:
    return owner is None or owner.allows_preparation


@dataclass(frozen=True, slots=True)
class _StemBackendEvent:
    sample_id: int
    source_version: str
    source_lease: OfflineStemSourceLease | None
    cache_dir: Path
    event_type: _StemBackendEventType
    content_id: str | None = None
    percent: float | None = None
    stage: str | None = None
    error: str | None = None
    result: StemGenerationResult | None = None
    generation_intent: StemGenerationIntent | None = None
    source_path: Path | None = None
    execution: StemExecution | None = None


@dataclass(slots=True)
class _PromotedStemGeneration:
    entry: StemCacheEntry
    lease: ProjectAssetLease
    queued: bool = False
    released: bool = False


@dataclass(slots=True)
class _StemJobEventDelivery:
    """Remaining actual subscriber custody, independent of a moved pad placement."""

    event: _StemBackendEvent
    job: StemGenerationJob
    remaining: tuple[StemSubscriber, ...]
    promotion_attempted: bool = False
    promotion_error: str | None = None
    promotion: _PromotedStemGeneration | None = None
    admission_route: Literal["inference", "reuse", "unavailable", "canonical_pending"] | None = None
    admitted: tuple[StemSubscriber, ...] = ()
    canonical: StemGenerationJob | None = None
    ready_sequence: int = 0


@dataclass(frozen=True, slots=True)
class _RequestedStemGeneration:
    """One accepted control intent, before any source/job/retirement admission."""

    capture: StemBatchItem
    request: StemGenerationRequest
    intent: StemGenerationIntent
    owner: StemGenerationInterests
    ready_sequence: int
    reuse_only: bool = False
    skip_completed: bool = False


@dataclass(frozen=True, slots=True)
class _RequestedStemPreparation:
    """Original unadmitted Pair metadata; actual readers stay in the existing queue."""

    spec: StemPairPreparationSpec
    ready_sequence: int


type _StemAdmissionOwner = (
    _RequestedStemGeneration | _RequestedStemPreparation | StemGenerationJob | StemSubscriber
)


@dataclass(slots=True)
class _PendingStemPublication:
    sample_id: int
    source_ticket: PreparedSourceTicket
    entry: StemCacheEntry
    previous_entry: StemCacheEntry | None
    previous_lease: ProjectAssetLease | None
    retirement: AssetRetirementReservation
    prepared_pair: PreparedStemPair | None = None
    content_id: str | None = None
    source_path: str | None = None
    generation_intent: StemGenerationIntent | None = None
    interest_owner: StemGenerationInterests | None = None


@dataclass(frozen=True, slots=True)
class StemGenerationCancellation:
    """Original cancellation readers; no new job registry or publication authority."""

    disposition: Literal["absent", "remaining", "detached"]
    owner: StemGenerationInterests
    jobs: tuple[StemGenerationJob, ...] = ()
    preparations: tuple[PairPreparation, ...] = ()
    deferred_jobs: tuple[StemGenerationJob, ...] = ()


@dataclass(frozen=True, slots=True)
class _VerifiedStemIntent:
    entry: StemCacheEntry
    intent: StemGenerationIntent
    request: StemGenerationRequest
    content_id: str | None
    source_path: str | None
    processing_files: SealedProcessingFiles


@dataclass(slots=True)
class _DiskSelectionSettlement:
    """Keep exact pre-acquired owners reachable after the durable selection commit."""

    request: PairPreparation
    entry: StemCacheEntry
    ticket: PreparedSourceTicket
    lease: ProjectAssetLease
    previous_lease: ProjectAssetLease | None
    retirement: AssetRetirementReservation
    content_retired: bool = False
    verified_restore: bool = False
    assignment_adopted: bool = False
    last_error: str | None = None


@dataclass(slots=True)
class _StemDemand:
    """One bounded control interest; PCM and callback authority stay entirely Native."""

    source_version: str
    content_id: str | None
    source_path: str | None
    selection_identity: str | None
    verify_disk: bool = False
    explicit: bool = False
    policy: bool = False
    withdraw_policy: bool = False
    state: str = "queued"
    error: str | None = None


@dataclass(frozen=True, slots=True)
class StemResidencySummary:
    """Component readiness and exact Native PCM accounting, never process RAM claims."""

    requested: int
    queued: int
    preparing: int
    acked: int
    deferred: int
    errors: int
    estimated_pcm_bytes: int
    unknown_shapes: int
    full_track_exceptions: int
    native: dict[str, int | bool | str | None]
    unconfirmed: int = 0
    accounting_error: str | None = None


class StemController(BaseController):  # noqa: PLR0904
    """Manage offline stem cache metadata and generation task gating."""

    def __init__(
        self,
        project: ProjectState,
        session: SessionState,
        audio: AudioEngine,
        on_project_changed: Callable[[], None] | None = None,
        stem_backend: StemGenerationBackend | None = None,
        stem_task_runner: StemTaskRunner | None = None,
        asset_lifecycle: ProjectAssetLifecycle | None = None,
        stem_resources: StemResourceGuard | None = None,
    ) -> None:
        super().__init__(project, session, audio, on_project_changed)
        self._pad_edit_guard: Callable[[int], None] | None = None
        self._restored_assignment_held: Callable[[int], bool] = lambda sample_id: False
        self._assets = asset_lifecycle or ProjectAssetLifecycle(project, audio)
        self._init_generation_admission()
        self._reuse_only_jobs: set[Path] = set()
        self._skip_completed_jobs: set[Path] = set()
        self._verified_sets: dict[int, _VerifiedStemIntent] = {}
        self._verified_candidates: dict[
            int, tuple[StemGenerationIntent, StemGenerationRequest, SealedProcessingFiles]
        ] = {}
        self._generation_event_lock = threading.Lock()
        self._shutting_down = False
        self._stem_backend = (
            stem_backend if stem_backend is not None else SelectedStemGenerationBackend()
        )
        self._stem_resources = stem_resources if stem_resources is not None else StemResourceGuard()
        self._stem_playback_active = threading.Event()
        self._stem_worker_pool: StemWorkerPool | None = None
        if stem_task_runner is None:
            self._stem_worker_pool = StemWorkerPool()
            self._stem_task_runner: StemTaskRunner = self._stem_worker_pool
        else:
            self._stem_task_runner = stem_task_runner
        self._stem_generation_events: SimpleQueue[_StemBackendEvent] = SimpleQueue()
        self._diagnostics: ConsoleDiagnostics | None = None
        self._generation_sources: dict[int, OfflineStemSourceLease] = {}
        self._disk_selection_settlements: dict[int, _DiskSelectionSettlement] = {}
        self._commit_disk_selection: (
            Callable[[int, StemCacheEntry | None, StemCacheEntry, Callable[[], bool]], None] | None
        ) = None
        self._flush_superseded_disk_selection: Callable[[], None] | None = None
        self._disk_selection_writer_blocked: Callable[[], bool] | None = None
        self._pending_stem_publications: dict[int, _PendingStemPublication] = {}
        self._delete_logical_interests: Callable[[], Mapping[int, StemGenerationInterest]] = dict
        self._restored_stem_candidates: dict[int, StemCacheEntry] = {}
        self._pair_preparations = StemPairPreparationQueue(
            audio, self._assets, self._stem_task_runner, self._stem_resources
        )
        self._resident_pairs: set[int] = set()
        self._publication_retries = StemPublicationRetries()
        self._pair_release_ordered: set[int] = set()
        self._fenced_pair_ids: set[int] = set()
        self._residency_errors: dict[int, str] = {}
        self._pending_full_mix_releases: set[int] = set()
        self._pair_held_generation_events: dict[
            tuple[StemGenerationJob, str], _StemJobEventDelivery
        ] = {}
        self._demands: dict[int, _StemDemand] = {}
        self._demand_direct_turn = True
        self._committed_residency = CommittedStemResidency(
            audio,
            self._committed_selection_current,
            self._report_residency_error,
            source_current=self._committed_source_current,
        )
        self._on_frame_render_callbacks.append(self._poll_generation_events)
        self._on_frame_render_callbacks.append(self._poll_stem_publications)
        self._on_frame_render_callbacks.append(self._poll_pair_preparations)
        self._on_frame_render_callbacks.append(self._poll_disk_selection_settlements)
        self._on_frame_render_callbacks.append(self._poll_publication_retries)
        self._on_frame_render_callbacks.append(self._poll_committed_residency)
        self._on_frame_render_callbacks.append(self._poll_residency_demands)
        self._on_frame_render_callbacks.append(self._poll_stem_admissions)

    def _init_generation_admission(self) -> None:
        self._jobs: dict[Path, StemGenerationJob] = {}
        self._intents = StemIntentRegistry()
        self._generation_intents: dict[int, StemGenerationIntent] = {}
        self._requested_generations: dict[int, _RequestedStemGeneration] = {}
        self._requested_preparations: dict[int, _RequestedStemPreparation] = {}
        self._stem_admission = StemAdmissionArbiter()
        self._stem_readiness_sequence = 0
        self._inference_jobs: set[Path] = set()
        self._waiting_jobs: set[Path] = set()

    def _bind_diagnostics(self, diagnostics: ConsoleDiagnostics) -> None:
        """Bind reporting to existing control-side completion and cleanup handlers."""
        self._diagnostics = diagnostics

    def _record_generation_diagnostic(self, sample_id: int, message: str) -> None:
        self._session.stem_generation_diagnostics[sample_id] = message
        if self._diagnostics is not None:
            self._diagnostics.observe("stem_generation_diagnostic", message, pad_id=sample_id)

    def shut_down(self) -> None:
        """Cancel separator publication while retaining every running file reader."""
        with self._generation_event_lock:
            self._shutting_down = True
            for delivery in tuple(self._pair_held_generation_events.values()):
                self._release_generation_promotion(delivery)
            self._pair_held_generation_events.clear()
            for job in self._jobs.values():
                job.cancel()
            self._jobs.clear()
            while True:
                try:
                    self._stem_generation_events.get_nowait()
                except Empty:
                    break
        self._generation_sources.clear()
        self._generation_intents.clear()
        self._withdraw_all_stem_requests()
        self._verified_sets.clear()
        self._verified_candidates.clear()
        self._waiting_jobs.clear()
        self._committed_residency.shut_down()
        self._demands.clear()
        for sample_id, pending in tuple(self._pending_stem_publications.items()):
            retry = self._publication_retries.pending.get(sample_id)
            if self._project.stem_cache[sample_id] is pending.entry and (
                retry is None or not self._publication_retry_current(retry)
            ):
                with pending.retirement.activate():
                    self._restore_previous_stems(sample_id, pending)
            self._release_pending_stems(pending)
        self._pending_stem_publications.clear()
        self._restored_stem_candidates.clear()
        for sample_id in self._pair_preparations.pending:
            self._pair_preparations.cancel(sample_id)
        self._poll_pair_preparations()
        self._poll_disk_selection_settlements()
        self._session.stem_generating_sample_ids.clear()
        self._session.stem_generation_source_versions.clear()
        if self._stem_worker_pool is not None:
            self._stem_worker_pool.shutdown()

    def _withdraw_all_stem_requests(self) -> None:
        for requested in self._requested_generations.values():
            requested.owner.invalidate_all()
        self._requested_generations.clear()
        for preparation in self._requested_preparations.values():
            owner = preparation.spec.binding.interest_owner if preparation.spec.binding else None
            if owner is not None:
                owner.invalidate_all()
        self._requested_preparations.clear()

    def bind_pad_edit_guard(self, guard: Callable[[int], None]) -> None:
        """Check assignment ownership before changing musical stem intent."""
        self._pad_edit_guard = guard

    def _ensure_pad_edit_allowed(self, sample_id: int) -> None:
        if self._pad_edit_guard is not None:
            self._pad_edit_guard(sample_id)

    def retire_overwritten_native_state(self, sample_id: int, *, original_content_id: str) -> None:
        """Retire predecessor control owners after its occupied Copy's own Native ACK.

        The coordinator keeps this target observer-fenced and its predecessor
        project image intact until retirement completes. Retained Native tickets
        and worker readers settle by their existing rules; no new slot command
        or durable musical assignment is issued here.
        """
        validate_sample_id(sample_id)
        if (
            sample_id not in getattr(self, "_pair_deferred_sample_ids", ())
            or self._content_id(sample_id) != original_content_id
        ):
            message = "Overwritten stem retirement requires the fenced original content"
            raise RuntimeError(message)
        self._committed_residency.cancel(sample_id)
        self._cancel_jobs(sample_id)
        self._pair_preparations.cancel(sample_id)
        settlement = self._disk_selection_settlements.get(sample_id)
        if settlement is not None:
            # A committed disk selection cannot be discarded as a precommit
            # worker. Keep its exact durable/lease owners until the coordinator's
            # superseding config has been written and normal settlement can run.
            settlement.content_retired = True
            settlement.request.cancelled.set()
        pending = self._pending_stem_publications.get(sample_id)
        retry = self._publication_retries.pending.get(sample_id)
        owners = (pending, retry.pending if retry is not None else None)
        for index, owner in enumerate(owners):
            if owner is not None and all(owner is not prior for prior in owners[:index]):
                self._release_pending_stems(owner)
        self._pending_stem_publications.pop(sample_id, None)
        self._demands.pop(sample_id, None)
        self._resident_pairs.discard(sample_id)
        self._pair_release_ordered.discard(sample_id)
        self._fenced_pair_ids.discard(sample_id)
        self._pending_full_mix_releases.discard(sample_id)
        self._residency_errors.pop(sample_id, None)
        self._clear_stem_generation_state(sample_id)
        self._clear_stem_generation_messages(sample_id)

    def project_copied_native_state(
        self,
        sample_id: int,
        *,
        components_available: bool,
        selection: CommittedStemSetTicket | None,
    ) -> None:
        """Project only component availability from the complete target's own Native ACK."""
        validate_sample_id(sample_id)
        entry = self._project.stem_cache[sample_id]
        if entry is None:
            if components_available or selection is not None:
                message = "Copied Native components have no committed target stem metadata"
                raise RuntimeError(message)
            return
        if (entry.pair is None) != (selection is None):
            message = "Copied stem metadata and own target registration do not match"
            raise RuntimeError(message)
        projected = entry.model_copy(update={"available": components_available})
        if selection is not None:
            self._committed_residency.adopt_copied_selection(
                sample_id,
                projected,
                self._content_id(sample_id),
                self._project.sample_paths[sample_id],
                selection,
            )
        if entry.available != components_available:
            self._project.stem_cache[sample_id] = projected

    def bind_disk_selection_commit(
        self,
        commit: Callable[[int, StemCacheEntry | None, StemCacheEntry, Callable[[], bool]], None],
        flush_superseded: Callable[[], None] | None = None,
        *,
        writer_blocked: Callable[[], bool] | None = None,
    ) -> None:
        """Use the project's existing fenced atomic writer for complete selections."""
        self._commit_disk_selection = commit
        self._flush_superseded_disk_selection = flush_superseded
        self._disk_selection_writer_blocked = writer_blocked

    def bind_restored_assignment_guard(self, held: Callable[[int], bool]) -> None:
        """Preserve journal-held saved metadata while fresh source custody is unresolved."""
        self._restored_assignment_held = held

    def recovered_copy_selection_ready(
        self, sample_id: int, expected_entry: StemCacheEntry | None
    ) -> bool:
        """Queue exact saved verification and require own registration/component ACKs."""
        validate_sample_id(sample_id)
        if expected_entry is None:
            return True
        current = self._project.stem_cache[sample_id]
        if (
            current is None
            or expected_entry.pair is None
            or current.model_dump(exclude={"available"})
            != expected_entry.model_dump(exclude={"available"})
            or not self._committed_residency.supported
        ):
            self._session.stem_generation_errors[sample_id] = (
                "Recovered Copy stem selection is missing, unsupported or changed"
            )
            return False
        demand = self._remember_demand(sample_id, current)
        demand.explicit = self._project.pad_stem_mix_mode[sample_id] == "all_stems"
        demand.policy = self._project.preload_stem_loops_on_startup
        if not self._committed_residency.contains(sample_id):
            if not self._entry_files_available(current):
                self._session.stem_generation_errors[sample_id] = (
                    "Recovered Copy selected stem custody is unavailable"
                )
                return False
            self._restored_stem_candidates[sample_id] = current
            demand.verify_disk = True
            if demand.state != "preparing":
                demand.state = "queued"
        try:
            return self._committed_residency.recovered_selection_ready(
                sample_id,
                current,
                mode=self._project.pad_stem_mix_mode[sample_id],
                enabled_mask=self._project.pad_stem_intent[sample_id].enabled_mask,
                preload=demand.policy,
            )
        except (RuntimeError, TypeError, ValueError) as error:
            self._session.stem_generation_errors[sample_id] = str(error)
            return False

    def _remember_demand(self, sample_id: int, entry: StemCacheEntry) -> _StemDemand:
        validate_sample_id(sample_id)
        identity = entry.pair.stem_set_identity if entry.pair is not None else None
        demand = self._demands.get(sample_id)
        binding = (
            entry.source_version,
            self._content_id(sample_id),
            self._project.sample_paths[sample_id],
            identity,
        )
        if (
            demand is None
            or (
                demand.source_version,
                demand.content_id,
                demand.source_path,
                demand.selection_identity,
            )
            != binding
        ):
            demand = _StemDemand(*binding)
            self._demands[sample_id] = demand
        return demand

    def _poll_committed_residency(self) -> None:
        """Retire observed past direct interest before the same frame can retry it."""
        deferred: frozenset[int] = getattr(self, "_pair_deferred_sample_ids", frozenset())
        for withdrawn in self._committed_residency.poll(deferred_sample_ids=deferred):
            sample_id = withdrawn.sample_id
            demand = self._demands.get(sample_id)
            if demand is None or not demand.explicit:
                continue
            if (
                self._demand_current(sample_id, demand)
                and (
                    demand.content_id,
                    demand.source_path,
                    demand.source_version,
                    demand.selection_identity,
                )
                == (
                    withdrawn.content_id,
                    withdrawn.source_path,
                    withdrawn.source_version,
                    withdrawn.stem_set_identity,
                )
                and withdrawn.mode == self._project.pad_stem_mix_mode[sample_id] == "all_stems"
                and withdrawn.enabled_mask == self._project.pad_stem_intent[sample_id].enabled_mask
            ):
                demand.explicit = False

        # A stopped PAD can accept its own residency while audio remains waiting.
        # Retire only resolved admission feedback here, before the fair retry
        # owner's later visit; this neither advances that owner nor infers audio.
        for sample_id, message in tuple(self._residency_errors.items()):
            if sample_id in deferred or not resident_stem_admission_pressure(message):
                continue
            demand = self._demands.get(sample_id)
            if (
                demand is None
                or not demand.explicit
                or demand.withdraw_policy
                or self._project.pad_stem_mix_mode[sample_id] != "all_stems"
                or not self._demand_acked_current(sample_id, demand)
            ):
                continue
            status = self.stem_residency_status(sample_id)
            if (
                status is None
                or status.effective not in {"waiting", "effective"}
                or self._residency_errors.get(sample_id) != message
            ):
                continue
            if demand.error == message:
                demand.error = None
            self._report_residency_error(sample_id, None)

    def refresh_residency_policy(self) -> None:
        """Coalesce bounded policy interest without cancelling direct requests or voice owners."""
        enabled = self._project.preload_stem_loops_on_startup
        for sample_id, entry in enumerate(self._project.stem_cache):
            if entry is None or sample_id in self._fenced_pair_ids:
                continue
            demand = self._demands.get(sample_id)
            if enabled and entry.available:
                demand = self._remember_demand(sample_id, entry)
            if demand is None:
                continue
            was_enabled = demand.policy
            demand.policy = enabled
            demand.withdraw_policy = demand.withdraw_policy or (was_enabled and not enabled)
            if was_enabled != enabled or demand.state == "deferred":
                demand.state = "queued"
                demand.error = None

    def _demand_current(self, sample_id: int, demand: _StemDemand) -> bool:
        entry = self._project.stem_cache[sample_id]
        return (
            not self._shutting_down
            and entry is not None
            and demand.content_id == self._content_id(sample_id)
            and demand.source_path == self._project.sample_paths[sample_id]
            and demand.source_version == entry.source_version
            and demand.selection_identity
            == (entry.pair.stem_set_identity if entry.pair is not None else None)
        )

    def _poll_residency_demands(self) -> None:
        """Visit at most eight owners fairly; resource pressure never erases their interest."""
        for sample_id, demand in tuple(self._demands.items()):
            if sample_id in getattr(self, "_pair_deferred_sample_ids", ()):
                continue
            if not self._demand_current(sample_id, demand):
                self._demands.pop(sample_id, None)
        candidates = [
            (sample_id, demand)
            for sample_id, demand in self._demands.items()
            if demand.state in {"queued", "deferred", "preparing"}
            and sample_id not in getattr(self, "_pair_deferred_sample_ids", ())
        ]
        for _ in range(min(8, len(candidates))):
            index = next(
                (
                    index
                    for index, (_, demand) in enumerate(candidates)
                    if demand.explicit == self._demand_direct_turn
                ),
                0,
            )
            sample_id, demand = candidates.pop(index)
            self._demand_direct_turn = not self._demand_direct_turn
            self._demands.pop(sample_id)
            self._demands[sample_id] = demand
            try:
                self._advance_residency_demand(sample_id, demand)
            except (OSError, RuntimeError, TypeError, ValueError) as error:
                message = str(error)
                demand.state = self._admission_wait_state(message)
                demand.error = message
                self._session.stem_generation_errors[sample_id] = message

    def _advance_disk_demand(self, sample_id: int, demand: _StemDemand) -> bool:
        """Complete authoritative disk verification before component admission."""
        entry = self._project.stem_cache[sample_id]
        if entry is None:
            return False
        if demand.state == "deferred":
            demand.state = "queued"
        if (
            sample_id in self._pair_preparations.pending
            or sample_id in self._requested_preparations
            or (sample_id in self._disk_selection_settlements)
        ):
            demand.state = "preparing"
            return False
        if demand.verify_disk:
            ticket = self._capture_source_ticket(sample_id, demand.source_version)
            self._prepare_pair(
                sample_id,
                entry,
                ticket,
                components=False,
            )
            demand.state = "preparing"
            return False
        if not demand.explicit and not demand.policy and not demand.withdraw_policy:
            demand.state = "verified"
            return False
        return True

    def _advance_residency_demand(self, sample_id: int, demand: _StemDemand) -> None:
        if not self._advance_disk_demand(sample_id, demand):
            return
        status = self._committed_residency.status(sample_id)
        if status is None or status.registration != "accepted":
            demand.state = "queued"
            if status is not None and status.registration == "rejected":
                demand.state = "error"
                demand.error = self._residency_errors.get(sample_id, "Stem registration rejected")
            return
        if demand.explicit and self._explicit_interest_pending(status):
            demand.state = "preparing"
            return
        if demand.withdraw_policy:
            if not self._advance_preload_interest(sample_id, demand, enabled=False):
                return
            demand.withdraw_policy = False
        self._advance_component_interests(sample_id, demand, status)

    def _advance_component_interests(
        self, sample_id: int, demand: _StemDemand, status: StemResidencyStatus
    ) -> None:
        explicit_ready = not demand.explicit or self._advance_explicit_interest(
            sample_id, demand, status
        )
        if demand.state in {"error", "deferred"}:
            return
        if demand.explicit and not explicit_ready:
            return
        policy_ready = not demand.policy or self._advance_preload_interest(
            sample_id, demand, enabled=True
        )
        if demand.state in {"error", "deferred"}:
            return
        if demand.policy and not policy_ready:
            return
        demand.state = "acked" if explicit_ready and policy_ready else "preparing"
        demand.error = None
        if demand.state == "acked":
            self._report_residency_error(sample_id, None)

    def _advance_explicit_interest(
        self, sample_id: int, demand: _StemDemand, status: StemResidencyStatus
    ) -> bool:
        if status.residency == "rejected" or status.effective == "rejected":
            error = self._residency_errors.get(sample_id)
            if not resident_stem_admission_pressure(error):
                self._observe_demand_receipt(demand, "rejected", error)
                return False
        if not self._explicit_residency_ready(sample_id):
            if not self._committed_residency.request(
                sample_id, "all_stems", self._project.pad_stem_intent[sample_id].enabled_mask
            ):
                self._defer_or_fail_demand(sample_id, demand)
                return False
            observed = self._committed_residency.status(sample_id)
            if observed is None:
                return False
            status = observed
        demand.state = "preparing"
        return self._explicit_residency_ready(sample_id)

    def _explicit_residency_ready(self, sample_id: int, *, preload: bool = False) -> bool:
        """Reuse the exact selected set, mask and own current receipt predicate."""
        entry = self._project.stem_cache[sample_id]
        return entry is not None and self._committed_residency.recovered_selection_ready(
            sample_id,
            entry,
            mode="all_stems",
            enabled_mask=self._project.pad_stem_intent[sample_id].enabled_mask,
            preload=preload,
        )

    def _advance_preload_interest(
        self, sample_id: int, demand: _StemDemand, *, enabled: bool
    ) -> bool:
        if not self._committed_residency.request_preload(sample_id, enabled=enabled):
            self._defer_or_fail_demand(sample_id, demand)
            return False
        state, error = self._committed_residency.preload_status(sample_id)
        if state != "accepted":
            self._observe_demand_receipt(demand, state, error)
            return False
        demand.state = "preparing"
        return True

    def _defer_or_fail_demand(self, sample_id: int, demand: _StemDemand) -> None:
        error = self._residency_errors.get(sample_id)
        demand.state = self._admission_wait_state(error) if error is not None else "queued"
        demand.error = error

    @staticmethod
    def _observe_demand_receipt(demand: _StemDemand, state: str | None, error: str | None) -> None:
        demand.state = (
            StemController._admission_wait_state(error)
            if resident_stem_admission_pressure(error)
            else "error"
            if state == "rejected"
            else "preparing"
        )
        demand.error = error

    @staticmethod
    def _admission_wait_state(error: object) -> str:
        if resident_pcm_budget_pressure(error):
            return "deferred"
        return "queued" if resident_stem_admission_pressure(error) else "error"

    @staticmethod
    def _explicit_interest_pending(status: StemResidencyStatus) -> bool:
        return status.requested_mode == "all_stems" and (
            status.residency in {"captured", "pending"} or status.effective == "transitioning"
        )

    def residency_summary(self) -> StemResidencySummary:
        """Project actual own ACK counts and conservative PCM demand, separate from RSS."""
        requested = [
            (sample_id, demand)
            for sample_id, demand in self._demands.items()
            if demand.explicit or demand.policy
        ]
        current_acks = sum(
            demand.state == "acked" and self._demand_acked_current(sample_id, demand)
            for sample_id, demand in requested
        )
        estimate, unknown, full_track = self._estimate_all_pad_pcm()
        native: dict[str, int | bool | str | None] = {}
        error = None
        try:
            native = self._resident_accounting_counters(
                self._audio.get_resident_pcm_budget_status()
            )
        except (RuntimeError, TypeError, ValueError) as caught:
            error = str(caught)
        return StemResidencySummary(
            len(requested),
            sum(demand.state == "queued" for _, demand in requested),
            sum(demand.state == "preparing" for _, demand in requested),
            current_acks,
            sum(demand.state == "deferred" for _, demand in requested),
            sum(demand.state == "error" for _, demand in requested),
            estimate,
            unknown,
            full_track,
            native,
            sum(demand.state == "acked" for _, demand in requested) - current_acks,
            error,
        )

    @staticmethod
    def _resident_accounting_counters(raw: object) -> dict[str, int | bool | str | None]:
        counters = {
            "budget_mib",
            "budget_bytes",
            "held_unique_pcm_bytes",
            "pending_resident_bytes",
            "admitted_total_bytes",
            "unique_backing_count",
            "pending_reservation_count",
            "available_bytes",
        }
        if (
            not isinstance(raw, dict)
            or any(type(raw.get(key)) is not int or raw[key] < 0 for key in counters)
            or type(raw.get("over_budget")) is not bool
        ):
            message = "Native resident accounting returned invalid counters"
            raise TypeError(message)
        return cast(
            "dict[str, int | bool | str | None]",
            {key: raw[key] for key in counters | {"over_budget"}},
        ) | StemController._resident_host_memory_counters(raw)

    @staticmethod
    def _resident_host_memory_counters(
        raw: dict[str, object],
    ) -> dict[str, int | bool | str | None]:
        optional: dict[str, int | bool | str | None] = {}
        for key in (
            "host_pending_allocation_bytes",
            "host_available_physical_bytes",
            "host_available_commit_bytes",
            "host_allocation_allowance_bytes",
        ):
            if key in raw:
                value = raw[key]
                if (value is None and key == "host_pending_allocation_bytes") or (
                    value is not None and (type(value) is not int or value < 0)
                ):
                    message = "Native host memory accounting returned invalid counters"
                    raise TypeError(message)
                optional[key] = value
        if "automatic" in raw:
            if type(raw["automatic"]) is not bool:
                message = "Native resident accounting returned invalid admission mode"
                raise TypeError(message)
            optional["automatic"] = raw["automatic"]
        if "host_memory_error" in raw:
            error = raw["host_memory_error"]
            if error is not None and not isinstance(error, str):
                message = "Native host memory accounting returned invalid status"
                raise TypeError(message)
            optional["host_memory_error"] = error
        return optional

    def _demand_acked_current(self, sample_id: int, demand: _StemDemand) -> bool:
        try:
            return (
                self._demand_current(sample_id, demand)
                and (
                    not demand.explicit
                    or self._explicit_residency_ready(sample_id, preload=demand.policy)
                )
                and (
                    not demand.policy
                    or self._committed_residency.preload_status(sample_id)[0] == "accepted"
                )
            )
        except RuntimeError, TypeError, ValueError:
            return False

    def _estimate_all_pad_pcm(self) -> tuple[int, int, int]:
        estimate = unknown = full_track = 0
        for sample_id, path in enumerate(self._project.sample_paths):
            if path is None:
                continue
            try:
                raw = self._audio.loaded_sample_shape(sample_id)
            except RuntimeError, TypeError, ValueError:
                unknown += 1
                continue
            if (
                not isinstance(raw, tuple)
                or len(raw) != 3
                or any(type(value) is not int or value <= 0 for value in raw)
            ):
                unknown += 1
                continue
            rate, channels, frames = raw
            window = saved_resident_loop(self._project, sample_id, sample_rate_hz=rate)
            if window is None:
                full_track += 1
            else:
                frames = min(
                    frames, max(1, round((window.end_seconds - window.start_seconds) * rate))
                )
            layers = 5 if self._project.stem_cache[sample_id] is not None else 1
            estimate += frames * channels * 4 * layers
        return estimate, unknown, full_track

    def generate_stems_async(self, sample_id: int) -> bool:
        """Schedule source-leased offline generation independently of pad playback."""
        return self.request_generation_interest(sample_id, DIRECT_STEM_INTEREST) is not None

    def capture_generation_inventory(self) -> StemBatchInventory:
        """Capture the all-slot warning image without source or worker admission."""
        return capture_stem_batch_inventory(
            self._project,
            source_identity=lambda pad_id: loaded_source_identity(self._audio, pad_id),
            target_shape=self._target_shape_for_pad,
        )

    def bind_delete_logical_interests(
        self, provider: Callable[[], Mapping[int, StemGenerationInterest]]
    ) -> None:
        """Bind the existing batch owner's fresh metadata-only unadmitted snapshot.

        Controllers without a batch owner default to no logical caller tokens.
        Application composition must bind its actual batch owner so a newer
        unadmitted operation invalidates an older deletion warning.

        Args:
            provider: Read the existing batch's exact frozen tokens without admission.
        """
        self._delete_logical_interests = provider

    def capture_deletion_inventory(self) -> StemDeleteInventory:
        """Capture all loaded original assignments without I/O or new custody.

        Returns:
            Detached exact current, retained-set and existing-admission metadata.

        Raises:
            RuntimeError: An arrangement fence prevents a coherent warning image.
        """
        if getattr(self, "_pair_deferred_sample_ids", ()):
            message = "Cannot capture stem deletion while PAD arrangement is pending"
            raise RuntimeError(message)
        logical_pending = self._delete_logical_interests()
        return capture_stem_delete_inventory(
            tuple(
                self._deletion_pad_metadata(sample_id, logical_pending)
                for sample_id in range(len(self._project.sample_paths))
            )
        )

    def _deletion_pad_metadata(
        self, sample_id: int, logical_pending: Mapping[int, StemGenerationInterest]
    ) -> StemDeletePadMetadata | None:
        path = self._project.sample_paths[sample_id]
        if path is None:
            return None
        try:
            source = loaded_source_identity(self._audio, sample_id)
        except RuntimeError, TypeError, ValueError:
            source = None
        token = logical_pending.get(sample_id)
        return StemDeletePadMetadata(
            pad_id=sample_id,
            path=path,
            content=self._project.pad_content[sample_id],
            source_identity=source,
            current_entry=self._project.stem_cache[sample_id],
            retained_entries=self._deletion_retained_entries(sample_id),
            requests=tuple(
                stem_delete_request_identity(intent)
                for intent in self._deletion_request_intents(sample_id)
                if intent is not None
            ),
            logical_pending=(token,) if token is not None else (),
        )

    def _deletion_publication_rows(self, sample_id: int) -> tuple[_PendingStemPublication, ...]:
        pending = self._pending_stem_publications.get(sample_id)
        retry = self._publication_retries.pending.get(sample_id)
        return tuple(
            row
            for row in (pending, retry.pending if retry is not None else None)
            if row is not None
        )

    def _deletion_retained_entries(self, sample_id: int) -> tuple[StemCacheEntry, ...]:
        entries: list[StemCacheEntry | None] = [self._restored_stem_candidates.get(sample_id)]
        requested = self._requested_preparations.get(sample_id)
        if requested is not None:
            entries.extend((requested.spec.entry, requested.spec.previous_entry))
        request = self._pair_preparations.pending.get(sample_id)
        if request is not None:
            entries.extend((request.entry, request.previous_entry))
        settlement = self._disk_selection_settlements.get(sample_id)
        if settlement is not None:
            entries.extend((settlement.entry, settlement.request.previous_entry))
        for publication in self._deletion_publication_rows(sample_id):
            entries.extend((publication.entry, publication.previous_entry))
        verified = self._verified_sets.get(sample_id)
        if verified is not None:
            entries.append(verified.entry)
        for row in self._committed_residency._pair_control_rows(sample_id):
            entry = getattr(row, "entry", None)
            if isinstance(entry, StemCacheEntry):
                entries.append(entry)
        for delivery in self._pair_held_generation_events.values():
            promotion = delivery.promotion
            if (
                promotion is not None
                and not promotion.released
                and not promotion.lease.released
                and any(row.sample_id == sample_id for row in delivery.job.subscribers())
            ):
                entries.append(promotion.entry)
        return tuple(entry for entry in entries if entry is not None)

    def _deletion_request_intents(self, sample_id: int) -> tuple[StemGenerationIntent | None, ...]:
        intents = [self._generation_intents.get(sample_id)]
        requested = self._requested_generations.get(sample_id)
        if requested is not None:
            intents.append(requested.intent)
        preparation = self._requested_preparations.get(sample_id)
        if preparation is not None and preparation.spec.binding is not None:
            intents.append(preparation.spec.binding.generation_intent)
        request = self._pair_preparations.pending.get(sample_id)
        if request is not None:
            intents.append(request.generation_intent)
        settlement = self._disk_selection_settlements.get(sample_id)
        if settlement is not None:
            intents.append(settlement.request.generation_intent)
        intents.extend(
            publication.generation_intent
            for publication in self._deletion_publication_rows(sample_id)
        )
        verified = self._verified_sets.get(sample_id)
        if verified is not None:
            intents.append(verified.intent)
        candidate = self._verified_candidates.get(sample_id)
        if candidate is not None:
            intents.append(candidate[0])
        intents.extend(
            row.intent
            for job in self._jobs.values()
            for row in job.subscribers()
            if row.sample_id == sample_id
        )
        intents.extend(
            row.intent
            for delivery in self._pair_held_generation_events.values()
            for row in delivery.job.subscribers()
            if row.sample_id == sample_id
        )
        return tuple(intents)

    def generation_item_is_deferred(self, item: StemBatchItem) -> bool:
        """Wait through the actual arrangement fence before observing its image."""
        return item.pad_id in getattr(self, "_pair_deferred_sample_ids", ())

    def generation_item_is_current(self, item: StemBatchItem) -> bool:
        """Validate an original warning target, without recapturing another occupant."""
        return stem_batch_item_is_current(
            self._project,
            item,
            source_identity=lambda pad_id: loaded_source_identity(self._audio, pad_id),
            target_shape=self._target_shape_for_pad,
        )

    def request_captured_generation_interest(
        self, item: StemBatchItem, interest: StemGenerationInterest
    ) -> StemGenerationInterests | None:
        """Join or request exactly one original confirmed item using current guards."""
        if self.generation_item_is_deferred(item) or not self.generation_item_is_current(item):
            return None
        sample_id = item.pad_id
        self._ensure_pad_edit_allowed(sample_id)
        self._expire_generation_interests()
        self._expire_requested_generations()
        joined = self._join_generation_interest(sample_id, interest)
        if joined is not None:
            return joined
        if self._stem_generation_blocker(sample_id) is not None:
            return None
        owner = StemGenerationInterests(frozenset({interest}))
        if self._verified_set_current(sample_id):
            owner.mark_already_present()
            return owner
        return self._request_new_captured_generation(item, owner)

    def _request_new_captured_generation(
        self, item: StemBatchItem, owner: StemGenerationInterests
    ) -> StemGenerationInterests | None:
        sample_id = item.pad_id
        version = self._loaded_source_version_for_pad(sample_id)
        if version is None:
            return None
        request = self._generation_request(sample_id, version, capture=item)
        if request is None:
            return None
        self._clear_stem_generation_messages(sample_id)
        if self._stem_worker_pool is None:
            return self._admit_injected_captured_generation(item, request, owner)
        return owner if self._request_captured_generation(request, owner, capture=item) else None

    def _admit_injected_captured_generation(
        self, item: StemBatchItem, request: StemGenerationRequest, owner: StemGenerationInterests
    ) -> StemGenerationInterests:
        """Keep explicitly injected runners on the existing synchronous test seam."""
        requested = _RequestedStemGeneration(
            item,
            request,
            self._intents.capture(request, item.processing_revision),
            owner,
            self._next_stem_readiness(),
        )
        try:
            self._assets.sync_assignments()
            source = self._capture_requested_source(requested)
            self._admit_stem_generation(
                request, source, interest_owner=owner, generation_intent=requested.intent
            )
        except (AttributeError, OSError, RuntimeError, TypeError, ValueError) as error:
            owner.mark_failed(str(error))
            self._session.stem_generation_errors[item.pad_id] = f"Stem admission failed: {error}"
        return owner

    def generation_interest_is_running(self, owner: StemGenerationInterests) -> bool:
        """Project actual worker entry separately from requested or queued ownership."""
        return (
            any(
                job.running and any(row.interest_owner is owner for row in job.subscribers())
                for job in self._jobs.values()
            )
            or any(
                request.interest_owner is owner and request.started.is_set()
                for request in self._pair_preparations.pending.values()
            )
            or any(
                row.request.interest_owner is owner
                for row in self._disk_selection_settlements.values()
            )
            or any(row.interest_owner is owner for row in self._pending_stem_publications.values())
        )

    def request_generation_interest(
        self, sample_id: int, interest: StemGenerationInterest
    ) -> StemGenerationInterests | None:
        """Join one current publication route or admit the ordinary source-leased job."""
        validate_sample_id(sample_id)
        self._ensure_pad_edit_allowed(sample_id)
        if self._stem_worker_pool is None:
            self._assets.sync_assignments()
        self._expire_generation_interests()
        self._expire_requested_generations()
        joined = self._join_generation_interest(sample_id, interest)
        if joined is not None:
            return joined
        self._clear_stem_generation_messages(sample_id)

        blocker = self.stem_generation_block_reason(sample_id)
        if blocker is not None:
            if self._stem_generation_blocker(sample_id) is None and self._verified_set_current(
                sample_id
            ):
                self._session.stem_generation_diagnostics[sample_id] = blocker
                owner = StemGenerationInterests(frozenset({interest}))
                owner.mark_already_present()
                return owner
            self._session.stem_generation_errors[sample_id] = blocker
            return None
        return self._start_generation_interest(sample_id, interest)

    def _start_generation_interest(
        self, sample_id: int, interest: StemGenerationInterest
    ) -> StemGenerationInterests | None:
        source_version = self._loaded_source_version_for_pad(sample_id)
        if source_version is None:
            self._session.stem_generation_errors[sample_id] = (
                "Cannot generate stems because the source file is missing"
            )
            return None

        if self._stem_worker_pool is not None:
            return self._start_requested_generation(sample_id, source_version, interest)
        self._session.stem_generating_sample_ids.add(sample_id)
        self._session.stem_generation_source_versions[sample_id] = source_version
        self._session.stem_generation_execution.pop(sample_id, None)
        request = self._generation_request(sample_id, source_version)
        if request is None:
            return None

        owner = StemGenerationInterests(frozenset({interest}))
        try:
            source = self._capture_offline_source(sample_id, source_version)
            if self._admit_stem_generation(request, source, interest_owner=owner):
                return owner
        except (AttributeError, OSError, RuntimeError, TypeError, ValueError) as err:
            self._clear_stem_generation_state(sample_id)
            self._session.stem_generation_errors[sample_id] = f"Stem admission failed: {err}"
        return None

    def _start_requested_generation(
        self, sample_id: int, source_version: str, interest: StemGenerationInterest
    ) -> StemGenerationInterests | None:
        capture = self._capture_generation_item(sample_id)
        if capture is None or capture.unavailable_reason is not None:
            self._session.stem_generation_errors[sample_id] = (
                capture.unavailable_reason if capture is not None else None
            ) or "Loaded source identity unavailable"
            return None
        request = self._generation_request(sample_id, source_version, capture=capture)
        if request is None:
            return None
        owner = StemGenerationInterests(frozenset({interest}))
        return owner if self._request_captured_generation(request, owner, capture=capture) else None

    def _join_generation_interest(
        self, sample_id: int, interest: StemGenerationInterest
    ) -> StemGenerationInterests | None:
        if (
            self._shutting_down
            or sample_id in self._session.loading_sample_ids
            or sample_id in self._session.analyzing_sample_ids
        ):
            return None
        requested = self._join_requested_generation(sample_id, interest)
        if requested is not None:
            return requested
        joined = self._join_job_generation(sample_id, interest)
        if joined is not None:
            return joined
        request = self._pair_preparations.pending.get(sample_id)
        if (
            request is not None
            and request.interest_owner is not None
            and request.interest_owner.current
            and self._pair_preparation_current(request)
        ):
            request.interest_owner.attach(interest)
            return request.interest_owner
        settlement = self._disk_selection_settlements.get(sample_id)
        if (
            settlement is not None
            and settlement.request.interest_owner is not None
            and settlement.request.interest_owner.committed
            and self._disk_selection_assignment_current(settlement)
        ):
            settlement.request.interest_owner.attach(interest)
            return settlement.request.interest_owner
        return None

    def _join_job_generation(
        self, sample_id: int, interest: StemGenerationInterest
    ) -> StemGenerationInterests | None:
        for job in self._jobs.values():
            for row in job.subscribers():
                if row.sample_id != sample_id or not self._generation_subscriber_current(row, job):
                    continue
                try:
                    job.add_interest(row, interest)
                except RuntimeError:
                    if not self._retained_preparation_interest(job, row):
                        continue  # Generic terminal jobs have no remaining delivery route.
                    row.interest_owner.attach(interest)
                return row.interest_owner
        return None

    def _retained_preparation_interest(
        self, job: StemGenerationJob, subscriber: StemSubscriber
    ) -> bool:
        return any(
            any(self._same_generation_interest(subscriber, own) for own in delivery.remaining)
            for delivery in self._pair_held_generation_events.values()
            if delivery.job is job
            and (delivery.event.event_type == "success" or delivery.admission_route == "reuse")
        )

    def _join_requested_generation(
        self, sample_id: int, interest: StemGenerationInterest
    ) -> StemGenerationInterests | None:
        requested = self._requested_generations.get(sample_id)
        if requested is not None and self._requested_generation_current(requested):
            requested.owner.attach(interest)
            return requested.owner
        preparation = self._requested_preparations.get(sample_id)
        if preparation is not None and self._requested_preparation_current(preparation.spec):
            binding = preparation.spec.binding
            if binding is not None and binding.interest_owner is not None:
                binding.interest_owner.attach(interest)
                return binding.interest_owner
        return None

    def _generation_subscriber_current(
        self, subscriber: StemSubscriber, job: StemGenerationJob
    ) -> bool:
        request = subscriber.request or job.request
        return self._is_current_generation(
            _StemBackendEvent(
                subscriber.sample_id,
                request.source_version,
                subscriber.source,
                job.request.cache_dir,
                "progress",
                content_id=subscriber.content_id,
                generation_intent=subscriber.intent,
                source_path=request.source_path,
            )
        )

    def cancel_generation_interest(
        self, owner: StemGenerationInterests, interest: StemGenerationInterest
    ) -> StemGenerationCancellation:
        """Detach only this caller, retaining exact readers until actual cancellation settles."""
        disposition = owner.detach(interest)
        if disposition != "detached" or owner.committed:
            return StemGenerationCancellation(disposition, owner)
        self._withdraw_requested_interest(owner)
        readers, deferred_jobs = self._cancel_generation_jobs(owner)
        requests = {
            id(request): request
            for request in self._pair_preparations.pending.values()
            if request.interest_owner is owner
        }
        requests.update(
            (id(row.request), row.request)
            for row in self._disk_selection_settlements.values()
            if row.request.interest_owner is owner
        )
        self._cancel_uninterested_preparations()
        return StemGenerationCancellation(
            disposition, owner, readers, tuple(requests.values()), deferred_jobs
        )

    def _cancel_generation_jobs(
        self, owner: StemGenerationInterests
    ) -> tuple[tuple[StemGenerationJob, ...], tuple[StemGenerationJob, ...]]:
        readers: list[StemGenerationJob] = []
        deferred_jobs: list[StemGenerationJob] = []
        deferred = getattr(self, "_pair_deferred_sample_ids", ())
        for path, job in tuple(self._jobs.items()):
            matched = False
            for row in job.subscribers():
                if row.interest_owner is not owner:
                    continue
                matched = True
                if row.sample_id in deferred:
                    if job not in deferred_jobs:
                        deferred_jobs.append(job)
                    continue
                job.remove_subscriber(row.sample_id)
                if self._generation_sources.get(row.sample_id) is row.source:
                    self._clear_stem_generation_state(row.sample_id)
            if matched and job not in deferred_jobs and job.cancel_event.is_set():
                if job.running:
                    readers.append(job)
                elif path not in self._inference_jobs:
                    self._discard_generation_artifacts(job.request.sample_id, path)
        return tuple(readers), tuple(deferred_jobs)

    def _cancel_uninterested_preparations(self) -> None:
        deferred = getattr(self, "_pair_deferred_sample_ids", ())
        # Pending worker rows retain their old sample_id until normal collection;
        # the current container key owns their actual arrangement placement.
        pending = tuple(self._pair_preparations.pending.items())
        settlements = tuple(
            (sample_id, row.request) for sample_id, row in self._disk_selection_settlements.items()
        )
        for sample_id, request in pending + settlements:
            if sample_id not in deferred and not _generation_interest_allows_preparation(
                request.interest_owner
            ):
                request.cancelled.set()

    def generation_cancellation_is_pending(self, cancellation: StemGenerationCancellation) -> bool:
        """Observe only this cancellation's original physical reads and returned-result route."""
        return (
            any(job.running for job in cancellation.jobs)
            or any(
                self._cancelled_preparation_is_pending(request, cancellation.owner)
                for request in cancellation.preparations
            )
            or any(
                (
                    not cancellation.owner.current
                    and any(row.interest_owner is cancellation.owner for row in job.subscribers())
                )
                or (job.cancel_event.is_set() and job.running)
                for job in cancellation.deferred_jobs
            )
        )

    def _cancelled_preparation_is_pending(
        self, request: PairPreparation, owner: StemGenerationInterests
    ) -> bool:
        if owner.current and not request.cancelled.is_set():
            # A new caller may take over still-fenced, uncancelled original work.
            # Its continuing reads do not belong to the detached batch token.
            return False
        return any(
            current is request for current in self._pair_preparations.pending.values()
        ) or any(row.request is request for row in self._disk_selection_settlements.values())

    def generation_interest_is_pending(self, owner: StemGenerationInterests) -> bool:
        """Read existing operation containers without borrowing slot or Native authority."""
        return (
            any(row.owner is owner for row in self._requested_generations.values())
            or any(
                row.spec.binding is not None and row.spec.binding.interest_owner is owner
                for row in self._requested_preparations.values()
            )
            or any(
                row.interest_owner is owner
                and self._generation_sources.get(row.sample_id) is row.source
                for job in self._jobs.values()
                for row in job.subscribers()
            )
            or any(
                request.interest_owner is owner
                for request in self._pair_preparations.pending.values()
            )
            or any(
                row.request.interest_owner is owner
                for row in self._disk_selection_settlements.values()
            )
            or any(row.interest_owner is owner for row in self._pending_stem_publications.values())
        )

    def _generation_request(
        self, sample_id: int, source_version: str, *, capture: StemBatchItem | None = None
    ) -> StemGenerationRequest | None:
        """Capture current transform settings and output shape for the ordinary worker."""
        target_shape = capture.target_shape if capture else self._target_shape_for_pad(sample_id)
        if target_shape is None:
            self._clear_stem_generation_state(sample_id)
            self._session.stem_generation_errors[sample_id] = (
                "Cannot generate stems because the loaded sample shape is unavailable"
            )
            return None

        sample_path = capture.path if capture else self._project.sample_paths[sample_id]
        if sample_path is None:
            self._clear_stem_generation_state(sample_id)
            self._session.stem_generation_errors[sample_id] = (
                "Cannot generate stems without a loaded sample"
            )
            return None

        source_path = Path(sample_path)
        if not source_path.is_absolute():
            source_path = Path.cwd() / source_path

        cache_dir = cache_dir_for_sample_id(sample_id, sample_path)
        separator = capture.processing_signature[0] if capture else self._project.stem_separator
        return StemGenerationRequest(
            sample_id=sample_id,
            source_path=source_path,
            source_version=source_version,
            cache_dir=Path.cwd() / cache_dir / f".generation-{uuid4().hex}",
            target_shape=target_shape,
            model_cache_dir=separator_model_cache_dir(separator),
            separator=separator,
            device_policy="auto",
            demucs_shifts=capture.processing_signature[1]
            if capture and capture.processing_signature[1] is not None
            else self._project.demucs_shifts,
            demucs_overlap=capture.processing_signature[2]
            if capture and capture.processing_signature[2] is not None
            else self._project.demucs_overlap,
        )

    def _next_stem_readiness(self) -> int:
        self._stem_readiness_sequence += 1
        return self._stem_readiness_sequence

    def _capture_generation_item(self, sample_id: int) -> StemBatchItem | None:
        return capture_stem_batch_item(
            self._project,
            sample_id,
            source_identity=lambda pad_id: loaded_source_identity(self._audio, pad_id),
            target_shape=self._target_shape_for_pad,
        )

    def _requested_generation_current(self, requested: _RequestedStemGeneration) -> bool:
        return (
            not self._shutting_down
            and bool(requested.owner.current)
            and self._intent_current(requested.intent)
            and stem_batch_item_is_current(
                self._project,
                requested.capture,
                source_identity=lambda pad_id: loaded_source_identity(self._audio, pad_id),
                target_shape=self._target_shape_for_pad,
            )
        )

    def _request_captured_generation(
        self,
        request: StemGenerationRequest,
        owner: StemGenerationInterests,
        *,
        capture: StemBatchItem,
        generation_intent: StemGenerationIntent | None = None,
        reuse_only: bool = False,
        skip_completed: bool = False,
    ) -> bool:
        if request.sample_id in self._requested_generations:
            # Discovery cannot replace another accepted original request.
            owner.mark_failed("Another original stem request is already pending")
            return False
        requested = _RequestedStemGeneration(
            capture,
            request,
            generation_intent or self._intents.capture(request, capture.processing_revision),
            owner,
            self._next_stem_readiness(),
            reuse_only,
            skip_completed,
        )
        if not self._requested_generation_current(requested):
            owner.mark_failed("Original stem request changed before admission")
            return False
        self._requested_generations[request.sample_id] = requested
        self._session.stem_generation_execution.pop(request.sample_id, None)
        return True

    def _expire_requested_generations(self) -> None:
        deferred = getattr(self, "_pair_deferred_sample_ids", ())
        for sample_id, requested in tuple(self._requested_generations.items()):
            if sample_id in deferred:
                continue
            if not self._requested_generation_current(requested):
                requested.owner.mark_failed("Original stem request changed before admission")
                self._requested_generations.pop(sample_id, None)

    def _withdraw_requested_interest(self, owner: StemGenerationInterests) -> None:
        for sample_id, requested in tuple(self._requested_generations.items()):
            if requested.owner is owner:
                self._requested_generations.pop(sample_id, None)
        for sample_id, preparation in tuple(self._requested_preparations.items()):
            binding = preparation.spec.binding
            if binding is not None and binding.interest_owner is owner:
                self._requested_preparations.pop(sample_id, None)

    @staticmethod
    def _stem_request_class(
        owner: StemGenerationInterests | None,
    ) -> Literal["direct", "batch"]:
        return "direct" if owner is None or DIRECT_STEM_INTEREST in owner.current else "batch"

    def _inference_admissible(self, job: StemGenerationJob) -> bool:
        # Generation has no proved whole-model live deadline bound. Cache lookup
        # and committed-stem preparation keep their existing independent lanes.
        if self._session.active_sample_ids:
            return False
        deferred = getattr(self, "_pair_deferred_sample_ids", ())
        if not any(
            row.sample_id not in deferred
            and _generation_interest_allows_preparation(row.interest_owner)
            and self._intent_current(row.intent)
            for row in job.subscribers()
        ):
            return False
        material = job.intent.material_key if job.intent is not None else None
        return not any(
            active_job.intent is not None and active_job.intent.material_key == material
            for active in self._inference_jobs
            if active in self._jobs
            for active_job in (self._jobs[active],)
        )

    def _stem_preparation_count(self) -> int:
        pending = {id(request) for request in self._pair_preparations.pending.values()}
        settlements = {id(row.request) for row in self._disk_selection_settlements.values()}
        return (
            self._pair_preparations.retained_preparation_count()
            + len(settlements - pending)
            + len(self._pending_stem_publications)
        )

    def _stem_admission_candidates(
        self,
    ) -> tuple[StemAdmissionCandidate[_StemAdmissionOwner], ...]:
        candidates: list[StemAdmissionCandidate[_StemAdmissionOwner]] = []
        deferred = getattr(self, "_pair_deferred_sample_ids", ())
        if len(self._jobs) < STEM_WORKERS + STEM_QUEUED_JOBS:
            candidates.extend(
                StemAdmissionCandidate(
                    row, row.ready_sequence, self._stem_request_class(row.owner), "resolve"
                )
                for sample_id, row in self._requested_generations.items()
                if sample_id not in deferred
            )
        candidates.extend(
            StemAdmissionCandidate(
                job,
                job.admission_ready_sequence,
                "direct"
                if any(DIRECT_STEM_INTEREST in row.interests for row in job.subscribers())
                else "batch",
                "infer",
            )
            for path in self._waiting_jobs
            if (job := self._jobs.get(path)) is not None and self._inference_admissible(job)
        )
        if self._stem_preparation_count() < STEM_WORKERS + STEM_QUEUED_JOBS:
            candidates.extend(
                StemAdmissionCandidate(
                    row,
                    delivery.ready_sequence,
                    self._stem_request_class(row.interest_owner),
                    "prepare",
                )
                for delivery in self._pair_held_generation_events.values()
                if delivery.event.event_type == "success" or delivery.admission_route == "reuse"
                for row in self._available_generation_subscribers(delivery, consume=False)
                if not self._pair_preparations.admission_pending(row.sample_id)
            )
            candidates.extend(
                StemAdmissionCandidate(
                    row,
                    row.ready_sequence,
                    self._stem_request_class(
                        row.spec.binding.interest_owner if row.spec.binding else None
                    ),
                    "prepare",
                )
                for sample_id, row in self._requested_preparations.items()
                if sample_id not in deferred
                and not self._pair_preparations.admission_pending(sample_id)
            )
        return tuple(candidates)

    def _poll_stem_admissions(self) -> None:
        if self._session.active_sample_ids:
            self._stem_playback_active.set()
        else:
            self._stem_playback_active.clear()
        pool = self._stem_worker_pool
        if self._shutting_down:
            return
        if pool is None:
            self._start_waiting_generations()
            return
        self._expire_requested_generations()
        deferred = getattr(self, "_pair_deferred_sample_ids", ())
        for sample_id, row in tuple(self._requested_preparations.items()):
            if sample_id not in deferred and not self._requested_preparation_current(row.spec):
                binding = row.spec.binding
                if binding is not None and binding.interest_owner is not None:
                    binding.interest_owner.mark_failed("Original stem preparation changed")
                self._requested_preparations.pop(sample_id, None)
        if not self._stem_admission_candidates():
            return
        resources = self._stem_resources.admission()
        if self._diagnostics is not None:
            self._diagnostics.observe("stem_resources", resources.message)
        if not resources.allowed:
            return
        self._stem_admission.run(
            self._stem_admission_candidates, pool.try_reserve, self._admit_stem_candidate
        )

    def _admit_stem_candidate(
        self,
        candidate: StemAdmissionCandidate[_StemAdmissionOwner],
        reservation: StemWorkerReservation,
    ) -> StemAdmissionResult:
        owner = candidate.owner
        if isinstance(owner, _RequestedStemGeneration):
            return self._admit_requested_generation(owner, reservation)
        if isinstance(owner, _RequestedStemPreparation):
            return self._admit_requested_preparation(owner, reservation)
        if isinstance(owner, StemGenerationJob):
            if (
                self._jobs.get(owner.request.cache_dir) is not owner
                or owner.request.cache_dir not in self._waiting_jobs
                or not self._inference_admissible(owner)
            ):
                return "deferred"
            self._waiting_jobs.discard(owner.request.cache_dir)
            return (
                "submitted"
                if self._schedule_stem_job(owner, reservation=reservation)
                else "settled"
            )
        return self._admit_generation_preparation(owner, reservation)

    def _admit_requested_generation(
        self, requested: _RequestedStemGeneration, reservation: StemWorkerReservation
    ) -> StemAdmissionResult:
        sample_id = requested.request.sample_id
        if self._requested_generations.get(sample_id) is not requested:
            return "settled"
        if sample_id in getattr(self, "_pair_deferred_sample_ids", ()):
            return "deferred"
        if not self._requested_generation_current(requested):
            requested.owner.mark_failed("Original stem request changed before admission")
            self._requested_generations.pop(sample_id, None)
            return "settled"
        if len(self._jobs) >= STEM_WORKERS + STEM_QUEUED_JOBS:
            return "deferred"
        self._requested_generations.pop(sample_id, None)
        try:
            self._assets.sync_assignments()
            source = self._capture_requested_source(requested)
            accepted = self._admit_stem_generation(
                requested.request,
                source,
                reuse_only=requested.reuse_only,
                skip_completed=requested.skip_completed,
                interest_owner=requested.owner,
                generation_intent=requested.intent,
                reservation=reservation,
            )
        except (AttributeError, OSError, RuntimeError, TypeError, ValueError) as error:
            requested.owner.mark_failed(str(error))
            self._session.stem_generation_errors[sample_id] = f"Stem admission failed: {error}"
            return "settled"
        return "submitted" if accepted else "settled"

    def _capture_requested_source(
        self, requested: _RequestedStemGeneration
    ) -> OfflineStemSourceLease:
        sample_id = requested.request.sample_id
        source = self._capture_offline_source(sample_id, requested.request.source_version)
        identity = requested.capture.source_identity
        if (
            identity is None
            or source.source_generation != identity[0]
            or not self._audio.offline_stem_source_is_current(sample_id, source)
        ):
            message = "Original loaded source changed during stem admission"
            raise RuntimeError(message)
        return source

    def _admit_requested_preparation(
        self, requested: _RequestedStemPreparation, reservation: StemWorkerReservation
    ) -> StemAdmissionResult:
        spec = requested.spec
        if self._requested_preparations.get(spec.sample_id) is not requested:
            return "settled"
        if spec.sample_id in getattr(self, "_pair_deferred_sample_ids", ()):
            return "deferred"
        if not self._requested_preparation_current(spec) or not self._requested_pair_ticket_current(
            spec
        ):
            binding = spec.binding
            if binding is not None and binding.interest_owner is not None:
                binding.interest_owner.mark_failed("Original stem preparation changed")
            self._requested_preparations.pop(spec.sample_id, None)
            return "settled"
        if (
            self._pair_preparations.admission_pending(spec.sample_id)
            or self._stem_preparation_count() >= STEM_WORKERS + STEM_QUEUED_JOBS
        ):
            return "deferred"
        try:
            self._pair_preparations.submit_reserved(spec, reservation)
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            binding = spec.binding
            if binding is not None and binding.interest_owner is not None:
                binding.interest_owner.mark_failed(str(error))
            self._session.stem_generation_errors[spec.sample_id] = (
                f"Stem preparation admission failed: {error}"
            )
            self._requested_preparations.pop(spec.sample_id, None)
            return "settled"
        self._requested_preparations.pop(spec.sample_id, None)
        return "submitted"

    def _requested_preparation_current(self, spec: StemPairPreparationSpec) -> bool:
        binding = spec.binding or StemPairIntentBinding()
        return (
            not self._shutting_down
            and _generation_interest_allows_preparation(binding.interest_owner)
            and self._intent_current(binding.generation_intent)
            and self._content_id(spec.sample_id) == spec.content_id
            and self._project.sample_paths[spec.sample_id] == binding.source_path
            and self._project.stem_cache[spec.sample_id] is spec.previous_entry
        )

    def _requested_pair_ticket_current(self, spec: StemPairPreparationSpec) -> bool:
        binding = spec.binding or StemPairIntentBinding()
        try:
            current = self._capture_source_ticket(
                spec.sample_id, binding.own_source_version or spec.entry.source_version
            )
            return spec.ticket.same_source_request(current)
        except OSError, RuntimeError, TypeError, ValueError:
            return False

    def _admit_generation_preparation(
        self, subscriber: StemSubscriber, reservation: StemWorkerReservation
    ) -> StemAdmissionResult:
        delivery = next(
            (
                row
                for row in self._pair_held_generation_events.values()
                if (row.event.event_type == "success" or row.admission_route == "reuse")
                and any(self._same_generation_interest(subscriber, own) for own in row.remaining)
            ),
            None,
        )
        if delivery is None:
            return "settled"
        sample_id = subscriber.sample_id
        if sample_id in getattr(self, "_pair_deferred_sample_ids", ()):
            return "deferred"
        if (
            self._pair_preparations.admission_pending(sample_id)
            or self._stem_preparation_count() >= STEM_WORKERS + STEM_QUEUED_JOBS
        ):
            return "deferred"
        submitted = False
        try:
            if self._generation_subscriber_current(subscriber, delivery.job):
                with subscriber.retirement.activate():
                    submitted = self._submit_generation_preparation(
                        delivery, subscriber, reservation
                    )
            else:
                subscriber.interest_owner.mark_failed(
                    "Original stem subscriber changed before preparation"
                )
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            subscriber.interest_owner.mark_failed(str(error))
            if self._generation_sources.get(sample_id) is subscriber.source:
                self._session.stem_generation_errors[sample_id] = (
                    f"Stem generation completed but publication failed: {error}"
                )
        self._consume_generation_subscribers(delivery, (subscriber,))
        subscriber.retirement.close()
        self._finish_generation_delivery(delivery, (subscriber,))
        if not delivery.remaining:
            self._pair_held_generation_events.pop(self._generation_delivery_key(delivery), None)
        return "submitted" if submitted else "settled"

    def _submit_generation_preparation(
        self,
        delivery: _StemJobEventDelivery,
        subscriber: StemSubscriber,
        reservation: StemWorkerReservation,
    ) -> bool:
        job = delivery.job
        own = subscriber.request or job.request
        if delivery.admission_route == "reuse":
            assert delivery.promotion is not None
            assert job.resolved_request is not None
            processing = replace(
                job.resolved_request,
                sample_id=subscriber.sample_id,
                source_path=own.source_path,
                source_version=own.source_version,
            )
            entry = delivery.promotion.entry
            self._remember_generation_candidate(subscriber, processing, job)
        else:
            processing = None
            self._record_generation_result(subscriber.sample_id, delivery.event.result)
            if not delivery.promotion_attempted:
                delivery.promotion_attempted = True
                eligible = self._eligible_generation_entry(subscriber.sample_id, own.source_version)
                if eligible is None:
                    message = "Original stem source changed before artifact promotion"
                    raise RuntimeError(message)
                with job.retirement.activate():
                    delivery.promotion = self._promote_generation_cache(
                        subscriber.sample_id, eligible, delivery.event.cache_dir, job
                    )
                delivery.promotion_error = self._session.stem_generation_errors.get(
                    subscriber.sample_id
                )
            if delivery.promotion is None:
                raise RuntimeError(delivery.promotion_error or "Stem cache promotion failed")
            entry = delivery.promotion.entry
            self._remember_generation_candidate(subscriber, own, job)
        accepted = self._prepare_pair(
            subscriber.sample_id,
            entry.model_copy(deep=True),
            self._capture_generation_ticket(subscriber),
            processing_request=processing,
            regenerate_on_incompatible=processing is not None
            and job.request.cache_dir not in self._reuse_only_jobs,
            binding=StemPairIntentBinding(
                generation_intent=subscriber.intent,
                own_source_version=own.source_version,
                artifact_source_version=entry.source_version,
                interest_owner=subscriber.interest_owner,
            ),
            reservation=reservation,
        )
        if accepted:
            delivery.promotion.queued = True
        return accepted

    def _admit_stem_generation(
        self,
        request: StemGenerationRequest,
        source_lease: OfflineStemSourceLease,
        *,
        reuse_only: bool = False,
        skip_completed: bool = False,
        interest_owner: StemGenerationInterests | None = None,
        generation_intent: StemGenerationIntent | None = None,
        reservation: StemWorkerReservation | None = None,
    ) -> bool:
        """Resolve complete producer identity before the control owner admits inference."""
        sample_id = request.sample_id
        content = self._project.pad_content[sample_id]
        content_id = content.instance_id if content is not None else None
        intent = generation_intent or self._intents.capture(
            request, self._project.stem_processing_revision
        )
        job = StemGenerationJob(
            request,
            source_lease,
            self._assets,
            content_id,
            intent=intent,
            interest_owner=interest_owner,
        )
        self._session.stem_generating_sample_ids.add(sample_id)
        self._session.stem_generation_source_versions[sample_id] = request.source_version
        self._session.stem_generation_execution.pop(sample_id, None)
        self._generation_sources[sample_id] = source_lease
        self._generation_intents[sample_id] = intent
        self._jobs[job.request.cache_dir] = job
        if reuse_only:
            self._reuse_only_jobs.add(job.request.cache_dir)
        if skip_completed:
            self._skip_completed_jobs.add(job.request.cache_dir)

        if not reuse_only and self._project.stem_cache[sample_id] is None:
            cache_dir = request.cache_dir.parent.relative_to(Path.cwd()).as_posix()
            self._project.stem_cache[sample_id] = StemCacheEntry(
                source_version=request.source_version,
                cache_dir=cache_dir,
                stems=expected_stem_files(cache_dir),
                available=False,
            )
            self._mark_project_changed()
        return self._schedule_stem_job(job, resolve=True, reservation=reservation)

    def _compatible_completed_entry(
        self, request: StemGenerationRequest, producer_identity: str | None
    ) -> StemCacheEntry | None:
        fingerprint = request_fingerprint(request, producer_identity)
        if fingerprint is None:
            return None
        return next(
            (
                entry.model_copy(deep=True)
                for entry in self._completed_generation_entries()
                if entry.fingerprint_schema == STEM_FINGERPRINT_SCHEMA
                and entry.generation_fingerprint == fingerprint
                and entry.producer_identity == producer_identity
            ),
            None,
        )

    def _completed_generation_entries(self) -> Iterator[StemCacheEntry]:
        """Discover existing complete owners before their Config selection is visible.

        These are hints for ordinary reuse admission, which acquires its own lease
        and freshly verifies the producer and artifacts. Metadata alone never lends
        custody, and a consumed canonical job remains consumed.
        """
        for entry in self._project.stem_cache:
            if entry is not None and (entry.available or entry.pair is not None):
                yield entry
        for delivery in self._pair_held_generation_events.values():
            promotion = delivery.promotion
            if promotion is not None and not promotion.released and not promotion.lease.released:
                yield promotion.entry
        for request in self._pair_preparations.pending.values():
            if (
                not request.cancelled.is_set()
                and request.error is None
                and not request.lease.released
            ):
                yield request.entry
        for settlement in self._disk_selection_settlements.values():
            request = settlement.request
            if (
                not request.cancelled.is_set()
                and request.error is None
                and not settlement.lease.released
            ):
                yield settlement.entry

    def reuse_compatible_stems_on_load(self, sample_id: int) -> bool:
        """Discover saved canonical users without hashing sources or models on the UI thread."""
        sample_path = self._project.sample_paths[sample_id]
        shape = self._target_shape_for_pad(sample_id)
        if sample_path is None or shape is None:
            return False
        source = original_asset(sample_path)
        if source.layout != "readable":
            return False
        for entry in self._project.stem_cache:
            if (
                entry is None
                or entry.fingerprint_schema != STEM_FINGERPRINT_SCHEMA
                or entry.generation_fingerprint is None
                or entry.producer_identity is None
            ):
                continue
            reference, separator, _digest = entry.source_version.rpartition("|sha256-v1:")
            if not separator:
                continue
            try:
                if original_asset(reference).path != source.path:
                    continue
                request = StemGenerationRequest(
                    sample_id=sample_id,
                    source_path=source.path,
                    source_version=entry.source_version,
                    cache_dir=Path.cwd() / cache_dir_for_sample_id(sample_id, sample_path),
                    target_shape=shape,
                    model_cache_dir=separator_model_cache_dir(self._project.stem_separator),
                    separator=self._project.stem_separator,
                    demucs_shifts=self._project.demucs_shifts,
                    demucs_overlap=self._project.demucs_overlap,
                )
                # Discovery is a hint. Full producer capture and the authoritative
                # equality decision still occur in the ordinary bounded lane.
                request = replace(
                    request,
                    cache_dir=request.cache_dir / f".generation-{uuid4().hex}",
                )
                return self._admit_discovered_reuse(request)
            except (OSError, RuntimeError, TypeError, ValueError) as error:
                self._session.stem_generation_errors[sample_id] = (
                    f"Compatible stem reuse failed: {error}"
                )
                return False
        return False

    def _admit_discovered_reuse(self, request: StemGenerationRequest) -> bool:
        if self._stem_worker_pool is not None:
            capture = self._capture_generation_item(request.sample_id)
            if capture is None or capture.unavailable_reason is not None:
                return False
            return self._request_captured_generation(
                request, StemGenerationInterests(), capture=capture, reuse_only=True
            )
        offline_source = self._capture_offline_source(request.sample_id, request.source_version)
        return self._admit_stem_generation(request, offline_source, reuse_only=True)

    def _schedule_stem_job(
        self,
        job: StemGenerationJob,
        *,
        resolve: bool = False,
        reservation: StemWorkerReservation | None = None,
    ) -> bool:
        try:
            if not resolve:
                self._inference_jobs.add(job.request.cache_dir)
            runner = reservation.submit if reservation is not None else self._stem_task_runner
            runner(
                lambda: self._resolve_stem_identity(job) if resolve else self._run_stem_backend(job)
            )
        except (OSError, RuntimeError) as error:
            self._inference_jobs.discard(job.request.cache_dir)
            self._waiting_jobs.discard(job.request.cache_dir)
            for subscriber in job.subscribers():
                subscriber.interest_owner.mark_failed(str(error))
                sample_id = subscriber.sample_id
                if self._generation_sources.get(sample_id) is subscriber.source:
                    self._cancel_jobs(sample_id)
                    self._clear_stem_generation_state(sample_id)
                    self._session.stem_generation_errors[sample_id] = (
                        f"Stem worker admission failed: {error}"
                    )
            self._discard_generation_artifacts(job.request.sample_id, job.request.cache_dir)
            return False
        return True

    def restore_stem_cache_from_project_state(self) -> None:
        """Validate restored stem cache metadata against current project-local files."""
        self._restored_stem_candidates.clear()
        changed = False
        for sample_id, entry in enumerate(self._project.stem_cache):
            if sample_id in self._fenced_pair_ids:
                continue
            if entry is None:
                continue

            source_version = self.source_version_for_pad(sample_id)
            if (
                source_version is None
                or source_version != entry.source_version
                or (
                    entry.pair is None
                    and not cache_dir_matches_sample_id(
                        sample_id, entry.cache_dir, self._project.sample_paths[sample_id]
                    )
                )
            ):
                if self._restored_assignment_held(sample_id):
                    self._session.stem_generation_errors[sample_id] = (
                        "Saved stem assignment retained by transaction recovery"
                    )
                    continue
                self._project.stem_cache[sample_id] = None
                changed = True
                continue

            if entry.available:
                self._project.stem_cache[sample_id] = entry.model_copy(update={"available": False})
                changed = True
            current = self._project.stem_cache[sample_id]
            if current is not None and self._entry_files_available(current):
                self._restored_stem_candidates[sample_id] = current
            else:
                self._restored_stem_candidates.pop(sample_id, None)

        if changed:
            self._mark_project_changed()
        self._assets.sync_assignments()

    def fence_pair_metadata(self, sample_ids: set[int]) -> None:
        """Preserve unsupported saved entries without inventing runtime eligibility."""
        self._fenced_pair_ids = set(sample_ids)
        for sample_id in sample_ids:
            self._session.stem_generation_errors[sample_id] = (
                "Unsupported stem pair metadata retained on disk"
            )

    def publish_restored_stem_cache_if_available(self, sample_id: int) -> bool:
        """Verify saved disk artifacts after source ACK without inferring component demand."""
        validate_sample_id(sample_id)
        if sample_id in self._fenced_pair_ids:
            message = "Unsupported stem pair metadata requires explicit recovery"
            self._session.stem_generation_errors[sample_id] = message
            return False
        if callable(getattr(self._audio, "prepare_stem_pair", None)):
            entry = self._project.stem_cache[sample_id]
            if entry is None:
                return True
            if not entry.available and self._restored_stem_candidates.get(sample_id) is not entry:
                return True
            demand = self._remember_demand(sample_id, entry)
            demand.verify_disk = not entry.available or not self._committed_residency.contains(
                sample_id
            )
            demand.policy = self._project.preload_stem_loops_on_startup
            if demand.state != "preparing":
                demand.state = "queued"
                demand.error = None
            return True
        return self._publish_restored_stem_cache(sample_id)

    def _publish_restored_stem_cache(self, sample_id: int) -> bool:
        entry = self._project.stem_cache[sample_id]
        if entry is None or (
            not entry.available and self._restored_stem_candidates.get(sample_id) is not entry
        ):
            return True
        if sample_id in self._pending_stem_publications:
            return True

        source_version = self.source_version_for_pad(sample_id)
        if source_version is None or source_version != entry.source_version:
            return self._settle_mismatched_restored_cache(sample_id)

        if not self._entry_files_available(entry):
            self._project.stem_cache[sample_id] = entry.model_copy(update={"available": False})
            self._mark_project_changed()
            return True

        try:
            source_ticket = self._capture_source_ticket(sample_id, source_version)
            return self._queue_stem_publication(sample_id, entry, source_ticket)
        except (AttributeError, RuntimeError, TypeError, ValueError) as err:
            self._project.stem_cache[sample_id] = entry.model_copy(update={"available": False})
            self._session.stem_generation_errors[sample_id] = (
                f"Restored stem publication failed: {err}"
            )
            self._mark_project_changed()
            return False

    def _settle_mismatched_restored_cache(self, sample_id: int) -> bool:
        if self._restored_assignment_held(sample_id):
            self._session.stem_generation_errors[sample_id] = (
                "Saved stem assignment retained by transaction recovery"
            )
            return False
        self._project.stem_cache[sample_id] = None
        self._mark_project_changed()
        self._assets.sync_assignments()
        return True

    def invalidate_stem_cache(self, sample_id: int) -> None:
        """Mark cached stem metadata unavailable for a pad."""
        validate_sample_id(sample_id)
        with self._assets.admission():
            self._invalidate_stem_cache(sample_id)

    def _invalidate_stem_cache(self, sample_id: int) -> None:
        self._assets.sync_assignments()
        self._committed_residency.cancel(sample_id)
        self._demands.pop(sample_id, None)
        self._cancel_jobs(sample_id)
        self._pair_preparations.cancel(sample_id)
        self._resident_pairs.discard(sample_id)
        self._clear_stem_generation_state(sample_id)
        self._clear_stem_generation_messages(sample_id)

        changed = False
        if self._project.stem_cache[sample_id] is not None:
            self._project.stem_cache[sample_id] = None
            changed = True
        if self._project.pad_stem_mix_mode[sample_id] != "full_mix":
            self._project.pad_stem_mix_mode[sample_id] = "full_mix"
            changed = True
        if changed:
            self._mark_project_changed()
        self._assets.sync_assignments()

    def stem_mix_mode(self, sample_id: int) -> StemMixMode:
        """Return the durable stem mix preference for a pad."""
        validate_sample_id(sample_id)
        return self._project.pad_stem_mix_mode[sample_id]

    def stem_residency_status(self, sample_id: int) -> StemResidencyStatus | None:
        """Return own disk/residency/effective receipts independently from saved mode."""
        validate_sample_id(sample_id)
        try:
            status = self._stem_residency_projection(sample_id)
        except (RuntimeError, TypeError, ValueError) as error:
            self._report_residency_error(sample_id, f"Stem residency status failed: {error}")
            return None
        return status

    def _stem_residency_projection(self, sample_id: int) -> StemResidencyStatus | None:
        status = self._committed_residency.status(sample_id)
        demand = self._demands.get(sample_id)
        if status is not None and demand is not None:
            return replace(status, demand=demand.state, demand_error=demand.error)
        if status is None and demand is not None:
            return StemResidencyStatus(
                "pending",
                None,
                None,
                None,
                desired_mode=self._project.pad_stem_mix_mode[sample_id],
                demand=demand.state,
                demand_error=demand.error,
            )
        return status

    def can_request_all_stems(self, sample_id: int) -> bool:
        """Allow fresh pair verification without claiming current audio readiness."""
        validate_sample_id(sample_id)
        if self._project.sample_paths[sample_id] is None:
            return False
        entry = self._project.stem_cache[sample_id]
        return self.stems_available(sample_id) or (
            entry is not None and entry.pair is not None and self._committed_residency.supported
        )

    def stems_available(self, sample_id: int) -> bool:
        """Return whether current stem cache metadata is available for UI controls."""
        validate_sample_id(sample_id)
        entry = self._project.stem_cache[sample_id]
        if entry is None:
            return False
        if entry.available:
            return True
        try:
            return self._committed_residency.registered_selection_current(
                sample_id, entry
            ) or self._committed_residency.component_residency_current(sample_id)
        except RuntimeError, TypeError, ValueError:
            return False

    def has_stem_cache(self, sample_id: int) -> bool:
        """Return whether a pad has any tracked stem cache metadata."""
        validate_sample_id(sample_id)
        return self._project.stem_cache[sample_id] is not None

    def stem_enabled_mask(self, sample_id: int) -> int:
        """Return the durable requested component-stem mask for a pad."""
        validate_sample_id(sample_id)
        return self._project.pad_stem_intent[sample_id].enabled_mask

    def remembered_custom_mask(self, sample_id: int) -> int:
        """Return the durable custom mask retained across preset choices."""
        validate_sample_id(sample_id)
        return self._project.pad_stem_intent[sample_id].last_custom_mask

    def stem_mask_display_mode(self, sample_id: int) -> StemMaskDisplayMode:
        """Return the durable requested bottom-bar stem mask display mode."""
        validate_sample_id(sample_id)
        return self._project.pad_stem_intent[sample_id].display_mode

    def stem_mask_controls_enabled(self, sample_id: int) -> bool:
        """Return whether selected-pad per-stem mask controls should be interactive."""
        validate_sample_id(sample_id)
        if self._project.pad_stem_mix_mode[sample_id] != "all_stems":
            return False
        return self.stems_available(sample_id)

    def stem_grid_indicator_state(self, sample_id: int) -> StemGridIndicatorState | None:
        """Return the compact stem status shown on a performance pad."""
        validate_sample_id(sample_id)
        if self._session.stem_generation_errors.get(sample_id):
            return "error"
        if self.is_stem_generation_running(sample_id):
            return "generating"
        if self.stems_available(sample_id):
            return "available"
        if self._project.sample_paths[sample_id] is not None and self._stem_generation_blocker(
            sample_id
        ):
            return "blocked"
        return None

    def set_stem_mix_mode(self, sample_id: int, mode: StemMixMode) -> bool:
        """Set the durable full-mix/all-stems mode for a pad."""
        validate_sample_id(sample_id)
        self._ensure_pad_edit_allowed(sample_id)
        if mode not in STEM_MIX_MODES:
            msg = "stem mix mode must be full_mix or all_stems"
            raise ValueError(msg)

        if mode == "full_mix":
            return self._set_full_mix(sample_id)
        return self._set_all_stems(sample_id)

    def _set_all_stems(self, sample_id: int) -> bool:
        if sample_id in self._publication_retries.pending:
            self._project.pad_stem_mix_mode[sample_id] = "all_stems"
            self._mark_project_changed()
            return True

        entry = self._project.stem_cache[sample_id]
        if (
            entry is not None
            and entry.pair is not None
            and (
                self._committed_residency.contains(sample_id)
                or (not entry.available and self.can_request_all_stems(sample_id))
            )
        ):
            # Revoked restored registrations leave disk metadata unavailable.
            # Only fresh source capture and worker verification can restore authority.
            return self._request_pair_mode(sample_id, entry, entry.source_version)

        source_version = self._current_prepared_source_version(sample_id)
        if source_version is None:
            return False

        if (
            entry is not None
            and entry.pair is not None
            and (self._committed_residency.supported or sample_id not in self._resident_pairs)
        ):
            return self._request_pair_mode(sample_id, entry, source_version)

        if not self._publish_all_stems_state(sample_id, source_version):
            return False

        if self._project.pad_stem_mix_mode[sample_id] != "all_stems":
            self._project.pad_stem_mix_mode[sample_id] = "all_stems"
            self._mark_project_changed()

        return True

    def _set_full_mix(self, sample_id: int) -> bool:
        demand = self._demands.get(sample_id)
        if demand is not None:
            demand.explicit = False
        had_committed_selection = self._committed_residency.contains(sample_id)
        if had_committed_selection:
            if self._committed_residency.request(
                sample_id, "full_mix", self._project.pad_stem_intent[sample_id].enabled_mask
            ):
                self._pending_full_mix_releases.add(sample_id)
                self._project.pad_stem_mix_mode[sample_id] = "full_mix"
                self._resident_pairs.discard(sample_id)
                self._mark_project_changed()
                return True
            # A rejected registration grants no activation rights. The existing
            # ordered FullMix release still withdraws components without new reads.
            self._committed_residency.cancel(sample_id)
            self._pending_full_mix_releases.add(sample_id)
        if (
            not had_committed_selection
            and sample_id not in self._pending_full_mix_releases
            and self._project.pad_stem_mix_mode[sample_id] == "full_mix"
            and sample_id not in self._resident_pairs
        ):
            return True
        try:
            entry = self._project.stem_cache[sample_id]
            if entry is not None and entry.pair is not None:
                self._audio.set_stem_pair_full_mix(sample_id)
                self._pair_release_ordered.add(sample_id)
            else:
                self._audio.set_stem_mix_mode(sample_id, "full_mix")
        except (RuntimeError, ValueError) as error:
            self._session.stem_generation_errors[sample_id] = f"Stem mix update failed: {error}"
            return False
        self._project.pad_stem_mix_mode[sample_id] = "full_mix"
        self._pending_full_mix_releases.discard(sample_id)
        self._resident_pairs.discard(sample_id)
        self._mark_project_changed()
        return True

    def _request_pair_mode(
        self, sample_id: int, entry: StemCacheEntry, source_version: str
    ) -> bool:
        demand = self._remember_demand(sample_id, entry)
        demand.explicit = True
        demand.state = "queued"
        demand.error = None
        handled, ticket = self._request_registered_pair_mode(sample_id, source_version, demand)
        if handled is not None:
            if handled:
                self._save_all_stems_intent(sample_id)
            else:
                demand.state = "error"
                demand.error = self._residency_errors.get(sample_id) or (
                    self._session.stem_generation_errors.get(sample_id)
                )
            return handled
        try:
            if ticket is None:
                ticket = self._capture_source_ticket(sample_id, source_version)
            residency_only = self._committed_residency.supported
            self._prepare_pair(
                sample_id,
                entry,
                ticket,
                components=not residency_only,
                residency_only=residency_only,
            )
        except (OSError, RuntimeError, ValueError) as error:
            self._session.stem_generation_errors[sample_id] = str(error)
            if resident_stem_admission_pressure(str(error)):
                demand.state = self._admission_wait_state(str(error))
                demand.error = str(error)
                self._save_all_stems_intent(sample_id)
                return True
            demand.state = "error"
            demand.error = str(error)
            return False
        self._save_all_stems_intent(sample_id)
        return True

    def _request_registered_pair_mode(
        self, sample_id: int, source_version: str, demand: _StemDemand
    ) -> tuple[bool | None, PreparedSourceTicket | None]:
        if not self._committed_residency.contains(sample_id):
            return None, None
        if self._committed_residency.request(
            sample_id, "all_stems", self._project.pad_stem_intent[sample_id].enabled_mask
        ):
            return True, None
        pressure = self._residency_errors.get(sample_id)
        if resident_stem_admission_pressure(pressure):
            demand.state = self._admission_wait_state(pressure)
            demand.error = pressure
            return True, None
        if self._committed_residency.contains(sample_id):
            return self._capture_pair_retry_ticket(sample_id, source_version)
        # This explicit gesture withdrew terminal registration. Fresh worker
        # preparation still requires its own source and Native ACK.
        return None, None

    def _capture_pair_retry_ticket(
        self, sample_id: int, source_version: str
    ) -> tuple[bool | None, PreparedSourceTicket | None]:
        try:
            ticket = self._capture_source_ticket(sample_id, source_version)
            if not self._committed_residency.requires_current_preparation(sample_id, ticket):
                return False, None
        except (OSError, RuntimeError, ValueError) as error:
            self._session.stem_generation_errors[sample_id] = str(error)
            return False, None
        # A changed sealed preparation context on the same source needs new work.
        return None, ticket

    def _save_all_stems_intent(self, sample_id: int) -> None:
        self._project.pad_stem_mix_mode[sample_id] = "all_stems"
        self._mark_project_changed()

    def delete_stems(self, sample_id: int) -> bool:
        """Retire cached stems after their final reader and return to full mix."""
        validate_sample_id(sample_id)
        self._ensure_pad_edit_allowed(sample_id)
        try:
            with self._assets.admission():
                return self._delete_stems(sample_id)
        except RuntimeError as error:
            self._session.stem_generation_errors[sample_id] = str(error)
            return False

    def delete_captured_stems(self, item: StemDeleteItem) -> StemDeleteOutcome:
        """Revalidate one warning target before ordinary guarded logical withdrawal.

        Logical Removed does not report Native effectiveness or physical cleanup.
        Exact known entries are comparison scope only; existing retirement owners
        continue deciding which files may retire after their actual final readers.

        Args:
            item: The original immutable deletion-warning PAD image.

        Returns:
            The guarded original target's logical result, including fence deferral.
        """
        validate_sample_id(item.pad_id)
        if item.pad_id in getattr(self, "_pair_deferred_sample_ids", ()):
            return StemDeleteOutcome("deferred", "Waiting for PAD arrangement")
        logical_pending = self._delete_logical_interests()
        currentness = stem_delete_item_currentness(
            item,
            read_metadata=lambda sample_id: self._deletion_pad_metadata(sample_id, logical_pending),
        )
        if currentness != "current":
            return StemDeleteOutcome(
                "skipped", item.unavailable_reason or "Original target changed"
            )
        try:
            self._ensure_pad_edit_allowed(item.pad_id)
            if self._deletion_has_no_control_work(item.pad_id):
                return StemDeleteOutcome("noop", "No stems or pending generation to remove")
            with self._assets.admission():
                _changed, outcome = self._delete_stems_with_outcome(item.pad_id)
        except (OSError, RuntimeError, ValueError) as error:
            self._session.stem_generation_errors[item.pad_id] = str(error)
            return StemDeleteOutcome("failed", str(error))
        return outcome

    def _deletion_has_no_control_work(self, sample_id: int) -> bool:
        """Avoid retirement admission only when no original control work can be withdrawn."""
        return (
            self._project.stem_cache[sample_id] is None
            and self._project.pad_stem_mix_mode[sample_id] == "full_mix"
            and not self._assets.has_stem_assignment(sample_id)
            and not self._deletion_has_pending_interest(sample_id)
            and not self._deletion_retained_entries(sample_id)
            and all(intent is None for intent in self._deletion_request_intents(sample_id))
        )

    def _delete_stems(self, sample_id: int) -> bool:
        changed, _outcome = self._delete_stems_with_outcome(sample_id)
        return changed

    def _delete_stems_with_outcome(self, sample_id: int) -> tuple[bool, StemDeleteOutcome]:
        self._assets.sync_assignments()

        entry = self._project.stem_cache[sample_id]
        if (
            self._project.pad_stem_mix_mode[sample_id] != "full_mix"
            or (entry is not None and entry.pair is not None)
            or sample_id in self._pending_full_mix_releases
        ):
            try:
                if entry is not None and entry.pair is not None:
                    self._audio.set_stem_pair_full_mix(sample_id)
                else:
                    self._audio.set_stem_mix_mode(sample_id, "full_mix")
            except (RuntimeError, ValueError) as error:
                self._session.stem_generation_errors[sample_id] = f"Stem mix update failed: {error}"
                return False, StemDeleteOutcome(
                    "failed", self._session.stem_generation_errors[sample_id]
                )

        withdrawn = self._deletion_has_pending_interest(sample_id)
        self._demands.pop(sample_id, None)
        self._committed_residency.cancel(sample_id)
        self._pending_full_mix_releases.discard(sample_id)
        self._cancel_jobs(sample_id)
        self._pair_preparations.cancel(sample_id)
        self._resident_pairs.discard(sample_id)
        self._clear_stem_generation_state(sample_id)
        self._clear_stem_generation_messages(sample_id)

        changed = False
        if entry is not None:
            self._project.stem_cache[sample_id] = None
            changed = True
        if self._project.pad_stem_mix_mode[sample_id] != "full_mix":
            self._project.pad_stem_mix_mode[sample_id] = "full_mix"
            changed = True

        if changed:
            self._mark_project_changed()

        self._assets.sync_assignments()
        outcome = (
            StemDeleteOutcome("removed", "Stem selection or pending generation withdrawn")
            if changed or withdrawn
            else StemDeleteOutcome("noop", "No stems or pending generation to remove")
        )
        return changed, outcome

    def _deletion_has_pending_interest(self, sample_id: int) -> bool:
        request = self._pair_preparations.pending.get(sample_id)
        return (
            sample_id in self._demands
            or sample_id in self._resident_pairs
            or sample_id in self._requested_generations
            or sample_id in self._requested_preparations
            or sample_id in self._generation_sources
            or sample_id in self._pending_stem_publications
            or sample_id in self._restored_stem_candidates
            or sample_id in self._pending_full_mix_releases
            or self._committed_residency.contains(sample_id)
            or (request is not None and not request.cancelled.is_set())
            or any(
                row.sample_id == sample_id
                for job in self._jobs.values()
                for row in job.subscribers()
            )
        )

    def set_stem_enabled_mask(
        self,
        sample_id: int,
        enabled_stem_mask: int,
        display_mode: StemMaskDisplayMode = "custom",
    ) -> bool:
        """Persist one musical mask/preset intent before guarded native publication."""
        validate_sample_id(sample_id)
        self._ensure_pad_edit_allowed(sample_id)
        current = self._project.pad_stem_intent[sample_id]
        intent = transition_stem_intent(current, enabled_stem_mask, display_mode)
        if intent == current:
            return True
        self._project.pad_stem_intent[sample_id] = intent
        self._mark_project_changed()
        entry = self._project.stem_cache[sample_id]
        if (
            self._project.pad_stem_mix_mode[sample_id] == "all_stems"
            and entry is not None
            and entry.pair is not None
            and self.stems_available(sample_id)
            and self._committed_residency.contains(sample_id)
        ):
            source_version = self._current_prepared_source_version(sample_id)
            if source_version is None:
                return False
            return self._request_pair_mode(sample_id, entry, source_version)
        return self.publish_stem_enabled_mask_if_available(sample_id)

    def publish_stem_mix_mode_if_available(self, sample_id: int) -> bool:
        """Publish all-stems mode to Rust when current prepared stems are available."""
        validate_sample_id(sample_id)
        if self._project.pad_stem_mix_mode[sample_id] != "all_stems":
            return True

        entry = self._project.stem_cache[sample_id]
        if entry is None or not self.stems_available(sample_id):
            return True

        source_version = self._current_prepared_source_version(sample_id)
        if source_version is None:
            return False

        return self._publish_stem_mix_mode(sample_id, source_version)

    def publish_stem_enabled_mask_if_available(self, sample_id: int) -> bool:
        """Publish durable mask intent when current prepared stems are available."""
        validate_sample_id(sample_id)
        if self._project.pad_stem_mix_mode[sample_id] != "all_stems":
            return True

        entry = self._project.stem_cache[sample_id]
        if entry is None or not self.stems_available(sample_id):
            return True

        source_version = self._current_prepared_source_version(sample_id)
        if source_version is None:
            return False

        return self._publish_stem_enabled_mask(sample_id, source_version)

    def _publish_all_stems_state(self, sample_id: int, source_version: str) -> bool:
        if not self._publish_stem_mix_mode(sample_id, source_version):
            return False
        return self._publish_stem_enabled_mask(sample_id, source_version)

    def _current_prepared_source_version(self, sample_id: int) -> str | None:
        entry = self._project.stem_cache[sample_id]
        if (
            entry is None
            or not self.stems_available(sample_id)
            or self._project.sample_paths[sample_id] is None
        ):
            return None
        try:
            self._capture_source_ticket(sample_id, entry.source_version)
        except (AttributeError, RuntimeError, TypeError, ValueError) as err:
            self._project.stem_cache[sample_id] = entry.model_copy(update={"available": False})
            self._session.stem_generation_errors[sample_id] = (
                f"Prepared stems no longer match the loaded source: {err}"
            )
            self._mark_project_changed()
            return None
        return entry.source_version

    def _publish_stem_mix_mode(self, sample_id: int, source_version: str) -> bool:
        if self._committed_residency.contains(sample_id):
            return self._committed_residency.request(
                sample_id, "all_stems", self._project.pad_stem_intent[sample_id].enabled_mask
            )
        if self._requires_committed_residency(sample_id):
            return False
        try:
            self._audio.set_stem_mix_mode(sample_id, "all_stems", source_version)
        except (RuntimeError, ValueError) as err:
            self._session.stem_generation_errors[sample_id] = f"Stem mix update failed: {err}"
            return False
        return True

    def _publish_stem_enabled_mask(self, sample_id: int, source_version: str) -> bool:
        if self._committed_residency.contains(sample_id):
            return self._committed_residency.request(
                sample_id, "all_stems", self._project.pad_stem_intent[sample_id].enabled_mask
            )
        if self._requires_committed_residency(sample_id):
            return False
        try:
            self._audio.set_stem_enabled_mask(
                sample_id,
                self._project.pad_stem_intent[sample_id].enabled_mask,
                source_version,
            )
        except (RuntimeError, ValueError) as err:
            self._session.stem_generation_errors[sample_id] = f"Stem mask update failed: {err}"
            return False
        return True

    def source_version_for_pad(self, sample_id: int) -> str | None:
        """Return the current loaded source-version token for a pad."""
        validate_sample_id(sample_id)
        sample_path = self._project.sample_paths[sample_id]
        if sample_path is None:
            return None
        return source_version_for_sample_path(sample_path)

    def _loaded_source_version_for_pad(self, sample_id: int) -> str | None:
        """Use actual Native loaded identity without source I/O on control/render."""
        sample_path = self._project.sample_paths[sample_id]
        if sample_path is None:
            return None
        getter = getattr(self._audio, "loaded_source_digest", None)
        if getter is None:
            return None
        try:
            digest = getter(sample_id)
        except RuntimeError, TypeError, ValueError:
            return None
        if not isinstance(digest, str) or re.fullmatch(r"[0-9a-f]{64}", digest) is None:
            return None
        path = Path(sample_path)
        if path.is_absolute():
            try:
                path = path.relative_to(Path.cwd())
            except ValueError:
                return None
        return f"{path.as_posix()}|sha256-v1:{digest}"

    def is_stem_generation_running(self, sample_id: int) -> bool:
        """Return whether a pad has an in-flight stem generation task."""
        validate_sample_id(sample_id)
        return (
            sample_id in self._requested_generations
            or sample_id in self._session.stem_generating_sample_ids
        )

    def stem_generation_error(self, sample_id: int) -> str | None:
        """Return the last stem generation error for a pad."""
        validate_sample_id(sample_id)
        return self._session.stem_generation_errors.get(sample_id)

    def stem_generation_progress(self, sample_id: int) -> float | None:
        """Return best-effort stem generation progress for a pad."""
        validate_sample_id(sample_id)
        value = self._session.stem_generation_progress.get(sample_id)
        return float(value) if value is not None else None

    def stem_generation_stage(self, sample_id: int) -> str | None:
        """Return the last reported stem generation stage for a pad."""
        validate_sample_id(sample_id)
        if self._session.active_sample_ids and any(
            (job := self._jobs.get(path)) is not None
            and any(row.sample_id == sample_id for row in job.subscribers())
            for path in self._waiting_jobs
        ):
            return "Waiting for playback to stop"
        if sample_id in self._requested_generations:
            resources = self._stem_resources.current
            return resources.message if not resources.allowed and resources.message else "Queued"
        return self._session.stem_generation_stage.get(sample_id)

    def stem_generation_block_reason(self, sample_id: int) -> str | None:
        """Return the current non-I/O blocker for performer stem generation."""
        validate_sample_id(sample_id)
        return self._stem_generation_blocker(sample_id) or (
            "Stems for the current model are already present"
            if self._verified_set_current(sample_id)
            else None
        )

    def stem_execution(self, sample_id: int) -> StemExecution | None:
        """Separate current completed inference from historical cache metadata."""
        validate_sample_id(sample_id)
        if (
            sample_id in self._session.loading_sample_ids
            or sample_id in self._requested_generations
        ):
            return None
        if self.is_stem_generation_running(sample_id):
            return self._session.stem_generation_execution.get(sample_id)
        entry = self._project.stem_cache[sample_id]
        return entry.execution if entry is not None else None

    def stem_generation_diagnostic(self, sample_id: int) -> str | None:
        """Return an admission or execution diagnostic independently of errors."""
        validate_sample_id(sample_id)
        return self._session.stem_generation_diagnostics.get(sample_id)

    def _intent_current(self, intent: StemGenerationIntent | None) -> bool:
        return intent is None or (
            intent.settings_revision == self._project.stem_processing_revision
            and self._intents.is_current(intent)
        )

    def _verified_set_current(self, sample_id: int) -> bool:
        verified = self._verified_sets.get(sample_id)
        entry = self._project.stem_cache[sample_id]
        return (
            verified is not None
            and self._intent_current(verified.intent)
            and entry is not None
            and entry.available
            and entry.pair == verified.entry.pair
            and entry.generation_fingerprint == verified.entry.generation_fingerprint
            and entry.producer_identity == verified.entry.producer_identity
            and self._content_id(sample_id) == verified.content_id
            and self._project.sample_paths[sample_id] == verified.source_path
            and separator_model_cache_dir(self._project.stem_separator)
            == verified.request.model_cache_dir
            and self._target_shape_for_pad(sample_id) == verified.request.target_shape
        )

    def _poll_generation_events(self) -> None:
        """Drain Python backend generation events from background worker threads."""
        self._expire_generation_interests()
        self._cancel_uninterested_preparations()
        self._expire_requested_generations()
        self._prune_verified_sets()
        self._prune_generation_intents()
        self._drain_held_generation_events()
        while True:
            try:
                event = self._stem_generation_events.get_nowait()
            except Empty:
                return
            job = self._jobs.get(event.cache_dir)
            if job is None:
                continue
            self._dispatch_job_event(event, job)

    def _prune_verified_sets(self) -> None:
        for sample_id in tuple(self._verified_sets):
            if sample_id in getattr(self, "_pair_deferred_sample_ids", ()):
                continue
            if not self._verified_set_current(sample_id):
                self._verified_sets.pop(sample_id, None)
        for sample_id, (intent, _request, _guard) in tuple(self._verified_candidates.items()):
            if sample_id in getattr(self, "_pair_deferred_sample_ids", ()):
                continue
            if not self._intent_current(intent):
                self._verified_candidates.pop(sample_id, None)

    def _drain_held_generation_events(self) -> None:
        for delivery in tuple(self._pair_held_generation_events.values()):
            if self._jobs.get(delivery.event.cache_dir) is delivery.job:
                self._dispatch_job_event(delivery.event, delivery.job, delivery=delivery)
            else:
                self._release_generation_promotion(delivery)
                self._pair_held_generation_events.pop(self._generation_delivery_key(delivery), None)

    def _prune_generation_intents(self) -> None:
        live = [
            subscriber.intent
            for job in self._jobs.values()
            for subscriber in job.subscribers()
            if subscriber.intent is not None
        ]
        live.extend(
            request.generation_intent
            for request in self._pair_preparations.pending.values()
            if request.generation_intent is not None
        )
        live.extend(
            pending.generation_intent
            for pending in self._pending_stem_publications.values()
            if pending.generation_intent is not None
        )
        live.extend(verified.intent for verified in self._verified_sets.values())
        live.extend(candidate[0] for candidate in self._verified_candidates.values())
        live.extend(row.intent for row in self._requested_generations.values())
        live.extend(
            row.spec.binding.generation_intent
            for row in self._requested_preparations.values()
            if row.spec.binding is not None and row.spec.binding.generation_intent is not None
        )
        self._intents.prune(live)

    @staticmethod
    def _generation_delivery_key(
        delivery: _StemJobEventDelivery,
    ) -> tuple[StemGenerationJob, str]:
        kind = delivery.event.event_type
        return delivery.job, kind if kind in {"progress", "identity"} else "terminal"

    def _capture_generation_delivery(
        self, event: _StemBackendEvent, job: StemGenerationJob
    ) -> _StemJobEventDelivery | None:
        delivery = _StemJobEventDelivery(
            event, job, job.subscribers(), ready_sequence=self._next_stem_readiness()
        )
        key = self._generation_delivery_key(delivery)
        if (
            event.event_type == "progress"
            and (job, "terminal") in self._pair_held_generation_events
        ):
            return None
        previous = self._pair_held_generation_events.get(key)
        if previous is not None:
            if event.event_type != "progress":
                # Repeated worker delivery cannot replace its exact terminal owner.
                return None
            delivery.event = replace(
                event,
                percent=event.percent if event.percent is not None else previous.event.percent,
                stage=event.stage if event.stage is not None else previous.event.stage,
                execution=event.execution
                if event.execution is not None
                else previous.event.execution,
            )
        if event.event_type not in {"progress", "identity"}:
            self._pair_held_generation_events.pop((job, "progress"), None)
        self._pair_held_generation_events[key] = delivery
        return delivery

    @staticmethod
    def _same_generation_interest(left: StemSubscriber, right: StemSubscriber) -> bool:
        return (
            left.source is right.source
            and left.retirement is right.retirement
            and left.source_lease is right.source_lease
            and left.intent is right.intent
            and left.content_id == right.content_id
        )

    def _available_generation_subscribers(
        self, delivery: _StemJobEventDelivery, *, consume: bool = True
    ) -> tuple[StemSubscriber, ...]:
        deferred = getattr(self, "_pair_deferred_sample_ids", ())
        subscribers = delivery.job.subscribers()
        remaining = tuple(
            subscriber
            for subscriber in subscribers
            if any(self._same_generation_interest(subscriber, own) for own in delivery.remaining)
        )
        delivery.remaining = (
            tuple(subscriber for subscriber in remaining if subscriber.sample_id in deferred)
            if consume
            else remaining
        )
        return tuple(subscriber for subscriber in remaining if subscriber.sample_id not in deferred)

    def _consume_generation_subscribers(
        self, delivery: _StemJobEventDelivery, consumed: tuple[StemSubscriber, ...]
    ) -> None:
        delivery.remaining = tuple(
            row
            for row in delivery.remaining
            if not any(self._same_generation_interest(row, own) for own in consumed)
        )

    def _dispatch_job_event(
        self,
        event: _StemBackendEvent,
        job: StemGenerationJob,
        *,
        delivery: _StemJobEventDelivery | None = None,
    ) -> None:
        delivery = delivery or self._capture_generation_delivery(event, job)
        if delivery is None:
            return
        event = delivery.event
        reserved_delivery = self._stem_worker_pool is not None and event.event_type in {
            "identity",
            "success",
        }
        available = self._available_generation_subscribers(delivery, consume=not reserved_delivery)
        if event.event_type == "identity":
            self._dispatch_resolved_identity(delivery, available)
            return
        current = self._current_generation_subscribers(event, job, available)
        if event.event_type == "success":
            if reserved_delivery:
                self._settle_obsolete_generation_rows(delivery, available, current)
                return
            self._finish_shared_generation(event, current)
        else:
            for subscriber in current:
                if event.event_type == "progress":
                    self._handle_stem_generation_progress(
                        subscriber.sample_id,
                        event.percent,
                        event.stage,
                        subscriber.source,
                        execution=event.execution,
                    )
                elif event.error is not None:
                    subscriber.interest_owner.mark_failed(event.error)
                    self._handle_stem_generation_error(
                        subscriber.sample_id, event.error, subscriber.source
                    )
        if event.event_type != "progress":
            self._finish_generation_delivery(delivery, available)
        if not delivery.remaining:
            self._pair_held_generation_events.pop(self._generation_delivery_key(delivery), None)

    def _settle_obsolete_generation_rows(
        self,
        delivery: _StemJobEventDelivery,
        available: tuple[StemSubscriber, ...],
        current: tuple[StemSubscriber, ...],
    ) -> None:
        obsolete = tuple(row for row in available if row not in current)
        for row in obsolete:
            row.interest_owner.mark_failed("Original stem subscriber changed before preparation")
            row.retirement.close()
        self._consume_generation_subscribers(delivery, obsolete)
        self._finish_generation_delivery(delivery, obsolete)
        if not delivery.remaining:
            self._pair_held_generation_events.pop(self._generation_delivery_key(delivery), None)

    def _current_generation_subscribers(
        self,
        event: _StemBackendEvent,
        job: StemGenerationJob,
        subscribers: tuple[StemSubscriber, ...],
    ) -> tuple[StemSubscriber, ...]:
        return tuple(
            subscriber
            for subscriber in subscribers
            if self._is_current_generation(
                replace(
                    event,
                    sample_id=subscriber.sample_id,
                    source_version=(subscriber.request or job.request).source_version,
                    source_lease=subscriber.source,
                    content_id=subscriber.content_id,
                    generation_intent=subscriber.intent,
                    source_path=(subscriber.request or job.request).source_path,
                )
            )
        )

    def _finish_generation_delivery(
        self, delivery: _StemJobEventDelivery, available: tuple[StemSubscriber, ...]
    ) -> None:
        # Already delivered interests stay in the shared job for its remaining
        # participants, but cannot receive the terminal event or finalize twice.
        for subscriber in available:
            if self._generation_sources.get(subscriber.sample_id) is subscriber.source:
                self._clear_stem_generation_state(subscriber.sample_id)
            if (
                subscriber.sample_id not in self._pair_preparations.pending
                and subscriber.sample_id not in self._pending_stem_publications
            ):
                self._verified_candidates.pop(subscriber.sample_id, None)
        if not delivery.remaining:
            self._release_generation_promotion(delivery)
            event = delivery.event
            self._discard_generation_artifacts(event.sample_id, event.cache_dir)
            self._start_waiting_generations()

    def _release_generation_promotion(self, delivery: _StemJobEventDelivery) -> None:
        promotion = delivery.promotion
        if promotion is None or promotion.released:
            return
        with delivery.job.retirement.activate():
            self._release_promoted_generation(promotion)

    def _release_promoted_generation(self, promotion: _PromotedStemGeneration) -> None:
        if promotion.queued:
            promotion.lease.release()
        else:
            self._assets.retire(
                Path(promotion.entry.cache_dir), recursive=True, lease=promotion.lease
            )
        promotion.released = True

    def _dispatch_resolved_identity(
        self, delivery: _StemJobEventDelivery, available: tuple[StemSubscriber, ...]
    ) -> None:
        event, job = delivery.event, delivery.job
        try:
            self._resolve_generation_admission(delivery, available)
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            for subscriber in available:
                subscriber.interest_owner.mark_failed(str(error))
                if self._generation_sources.get(subscriber.sample_id) is subscriber.source:
                    self._clear_stem_generation_state(subscriber.sample_id)
                    if self._intent_current(subscriber.intent):
                        self._session.stem_generation_errors[subscriber.sample_id] = (
                            f"Stem resolved admission failed: {error}"
                        )
        if delivery.remaining or delivery.admission_route == "canonical_pending":
            return
        if delivery.admission_route != "inference" and self._jobs.get(event.cache_dir) is job:
            self._release_generation_promotion(delivery)
            self._discard_generation_artifacts(event.sample_id, event.cache_dir)
            self._start_waiting_generations()
        self._pair_held_generation_events.pop(self._generation_delivery_key(delivery), None)

    def _expire_generation_interests(self) -> None:
        for path, job in tuple(self._jobs.items()):
            for subscriber in job.subscribers():
                if subscriber.sample_id in getattr(self, "_pair_deferred_sample_ids", ()):
                    continue
                intent = subscriber.intent
                if _generation_interest_allows_preparation(subscriber.interest_owner) and (
                    intent is None
                    or (
                        intent.settings_revision == self._project.stem_processing_revision
                        and (
                            path not in self._inference_jobs | self._waiting_jobs
                            or self._intents.is_current(intent)
                        )
                    )
                ):
                    continue
                job.remove_subscriber(subscriber.sample_id)
                if self._generation_sources.get(subscriber.sample_id) is subscriber.source:
                    self._clear_stem_generation_state(subscriber.sample_id)
            if not job.subscribers() and path not in self._inference_jobs and not job.running:
                self._discard_generation_artifacts(job.request.sample_id, path)

    def _resolve_generation_admission(
        self, delivery: _StemJobEventDelivery, available: tuple[StemSubscriber, ...]
    ) -> None:
        event, job = delivery.event, delivery.job
        new = tuple(
            row
            for row in available
            if not any(self._same_generation_interest(row, own) for own in delivery.admitted)
        )
        current = self._bind_generation_subscribers(event, job, new)
        delivery.admitted += current
        if not delivery.admitted:
            return
        resolved = job.resolved_request
        if resolved is None:
            message = "Stem identity worker returned no resolved request"
            raise RuntimeError(message)
        if delivery.admission_route is None:
            self._select_generation_admission(delivery, resolved)
        if self._stem_worker_pool is not None and delivery.admission_route != "reuse":
            # Identity binding is complete; only reusable terminal preparation
            # retains subscriber custody in this delivery until a worker grant.
            self._consume_generation_subscribers(delivery, available + current)
        if delivery.admission_route == "canonical_pending":
            self._advance_canonical_admission(delivery, resolved)
            return
        if delivery.admission_route == "reuse":
            promotion = delivery.promotion
            assert promotion is not None
            if self._stem_worker_pool is None:
                self._reuse_resolved_generation(job, promotion.entry, resolved, current)
        elif delivery.admission_route == "unavailable":
            for subscriber in current:
                self._clear_stem_generation_state(subscriber.sample_id)
                self._session.stem_generation_diagnostics[subscriber.sample_id] = (
                    "Saved stems differ from the current producer; Generate to replace them"
                )

    def _select_generation_admission(
        self, delivery: _StemJobEventDelivery, resolved: StemGenerationRequest
    ) -> None:
        event, job = delivery.event, delivery.job
        compatible = (
            None
            if event.cache_dir in self._skip_completed_jobs
            else self._compatible_completed_entry(resolved, job.producer_identity)
        )
        if compatible is not None:
            self._retain_reuse_admission(delivery, compatible)
            return
        if event.cache_dir in self._reuse_only_jobs:
            delivery.admission_route = "unavailable"
            return
        shared = self._shared_generation_job(job)
        if shared is not None:
            delivery.canonical = shared
            delivery.admission_route = "canonical_pending"
            return
        delivery.admission_route = "inference"
        self._waiting_jobs.add(event.cache_dir)
        job.admission_ready_sequence = self._next_stem_readiness()
        self._start_waiting_generations()

    def _shared_generation_job(self, job: StemGenerationJob) -> StemGenerationJob | None:
        """Find actual equal-fingerprint work, including retained terminal custody."""
        return next(
            (
                existing
                for path, existing in self._jobs.items()
                if existing is not job
                and job.generation_fingerprint is not None
                and existing.generation_fingerprint == job.generation_fingerprint
                and path in self._inference_jobs | self._waiting_jobs
                and self._canonical_generation_interests_current(existing)
            ),
            None,
        )

    def _canonical_generation_interests_current(self, job: StemGenerationJob) -> bool:
        """Old readers serialize new work but cannot lend obsolete intent authority."""
        return not job.cancel_event.is_set() and any(
            self._intent_current(sub.intent) for sub in job.subscribers()
        )

    def _retain_reuse_admission(
        self, delivery: _StemJobEventDelivery, compatible: StemCacheEntry
    ) -> None:
        delivery.promotion = _PromotedStemGeneration(
            compatible, self._assets.acquire(Path(compatible.cache_dir)), queued=True
        )
        delivery.admission_route = "reuse"
        if self._stem_worker_pool is not None:
            delivery.ready_sequence = self._next_stem_readiness()
            # A canonical waiter may already have consumed its identity rows.
            # Reuse returns only its original admitted interests to this owner.
            delivery.remaining = tuple(
                row
                for row in delivery.job.subscribers()
                if any(
                    self._same_generation_interest(row, own)
                    for own in delivery.remaining + delivery.admitted
                )
            )

    def _report_canonical_admission_pending(self, delivery: _StemJobEventDelivery) -> None:
        for subscriber in delivery.admitted:
            self._session.stem_generation_diagnostics[subscriber.sample_id] = (
                "Shared stem admission awaits Pair placement before joining canonical inference"
            )

    def _advance_canonical_admission(
        self, delivery: _StemJobEventDelivery, resolved: StemGenerationRequest
    ) -> None:
        job = delivery.job
        compatible = (
            None
            if job.request.cache_dir in self._skip_completed_jobs
            else self._compatible_completed_entry(resolved, job.producer_identity)
        )
        if compatible is not None:
            admitted = tuple(
                subscriber
                for subscriber in job.subscribers()
                if any(self._same_generation_interest(subscriber, own) for own in delivery.admitted)
            )
            current = self._current_generation_subscribers(delivery.event, job, admitted)
            self._retain_reuse_admission(delivery, compatible)
            if self._stem_worker_pool is None:
                self._reuse_resolved_generation(job, compatible, resolved, current)
            return
        canonical = delivery.canonical
        retained = (
            canonical is not None
            and self._jobs.get(canonical.request.cache_dir) is canonical
            and canonical.request.cache_dir in self._inference_jobs | self._waiting_jobs
            and self._canonical_generation_interests_current(canonical)
        )
        if not retained:
            # The candidate still owns its original resolved source/model and
            # subscribers. Rejoin existing admission instead of waiting forever
            # for a disappeared physical owner or manufacturing a new request.
            delivery.canonical = None
            delivery.admission_route = None
            self._select_generation_admission(delivery, resolved)
            canonical = delivery.canonical
            if canonical is None:
                return
        assert canonical is not None
        deferred = getattr(self, "_pair_deferred_sample_ids", ())
        if (
            not delivery.remaining
            and canonical.phase in {"resolved", "running"}
            and (canonical, "terminal") not in self._pair_held_generation_events
            and not any(sub.sample_id in deferred for sub in canonical.subscribers())
            and job.transfer_subscribers_to(canonical, require_active=True)
        ):
            self._discard_generation_artifacts(job.request.sample_id, job.request.cache_dir)
            delivery.admission_route = "inference"
            return
        self._report_canonical_admission_pending(delivery)

    def _bind_generation_subscribers(
        self,
        event: _StemBackendEvent,
        job: StemGenerationJob,
        subscribers: tuple[StemSubscriber, ...],
    ) -> tuple[StemSubscriber, ...]:
        admitted: list[StemSubscriber] = []
        for subscriber in subscribers:
            own_event = replace(
                event,
                sample_id=subscriber.sample_id,
                source_version=(subscriber.request or job.request).source_version,
                source_lease=subscriber.source,
                content_id=subscriber.content_id,
                generation_intent=subscriber.intent,
                source_path=(subscriber.request or job.request).source_path,
            )
            if not self._is_current_generation(own_event, resolving=True):
                job.remove_subscriber(subscriber.sample_id)
                if self._generation_sources.get(subscriber.sample_id) is subscriber.source:
                    self._clear_stem_generation_state(subscriber.sample_id)
                continue
            intent = subscriber.intent
            bound = (
                self._intents.bind(
                    intent,
                    job.generation_fingerprint
                    or hashlib.sha256(
                        f"unattested admission {intent.admission_sequence}".encode()
                    ).hexdigest(),
                )
                if intent is not None
                else None
            )
            if bound is None:
                job.remove_subscriber(subscriber.sample_id)
                self._clear_stem_generation_state(subscriber.sample_id)
                continue
            job.update_intent(subscriber.sample_id, bound)
            self._generation_intents[subscriber.sample_id] = bound
            updated = replace(subscriber, intent=bound)
            admitted.append(updated)
            self._update_generation_delivery_interest(subscriber, updated)
            resolved = job.resolved_request
            if resolved is not None:
                self._remember_generation_candidate(updated, resolved, job)
        return tuple(admitted)

    def _update_generation_delivery_interest(
        self, before: StemSubscriber, after: StemSubscriber
    ) -> None:
        for delivery in self._pair_held_generation_events.values():
            delivery.remaining = tuple(
                after if self._same_generation_interest(subscriber, before) else subscriber
                for subscriber in delivery.remaining
            )

    def _reuse_resolved_generation(
        self,
        job: StemGenerationJob,
        compatible: StemCacheEntry,
        resolved: StemGenerationRequest,
        subscribers: tuple[StemSubscriber, ...],
    ) -> None:
        for subscriber in subscribers:
            self._clear_stem_generation_state(subscriber.sample_id)
            own = subscriber.request or job.request
            processing = replace(
                resolved,
                sample_id=subscriber.sample_id,
                source_path=own.source_path,
                source_version=own.source_version,
            )
            self._remember_generation_candidate(subscriber, processing, job)
            self._prepare_pair(
                subscriber.sample_id,
                compatible.model_copy(deep=True),
                self._capture_generation_ticket(subscriber),
                processing_request=processing,
                regenerate_on_incompatible=job.request.cache_dir not in self._reuse_only_jobs,
                binding=StemPairIntentBinding(
                    generation_intent=subscriber.intent,
                    own_source_version=own.source_version,
                    artifact_source_version=compatible.source_version,
                    interest_owner=subscriber.interest_owner,
                ),
            )
            self._session.stem_generation_diagnostics[subscriber.sample_id] = (
                "Checking saved stems for the current model"
            )

    def _start_waiting_generations(self) -> None:
        if self._stem_worker_pool is not None:
            # Production continuations compete in the same six-lane arbiter.
            return
        for path in tuple(self._waiting_jobs):
            job = self._jobs.get(path)
            if job is None:
                self._waiting_jobs.discard(path)
                continue
            for subscriber in job.subscribers():
                if subscriber.sample_id in self._pair_deferred_sample_ids:
                    continue
                if not self._intent_current(subscriber.intent):
                    job.remove_subscriber(subscriber.sample_id)
                    if self._generation_sources.get(subscriber.sample_id) is subscriber.source:
                        self._clear_stem_generation_state(subscriber.sample_id)
            if not job.subscribers():
                self._discard_generation_artifacts(job.request.sample_id, path)
                continue
            if not self._inference_admissible(job):
                continue
            self._waiting_jobs.discard(path)
            self._schedule_stem_job(job)

    def _remember_generation_candidate(
        self, subscriber: StemSubscriber, request: StemGenerationRequest, job: StemGenerationJob
    ) -> None:
        if (
            subscriber.intent is not None
            and job.processing_files is not None
            and job.generation_fingerprint is not None
        ):
            candidate = self._verified_candidates.get(subscriber.sample_id)
            if candidate is None or candidate[0] != subscriber.intent:
                # A relocated subscriber keeps its own sealed producer locators
                # after joining another root's physical inference.
                self._verified_candidates[subscriber.sample_id] = (
                    subscriber.intent,
                    request,
                    job.processing_files,
                )
        elif job.generation_fingerprint is None:
            self._verified_candidates.pop(subscriber.sample_id, None)

    def _remember_verified_selection(self, sample_id: int, entry: StemCacheEntry) -> None:
        candidate = self._verified_candidates.pop(sample_id, None)
        if (
            candidate is not None
            and self._intent_current(candidate[0])
            and entry.fingerprint_schema == STEM_FINGERPRINT_SCHEMA
            and entry.generation_fingerprint
            == request_fingerprint(candidate[1], entry.producer_identity)
        ):
            intent, request, guard = candidate
            self._verified_sets[sample_id] = _VerifiedStemIntent(
                entry,
                intent,
                request,
                self._content_id(sample_id),
                self._project.sample_paths[sample_id],
                guard,
            )
            self._session.stem_generation_diagnostics[sample_id] = (
                "Stems for the current model are already present"
            )

    def _finish_shared_generation(
        self, event: _StemBackendEvent, current: tuple[StemSubscriber, ...]
    ) -> None:
        eligible = tuple(
            subscriber
            for subscriber in current
            if self._eligible_generation_entry(
                subscriber.sample_id,
                (subscriber.request or self._jobs[event.cache_dir].request).source_version,
            )
            is not None
        )
        for subscriber in current:
            self._record_generation_result(subscriber.sample_id, event.result)
        first = eligible[0] if eligible else None
        for subscriber in current:
            if subscriber is not first:
                self._clear_stem_generation_state(subscriber.sample_id)
        if first is not None:
            self._handle_stem_generation_success(
                first.sample_id, first.source, event.cache_dir, eligible
            )

    def _handle_stem_generation_started(self, sample_id: int) -> None:
        """Apply a stem-generation start event from a backend event source."""
        validate_sample_id(sample_id)
        if sample_id in self._generation_sources or sample_id in self._pending_stem_publications:
            return
        self._session.stem_generating_sample_ids.add(sample_id)
        self._clear_stem_generation_messages(sample_id)

    def _handle_stem_generation_progress(
        self,
        sample_id: int,
        percent: float | None,
        stage: str | None,
        source_lease: OfflineStemSourceLease | None = None,
        *,
        execution: StemExecution | None = None,
    ) -> None:
        """Apply a stem-generation progress event from a backend event source."""
        validate_sample_id(sample_id)
        if sample_id not in self._session.stem_generating_sample_ids:
            return
        if source_lease is None and (
            sample_id in self._generation_sources or sample_id in self._pending_stem_publications
        ):
            return

        if stage is not None:
            self._session.stem_generation_stage[sample_id] = stage
        if percent is not None:
            self._session.stem_generation_progress[sample_id] = float(percent)
        if execution is not None:
            self._session.stem_generation_execution[sample_id] = execution

    def _handle_stem_generation_success(
        self,
        sample_id: int,
        source_lease: OfflineStemSourceLease | None = None,
        generation_cache_dir: Path | None = None,
        subscribers: tuple[StemSubscriber, ...] = (),
    ) -> None:
        """Verify completion against its offline interest before fresh publication capture."""
        validate_sample_id(sample_id)
        if sample_id not in self._session.stem_generating_sample_ids:
            return
        if source_lease is None:
            self._reject_legacy_stem_completion(sample_id)
            return
        if self._generation_sources.get(sample_id) is not source_lease:
            return

        source_version = self._session.stem_generation_source_versions.get(sample_id)
        self._clear_stem_generation_state(sample_id)
        if source_version is None:
            return
        entry = self._eligible_generation_entry(sample_id, source_version)
        if entry is not None:
            job = self._jobs.get(generation_cache_dir) if generation_cache_dir is not None else None
            if job is not None:
                with job.retirement.activate():
                    self._publish_generation_cache(
                        sample_id, source_lease, entry, generation_cache_dir, subscribers
                    )

    def _reject_legacy_stem_completion(self, sample_id: int) -> None:
        # Unbound legacy events cannot finish a current Python job or capture a fresh
        # ticket that would launder artifacts prepared for a prior source/timing intent.
        if sample_id in self._generation_sources or sample_id in self._pending_stem_publications:
            return
        self._clear_stem_generation_state(sample_id)
        self._session.stem_generation_errors[sample_id] = (
            "Stem completion rejected because its admission ticket is missing"
        )

    def _eligible_generation_entry(
        self, sample_id: int, source_version: str
    ) -> StemCacheEntry | None:
        if self._loaded_source_version_for_pad(sample_id) != source_version:
            return None
        cache_dir = cache_dir_for_sample_id(sample_id, self._project.sample_paths[sample_id])
        return StemCacheEntry(
            source_version=source_version,
            cache_dir=cache_dir,
            stems=expected_stem_files(cache_dir),
            available=False,
        )

    def _publish_generation_cache(
        self,
        sample_id: int,
        source_lease: OfflineStemSourceLease,
        entry: StemCacheEntry,
        generation_cache_dir: Path | None,
        subscribers: tuple[StemSubscriber, ...] = (),
    ) -> None:
        if generation_cache_dir is None:
            self._fail_generation_targets(
                sample_id,
                "Stem completion rejected because its private artifact directory is missing",
                subscribers,
            )
            return
        job = self._jobs.get(generation_cache_dir)
        delivery = self._pair_held_generation_events.get((job, "terminal")) if job else None
        if delivery is not None and delivery.promotion_attempted:
            promotion = delivery.promotion
            if promotion is None:
                self._fail_generation_targets(
                    sample_id,
                    delivery.promotion_error or "Stem cache promotion failed",
                    subscribers,
                )
                return
        else:
            if delivery is not None:
                delivery.promotion_attempted = True
            promotion = self._promote_generation_cache(sample_id, entry, generation_cache_dir, job)
            if delivery is not None:
                delivery.promotion = promotion
                delivery.promotion_error = self._session.stem_generation_errors.get(sample_id)
            if promotion is None:
                self._fail_generation_targets(
                    sample_id,
                    self._session.stem_generation_errors.get(sample_id)
                    or "Stem cache promotion failed",
                    subscribers,
                )
                return
        promotion.queued |= self._publish_generation_targets(
            sample_id, source_lease, promotion.entry, subscribers, job
        )
        if delivery is None:
            self._release_promoted_generation(promotion)

    def _fail_generation_targets(
        self, sample_id: int, message: str, subscribers: tuple[StemSubscriber, ...]
    ) -> None:
        self._session.stem_generation_errors[sample_id] = message
        for subscriber in subscribers:
            subscriber.interest_owner.mark_failed(message)
            self._session.stem_generation_errors[subscriber.sample_id] = message

    def _promote_generation_cache(
        self,
        sample_id: int,
        entry: StemCacheEntry,
        generation_cache_dir: Path,
        job: StemGenerationJob | None,
    ) -> _PromotedStemGeneration | None:
        published_path = generation_cache_dir.with_name(
            generation_cache_dir.name.replace(".generation-", ".ready-", 1)
        )
        lease = None
        try:
            delivery = self._pair_held_generation_events.get((job, "terminal")) if job else None
            result = delivery.event.result if delivery is not None else None
            # New production overwrites optional history even when its execution
            # is unknown. Reuse takes its existing immutable entry elsewhere.
            entry = entry.model_copy(update={"execution": result.execution if result else None})
            if job is not None:
                entry = entry.model_copy(
                    update={
                        "source_version": job.request.source_version,
                        "fingerprint_schema": STEM_FINGERPRINT_SCHEMA
                        if job.generation_fingerprint is not None
                        else None,
                        "generation_fingerprint": job.generation_fingerprint,
                        "producer_identity": job.producer_identity,
                    }
                )
            # Admission can fail while the private directory is still intact.
            # The future immutable path is pinned before the atomic rename.
            lease = self._assets.acquire(published_path)
            # Legacy #N containers locate data; a surviving subscriber may be in
            # another slot after the producer's origin assignment disappears.
            cache_root = generation_cache_dir.parent.relative_to(Path.cwd()).as_posix()
            entry = entry.model_copy(
                update={"cache_dir": cache_root, "stems": expected_stem_files(cache_root)}
            )
            entry = promote_generation_artifacts(entry, generation_cache_dir)
        except (OSError, RuntimeError, ValueError) as err:
            self._session.stem_generation_errors[sample_id] = f"Stem cache promotion failed: {err}"
            if lease is not None:
                self._assets.retire(published_path, recursive=True, lease=lease)
            return None
        if not self._entry_files_available(entry):
            self._session.stem_generation_errors[sample_id] = (
                "Stem cache integrity changed before publication"
            )
            self._assets.retire(Path(entry.cache_dir), recursive=True, lease=lease)
            return None
        return _PromotedStemGeneration(entry, lease)

    def _publish_generation_targets(
        self,
        sample_id: int,
        source_lease: OfflineStemSourceLease,
        entry: StemCacheEntry,
        subscribers: tuple[StemSubscriber, ...],
        job: StemGenerationJob | None,
    ) -> bool:
        queued = False
        targets = subscribers or (
            StemSubscriber(sample_id, source_lease, None, self._assets.reserve_pending(16)),
        )
        for subscriber in targets:
            try:
                with subscriber.retirement.activate():
                    if not self._intent_current(subscriber.intent):
                        continue
                    own = subscriber.request
                    if job is not None and own is not None:
                        self._remember_generation_candidate(subscriber, own, job)
                    accepted = self._queue_stem_publication(
                        subscriber.sample_id,
                        entry.model_copy(deep=True),
                        self._capture_generation_ticket(subscriber),
                        generation_intent=subscriber.intent,
                        own_source_version=own.source_version
                        if own is not None
                        else entry.source_version,
                        artifact_source_version=entry.source_version,
                        interest_owner=subscriber.interest_owner,
                    )
            except (OSError, RuntimeError, ValueError) as err:
                subscriber.interest_owner.mark_failed(str(err))
                self._session.stem_generation_errors[subscriber.sample_id] = (
                    f"Stem generation completed but publication failed: {err}"
                )
            else:
                queued |= accepted
            finally:
                subscriber.retirement.close()
        return queued

    def _queue_stem_publication(
        self,
        sample_id: int,
        entry: StemCacheEntry,
        source_ticket: PreparedSourceTicket,
        prepared_pair: PreparedStemPair | None = None,
        *,
        generation_intent: StemGenerationIntent | None = None,
        own_source_version: str | None = None,
        artifact_source_version: str | None = None,
        interest_owner: StemGenerationInterests | None = None,
    ) -> bool:
        if not self._intent_current(
            generation_intent
        ) or not _generation_interest_allows_preparation(interest_owner):
            return False
        if prepared_pair is None and callable(getattr(self._audio, "prepare_stem_pair", None)):
            return self._prepare_pair(
                sample_id,
                entry,
                source_ticket,
                binding=StemPairIntentBinding(
                    generation_intent=generation_intent,
                    own_source_version=own_source_version,
                    artifact_source_version=artifact_source_version,
                    interest_owner=interest_owner,
                ),
            )
        previous_pending = self._publication_replacement_owner(sample_id, entry)
        retirement = self._assets.reserve_pending(16)
        previous_entry = self._project.stem_cache[sample_id]
        previous_lease = None
        new_lease = None
        try:
            new_lease = self._assets.acquire(Path(entry.cache_dir))
            if previous_entry is not None:
                previous_lease = self._assets.acquire(Path(previous_entry.cache_dir))
            if prepared_pair is None:
                self._audio.publish_prepared_stems(
                    sample_id, entry.source_version, entry.cache_dir, source_ticket
                )
            else:
                self._audio.publish_stem_pair(prepared_pair, source_ticket)
                # This publication follows any older ordered FULL MIX release.
                self._pair_release_ordered.discard(sample_id)
        except OSError, RuntimeError, ValueError:
            if new_lease is not None:
                new_lease.release()
            if previous_lease is not None:
                previous_lease.release()
            retirement.close()
            raise
        queued_entry = entry
        if entry.available:
            queued_entry = entry.model_copy(update={"available": False})
        self._project.stem_cache[sample_id] = queued_entry
        with retirement.activate():
            self._assets.adopt_stems(sample_id, (Path(entry.cache_dir).absolute(), new_lease))
        self._mark_project_changed()
        self._restored_stem_candidates.pop(sample_id, None)
        pending = _PendingStemPublication(
            sample_id=sample_id,
            source_ticket=source_ticket,
            entry=queued_entry,
            previous_entry=previous_entry,
            previous_lease=previous_lease,
            retirement=retirement,
            prepared_pair=prepared_pair,
            content_id=self._content_id(sample_id),
            source_path=self._project.sample_paths[sample_id],
            generation_intent=generation_intent,
            interest_owner=interest_owner,
        )
        if previous_pending is not None:
            self._transfer_publication_retry(previous_pending, pending)
        self._pending_stem_publications[sample_id] = pending
        return self._poll_stem_publication(sample_id, pending)

    def _prepare_pair(
        self,
        sample_id: int,
        entry: StemCacheEntry,
        ticket: PreparedSourceTicket,
        *,
        components: bool | None = None,
        processing_request: StemGenerationRequest | None = None,
        regenerate_on_incompatible: bool = False,
        residency_only: bool = False,
        binding: StemPairIntentBinding | None = None,
        reservation: StemWorkerReservation | None = None,
    ) -> bool:
        if sample_id in self._fenced_pair_ids:
            message = "Unsupported stem pair metadata requires explicit recovery"
            raise RuntimeError(message)
        content = self._project.pad_content[sample_id]
        captured = StemPairIntentBinding() if binding is None else binding
        if not _generation_interest_allows_preparation(captured.interest_owner):
            return False
        spec = StemPairPreparationSpec(
            sample_id,
            entry,
            ticket,
            content.instance_id if content else None,
            self._project.stem_cache[sample_id],
            components=captured.generation_intent is None
            and self._project.pad_stem_mix_mode[sample_id] == "all_stems"
            if components is None
            else components,
            processing_request=processing_request,
            regenerate_on_incompatible=regenerate_on_incompatible,
            residency_only=residency_only,
            binding=replace(
                captured,
                source_path=self._project.sample_paths[sample_id],
                verify_saved_selection=entry.pair is not None
                and self._restored_stem_candidates.get(sample_id) is entry
                and captured.generation_intent is None
                and processing_request is None
                and captured.own_source_version is None
                and captured.artifact_source_version is None,
            ),
        )
        if reservation is not None:
            self._pair_preparations.submit_reserved(spec, reservation)
        elif self._stem_worker_pool is not None:
            previous = self._requested_preparations.get(sample_id)
            if previous is None or not self._requested_preparation_current(previous.spec):
                self._requested_preparations[sample_id] = _RequestedStemPreparation(
                    spec, self._next_stem_readiness()
                )
        else:
            self._pair_preparations.submit(
                spec.sample_id,
                spec.entry,
                spec.ticket,
                spec.content_id,
                spec.previous_entry,
                components=spec.components,
                processing_request=spec.processing_request,
                regenerate_on_incompatible=spec.regenerate_on_incompatible,
                residency_only=spec.residency_only,
                binding=spec.binding,
            )
        return True

    def _poll_pair_preparations(self) -> None:
        self._cancel_uninterested_preparations()
        for sample_id in tuple(self._resident_pairs)[:8]:
            if sample_id in getattr(self, "_pair_deferred_sample_ids", ()):
                continue
            if self._project.pad_stem_mix_mode[sample_id] == "full_mix":
                self._set_full_mix(sample_id)
        for sample_id, error in self._pair_preparations.retry_discards():
            self._session.stem_generation_errors[sample_id] = f"Stem pair cleanup deferred: {error}"
        for request in self._pair_preparations.collect(
            getattr(self, "_pair_deferred_sample_ids", frozenset())
        ):
            self._accept_returned_pair(request)

    def _accept_returned_pair(self, request: PairPreparation) -> None:
        self._project_returned_pair_placement(request)
        retained = False
        try:
            retained = self._finish_pair_preparation(request)
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            if request.interest_owner is not None:
                request.interest_owner.mark_failed(str(error))
            demand = self._demands.get(request.sample_id)
            if demand is not None:
                demand.state = self._admission_wait_state(str(error))
                demand.error = str(error)
            self._session.stem_generation_errors[request.sample_id] = (
                str(error)
                if resident_stem_admission_pressure(str(error))
                else f"Complete stem preparation failed: {error}"
            )
        finally:
            if not retained:
                try:
                    if request.prepared is not None:
                        pending = self._pending_stem_publications.get(request.sample_id)
                        settlement = self._disk_selection_settlements.get(request.sample_id)
                        if (pending is None or pending.prepared_pair is not request.prepared) and (
                            settlement is None
                            or settlement.request.prepared is not request.prepared
                        ):
                            self._discard_pair(request.sample_id, request.prepared)
                finally:
                    queued = self._pair_preparations.pending.get(request.sample_id)
                    if (
                        request.sample_id not in self._pending_stem_publications
                        and request.sample_id not in self._disk_selection_settlements
                        and (queued is None or queued is request)
                    ):
                        self._verified_candidates.pop(request.sample_id, None)
                    request.close()

    def _project_returned_pair_placement(self, request: PairPreparation) -> None:
        """Follow a surviving pending key only after the exact worker has returned.

        Live Pair mirrors retain the worker's original request and its actual
        readers. The queue collector may therefore find that its immutable
        capture ID differs from the current delivery key. No worker can read
        this projection update after its own result has returned.
        """
        for sample_id, pending in tuple(self._pair_preparations.pending.items()):
            if pending is request:
                self._pair_preparations.pending.pop(sample_id)
                request.sample_id = sample_id
                return

    def _finish_pair_preparation(self, request: PairPreparation) -> bool:
        """Finish one returned result, or retain its exact owners while its writer is busy."""
        sample_id = request.sample_id
        if not self._pair_preparation_current(request):
            return False
        if request.error is not None:
            if request.regenerate_on_incompatible and request.compatibility_rejected:
                self._regenerate_incompatible_pair(request)
                return False
            raise RuntimeError(request.error)
        selected = self._pair_preparations.selected_entry(request)
        prepared = request.prepared
        if prepared is None:
            return False
        # Recheck current native source/request/timing even for disk-only selection.
        ticket = self._capture_source_ticket(sample_id, selected.source_version)
        if not request.ticket.same_source_request(ticket):
            message = "Stem source request changed during complete pair preparation"
            raise RuntimeError(message)
        if request.verify_saved_selection:
            self._retain_verified_restored_pair(request, selected, ticket)
            return False
        publish_components = (
            not request.residency_only
            and prepared.has_components()
            and self._project.pad_stem_mix_mode[sample_id] == "all_stems"
        )
        if (
            not publish_components
            and self._commit_disk_selection is not None
            and self._disk_selection_writer_blocked is not None
            and self._disk_selection_writer_blocked()
        ):
            # This is an advisory wait before consuming the original rollback
            # reservation. The unchanged commit still validates its real writer,
            # revision, content, source and selected predecessor after the wait.
            return self._pair_preparations.retain_returned(request)
        with request.retirement.activate():
            if publish_components:
                self._queue_stem_publication(
                    sample_id,
                    selected,
                    ticket,
                    prepared,
                    generation_intent=request.generation_intent,
                    interest_owner=request.interest_owner,
                )
            else:
                self._select_prepared_disk_pair(request, selected, ticket)
        return False

    def _retain_verified_restored_pair(
        self, request: PairPreparation, selected: StemCacheEntry, ticket: PreparedSourceTicket
    ) -> None:
        """Verify unchanged durable selection, retaining its exact preparation until admission."""
        current = self._project.stem_cache[request.sample_id]
        if (
            current is not request.previous_entry
            or current is None
            or current.pair is None
            or selected.model_dump(exclude={"available"})
            != current.model_dump(exclude={"available"})
        ):
            message = "Restored stem verification changed the saved immutable selection"
            raise RuntimeError(message)
        with request.retirement.activate():
            lease = self._assets.acquire(Path(current.cache_dir))
            try:
                retirement = request.retirement.split(16)
            except BaseException:
                lease.release()
                raise
            settlement = _DiskSelectionSettlement(
                request, current, ticket, lease, None, retirement, verified_restore=True
            )
            self._disk_selection_settlements[request.sample_id] = settlement
            self._settle_verified_restored_pair(settlement)

    def _settle_verified_restored_pair(self, settlement: _DiskSelectionSettlement) -> None:
        request = settlement.request
        prepared = request.prepared
        if prepared is None:
            return
        sample_id = request.sample_id
        if not self._disk_selection_assignment_current(settlement):
            self._settle_written_disk_selection(settlement)
            return
        try:
            self._require_restored_pair_current(settlement)
            with settlement.retirement.activate():
                prepared.select()
                if not settlement.assignment_adopted:
                    self._assets.adopt_stems(
                        sample_id, (Path(settlement.entry.cache_dir).absolute(), settlement.lease)
                    )
                    settlement.assignment_adopted = True
                self._committed_residency.register(
                    sample_id,
                    prepared,
                    settlement.ticket,
                    settlement.entry,
                    self._content_id(sample_id),
                    self._project.sample_paths[sample_id],
                    active=sample_id in self._session.active_sample_ids,
                )
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self._session.stem_generation_errors[sample_id] = (
                f"Restored stem registration deferred: {error}"
            )
            return
        self._disk_selection_settlements.pop(sample_id, None)
        settlement.retirement.close()
        if self._session.stem_generation_errors.get(sample_id, "").startswith(
            "Restored stem registration deferred:"
        ):
            self._session.stem_generation_errors.pop(sample_id, None)
        self._remember_verified_selection(sample_id, settlement.entry)
        self._restored_stem_candidates.pop(sample_id, None)
        self._settled_residency_demand(sample_id, settlement.entry)

    def _require_restored_pair_current(self, settlement: _DiskSelectionSettlement) -> None:
        if not self._pair_disk_commit_current(settlement.request, settlement.ticket):
            message = "Restored stem source or saved selection changed before registration"
            raise RuntimeError(message)

    def _select_prepared_disk_pair(
        self, request: PairPreparation, selected: StemCacheEntry, ticket: PreparedSourceTicket
    ) -> None:
        if not self._pair_preparation_current(request) or request.prepared is None:
            return
        sample_id = request.sample_id
        with request.retirement.activate():
            lease = self._assets.acquire(Path(selected.cache_dir))
            previous_lease = None
            retirement = None
            try:
                if request.previous_entry is not None:
                    previous_lease = self._assets.acquire(Path(request.previous_entry.cache_dir))
                available = selected.model_copy(update={"available": True})
                retirement = request.retirement.split(16)
                settlement = _DiskSelectionSettlement(
                    request, available, ticket, lease, previous_lease, retirement
                )
                self._disk_selection_settlements[sample_id] = settlement
                if self._commit_disk_selection is not None:
                    self._commit_disk_selection(
                        sample_id,
                        request.previous_entry,
                        available,
                        lambda: self._pair_disk_commit_current(request, ticket),
                    )
                else:
                    self._require_pair_disk_commit_current(request, ticket)
            except BaseException:
                self._disk_selection_settlements.pop(sample_id, None)
                lease.release()
                if previous_lease is not None:
                    previous_lease.release()
                if retirement is not None:
                    retirement.close()
                raise
            if request.interest_owner is not None and self._commit_disk_selection is not None:
                request.interest_owner.mark_committed()
            # No further cancellation check can turn this durable selection into
            # a precommit result. Later source/content changes use normal invalidation.
            if (
                self._project.stem_cache[sample_id] is request.previous_entry
                and self._content_id(sample_id) == request.content_id
                and self._project.sample_paths[sample_id] == request.source_path
            ):
                self._project.stem_cache[sample_id] = available
            self._settle_disk_selection(settlement)

    def _pair_disk_commit_current(
        self, request: PairPreparation, ticket: PreparedSourceTicket
    ) -> bool:
        if not self._pair_preparation_current(request):
            return False
        fresh = self._capture_source_ticket(
            request.sample_id, request.own_source_version or request.entry.source_version
        )
        return ticket.same_source_request(fresh)

    def _require_pair_disk_commit_current(
        self, request: PairPreparation, ticket: PreparedSourceTicket
    ) -> None:
        if not self._pair_disk_commit_current(request, ticket):
            message = "Stem selection changed before disk commit"
            raise RuntimeError(message)

    def _poll_disk_selection_settlements(self) -> None:
        for settlement in tuple(self._disk_selection_settlements.values())[:8]:
            if settlement.request.sample_id in getattr(self, "_pair_deferred_sample_ids", ()):
                continue
            self._settle_disk_selection(settlement)

    def _settle_disk_selection(self, settlement: _DiskSelectionSettlement) -> None:
        if settlement.verified_restore:
            self._settle_verified_restored_pair(settlement)
        else:
            self._settle_written_disk_selection(settlement)

    def _report_disk_selection_settlement_error(
        self, settlement: _DiskSelectionSettlement, error: Exception
    ) -> None:
        settlement.last_error = f"Stem selection committed; owner settlement deferred: {error}"
        if not settlement.verified_restore or self._disk_selection_assignment_current(settlement):
            self._session.stem_generation_errors[settlement.request.sample_id] = (
                settlement.last_error
            )

    def _disk_selection_assignment_current(self, settlement: _DiskSelectionSettlement) -> bool:
        """Check Python assignment ownership without interpreting Native admission pressure."""
        request = settlement.request
        sample_id = request.sample_id
        return (
            not settlement.content_retired
            and self._project.stem_cache[sample_id] is settlement.entry
            and self._content_id(sample_id) == request.content_id
            and self._project.sample_paths[sample_id] == request.source_path
        )

    def _settle_written_disk_selection(self, settlement: _DiskSelectionSettlement) -> None:
        request = settlement.request
        prepared = request.prepared
        if prepared is None:
            return
        sample_id = request.sample_id
        try:
            with settlement.retirement.activate():
                prepared.select()
                current = self._disk_selection_assignment_current(settlement)
                if current:
                    self._assets.adopt_stems(
                        sample_id, (Path(settlement.entry.cache_dir).absolute(), settlement.lease)
                    )
                else:
                    # Newer content can arrive after the actual config replacement.
                    # Preserve that memory image and first replace its old durable
                    # references through the same writer, retaining both owner sets
                    # if this superseding write fails.
                    if self._flush_superseded_disk_selection is not None:
                        self._flush_superseded_disk_selection()
                    self._assets.retire(
                        Path(settlement.entry.cache_dir), recursive=True, lease=settlement.lease
                    )
                self._retire_disk_selection_predecessor(settlement)
        except (OSError, RuntimeError, ValueError) as error:
            self._report_disk_selection_settlement_error(settlement, error)
            return
        self._disk_selection_settlements.pop(sample_id, None)
        settlement.retirement.close()
        if not current:
            return
        message = self._session.stem_generation_errors.get(sample_id, "")
        if message.startswith("Stem selection committed; owner settlement deferred:"):
            self._session.stem_generation_errors.pop(sample_id, None)
        self._remember_verified_selection(sample_id, settlement.entry)
        pending = self._pending_stem_publications.pop(sample_id, None)
        if pending is not None:
            self._release_pending_stems(pending)
        self._restored_stem_candidates.pop(sample_id, None)
        if self._commit_disk_selection is None:
            self._mark_project_changed()
        self._register_committed_pair(sample_id, prepared, settlement.ticket)
        self._settled_residency_demand(sample_id, settlement.entry)
        if request.residency_only or (
            request.generation_intent is not None
            and (
                sample_id not in self._session.active_sample_ids
                or self._committed_residency.pending_supported
            )
        ):
            self._publish_all_stems_mode_if_preferred(sample_id, settlement.entry.source_version)

    def _settled_residency_demand(self, sample_id: int, entry: StemCacheEntry) -> None:
        demand = self._demands.get(sample_id)
        if demand is not None:
            demand.selection_identity = (
                entry.pair.stem_set_identity if entry.pair is not None else None
            )
            demand.verify_disk = False
            demand.state = "queued" if demand.explicit or demand.policy else "verified"

    def _retire_disk_selection_predecessor(self, settlement: _DiskSelectionSettlement) -> None:
        previous = settlement.request.previous_entry
        lease = settlement.previous_lease
        if previous is None or lease is None:
            return
        if previous.cache_dir == settlement.entry.cache_dir:
            lease.release()
        else:
            self._assets._retire_assignment("stems", Path(previous.cache_dir).absolute(), lease)
        settlement.previous_lease = None

    def _pair_preparation_current(self, request: PairPreparation) -> bool:
        sample_id = request.sample_id
        pending = self._pair_preparations.pending.get(sample_id)
        if pending is not None and pending is not request:
            return False
        if (
            self._shutting_down
            or request.cancelled.is_set()
            or not _generation_interest_allows_preparation(request.interest_owner)
            or not self._intent_current(request.generation_intent)
        ):
            return False
        if (
            request.source_path is not None
            and self._project.sample_paths[sample_id] != request.source_path
        ):
            return False
        content = self._project.pad_content[sample_id]
        if (
            content.instance_id if content else None
        ) != request.content_id or self._project.stem_cache[
            sample_id
        ] is not request.previous_entry:
            return False
        processing = request.processing_request
        return processing is None or (
            processing.separator == self._project.stem_separator
            and (
                processing.separator == "bs-roformer:musdb18hq"
                or (
                    processing.demucs_shifts == self._project.demucs_shifts
                    and processing.demucs_overlap == self._project.demucs_overlap
                )
            )
            and processing.target_shape == self._target_shape_for_pad(sample_id)
        )

    def _regenerate_incompatible_pair(self, request: PairPreparation) -> None:
        """One explicit Generate may replace incompatible derivatives via its own admission."""
        processing = request.processing_request
        if processing is None:
            message = "Incompatible reuse lost its captured generation request"
            raise RuntimeError(message)
        blocker = self.stem_generation_block_reason(request.sample_id)
        if blocker is not None:
            raise RuntimeError(blocker)
        source = self._project.sample_paths[request.sample_id]
        if source is None or original_asset(source).path != processing.source_path:
            message = "Stem source assignment changed during compatible reuse"
            raise RuntimeError(message)
        if separator_model_cache_dir(self._project.stem_separator) != processing.model_cache_dir:
            message = "Stem model location changed during compatible reuse"
            raise RuntimeError(message)
        ticket = self._capture_source_ticket(request.sample_id, processing.source_version)
        if not request.ticket.same_source_request(ticket):
            message = "Stem source request changed during compatible reuse"
            raise RuntimeError(message)
        self._clear_stem_generation_messages(request.sample_id)
        fresh = replace(
            processing,
            sample_id=request.sample_id,
            cache_dir=processing.cache_dir.with_name(f".generation-{uuid4().hex}"),
        )
        if self._stem_worker_pool is not None:
            capture = self._capture_generation_item(request.sample_id)
            if capture is None or capture.unavailable_reason is not None:
                message = "Original stem source metadata unavailable during incompatible reuse"
                raise RuntimeError(message)
            owner = request.interest_owner or StemGenerationInterests()
            if not self._request_captured_generation(
                fresh,
                owner,
                capture=capture,
                generation_intent=request.generation_intent,
                skip_completed=True,
            ):
                message = "Original stem generation intent changed during incompatible reuse"
                raise RuntimeError(message)
            return
        offline_source = self._capture_offline_source(request.sample_id, processing.source_version)
        self._admit_stem_generation(
            fresh,
            offline_source,
            skip_completed=True,
            interest_owner=request.interest_owner,
        )

    def _poll_stem_publications(self) -> None:
        for sample_id, pending in tuple(self._pending_stem_publications.items()):
            if sample_id in getattr(self, "_pair_deferred_sample_ids", ()):
                continue
            self._poll_stem_publication(sample_id, pending)

    def _poll_stem_publication(self, sample_id: int, pending: _PendingStemPublication) -> bool:
        with pending.retirement.activate():
            return self._apply_stem_publication(sample_id, pending)

    def _apply_stem_publication(self, sample_id: int, pending: _PendingStemPublication) -> bool:
        if not self._intent_current(pending.generation_intent):
            if self._project.stem_cache[sample_id] is pending.entry:
                self._reject_stem_publication(sample_id, pending)
            else:
                self._pending_stem_publications.pop(sample_id, None)
                self._release_pending_stems(pending)
            self._verified_candidates.pop(sample_id, None)
            return False
        if (
            self._project.stem_cache[sample_id] is not pending.entry
            or self._content_id(sample_id) != pending.content_id
            or self._project.sample_paths[sample_id] != pending.source_path
        ):
            self._pending_stem_publications.pop(sample_id, None)
            self._release_pending_stems(pending)
            return True
        return self._apply_current_stem_publication(sample_id, pending)

    def _apply_current_stem_publication(
        self, sample_id: int, pending: _PendingStemPublication
    ) -> bool:
        try:
            status = pending.source_ticket.publication_status()
        except (AttributeError, RuntimeError, TypeError, ValueError) as err:
            self._session.stem_generation_errors[sample_id] = (
                f"Stem publication status failed: {err}"
            )
            self._reject_stem_publication(sample_id, pending)
            return False
        if status in {"captured", "pending"}:
            return True
        self._restored_stem_candidates.pop(sample_id, None)
        if status != "accepted":
            reason = pending.source_ticket.rejection_reason() or "unspecified"
            self._session.stem_generation_errors[sample_id] = (
                "Prepared stem publication rejected by native source/request/timing "
                f"validation ({reason})"
            )
            if pending.prepared_pair is not None and reason != "invalid-geometry":
                # Choosing already verified disk references cancels this producer's
                # rollback rights only. It grants no live ACK or availability.
                pending.prepared_pair.select()
                self._publication_retries.retain(pending, pending.content_id, pending.source_path)
                return False
            self._reject_stem_publication(sample_id, pending)
            return False
        self._pending_stem_publications.pop(sample_id, None)
        available = pending.entry.model_copy(update={"available": True})
        self._project.stem_cache[sample_id] = available
        self._remember_verified_selection(sample_id, available)
        if sample_id in self._publication_retries.pending:
            self._session.stem_generation_errors.pop(sample_id, None)
        if pending.prepared_pair is not None:
            pending.prepared_pair.select()
            self._register_committed_pair(sample_id, pending.prepared_pair, pending.source_ticket)
            if sample_id not in self._pair_release_ordered:
                self._resident_pairs.add(sample_id)
                if self._project.pad_stem_mix_mode[sample_id] == "full_mix":
                    self._set_full_mix(sample_id)
        self._release_pending_stems(pending)
        self._mark_project_changed()
        return self._publish_all_stems_mode_if_preferred(sample_id, pending.entry.source_version)

    def _reject_stem_publication(self, sample_id: int, pending: _PendingStemPublication) -> None:
        try:
            self._restore_previous_stems(sample_id, pending)
        except (OSError, RuntimeError, ValueError) as err:
            self._session.stem_generation_errors[sample_id] = (
                f"Previous stem assignment restore deferred: {err}"
            )
            return  # Same held owner and bounded reservation remain reachable for retry.
        self._pending_stem_publications.pop(sample_id, None)
        self._release_pending_stems(pending)

    def _restore_previous_stems(self, sample_id: int, pending: _PendingStemPublication) -> None:
        prepared = None
        if pending.previous_entry is not None and pending.previous_lease is not None:
            prepared = (Path(pending.previous_entry.cache_dir).absolute(), pending.previous_lease)
        self._assets.restore_stems(sample_id, prepared)
        if prepared is not None:
            pending.previous_lease = None
        self._project.stem_cache[sample_id] = pending.previous_entry
        self._mark_project_changed()

    def _discard_pair(self, sample_id: int, prepared: PreparedStemPair) -> None:
        error = self._pair_preparations.discard(sample_id, prepared)
        if error is not None:
            self._session.stem_generation_errors[sample_id] = f"Stem pair cleanup deferred: {error}"

    def _release_pending_stems(self, pending: _PendingStemPublication) -> None:
        self._publication_retries.forget(pending)
        try:
            if pending.prepared_pair is not None:
                self._discard_pair(pending.sample_id, pending.prepared_pair)
        finally:
            try:
                if pending.previous_lease is not None:
                    pending.previous_lease.release()
            finally:
                pending.retirement.close()

    def _content_id(self, sample_id: int) -> str | None:
        content = self._project.pad_content[sample_id]
        return content.instance_id if content else None

    def _publication_replacement_owner(
        self, sample_id: int, entry: StemCacheEntry
    ) -> _PendingStemPublication | None:
        previous = self._pending_stem_publications.get(sample_id)
        if previous is None:
            return None
        retry = self._publication_retries.pending.get(sample_id)
        if (
            retry is None
            or retry.pending is not previous
            or not self._publication_retry_current(retry)
            or previous.entry.pair != entry.pair
            or previous.entry.source_version != entry.source_version
        ):
            message = "Another stem publication still owns this pad"
            raise RuntimeError(message)
        return previous

    def _transfer_publication_retry(
        self, previous: _PendingStemPublication, replacement: _PendingStemPublication
    ) -> None:
        if not self._publication_retries.replace(previous, replacement):
            return
        if replacement.previous_lease is not None:
            replacement.previous_lease.release()
        replacement.previous_entry = previous.previous_entry
        replacement.previous_lease = previous.previous_lease
        previous.previous_lease = None
        self._release_pending_stems(previous)

    def _poll_publication_retries(self) -> None:
        for retry in tuple(self._publication_retries.pending.values())[:8]:
            sample_id = retry.pending.sample_id
            if sample_id in getattr(self, "_pair_deferred_sample_ids", ()):
                continue
            if self._shutting_down or not self._publication_retry_current(retry):
                self._settle_publication_retry(retry)
                continue
            # A newly admitted retry owns an independent ACK. Polling must not
            # start another worker or exhaust its budget while that ACK is pending.
            if retry.pending.source_ticket.publication_status() != "rejected":
                continue
            if (
                sample_id in self._pair_preparations.pending
                or sample_id in self._session.active_sample_ids
                or sample_id in self._session.loading_sample_ids
                or sample_id in self._session.analyzing_sample_ids
            ):
                continue
            if retry.attempts >= self._publication_retries.MAX_ATTEMPTS:
                self._session.stem_generation_errors[sample_id] = (
                    "Stem publication retry limit reached; verified disk selection retained"
                )
                self._settle_publication_retry(retry)
                continue
            self._submit_publication_retry(retry)

    def _publication_retry_current(self, retry: StemPublicationRetry) -> bool:
        sample_id = retry.pending.sample_id
        return (
            self._project.stem_cache[sample_id] is retry.pending.entry
            and self._intent_current(retry.pending.generation_intent)
            and self._content_id(sample_id) == retry.content_id
            and self._project.sample_paths[sample_id] == retry.source_path
        )

    def _submit_publication_retry(self, retry: StemPublicationRetry) -> None:
        pending = retry.pending
        sample_id = pending.sample_id
        retry.attempts += 1
        try:
            ticket = self._capture_source_ticket(sample_id, pending.entry.source_version)
            if not pending.source_ticket.same_source_request(ticket):
                self._settle_publication_retry(retry)
                self._session.stem_generation_errors[sample_id] = (
                    "Stem publication cancelled because its source request changed"
                )
                return
            self._prepare_pair(
                sample_id,
                pending.entry,
                ticket,
                components=True,
                binding=StemPairIntentBinding(
                    generation_intent=pending.generation_intent,
                    interest_owner=pending.interest_owner,
                ),
            )
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            self._session.stem_generation_errors[sample_id] = (
                f"Stem publication reprepare deferred: {error}"
            )

    def _settle_publication_retry(self, retry: StemPublicationRetry) -> None:
        pending = retry.pending
        if self._pending_stem_publications.get(pending.sample_id) is pending:
            self._pending_stem_publications.pop(pending.sample_id)
        request = self._pair_preparations.pending.get(pending.sample_id)
        if (
            request is not None
            and request.previous_entry is pending.entry
            and request.content_id == retry.content_id
        ):
            self._pair_preparations.cancel(pending.sample_id)
        self._release_pending_stems(pending)

    def _capture_source_ticket(self, sample_id: int, source_version: str) -> PreparedSourceTicket:
        ticket = self._audio.capture_prepared_source(sample_id, source_version)
        if ticket is None:
            msg = "Native source admission returned no ticket"
            raise RuntimeError(msg)
        return ticket

    def _capture_offline_source(
        self, sample_id: int, source_version: str
    ) -> OfflineStemSourceLease:
        """Capture the own immutable original using only loaded Native metadata."""
        source = self._audio.capture_offline_stem_source(sample_id, source_version)
        path = self._project.sample_paths[sample_id]
        shape = self._target_shape_for_pad(sample_id)
        message = "Native offline source differs from its own loaded original"
        if source is None or path is None or shape is None:
            raise RuntimeError(message)
        if (
            source.source_version != source_version
            or Path(source.source_path) != Path(path).absolute()
            or source.target_shape != (shape.sample_rate_hz, shape.channels, shape.frame_count)
            or source.original_bytes <= 0
        ):
            raise RuntimeError(message)
        return source

    def _capture_generation_ticket(self, subscriber: StemSubscriber) -> PreparedSourceTicket:
        """Capture fresh live authority without laundering an obsolete offline source."""
        sample_id = subscriber.sample_id
        request = subscriber.request
        message = "Stem subscriber changed before fresh publication capture"
        if (
            not self._intent_current(subscriber.intent)
            or self._content_id(sample_id) != subscriber.content_id
        ):
            raise RuntimeError(message)
        path = self._project.sample_paths[sample_id]
        if request is not None and (
            path is None
            or self._loaded_source_version_for_pad(sample_id) != request.source_version
            or Path(path).absolute() != request.source_path
        ):
            raise RuntimeError(message)
        # Native checks the captured original generation and captures current
        # timing/window authority atomically; same-path/hash reload is still stale.
        ticket = self._audio.capture_prepared_stem_source(sample_id, subscriber.source)
        if ticket is None:
            message = "Native stem publication capture returned no ticket"
            raise RuntimeError(message)
        return ticket

    def _handle_stem_generation_error(
        self,
        sample_id: int,
        message: str,
        source_lease: OfflineStemSourceLease | None = None,
    ) -> None:
        """Apply a stem-generation failure event from a backend event source."""
        validate_sample_id(sample_id)
        if sample_id not in self._session.stem_generating_sample_ids:
            return
        if source_lease is None and (
            sample_id in self._generation_sources or sample_id in self._pending_stem_publications
        ):
            return

        self._clear_stem_generation_state(sample_id)
        self._session.stem_generation_progress.pop(sample_id, None)
        self._session.stem_generation_stage.pop(sample_id, None)
        self._session.stem_generation_errors[sample_id] = message

    def _stem_generation_blocker(self, sample_id: int) -> str | None:
        blockers = (
            (self._shutting_down, "Stem generation is unavailable after shutdown"),
            (
                self._project.sample_paths[sample_id] is None,
                "Cannot generate stems without a loaded sample",
            ),
            (
                sample_id in self._session.loading_sample_ids,
                "Cannot generate stems while the pad is loading",
            ),
            (
                sample_id in self._session.analyzing_sample_ids
                or sample_id in self._disk_selection_settlements
                or sample_id in self._pending_stem_publications
                or sample_id in self._pair_preparations.pending
                or sample_id in self._requested_preparations,
                "Cannot generate stems while another pad task is running",
            ),
            (
                self.is_stem_generation_running(sample_id),
                "Stem generation is already running for this pad",
            ),
        )
        for blocked, reason in blockers:
            if blocked:
                return reason
        return None

    def _entry_files_available(self, entry: StemCacheEntry) -> bool:
        if entry.pair is not None:
            # Durable structure is not live authority: the background native opener
            # verifies both complete areas before issuing any component publication.
            return entry.cache_dir == entry.pair.wav_generation
        return verified_stem_cache_available(entry)

    def _clear_stem_generation_state(self, sample_id: int) -> None:
        self._session.stem_generating_sample_ids.discard(sample_id)
        self._session.stem_generation_source_versions.pop(sample_id, None)
        self._generation_sources.pop(sample_id, None)
        self._generation_intents.pop(sample_id, None)
        pending = self._pending_stem_publications.pop(sample_id, None)
        if pending is not None:
            self._release_pending_stems(pending)
        self._restored_stem_candidates.pop(sample_id, None)
        self._session.stem_generation_progress.pop(sample_id, None)
        self._session.stem_generation_stage.pop(sample_id, None)
        self._session.stem_generation_execution.pop(sample_id, None)

    def _clear_stem_generation_messages(self, sample_id: int) -> None:
        self._session.stem_generation_errors.pop(sample_id, None)
        self._session.stem_generation_diagnostics.pop(sample_id, None)
        if self._diagnostics is not None:
            self._diagnostics.observe("stem_generation_diagnostic", None, pad_id=sample_id)
        self._session.stem_generation_progress.pop(sample_id, None)
        self._session.stem_generation_stage.pop(sample_id, None)
        self._session.stem_generation_execution.pop(sample_id, None)

    def _target_shape_for_pad(self, sample_id: int) -> AudioShape | None:
        fn = getattr(self._audio, "loaded_sample_shape", None)
        if fn is None:
            return self._duration_based_target_shape(sample_id)

        try:
            raw_shape = fn(sample_id)
        except RuntimeError, TypeError, ValueError:
            return self._duration_based_target_shape(sample_id)

        if not isinstance(raw_shape, tuple) or len(raw_shape) != 3:
            return self._duration_based_target_shape(sample_id)
        sample_rate_hz, channels, frame_count = raw_shape
        if (
            not isinstance(sample_rate_hz, int)
            or not isinstance(channels, int)
            or not isinstance(frame_count, int)
        ):
            return self._duration_based_target_shape(sample_id)
        if sample_rate_hz <= 0 or channels <= 0 or frame_count <= 0:
            return self._duration_based_target_shape(sample_id)
        return AudioShape(
            sample_rate_hz=sample_rate_hz,
            channels=channels,
            frame_count=frame_count,
        )

    def _duration_based_target_shape(self, sample_id: int) -> AudioShape | None:
        sample_rate_hz = self._output_sample_rate_hz()
        duration_s = self._project.sample_durations[sample_id]
        if sample_rate_hz is None or sample_rate_hz <= 0 or duration_s is None:
            return None
        frame_count = round(duration_s * sample_rate_hz)
        if frame_count <= 0:
            return None
        return AudioShape(sample_rate_hz=sample_rate_hz, channels=1, frame_count=frame_count)

    def _resolve_stem_identity(self, job: StemGenerationJob) -> None:
        """Seal full producer files off-thread, then return admission to control."""
        source_lease = job.begin_resolution()
        if source_lease is None:
            self._put_generation_event(self._settled_job_event(job))
            return
        request = job.request
        event_type: _StemBackendEventType = "identity"
        error = None
        completed = False
        try:
            self._stem_resources.entry()
            request = self._resolved_processing_request(
                replace(
                    request, original_bytes=self._verified_original_extent(request, source_lease)
                )
            )
            processing = capture_processing_files(request)
            job.processing_files = processing[0] if processing is not None else None
            job.producer_identity = processing[1] if processing is not None else None
            if processing is not None:
                processing[0].verify()
                request = replace(
                    request,
                    encoder_contract=self._processing_contract(request, processing[1]),
                    python_source_roots=(
                        processing_source_roots(request, processing[0])
                        if processing[1] is not None
                        else ()
                    ),
                )
            job.resolved_request = request
            job.generation_fingerprint = request_fingerprint(request, job.producer_identity)
            completed = True
        except (OSError, RuntimeError, TypeError, ValueError) as err:
            event_type = "error"
            error = str(err) or type(err).__name__
            completed = True
        finally:
            job.finish_resolution()
            if not completed:
                self._put_generation_event(
                    self._settled_job_event(job, error="Stem identity worker failed unexpectedly")
                )
        self._put_generation_event(
            _StemBackendEvent(
                sample_id=job.request.sample_id,
                source_version=job.request.source_version,
                source_lease=source_lease,
                cache_dir=job.request.cache_dir,
                event_type=event_type,
                error=error,
                source_path=job.request.source_path,
            )
        )

    @staticmethod
    def _verified_original_extent(
        request: StemGenerationRequest, source: OfflineStemSourceLease
    ) -> int:
        if (
            source.source_version != request.source_version
            or Path(source.source_path) != request.source_path
            or source.target_shape
            != (
                request.target_shape.sample_rate_hz,
                request.target_shape.channels,
                request.target_shape.frame_count,
            )
        ):
            message = "Stem offline source differs from its captured request"
            raise ValueError(message)
        source.verify()
        return source.original_bytes

    @staticmethod
    def _settled_job_event(
        job: StemGenerationJob, *, error: str | None = None
    ) -> _StemBackendEvent:
        return _StemBackendEvent(
            sample_id=job.request.sample_id,
            source_version=job.request.source_version,
            source_lease=job.source,
            cache_dir=job.request.cache_dir,
            event_type="settled",
            error=error,
            source_path=job.request.source_path,
        )

    def _run_stem_backend(self, job: StemGenerationJob) -> None:
        source_lease = job.begin()
        if source_lease is None:
            self._put_generation_event(self._settled_job_event(job))
            return
        request = job.resolved_request
        if request is None:
            job.finish_read()
            self._put_generation_event(self._settled_job_event(job))
            return

        def report_progress(
            percent: float | None, stage: str | None, execution: StemExecution | None
        ) -> None:
            self._put_generation_event(
                _StemBackendEvent(
                    sample_id=request.sample_id,
                    source_version=request.source_version,
                    source_lease=source_lease,
                    cache_dir=request.cache_dir,
                    event_type="progress",
                    percent=percent,
                    stage=stage,
                    source_path=request.source_path,
                    execution=execution,
                )
            )

        event = None
        processing = (
            (job.processing_files, job.producer_identity)
            if job.processing_files is not None
            else None
        )
        resources = ExitStack()
        try:
            resources.enter_context(
                offline_work_scope(
                    job.cancel_event,
                    lambda: report_progress(None, "Waiting for background resources", None),
                )
            )
            self._stem_resources.entry()
            if processing is not None:
                processing[0].verify()
            with stem_command_scope(
                job.cancel_event,
                directory=request.cache_dir,
                resources=self._stem_resources,
                playback=self._stem_playback_active,
            ):
                result = self._stem_backend.generate(
                    request, StemExecutionProgress(report_progress)
                )
            self._verified_original_extent(request, source_lease)
            self._verify_generation_provenance(job, request, processing, result)
            event = _StemBackendEvent(
                sample_id=request.sample_id,
                source_version=request.source_version,
                source_lease=source_lease,
                cache_dir=request.cache_dir,
                event_type="success",
                result=result,
                source_path=request.source_path,
            )
        except (EOFError, OSError, RuntimeError, TypeError, ValueError, wave.Error) as err:
            event = _StemBackendEvent(
                sample_id=request.sample_id,
                source_version=request.source_version,
                source_lease=source_lease,
                cache_dir=request.cache_dir,
                event_type="error",
                error=str(err) or type(err).__name__,
                source_path=request.source_path,
            )
        finally:
            processing = None
            try:
                job.finish_read()
            finally:
                resources.close()
            if event is None:
                # An unexpected exception still settles actual reads and private
                # cleanup capacity before it propagates to the worker boundary.
                self._put_generation_event(
                    self._settled_job_event(job, error="Stem inference worker failed unexpectedly")
                )
        if event is not None:
            self._put_generation_event(event)

    @staticmethod
    def _resolved_processing_request(request: StemGenerationRequest) -> StemGenerationRequest:
        return replace(request, encoder_executable=resolved_demucs_encoder(request.model_cache_dir))

    @staticmethod
    def _processing_contract(request: StemGenerationRequest, producer: str | None) -> str | None:
        if request.encoder_executable is None or producer is None:
            return None
        if request.separator == "bs-roformer:musdb18hq":
            return bs_input_contract(request.encoder_executable)
        return encoder_contract(request.encoder_executable)

    @staticmethod
    def _verify_generation_provenance(
        job: StemGenerationJob,
        request: StemGenerationRequest,
        processing: tuple[SealedProcessingFiles, str | None] | None,
        result: StemGenerationResult,
    ) -> None:
        if processing is None:
            return
        processing[0].verify()
        expected_producer = (
            ("bs-roformer", BS_ROFORMER_MODEL_NAME)
            if request.separator == "bs-roformer:musdb18hq"
            else ("demucs", "htdemucs")
        )
        if (
            processing[1] is not None
            and (result.backend_name, result.model_name) == expected_producer
            and (
                result.decoder_verified
                if request.separator == "bs-roformer:musdb18hq"
                else result.encoder_verified
            )
        ):
            job.generation_fingerprint = request_fingerprint(request, processing[1])
            job.producer_identity = processing[1] if job.generation_fingerprint else None
        else:
            # A known captured producer is not proof that a backend executed it.
            job.generation_fingerprint = None
            job.producer_identity = None

    def _put_generation_event(self, event: _StemBackendEvent) -> None:
        with self._generation_event_lock:
            if not self._shutting_down:
                self._stem_generation_events.put(event)

    def _cancel_jobs(self, sample_id: int) -> None:
        requested = self._requested_generations.pop(sample_id, None)
        if requested is not None:
            requested.owner.invalidate_all()
        preparation = self._requested_preparations.pop(sample_id, None)
        binding = preparation.spec.binding if preparation is not None else None
        if binding is not None and binding.interest_owner is not None:
            binding.interest_owner.invalidate_all()
        self._verified_candidates.pop(sample_id, None)
        self._verified_sets.pop(sample_id, None)
        for path, job in tuple(self._jobs.items()):
            if (
                not job.remove_subscriber(sample_id)
                and path not in self._inference_jobs
                and not job.running
            ):
                self._discard_generation_artifacts(sample_id, path)

    def _is_current_generation(self, event: _StemBackendEvent, *, resolving: bool = False) -> bool:
        content = self._project.pad_content[event.sample_id]
        content_id = content.instance_id if content is not None else None
        source_path = self._project.sample_paths[event.sample_id]
        return (
            event.sample_id in self._session.stem_generating_sample_ids
            and self._session.stem_generation_source_versions.get(event.sample_id)
            == event.source_version
            and self._generation_sources.get(event.sample_id) is event.source_lease
            and event.source_lease is not None
            and self._audio.offline_stem_source_is_current(event.sample_id, event.source_lease)
            and event.content_id == content_id
            and (
                event.source_path is None
                or (source_path is not None and Path(source_path).absolute() == event.source_path)
            )
            and (
                event.generation_intent is None
                or (
                    self._generation_intents.get(event.sample_id) == event.generation_intent
                    and event.generation_intent.settings_revision
                    == self._project.stem_processing_revision
                    and (resolving or self._intents.is_current(event.generation_intent))
                )
            )
        )

    def _discard_generation_artifacts(self, sample_id: int, cache_dir: Path) -> None:
        try:
            job = self._jobs.pop(cache_dir, None)
            self._inference_jobs.discard(cache_dir)
            self._waiting_jobs.discard(cache_dir)
            self._reuse_only_jobs.discard(cache_dir)
            self._skip_completed_jobs.discard(cache_dir)
            if job is not None:
                job.dispose()
        except (OSError, ValueError) as err:
            self._record_generation_diagnostic(
                sample_id, f"Private stem artifact cleanup failed: {err}"
            )

    def _record_generation_result(
        self,
        sample_id: int,
        result: StemGenerationResult | None,
    ) -> None:
        if result is None:
            return
        if result.execution is not None:
            self._session.stem_generation_execution[sample_id] = result.execution
        else:
            self._session.stem_generation_execution.pop(sample_id, None)
        if result.diagnostic:
            self._record_generation_diagnostic(sample_id, result.diagnostic)
        elif result.cpu_fallback:
            self._record_generation_diagnostic(
                sample_id, "Stem generation completed on CPU after CUDA fallback"
            )

    def _publish_all_stems_mode_if_preferred(self, sample_id: int, source_version: str) -> bool:
        entry = self._project.stem_cache[sample_id]
        if entry is not None and self._committed_residency.contains(sample_id):
            demand = self._remember_demand(sample_id, entry)
            demand.policy = self._project.preload_stem_loops_on_startup
            if self._project.pad_stem_mix_mode[sample_id] == "all_stems":
                demand.explicit = True
                demand.state = "queued"
                demand.error = None
        if self._project.pad_stem_mix_mode[sample_id] != "all_stems":
            return True

        if self._committed_residency.contains(sample_id):
            return self._committed_residency.request(
                sample_id, "all_stems", self._project.pad_stem_intent[sample_id].enabled_mask
            )
        if self._requires_committed_residency(sample_id):
            return False

        try:
            self._audio.set_stem_mix_mode(sample_id, "all_stems", source_version)
            self._audio.set_stem_enabled_mask(
                sample_id,
                self._project.pad_stem_intent[sample_id].enabled_mask,
                source_version,
            )
        except (RuntimeError, ValueError) as err:
            self._session.stem_generation_errors[sample_id] = (
                f"Stem generation completed but mix update failed: {err}"
            )
            return False
        return True

    def _register_committed_pair(
        self, sample_id: int, prepared: PreparedStemPair, ticket: PreparedSourceTicket
    ) -> None:
        if not self._committed_residency.supported:
            return
        entry = self._project.stem_cache[sample_id]
        if entry is None or entry.pair is None:
            return
        try:
            self._committed_residency.register(
                sample_id,
                prepared,
                ticket,
                entry,
                self._content_id(sample_id),
                self._project.sample_paths[sample_id],
                active=sample_id in self._session.active_sample_ids,
            )
        except (RuntimeError, TypeError, ValueError) as error:
            self._report_residency_error(sample_id, f"Committed stem registration failed: {error}")

    def _requires_committed_residency(self, sample_id: int) -> bool:
        entry = self._project.stem_cache[sample_id]
        return entry is not None and entry.pair is not None and self._committed_residency.supported

    def _committed_selection_current(
        self,
        sample_id: int,
        entry: StemCacheEntry,
        content_id: str | None,
        source_path: str | None,
    ) -> bool:
        current = self._project.stem_cache[sample_id]
        return (
            not self._shutting_down
            and self._content_id(sample_id) == content_id
            and self._project.sample_paths[sample_id] == source_path
            and current is not None
            and current.source_version == entry.source_version
            and current.pair == entry.pair
        )

    def _committed_source_current(
        self,
        sample_id: int,
        entry: StemCacheEntry,
        content_id: str | None,
        source_path: str | None,
    ) -> bool:
        """Retain old audio control for the same content while disk selects a newer pair."""
        current = self._project.stem_cache[sample_id]
        return (
            not self._shutting_down
            and self._content_id(sample_id) == content_id
            and self._project.sample_paths[sample_id] == source_path
            and current is not None
            and current.source_version == entry.source_version
        )

    def _report_residency_error(self, sample_id: int, message: str | None) -> None:
        if message is None:
            status = self._committed_residency.status(sample_id)
            if (
                status is not None
                and status.requested_mode == "full_mix"
                and status.effective == "effective"
            ):
                self._pending_full_mix_releases.discard(sample_id)
            previous = self._residency_errors.pop(sample_id, None)
            if (
                previous is not None
                and self._session.stem_generation_errors.get(sample_id) == previous
            ):
                self._session.stem_generation_errors.pop(sample_id, None)
            return
        self._residency_errors[sample_id] = message
        self._session.stem_generation_errors[sample_id] = message
