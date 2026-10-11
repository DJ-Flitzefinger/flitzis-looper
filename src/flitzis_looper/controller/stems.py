import hashlib
import re
import threading
import wave
from collections.abc import Callable
from dataclasses import dataclass, replace
from pathlib import Path
from queue import Empty, SimpleQueue
from typing import TYPE_CHECKING, Literal
from uuid import uuid4

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
)
from flitzis_looper.controller.stem_cache import (
    cache_dir_for_sample_id,
    cache_dir_matches_sample_id,
    expected_stem_files,
    promote_generation_artifacts,
    source_version_for_sample_path,
    verified_stem_cache_available,
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
from flitzis_looper.controller.stem_job import StemGenerationJob, StemSubscriber
from flitzis_looper.controller.stem_pair_preparation import (
    PairPreparation,
    StemPairIntentBinding,
    StemPairPreparationQueue,
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
from flitzis_looper.controller.stem_separators import (
    SelectedStemGenerationBackend,
    separator_model_cache_dir,
)
from flitzis_looper.controller.stem_workers import StemWorkerPool
from flitzis_looper.encoder_provenance import encoder_contract
from flitzis_looper.models import (
    STEM_COMPONENT_MASK,
    STEM_MASK_DISPLAY_MODES,
    STEM_MIX_MODES,
    StemCacheEntry,
    StemGridIndicatorState,
    StemMaskDisplayMode,
    StemMixMode,
    validate_sample_id,
)
from flitzis_looper.project_materials import original_asset
from flitzis_looper.stem_pair_selection import STEM_FINGERPRINT_SCHEMA

if TYPE_CHECKING:
    from flitzis_looper.models import ProjectState, SessionState
    from flitzis_looper_audio import (
        AudioEngine,
        OfflineStemSourceLease,
        PreparedSourceTicket,
        PreparedStemPair,
        ProjectAssetLease,
        SealedProcessingFiles,
    )

type StemTaskRunner = Callable[[Callable[[], None]], None]
type _StemBackendEventType = Literal["identity", "progress", "success", "error", "settled"]


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
    ) -> None:
        super().__init__(project, session, audio, on_project_changed)
        self._assets = asset_lifecycle or ProjectAssetLifecycle(project, audio)
        self._jobs: dict[Path, StemGenerationJob] = {}
        self._intents = StemIntentRegistry()
        self._generation_intents: dict[int, StemGenerationIntent] = {}
        self._inference_jobs: set[Path] = set()
        self._waiting_jobs: set[Path] = set()
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
        self._stem_worker_pool: StemWorkerPool | None = None
        if stem_task_runner is None:
            self._stem_worker_pool = StemWorkerPool()
            self._stem_task_runner: StemTaskRunner = self._stem_worker_pool
        else:
            self._stem_task_runner = stem_task_runner
        self._stem_generation_events: SimpleQueue[_StemBackendEvent] = SimpleQueue()
        self._generation_sources: dict[int, OfflineStemSourceLease] = {}
        self._disk_selection_settlements: dict[int, _DiskSelectionSettlement] = {}
        self._commit_disk_selection: (
            Callable[[int, StemCacheEntry | None, StemCacheEntry, Callable[[], bool]], None] | None
        ) = None
        self._flush_superseded_disk_selection: Callable[[], None] | None = None
        self._pending_stem_publications: dict[int, _PendingStemPublication] = {}
        self._restored_stem_candidates: dict[int, StemCacheEntry] = {}
        self._pair_preparations = StemPairPreparationQueue(
            audio, self._assets, self._stem_task_runner
        )
        self._resident_pairs: set[int] = set()
        self._publication_retries = StemPublicationRetries()
        self._pair_release_ordered: set[int] = set()
        self._fenced_pair_ids: set[int] = set()
        self._residency_errors: dict[int, str] = {}
        self._pending_full_mix_releases: set[int] = set()
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
        self._on_frame_render_callbacks.append(self._committed_residency.poll)

    def shut_down(self) -> None:
        """Cancel separator publication while retaining every running file reader."""
        with self._generation_event_lock:
            self._shutting_down = True
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
        self._verified_sets.clear()
        self._verified_candidates.clear()
        self._waiting_jobs.clear()
        self._committed_residency.shut_down()
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

    def bind_disk_selection_commit(
        self,
        commit: Callable[[int, StemCacheEntry | None, StemCacheEntry, Callable[[], bool]], None],
        flush_superseded: Callable[[], None] | None = None,
    ) -> None:
        """Use the project's existing fenced atomic writer for complete selections."""
        self._commit_disk_selection = commit
        self._flush_superseded_disk_selection = flush_superseded

    def generate_stems_async(self, sample_id: int) -> bool:
        """Schedule source-leased offline generation independently of pad playback."""
        validate_sample_id(sample_id)
        self._assets.sync_assignments()
        self._expire_generation_interests()
        self._clear_stem_generation_messages(sample_id)

        blocker = self.stem_generation_block_reason(sample_id)
        if blocker is not None:
            if self._stem_generation_blocker(sample_id) is None and self._verified_set_current(
                sample_id
            ):
                self._session.stem_generation_diagnostics[sample_id] = blocker
                return True
            self._session.stem_generation_errors[sample_id] = blocker
            return False

        source_version = self._loaded_source_version_for_pad(sample_id)
        if source_version is None:
            self._session.stem_generation_errors[sample_id] = (
                "Cannot generate stems because the source file is missing"
            )
            return False

        self._session.stem_generating_sample_ids.add(sample_id)
        self._session.stem_generation_source_versions[sample_id] = source_version
        request = self._generation_request(sample_id, source_version)
        if request is None:
            return False

        try:
            source = self._capture_offline_source(sample_id, source_version)
            return self._admit_stem_generation(request, source)
        except (AttributeError, OSError, RuntimeError, TypeError, ValueError) as err:
            self._clear_stem_generation_state(sample_id)
            self._session.stem_generation_errors[sample_id] = f"Stem admission failed: {err}"
            return False

    def _generation_request(
        self, sample_id: int, source_version: str
    ) -> StemGenerationRequest | None:
        """Capture current transform settings and output shape for the ordinary worker."""
        target_shape = self._target_shape_for_pad(sample_id)
        if target_shape is None:
            self._clear_stem_generation_state(sample_id)
            self._session.stem_generation_errors[sample_id] = (
                "Cannot generate stems because the loaded sample shape is unavailable"
            )
            return None

        sample_path = self._project.sample_paths[sample_id]
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
        return StemGenerationRequest(
            sample_id=sample_id,
            source_path=source_path,
            source_version=source_version,
            cache_dir=Path.cwd() / cache_dir / f".generation-{uuid4().hex}",
            target_shape=target_shape,
            model_cache_dir=separator_model_cache_dir(self._project.stem_separator),
            separator=self._project.stem_separator,
            device_policy="auto",
            demucs_shifts=self._project.demucs_shifts,
            demucs_overlap=self._project.demucs_overlap,
        )

    def _admit_stem_generation(
        self,
        request: StemGenerationRequest,
        source_lease: OfflineStemSourceLease,
        *,
        reuse_only: bool = False,
        skip_completed: bool = False,
    ) -> bool:
        """Resolve complete producer identity before the control owner admits inference."""
        sample_id = request.sample_id
        content = self._project.pad_content[sample_id]
        content_id = content.instance_id if content is not None else None
        intent = self._intents.capture(request, self._project.stem_processing_revision)
        job = StemGenerationJob(request, source_lease, self._assets, content_id, intent=intent)
        self._session.stem_generating_sample_ids.add(sample_id)
        self._session.stem_generation_source_versions[sample_id] = request.source_version
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
        return self._schedule_stem_job(job, resolve=True)

    def _compatible_completed_entry(
        self, request: StemGenerationRequest, producer_identity: str | None
    ) -> StemCacheEntry | None:
        fingerprint = request_fingerprint(request, producer_identity)
        if fingerprint is None:
            return None
        return next(
            (
                entry.model_copy(deep=True)
                for entry in self._project.stem_cache
                if entry is not None
                and entry.fingerprint_schema == STEM_FINGERPRINT_SCHEMA
                and entry.generation_fingerprint == fingerprint
                and entry.producer_identity == producer_identity
                and (entry.available or entry.pair is not None)
            ),
            None,
        )

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
                offline_source = self._capture_offline_source(sample_id, entry.source_version)
                # Discovery is a hint. Full producer capture and the authoritative
                # equality decision still occur in the ordinary bounded lane.
                request = replace(
                    request,
                    cache_dir=request.cache_dir / f".generation-{uuid4().hex}",
                )
                return self._admit_stem_generation(request, offline_source, reuse_only=True)
            except (OSError, RuntimeError, TypeError, ValueError) as error:
                self._session.stem_generation_errors[sample_id] = (
                    f"Compatible stem reuse failed: {error}"
                )
                return False
        return False

    def _schedule_stem_job(self, job: StemGenerationJob, *, resolve: bool = False) -> bool:
        try:
            if not resolve:
                self._inference_jobs.add(job.request.cache_dir)
            self._stem_task_runner(
                lambda: self._resolve_stem_identity(job) if resolve else self._run_stem_backend(job)
            )
        except (OSError, RuntimeError) as error:
            self._inference_jobs.discard(job.request.cache_dir)
            self._waiting_jobs.discard(job.request.cache_dir)
            for subscriber in job.subscribers():
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
                self._project.stem_cache[sample_id] = None
                if self._project.pad_stem_mix_mode[sample_id] != "full_mix":
                    self._project.pad_stem_mix_mode[sample_id] = "full_mix"
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
        """Publish restored prepared stems after the restored full mix has loaded."""
        validate_sample_id(sample_id)
        if sample_id in self._fenced_pair_ids:
            message = "Unsupported stem pair metadata requires explicit recovery"
            self._session.stem_generation_errors[sample_id] = message
            return False
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
            self._project.stem_cache[sample_id] = None
            if self._project.pad_stem_mix_mode[sample_id] != "full_mix":
                self._project.pad_stem_mix_mode[sample_id] = "full_mix"
            self._mark_project_changed()
            self._assets.sync_assignments()
            return True

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

    def invalidate_stem_cache(self, sample_id: int) -> None:
        """Mark cached stem metadata unavailable for a pad."""
        validate_sample_id(sample_id)
        with self._assets.admission():
            self._invalidate_stem_cache(sample_id)

    def _invalidate_stem_cache(self, sample_id: int) -> None:
        self._assets.sync_assignments()
        self._committed_residency.cancel(sample_id)
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
        self._reset_stem_mask_state(sample_id)
        self._assets.sync_assignments()

    def stem_mix_mode(self, sample_id: int) -> StemMixMode:
        """Return the durable stem mix preference for a pad."""
        validate_sample_id(sample_id)
        return self._project.pad_stem_mix_mode[sample_id]

    def stem_residency_status(self, sample_id: int) -> StemResidencyStatus | None:
        """Return own disk/residency/effective receipts independently from saved mode."""
        validate_sample_id(sample_id)
        try:
            return self._committed_residency.status(sample_id)
        except (RuntimeError, TypeError, ValueError) as error:
            self._report_residency_error(sample_id, f"Stem residency status failed: {error}")
            return None

    def stems_available(self, sample_id: int) -> bool:
        """Return whether current stem cache metadata is available for UI controls."""
        validate_sample_id(sample_id)
        entry = self._project.stem_cache[sample_id]
        return entry is not None and entry.available

    def has_stem_cache(self, sample_id: int) -> bool:
        """Return whether a pad has any tracked stem cache metadata."""
        validate_sample_id(sample_id)
        return self._project.stem_cache[sample_id] is not None

    def stem_enabled_mask(self, sample_id: int) -> int:
        """Return the session-only enabled component-stem mask for a pad."""
        validate_sample_id(sample_id)
        return int(self._session.pad_stem_enabled_mask[sample_id])

    def stem_mask_display_mode(self, sample_id: int) -> StemMaskDisplayMode:
        """Return the session-only bottom-bar stem mask display mode."""
        validate_sample_id(sample_id)
        return self._session.pad_stem_mask_display_mode[sample_id]

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
        if sample_id in self._session.stem_generating_sample_ids:
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
        if mode not in STEM_MIX_MODES:
            msg = "stem mix mode must be full_mix or all_stems"
            raise ValueError(msg)

        if mode == "full_mix":
            return self._set_full_mix(sample_id)
        if sample_id in self._publication_retries.pending:
            self._project.pad_stem_mix_mode[sample_id] = mode
            self._mark_project_changed()
            return True

        source_version = self._current_prepared_source_version(sample_id)
        if source_version is None:
            return False

        entry = self._project.stem_cache[sample_id]
        if (
            entry is not None
            and entry.pair is not None
            and (self._committed_residency.supported or sample_id not in self._resident_pairs)
        ):
            return self._request_pair_mode(sample_id, entry, source_version)

        if not self._publish_all_stems_state(sample_id, source_version):
            return False

        if mode != self._project.pad_stem_mix_mode[sample_id]:
            self._project.pad_stem_mix_mode[sample_id] = mode
            self._mark_project_changed()

        return True

    def _set_full_mix(self, sample_id: int) -> bool:
        had_committed_selection = self._committed_residency.contains(sample_id)
        if had_committed_selection:
            if self._committed_residency.request(
                sample_id, "full_mix", self._session.pad_stem_enabled_mask[sample_id]
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
        if self._committed_residency.contains(sample_id):
            if self._committed_residency.request(
                sample_id, "all_stems", self._session.pad_stem_enabled_mask[sample_id]
            ):
                self._project.pad_stem_mix_mode[sample_id] = "all_stems"
                self._mark_project_changed()
                return True
            if self._committed_residency.contains(sample_id):
                return False
            # A terminal registration was withdrawn by this explicit gesture.
            # Retry through fresh source/pair preparation and its own callback ACK.
        try:
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
            return False
        self._project.pad_stem_mix_mode[sample_id] = "all_stems"
        self._mark_project_changed()
        return True

    def delete_stems(self, sample_id: int) -> bool:
        """Retire cached stems after their final reader and return to full mix."""
        validate_sample_id(sample_id)
        try:
            with self._assets.admission():
                return self._delete_stems(sample_id)
        except RuntimeError as error:
            self._session.stem_generation_errors[sample_id] = str(error)
            return False

    def _delete_stems(self, sample_id: int) -> bool:
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
                return False

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

        self._reset_stem_mask_state(sample_id)

        if changed:
            self._mark_project_changed()

        self._assets.sync_assignments()
        return changed

    def set_stem_enabled_mask(
        self,
        sample_id: int,
        enabled_stem_mask: int,
        display_mode: StemMaskDisplayMode = "custom",
    ) -> bool:
        """Set the session-only enabled component-stem mask for a pad."""
        validate_sample_id(sample_id)
        if enabled_stem_mask < 0 or enabled_stem_mask & ~STEM_COMPONENT_MASK:
            msg = "stem enabled mask must contain only component stems"
            raise ValueError(msg)
        if display_mode not in STEM_MASK_DISPLAY_MODES:
            msg = "stem mask display mode must be custom, instrumental, or all"
            raise ValueError(msg)

        current_display_mode = self._session.pad_stem_mask_display_mode[sample_id]
        if display_mode == "custom":
            self._session.pad_stem_last_custom_mask[sample_id] = enabled_stem_mask
        elif current_display_mode == "custom":
            self._session.pad_stem_last_custom_mask[sample_id] = (
                self._session.pad_stem_enabled_mask[sample_id]
            )

        if (
            enabled_stem_mask == self._session.pad_stem_enabled_mask[sample_id]
            and display_mode == current_display_mode
        ):
            return True

        self._session.pad_stem_enabled_mask[sample_id] = enabled_stem_mask
        self._session.pad_stem_mask_display_mode[sample_id] = display_mode
        return self.publish_stem_enabled_mask_if_available(sample_id)

    def publish_stem_mix_mode_if_available(self, sample_id: int) -> bool:
        """Publish all-stems mode to Rust when current prepared stems are available."""
        validate_sample_id(sample_id)
        if self._project.pad_stem_mix_mode[sample_id] != "all_stems":
            return True

        entry = self._project.stem_cache[sample_id]
        if entry is None or not entry.available:
            return True

        source_version = self._current_prepared_source_version(sample_id)
        if source_version is None:
            return False

        return self._publish_stem_mix_mode(sample_id, source_version)

    def publish_stem_enabled_mask_if_available(self, sample_id: int) -> bool:
        """Publish the session stem mask to Rust when current prepared stems are available."""
        validate_sample_id(sample_id)
        if self._project.pad_stem_mix_mode[sample_id] != "all_stems":
            return True

        entry = self._project.stem_cache[sample_id]
        if entry is None or not entry.available:
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
        if entry is None or not entry.available or self._project.sample_paths[sample_id] is None:
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
                sample_id, "all_stems", self._session.pad_stem_enabled_mask[sample_id]
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
                sample_id, "all_stems", self._session.pad_stem_enabled_mask[sample_id]
            )
        if self._requires_committed_residency(sample_id):
            return False
        try:
            self._audio.set_stem_enabled_mask(
                sample_id,
                self._session.pad_stem_enabled_mask[sample_id],
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
        return sample_id in self._session.stem_generating_sample_ids

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
        return self._session.stem_generation_stage.get(sample_id)

    def stem_generation_block_reason(self, sample_id: int) -> str | None:
        """Return the current non-I/O blocker for performer stem generation."""
        validate_sample_id(sample_id)
        return self._stem_generation_blocker(sample_id) or (
            "Stems for the current model are already present"
            if self._verified_set_current(sample_id)
            else None
        )

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
        for sample_id in tuple(self._verified_sets):
            if not self._verified_set_current(sample_id):
                self._verified_sets.pop(sample_id, None)
        for sample_id, (intent, _request, _guard) in tuple(self._verified_candidates.items()):
            if not self._intent_current(intent):
                self._verified_candidates.pop(sample_id, None)
        self._prune_generation_intents()
        while True:
            try:
                event = self._stem_generation_events.get_nowait()
            except Empty:
                return

            job = self._jobs.get(event.cache_dir)
            if job is None:
                continue
            self._dispatch_job_event(event, job)

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
        self._intents.prune(live)

    def _dispatch_job_event(self, event: _StemBackendEvent, job: StemGenerationJob) -> None:
        if event.event_type == "identity":
            self._dispatch_resolved_identity(event, job)
            return
        current = tuple(
            subscriber
            for subscriber in job.subscribers()
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
        if event.event_type == "success":
            self._finish_shared_generation(event, current)
        else:
            for subscriber in current:
                if event.event_type == "progress":
                    self._handle_stem_generation_progress(
                        subscriber.sample_id, event.percent, event.stage, subscriber.source
                    )
                elif event.error is not None:
                    self._handle_stem_generation_error(
                        subscriber.sample_id, event.error, subscriber.source
                    )
        if event.event_type != "progress":
            # Promote/fan out once before dropping the job's physical ownership.
            for subscriber in job.subscribers():
                if self._generation_sources.get(subscriber.sample_id) is subscriber.source:
                    self._clear_stem_generation_state(subscriber.sample_id)
                if (
                    subscriber.sample_id not in self._pair_preparations.pending
                    and subscriber.sample_id not in self._pending_stem_publications
                ):
                    self._verified_candidates.pop(subscriber.sample_id, None)
            self._discard_generation_artifacts(event.sample_id, event.cache_dir)
            self._start_waiting_generations()

    def _dispatch_resolved_identity(self, event: _StemBackendEvent, job: StemGenerationJob) -> None:
        try:
            self._resolve_generation_admission(event, job)
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            for subscriber in job.subscribers():
                if self._generation_sources.get(subscriber.sample_id) is subscriber.source:
                    self._clear_stem_generation_state(subscriber.sample_id)
                    if self._intent_current(subscriber.intent):
                        self._session.stem_generation_errors[subscriber.sample_id] = (
                            f"Stem resolved admission failed: {error}"
                        )
            self._discard_generation_artifacts(event.sample_id, event.cache_dir)

    def _expire_generation_interests(self) -> None:
        for path, job in tuple(self._jobs.items()):
            for subscriber in job.subscribers():
                intent = subscriber.intent
                if intent is None or (
                    intent.settings_revision == self._project.stem_processing_revision
                    and (
                        path not in self._inference_jobs | self._waiting_jobs
                        or self._intents.is_current(intent)
                    )
                ):
                    continue
                job.remove_subscriber(subscriber.sample_id)
                if self._generation_sources.get(subscriber.sample_id) is subscriber.source:
                    self._clear_stem_generation_state(subscriber.sample_id)
            if not job.subscribers() and path not in self._inference_jobs and not job.running:
                self._discard_generation_artifacts(job.request.sample_id, path)

    def _resolve_generation_admission(
        self, event: _StemBackendEvent, job: StemGenerationJob
    ) -> None:
        self._bind_generation_subscribers(event, job)
        current = job.subscribers()
        if not current:
            self._discard_generation_artifacts(event.sample_id, event.cache_dir)
            return
        resolved = job.resolved_request
        if resolved is None:
            message = "Stem identity worker returned no resolved request"
            raise RuntimeError(message)
        compatible = (
            None
            if event.cache_dir in self._skip_completed_jobs
            else self._compatible_completed_entry(resolved, job.producer_identity)
        )
        if compatible is not None:
            self._reuse_resolved_generation(job, compatible, resolved)
            self._discard_generation_artifacts(event.sample_id, event.cache_dir)
            return
        if event.cache_dir in self._reuse_only_jobs:
            for subscriber in current:
                self._clear_stem_generation_state(subscriber.sample_id)
                self._session.stem_generation_diagnostics[subscriber.sample_id] = (
                    "Saved stems differ from the current producer; Generate to replace them"
                )
            self._discard_generation_artifacts(event.sample_id, event.cache_dir)
            return
        shared = next(
            (
                existing
                for path, existing in self._jobs.items()
                if path != event.cache_dir
                and job.generation_fingerprint is not None
                and existing.generation_fingerprint == job.generation_fingerprint
                and any(self._intent_current(sub.intent) for sub in existing.subscribers())
                and path in self._inference_jobs | self._waiting_jobs
            ),
            None,
        )
        if shared is not None:
            job.transfer_subscribers_to(shared)
            self._discard_generation_artifacts(event.sample_id, event.cache_dir)
            return
        self._waiting_jobs.add(event.cache_dir)
        self._start_waiting_generations()

    def _bind_generation_subscribers(
        self, event: _StemBackendEvent, job: StemGenerationJob
    ) -> None:
        for subscriber in job.subscribers():
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
            resolved = job.resolved_request
            if resolved is not None:
                self._remember_generation_candidate(
                    replace(subscriber, intent=bound), resolved, job
                )

    def _reuse_resolved_generation(
        self, job: StemGenerationJob, compatible: StemCacheEntry, resolved: StemGenerationRequest
    ) -> None:
        for subscriber in job.subscribers():
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
                generation_intent=subscriber.intent,
                own_source_version=own.source_version,
                artifact_source_version=compatible.source_version,
            )
            self._session.stem_generation_diagnostics[subscriber.sample_id] = (
                "Checking saved stems for the current model"
            )

    def _start_waiting_generations(self) -> None:
        for path in tuple(self._waiting_jobs):
            job = self._jobs.get(path)
            if job is None:
                self._waiting_jobs.discard(path)
                continue
            for subscriber in job.subscribers():
                if not self._intent_current(subscriber.intent):
                    job.remove_subscriber(subscriber.sample_id)
                    if self._generation_sources.get(subscriber.sample_id) is subscriber.source:
                        self._clear_stem_generation_state(subscriber.sample_id)
            if not job.subscribers():
                self._discard_generation_artifacts(job.request.sample_id, path)
                continue
            material = job.intent.material_key if job.intent is not None else None
            if any(
                active_job.intent is not None and active_job.intent.material_key == material
                for active in self._inference_jobs
                if active in self._jobs
                for active_job in (self._jobs[active],)
            ):
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
            self._session.stem_generation_errors[sample_id] = (
                "Stem completion rejected because its private artifact directory is missing"
            )
            return
        published_path = generation_cache_dir.with_name(
            generation_cache_dir.name.replace(".generation-", ".ready-", 1)
        )
        lease = None
        try:
            job = self._jobs.get(generation_cache_dir)
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
            return
        if not self._entry_files_available(entry):
            self._session.stem_generation_errors[sample_id] = (
                "Stem cache integrity changed before publication"
            )
            self._assets.retire(Path(entry.cache_dir), recursive=True, lease=lease)
            return

        queued = self._publish_generation_targets(sample_id, source_lease, entry, subscribers, job)
        if not queued:
            self._assets.retire(Path(entry.cache_dir), recursive=True, lease=lease)
        else:
            lease.release()

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
                    )
            except (OSError, RuntimeError, ValueError) as err:
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
    ) -> bool:
        if not self._intent_current(generation_intent):
            return False
        if prepared_pair is None and callable(getattr(self._audio, "prepare_stem_pair", None)):
            return self._prepare_pair(
                sample_id,
                entry,
                source_ticket,
                generation_intent=generation_intent,
                own_source_version=own_source_version,
                artifact_source_version=artifact_source_version,
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
        generation_intent: StemGenerationIntent | None = None,
        own_source_version: str | None = None,
        artifact_source_version: str | None = None,
    ) -> bool:
        if sample_id in self._fenced_pair_ids:
            message = "Unsupported stem pair metadata requires explicit recovery"
            raise RuntimeError(message)
        content = self._project.pad_content[sample_id]
        self._pair_preparations.submit(
            sample_id,
            entry,
            ticket,
            content.instance_id if content else None,
            self._project.stem_cache[sample_id],
            components=generation_intent is None
            and self._project.pad_stem_mix_mode[sample_id] == "all_stems"
            if components is None
            else components,
            processing_request=processing_request,
            regenerate_on_incompatible=regenerate_on_incompatible,
            residency_only=residency_only,
            binding=StemPairIntentBinding(
                generation_intent=generation_intent,
                own_source_version=own_source_version,
                artifact_source_version=artifact_source_version,
                source_path=self._project.sample_paths[sample_id],
            ),
        )
        return True

    def _poll_pair_preparations(self) -> None:
        for sample_id in tuple(self._resident_pairs)[:8]:
            if self._project.pad_stem_mix_mode[sample_id] == "full_mix":
                self._set_full_mix(sample_id)
        for sample_id, error in self._pair_preparations.retry_discards():
            self._session.stem_generation_errors[sample_id] = f"Stem pair cleanup deferred: {error}"
        for request in self._pair_preparations.collect():
            try:
                self._finish_pair_preparation(request)
            except (OSError, RuntimeError, TypeError, ValueError) as error:
                self._session.stem_generation_errors[request.sample_id] = (
                    f"Complete stem preparation failed: {error}"
                )
            finally:
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
                    if (
                        request.sample_id not in self._pending_stem_publications
                        and request.sample_id not in self._disk_selection_settlements
                    ):
                        self._verified_candidates.pop(request.sample_id, None)
                    request.close()

    def _finish_pair_preparation(self, request: PairPreparation) -> None:
        sample_id = request.sample_id
        if not self._pair_preparation_current(request):
            return
        if request.error is not None:
            if request.regenerate_on_incompatible and request.compatibility_rejected:
                self._regenerate_incompatible_pair(request)
                return
            raise RuntimeError(request.error)
        selected = self._pair_preparations.selected_entry(request)
        prepared = request.prepared
        if prepared is None:
            return
        # Recheck current native source/request/timing even for disk-only selection.
        ticket = self._capture_source_ticket(sample_id, selected.source_version)
        if not request.ticket.same_source_request(ticket):
            message = "Stem source request changed during complete pair preparation"
            raise RuntimeError(message)
        with request.retirement.activate():
            if (
                not request.residency_only
                and prepared.has_components()
                and self._project.pad_stem_mix_mode[sample_id] == "all_stems"
            ):
                self._queue_stem_publication(
                    sample_id,
                    selected,
                    ticket,
                    prepared,
                    generation_intent=request.generation_intent,
                )
                return
            self._select_prepared_disk_pair(request, selected, ticket)

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
            self._settle_disk_selection(settlement)

    def _settle_disk_selection(self, settlement: _DiskSelectionSettlement) -> None:
        request = settlement.request
        prepared = request.prepared
        if prepared is None:
            return
        sample_id = request.sample_id
        try:
            with settlement.retirement.activate():
                prepared.select()
                current = (
                    self._project.stem_cache[sample_id] is settlement.entry
                    and self._content_id(sample_id) == request.content_id
                    and self._project.sample_paths[sample_id] == request.source_path
                )
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
            self._session.stem_generation_errors[sample_id] = (
                f"Stem selection committed; owner settlement deferred: {error}"
            )
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
        if request.residency_only or (
            request.generation_intent is not None
            and (
                sample_id not in self._session.active_sample_ids
                or self._committed_residency.pending_supported
            )
        ):
            self._publish_all_stems_mode_if_preferred(sample_id, settlement.entry.source_version)

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
        if (
            self._shutting_down
            or request.cancelled.is_set()
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
            cache_dir=processing.cache_dir.with_name(f".generation-{uuid4().hex}"),
        )
        offline_source = self._capture_offline_source(request.sample_id, processing.source_version)
        self._admit_stem_generation(fresh, offline_source, skip_completed=True)

    def _poll_stem_publications(self) -> None:
        for sample_id, pending in tuple(self._pending_stem_publications.items()):
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
                generation_intent=pending.generation_intent,
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
                or sample_id in self._pair_preparations.pending,
                "Cannot generate stems while another pad task is running",
            ),
            (
                sample_id in self._session.stem_generating_sample_ids,
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

    def _clear_stem_generation_messages(self, sample_id: int) -> None:
        self._session.stem_generation_errors.pop(sample_id, None)
        self._session.stem_generation_diagnostics.pop(sample_id, None)
        self._session.stem_generation_progress.pop(sample_id, None)
        self._session.stem_generation_stage.pop(sample_id, None)

    def _reset_stem_mask_state(self, sample_id: int) -> None:
        self._session.pad_stem_enabled_mask[sample_id] = STEM_COMPONENT_MASK
        self._session.pad_stem_last_custom_mask[sample_id] = STEM_COMPONENT_MASK
        self._session.pad_stem_mask_display_mode[sample_id] = "all"

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

        def report_progress(percent: float, stage: str) -> None:
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
                )
            )

        event = None
        processing = (
            (job.processing_files, job.producer_identity)
            if job.processing_files is not None
            else None
        )
        try:
            if processing is not None:
                processing[0].verify()
            with stem_command_scope(job.cancel_event, directory=request.cache_dir):
                result = self._stem_backend.generate(request, report_progress)
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
            job.finish_read()
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
            self._session.stem_generation_diagnostics[sample_id] = (
                f"Private stem artifact cleanup failed: {err}"
            )

    def _record_generation_result(
        self,
        sample_id: int,
        result: StemGenerationResult | None,
    ) -> None:
        if result is None:
            return
        if result.diagnostic:
            self._session.stem_generation_diagnostics[sample_id] = result.diagnostic
        elif result.cpu_fallback:
            self._session.stem_generation_diagnostics[sample_id] = (
                "Stem generation completed on CPU after CUDA fallback"
            )

    def _publish_all_stems_mode_if_preferred(self, sample_id: int, source_version: str) -> bool:
        if self._project.pad_stem_mix_mode[sample_id] != "all_stems":
            return True

        if self._committed_residency.contains(sample_id):
            return self._committed_residency.request(
                sample_id, "all_stems", self._session.pad_stem_enabled_mask[sample_id]
            )
        if self._requires_committed_residency(sample_id):
            return False

        try:
            self._audio.set_stem_mix_mode(sample_id, "all_stems", source_version)
            self._audio.set_stem_enabled_mask(
                sample_id,
                self._session.pad_stem_enabled_mask[sample_id],
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
