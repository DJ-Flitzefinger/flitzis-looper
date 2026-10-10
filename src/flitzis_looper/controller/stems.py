import threading
import wave
from collections.abc import Callable
from dataclasses import dataclass, replace
from pathlib import Path
from queue import Empty, SimpleQueue
from typing import TYPE_CHECKING, Literal
from uuid import uuid4

from flitzis_looper.controller.asset_lifecycle import (
    AssetRetirementReservation,
    ProjectAssetLifecycle,
)
from flitzis_looper.controller.base import BaseController
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
)
from flitzis_looper.controller.stem_job import StemGenerationJob, StemSubscriber
from flitzis_looper.controller.stem_pair_preparation import (
    PairPreparation,
    StemPairPreparationQueue,
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

if TYPE_CHECKING:
    from flitzis_looper.models import ProjectState, SessionState
    from flitzis_looper_audio import (
        AudioEngine,
        PreparedSourceTicket,
        PreparedStemPair,
        ProjectAssetLease,
    )

type StemTaskRunner = Callable[[Callable[[], None]], None]
type _StemBackendEventType = Literal["progress", "success", "error"]


@dataclass(frozen=True, slots=True)
class _StemBackendEvent:
    sample_id: int
    source_version: str
    source_ticket: PreparedSourceTicket
    cache_dir: Path
    event_type: _StemBackendEventType
    content_id: str | None = None
    percent: float | None = None
    stage: str | None = None
    error: str | None = None
    result: StemGenerationResult | None = None


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
        self._generation_source_tickets: dict[int, PreparedSourceTicket] = {}
        self._pending_stem_publications: dict[int, _PendingStemPublication] = {}
        self._restored_stem_candidates: dict[int, StemCacheEntry] = {}
        self._pair_preparations = StemPairPreparationQueue(
            audio, self._assets, self._stem_task_runner
        )
        self._resident_pairs: set[int] = set()
        self._publication_retries = StemPublicationRetries()
        self._pair_release_ordered: set[int] = set()
        self._fenced_pair_ids: set[int] = set()
        self._on_frame_render_callbacks.append(self._poll_generation_events)
        self._on_frame_render_callbacks.append(self._poll_stem_publications)
        self._on_frame_render_callbacks.append(self._poll_pair_preparations)
        self._on_frame_render_callbacks.append(self._poll_publication_retries)

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
        self._generation_source_tickets.clear()
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
        self._session.stem_generating_sample_ids.clear()
        self._session.stem_generation_source_versions.clear()
        if self._stem_worker_pool is not None:
            self._stem_worker_pool.shutdown()

    def generate_stems_async(self, sample_id: int) -> bool:
        """Schedule offline stem generation for a stopped loaded pad when allowed."""
        validate_sample_id(sample_id)
        self._assets.sync_assignments()
        self._clear_stem_generation_messages(sample_id)

        blocker = self.stem_generation_block_reason(sample_id)
        if blocker is not None:
            self._session.stem_generation_errors[sample_id] = blocker
            return False

        source_version = self.source_version_for_pad(sample_id)
        if source_version is None:
            self._session.stem_generation_errors[sample_id] = (
                "Cannot generate stems because the source file is missing"
            )
            return False

        self._session.stem_generating_sample_ids.add(sample_id)
        self._session.stem_generation_source_versions[sample_id] = source_version
        cache_dir = cache_dir_for_sample_id(sample_id, self._project.sample_paths[sample_id])
        target_shape = self._target_shape_for_pad(sample_id)
        if target_shape is None:
            self._clear_stem_generation_state(sample_id)
            self._session.stem_generation_errors[sample_id] = (
                "Cannot generate stems because the loaded sample shape is unavailable"
            )
            return False

        sample_path = self._project.sample_paths[sample_id]
        if sample_path is None:
            self._clear_stem_generation_state(sample_id)
            self._session.stem_generation_errors[sample_id] = (
                "Cannot generate stems without a loaded sample"
            )
            return False

        source_path = Path(sample_path)
        if not source_path.is_absolute():
            source_path = Path.cwd() / source_path

        request = StemGenerationRequest(
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

        try:
            source_ticket = self._capture_source_ticket(sample_id, source_version)
            content = self._project.pad_content[sample_id]
            content_id = content.instance_id if content is not None else None
            job = next((job for job in self._jobs.values() if job.matches(request)), None)
            shared = job is not None
            if job is None:
                job = StemGenerationJob(request, source_ticket, self._assets, content_id)
            else:
                job.add_subscriber(sample_id, source_ticket, content_id)
        except (AttributeError, OSError, RuntimeError, TypeError, ValueError) as err:
            self._clear_stem_generation_state(sample_id)
            self._session.stem_generation_errors[sample_id] = f"Stem admission failed: {err}"
            return False
        self._generation_source_tickets[sample_id] = source_ticket
        self._jobs[job.request.cache_dir] = job

        if self._project.stem_cache[sample_id] is None:
            self._project.stem_cache[sample_id] = StemCacheEntry(
                source_version=source_version,
                cache_dir=cache_dir,
                stems=expected_stem_files(cache_dir),
                available=False,
            )
            self._mark_project_changed()
        return True if shared else self._schedule_stem_job(job)

    def _schedule_stem_job(self, job: StemGenerationJob) -> bool:
        try:
            self._stem_task_runner(lambda: self._run_stem_backend(job))
        except (OSError, RuntimeError) as error:
            self._cancel_jobs(job.request.sample_id)
            self._clear_stem_generation_state(job.request.sample_id)
            self._session.stem_generation_errors[job.request.sample_id] = (
                f"Stem worker admission failed: {error}"
            )
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
        if entry is not None and entry.pair is not None and sample_id not in self._resident_pairs:
            return self._request_pair_mode(sample_id, entry, source_version)

        if not self._publish_all_stems_state(sample_id, source_version):
            return False

        if mode != self._project.pad_stem_mix_mode[sample_id]:
            self._project.pad_stem_mix_mode[sample_id] = mode
            self._mark_project_changed()

        return True

    def _set_full_mix(self, sample_id: int) -> bool:
        if (
            self._project.pad_stem_mix_mode[sample_id] == "full_mix"
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
        self._resident_pairs.discard(sample_id)
        self._mark_project_changed()
        return True

    def _request_pair_mode(
        self, sample_id: int, entry: StemCacheEntry, source_version: str
    ) -> bool:
        try:
            ticket = self._capture_source_ticket(sample_id, source_version)
            self._prepare_pair(sample_id, entry, ticket, components=True)
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
        if self._project.pad_stem_mix_mode[sample_id] != "full_mix":
            try:
                self._audio.set_stem_mix_mode(sample_id, "full_mix")
            except (RuntimeError, ValueError) as err:
                self._session.stem_generation_errors[sample_id] = f"Stem mix update failed: {err}"
                return False

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
        try:
            self._audio.set_stem_mix_mode(sample_id, "all_stems", source_version)
        except (RuntimeError, ValueError) as err:
            self._session.stem_generation_errors[sample_id] = f"Stem mix update failed: {err}"
            return False
        return True

    def _publish_stem_enabled_mask(self, sample_id: int, source_version: str) -> bool:
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
        return self._stem_generation_blocker(sample_id)

    def _poll_generation_events(self) -> None:
        """Drain Python backend generation events from background worker threads."""
        while True:
            try:
                event = self._stem_generation_events.get_nowait()
            except Empty:
                return

            job = self._jobs.get(event.cache_dir)
            if job is None:
                continue
            self._dispatch_job_event(event, job)

    def _dispatch_job_event(self, event: _StemBackendEvent, job: StemGenerationJob) -> None:
        current = tuple(
            subscriber
            for subscriber in job.subscribers()
            if self._is_current_generation(
                replace(
                    event,
                    sample_id=subscriber.sample_id,
                    source_ticket=subscriber.ticket,
                    content_id=subscriber.content_id,
                )
            )
        )
        if event.event_type == "success":
            self._finish_shared_generation(event, current)
        else:
            for subscriber in current:
                if event.event_type == "progress":
                    self._handle_stem_generation_progress(
                        subscriber.sample_id, event.percent, event.stage, subscriber.ticket
                    )
                elif event.error is not None:
                    self._handle_stem_generation_error(
                        subscriber.sample_id, event.error, subscriber.ticket
                    )
        if event.event_type != "progress":
            # Promote/fan out once before dropping the job's physical ownership.
            for subscriber in job.subscribers():
                if self._generation_source_tickets.get(subscriber.sample_id) is subscriber.ticket:
                    self._clear_stem_generation_state(subscriber.sample_id)
            self._discard_generation_artifacts(event.sample_id, event.cache_dir)

    def _finish_shared_generation(
        self, event: _StemBackendEvent, current: tuple[StemSubscriber, ...]
    ) -> None:
        eligible = tuple(
            subscriber
            for subscriber in current
            if self._eligible_generation_entry(subscriber.sample_id, event.source_version)
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
                first.sample_id, first.ticket, event.cache_dir, eligible
            )

    def _handle_stem_generation_started(self, sample_id: int) -> None:
        """Apply a stem-generation start event from a backend event source."""
        validate_sample_id(sample_id)
        if (
            sample_id in self._generation_source_tickets
            or sample_id in self._pending_stem_publications
        ):
            return
        self._session.stem_generating_sample_ids.add(sample_id)
        self._clear_stem_generation_messages(sample_id)

    def _handle_stem_generation_progress(
        self,
        sample_id: int,
        percent: float | None,
        stage: str | None,
        source_ticket: PreparedSourceTicket | None = None,
    ) -> None:
        """Apply a stem-generation progress event from a backend event source."""
        validate_sample_id(sample_id)
        if sample_id not in self._session.stem_generating_sample_ids:
            return
        if source_ticket is None and (
            sample_id in self._generation_source_tickets
            or sample_id in self._pending_stem_publications
        ):
            return

        if stage is not None:
            self._session.stem_generation_stage[sample_id] = stage
        if percent is not None:
            self._session.stem_generation_progress[sample_id] = float(percent)

    def _handle_stem_generation_success(
        self,
        sample_id: int,
        source_ticket: PreparedSourceTicket | None = None,
        generation_cache_dir: Path | None = None,
        subscribers: tuple[StemSubscriber, ...] = (),
    ) -> None:
        """Publish a completed job using its original native admission ticket."""
        validate_sample_id(sample_id)
        if sample_id not in self._session.stem_generating_sample_ids:
            return
        if source_ticket is None:
            self._reject_legacy_stem_completion(sample_id)
            return
        if self._generation_source_tickets.get(sample_id) is not source_ticket:
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
                        sample_id, source_ticket, entry, generation_cache_dir, subscribers
                    )

    def _reject_legacy_stem_completion(self, sample_id: int) -> None:
        # Unbound legacy events cannot finish a current Python job or capture a fresh
        # ticket that would launder artifacts prepared for a prior source/timing intent.
        if (
            sample_id in self._generation_source_tickets
            or sample_id in self._pending_stem_publications
        ):
            return
        self._clear_stem_generation_state(sample_id)
        self._session.stem_generation_errors[sample_id] = (
            "Stem completion rejected because its admission ticket is missing"
        )

    def _eligible_generation_entry(
        self, sample_id: int, source_version: str
    ) -> StemCacheEntry | None:
        if self.source_version_for_pad(sample_id) != source_version:
            return None
        if sample_id in self._session.active_sample_ids:
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
        source_ticket: PreparedSourceTicket,
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

        queued = False
        targets = subscribers or (
            StemSubscriber(sample_id, source_ticket, None, self._assets.reserve_pending(16)),
        )
        for subscriber in targets:
            try:
                with subscriber.retirement.activate():
                    self._queue_stem_publication(
                        subscriber.sample_id, entry.model_copy(deep=True), subscriber.ticket
                    )
            except (OSError, RuntimeError, ValueError) as err:
                self._session.stem_generation_errors[subscriber.sample_id] = (
                    f"Stem generation completed but publication failed: {err}"
                )
            else:
                queued = True
            finally:
                subscriber.retirement.close()
        if not queued:
            self._assets.retire(Path(entry.cache_dir), recursive=True, lease=lease)
        else:
            lease.release()

    def _queue_stem_publication(
        self,
        sample_id: int,
        entry: StemCacheEntry,
        source_ticket: PreparedSourceTicket,
        prepared_pair: PreparedStemPair | None = None,
    ) -> bool:
        if prepared_pair is None and callable(getattr(self._audio, "prepare_stem_pair", None)):
            return self._prepare_pair(sample_id, entry, source_ticket)
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
            components=self._project.pad_stem_mix_mode[sample_id] == "all_stems"
            if components is None
            else components,
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
                        if pending is None or pending.prepared_pair is not request.prepared:
                            self._discard_pair(request.sample_id, request.prepared)
                finally:
                    request.close()

    def _finish_pair_preparation(self, request: PairPreparation) -> None:
        sample_id = request.sample_id
        content = self._project.pad_content[sample_id]
        if (
            self._shutting_down
            or request.cancelled.is_set()
            or (content.instance_id if content else None) != request.content_id
            or self._project.stem_cache[sample_id] is not request.previous_entry
        ):
            return
        if request.error is not None:
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
                prepared.has_components()
                and self._project.pad_stem_mix_mode[sample_id] == "all_stems"
            ):
                self._queue_stem_publication(sample_id, selected, ticket, prepared)
                return
            lease = self._assets.acquire(Path(selected.cache_dir))
            self._project.stem_cache[sample_id] = selected.model_copy(update={"available": True})
            self._assets.adopt_stems(sample_id, (Path(selected.cache_dir).absolute(), lease))
            prepared.select()
            if sample_id in self._publication_retries.pending:
                self._session.stem_generation_errors.pop(sample_id, None)
            pending = self._pending_stem_publications.pop(sample_id, None)
            if pending is not None:
                self._release_pending_stems(pending)
            self._resident_pairs.discard(sample_id)
            self._restored_stem_candidates.pop(sample_id, None)
            self._mark_project_changed()

    def _poll_stem_publications(self) -> None:
        for sample_id, pending in tuple(self._pending_stem_publications.items()):
            self._poll_stem_publication(sample_id, pending)

    def _poll_stem_publication(self, sample_id: int, pending: _PendingStemPublication) -> bool:
        with pending.retirement.activate():
            return self._apply_stem_publication(sample_id, pending)

    def _apply_stem_publication(self, sample_id: int, pending: _PendingStemPublication) -> bool:
        if (
            self._project.stem_cache[sample_id] is not pending.entry
            or self._content_id(sample_id) != pending.content_id
            or self._project.sample_paths[sample_id] != pending.source_path
        ):
            self._pending_stem_publications.pop(sample_id, None)
            self._release_pending_stems(pending)
            return True
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
        self._project.stem_cache[sample_id] = pending.entry.model_copy(update={"available": True})
        if sample_id in self._publication_retries.pending:
            self._session.stem_generation_errors.pop(sample_id, None)
        if pending.prepared_pair is not None:
            pending.prepared_pair.select()
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
            self._prepare_pair(sample_id, pending.entry, ticket)
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

    def _handle_stem_generation_error(
        self,
        sample_id: int,
        message: str,
        source_ticket: PreparedSourceTicket | None = None,
    ) -> None:
        """Apply a stem-generation failure event from a backend event source."""
        validate_sample_id(sample_id)
        if sample_id not in self._session.stem_generating_sample_ids:
            return
        if source_ticket is None and (
            sample_id in self._generation_source_tickets
            or sample_id in self._pending_stem_publications
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
                sample_id in self._session.active_sample_ids,
                "Cannot generate stems while the pad is playing",
            ),
            (
                sample_id in self._session.loading_sample_ids,
                "Cannot generate stems while the pad is loading",
            ),
            (
                sample_id in self._session.analyzing_sample_ids
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
        self._generation_source_tickets.pop(sample_id, None)
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

    def _run_stem_backend(self, job: StemGenerationJob) -> None:
        source_ticket = job.begin()
        if source_ticket is None:
            return
        request = job.request

        def report_progress(percent: float, stage: str) -> None:
            self._put_generation_event(
                _StemBackendEvent(
                    sample_id=request.sample_id,
                    source_version=request.source_version,
                    source_ticket=source_ticket,
                    cache_dir=request.cache_dir,
                    event_type="progress",
                    percent=percent,
                    stage=stage,
                )
            )

        event = None
        try:
            result = self._stem_backend.generate(request, report_progress)
            event = _StemBackendEvent(
                sample_id=request.sample_id,
                source_version=request.source_version,
                source_ticket=source_ticket,
                cache_dir=request.cache_dir,
                event_type="success",
                result=result,
            )
        except (EOFError, OSError, RuntimeError, TypeError, ValueError, wave.Error) as err:
            event = _StemBackendEvent(
                sample_id=request.sample_id,
                source_version=request.source_version,
                source_ticket=source_ticket,
                cache_dir=request.cache_dir,
                event_type="error",
                error=str(err) or type(err).__name__,
            )
        finally:
            current = job.finish_read()
            if event is None:
                # An unexpected exception still settles actual reads and private
                # cleanup capacity before it propagates to the worker boundary.
                job.cancel()
        if current and event is not None:
            self._put_generation_event(event)

    def _put_generation_event(self, event: _StemBackendEvent) -> None:
        with self._generation_event_lock:
            if not self._shutting_down:
                self._stem_generation_events.put(event)

    def _cancel_jobs(self, sample_id: int) -> None:
        for path, job in tuple(self._jobs.items()):
            if not job.remove_subscriber(sample_id):
                self._jobs.pop(path, None)

    def _is_current_generation(self, event: _StemBackendEvent) -> bool:
        content = self._project.pad_content[event.sample_id]
        content_id = content.instance_id if content is not None else None
        return (
            event.sample_id in self._session.stem_generating_sample_ids
            and self._session.stem_generation_source_versions.get(event.sample_id)
            == event.source_version
            and self._generation_source_tickets.get(event.sample_id) is event.source_ticket
            and event.content_id == content_id
        )

    def _discard_generation_artifacts(self, sample_id: int, cache_dir: Path) -> None:
        try:
            job = self._jobs.pop(cache_dir, None)
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
