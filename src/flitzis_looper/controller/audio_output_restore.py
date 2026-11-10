"""Stopped output restoration preserves musical intent and requires fresh own ACKs."""

from dataclasses import dataclass
from functools import partial
from typing import TYPE_CHECKING

from flitzis_looper.audio_output_options import AudioOutputOptions
from flitzis_looper.constants import NUM_SAMPLES
from flitzis_looper.controller.audio_output_legacy import legacy_restore_ready
from flitzis_looper.controller.committed_stem_residency import resident_stem_admission_pressure

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller.app import AppController
    from flitzis_looper.models import StemCacheEntry


@dataclass(frozen=True, slots=True)
class OutputRestoreSnapshot:
    """Original requested source/selection and integer grid coordinates."""

    rate: int | None
    grid_offsets: tuple[int, ...]
    paths: dict[int, Path]
    stems: tuple[StemCacheEntry | None, ...]


@dataclass(frozen=True, slots=True)
class OutputRollbackPlan:
    """Exact installed configuration with its original shared-engine policy."""

    options: AudioOutputOptions
    match_engine_format: bool | None


def rescale_grid_offset(offset: int, original_rate: int, new_rate: int) -> int:
    """Round rational sample coordinates to nearest, with halves away from zero."""
    if type(offset) is not int or min(original_rate, new_rate) <= 0:
        message = "Grid conversion requires integer samples and positive rates"
        raise ValueError(message)
    magnitude, remainder = divmod(abs(offset) * new_rate, original_rate)
    magnitude += 2 * remainder >= original_rate
    return -magnitude if offset < 0 else magnitude


def require_frontend_idle(app: AppController, *, exclusive_worker: bool = False) -> None:
    """Refuse before touching the device if an original control owner is unsettled."""
    loader, stems, timing = app.loader, app.stems, app.accepted_timing
    residency = app.transport.residency
    playback = app.transport.playback._global_playback
    pair = stems._pair_preparations
    migration = app.material_migration
    if exclusive_worker:
        if not app.audio_output.exclusive_native_access:
            message = "Cached recording custody requires exclusive configuration access"
            raise RuntimeError(message)
        recording_retired = app.audio_output.recording_retired_before_worker
    else:
        recording_retired = app.recording.status().retired
    pending = (
        not recording_retired,
        app.arrangement.active,
        app.pairs.active,
        app.banks.active,
        app.transport.waveform.has_pending_holds,
        app.stem_batch.active,
        app.stem_delete.active,
        app.session.loading_sample_ids,
        app.session.analyzing_sample_ids,
        app.session.stem_generating_sample_ids,
        loader._load_retirements,
        loader._incoming_loads,
        loader._deferred_restores,
        loader._pair_held_loader_events,
        loader._material_creation_pending,
        loader._analysis_request_ids,
        loader._key_analysis._requests,
        loader._accepted_restore._pending,
        any(not future.done() for future in loader._accepted_restore._migration_futures),
        timing._publications,
        timing._waiting_refresh,
        timing._refreshes,
        any(not future.done() for future in timing._preparations.values()),
        residency._pending,
        residency._captured_starts,
        residency._captured_triggers,
        app.transport.pitch._owners,
        playback._pending is not None,
        playback._deferred is not None,
        playback._fenced_batches,
        any(owner.pending for owner in app.transport.playback._pause_intent._owners.values()),
        stems._jobs,
        stems._generation_sources,
        stems._requested_generations,
        stems._requested_preparations,
        stems._inference_jobs,
        stems._waiting_jobs,
        stems._disk_selection_settlements,
        stems._pending_stem_publications,
        stems._publication_retries.pending,
        stems._pair_held_generation_events,
        pair.pending,
        pair._held_results,
        pair._discard_retries,
        migration.status not in {"idle", "failed", "config_committed"},
        migration._preparation is not None,
        migration._hold is not None,
        migration._recovery_active is not None,
        migration._recovery_hold is not None,
        migration._recovery_pending,
        migration._artifact_future is not None,
        migration._history_future is not None,
        migration._creation_restore_future is not None,
        migration._creation_restores,
        migration._startup_scheduled,
        migration._cleanup.preparing,
    )
    if any(pending):
        message = (
            "Audio output change is waiting for original recording, source, stem "
            "or arrangement owners"
        )
        raise RuntimeError(message)


def claim_output_change(app: AppController) -> tuple[int, OutputRestoreSnapshot]:
    """Fence pool admissions and atomically claim Native's actual stopped state."""
    require_frontend_idle(app)
    paths: dict[int, Path] = {}
    for sample_id, reference in enumerate(app.project.sample_paths):
        if reference is None:
            continue
        path = app.loader._parse_cached_sample_path(reference)
        if path is None:
            message = f"PAD {sample_id + 1} has no restorable original reference"
            raise ValueError(message)
        paths[sample_id] = path
    observed = app.audio_output.observed
    rate = observed.get("sample_rate_hz") if observed is not None else None
    snapshot = OutputRestoreSnapshot(
        rate if type(rate) is int and rate > 0 else None,
        tuple(app.project.pad_grid_offset_samples),
        paths,
        tuple(
            entry.model_copy(deep=True) if entry is not None else None
            for entry in app.project.stem_cache
        ),
    )
    pool = app.stems._stem_worker_pool
    if pool is None:
        message = "The custom stem runner supplies no actual worker-idle barrier"
        raise RuntimeError(message)
    epoch = pool.with_idle(app.audio.begin_audio_output_change)
    return epoch, snapshot


def resolved_output(info: dict[str, object]) -> AudioOutputOptions:
    """Resolve Auto to the original installed endpoint and initialized geometry."""
    asio = info.get("asio_control")
    asio = asio if isinstance(asio, dict) else {}
    frames = (
        info.get("initialized_buffer_frames")
        or info.get("accepted_initialization_period_frames")
        or info.get("initialized_engine_period_frames")
    )
    mono_fold = (
        info.get("physical_output_channels") == 1
        and info.get("left_channel") == info.get("right_channel") == 0
    )
    return AudioOutputOptions.model_validate({
        "backend": str(info["host"]).lower(),
        "device_id": asio.get("device_id") or info.get("device_id"),
        "sample_rate_hz": info["sample_rate_hz"],
        "buffer_frames": frames,
        "left_channel": None if mono_fold else info.get("left_channel"),
        "right_channel": None if mono_fold else info.get("right_channel"),
    })


def capture_output_rollback(info: dict[str, object]) -> OutputRollbackPlan:
    """Prevent Auto/NONE from becoming a new format-lock request during rollback."""
    policy: bool | None = None
    if str(info.get("host", "")).lower() == "wasapi":
        captured = info.get("match_engine_format")
        if type(captured) is not bool:
            message = "Original WASAPI client format policy is unavailable for exact rollback"
            raise ValueError(message)
        policy = captured
    options = resolved_output(info)
    if options.device_id is None:
        message = "Original output endpoint identity is unavailable for exact rollback"
        raise ValueError(message)
    return OutputRollbackPlan(options, policy)


def retire_runtime_projections(app: AppController) -> None:
    """Retire idle projections on the exclusive worker after actual Native END.

    The UI accesses only cached configuration during this operation. No original
    controller is replaced, no closed admission flag is reopened, and project
    asset assignments/history and musical choices keep their existing owners.
    """
    if not app.audio_output.exclusive_native_access:
        message = "Output projections require exclusive configuration custody"
        raise RuntimeError(message)
    require_frontend_idle(app, exclusive_worker=True)
    pool = app.stems._stem_worker_pool
    if pool is None or not pool.idle:
        message = "Original legacy readers have not returned their worker closures"
        raise RuntimeError(message)
    app._output_legacy_restores.clear()
    app.loader._output_restore_cancelling = False
    app.accepted_timing.shut_down()
    app.loader._accepted_restore.shut_down()
    app.loader._load_request_ids.clear()
    app.loader._output_restore_requests.clear()
    app.loader._output_restore_acknowledged.clear()
    app.loader._key_analysis._errors.clear()
    app.transport.pitch._projections.clear()
    for sample_id in range(NUM_SAMPLES):
        app.transport.residency.cancel(sample_id)
        app.transport.playback.forget_pause_intent(sample_id)
    app.transport.playback._forget_global_start_stop_restore()
    app.transport.waveform._readiness.clear()
    app.transport.waveform._view_revision += 1
    global_ = app.transport.global_params
    global_._rate_status = None
    global_._rate_status_error = None
    global_._rate_request_unobserved = False
    global_._bpm_feedback = None
    global_._bpm_application = None
    global_._bpm_application_error = None
    app.session.master_bpm = None
    app.session.master_period_seconds = None
    app.session.bpm_lock_anchor_bpm = None
    app.session.bpm_lock_anchor_revision = None
    app.stems._committed_residency.shut_down()
    app.stems._resident_pairs.clear()
    app.stems._demands.clear()
    app.stems._verified_sets.clear()
    app.stems._verified_candidates.clear()
    app.stems._restored_stem_candidates.clear()
    app.stems._pair_release_ordered.clear()
    app.stems._pending_full_mix_releases.clear()
    app.stems._residency_errors.clear()
    app._init_audio_feedback()
    _reset_output_activity_projections(app)


def _selection_current(app: AppController, sample_id: int, previous: StemCacheEntry | None) -> bool:
    return app.project.stem_cache[sample_id] is previous


def restore_sources(app: AppController, snapshot: OutputRestoreSnapshot, *, rollback: bool) -> None:
    """Restore original controls first and existing complete sources without inference."""
    if not app._runtime_activated:
        app._activate_startup_owners()
    app.loader._output_restore_cancelling = False
    for sample_id in snapshot.paths:
        for label, errors in (
            ("load", app.session.sample_load_errors),
            ("stems", app.session.stem_generation_errors),
        ):
            prior = errors.pop(sample_id, None)
            if prior:
                app.audio_output.previous_errors[sample_id, label] = prior
    rate = app.audio.output_sample_rate()
    offsets = snapshot.grid_offsets
    app.project.pad_grid_offset_samples = (
        list(offsets)
        if rollback or snapshot.rate is None
        else [rescale_grid_offset(offset, snapshot.rate, rate) for offset in offsets]
    )
    for sample_id, expected in enumerate(snapshot.stems):
        current = app.project.stem_cache[sample_id]
        if expected is None:
            if current is not None:
                message = "Stem selection changed while output restoration was fenced"
                raise RuntimeError(message)
            continue
        if current is None or current.model_dump(exclude={"available"}) != expected.model_dump(
            exclude={"available"}
        ):
            replacement = app.stems._output_pair_replacements.get(sample_id)
            if (
                not rollback
                or replacement is None
                or replacement[0] is None
                or (
                    replacement[0].model_dump(exclude={"available"})
                    != expected.model_dump(exclude={"available"})
                    or current is not replacement[1]
                )
            ):
                message = "Original stem selection changed while output restoration was fenced"
                raise RuntimeError(message)
            previous = current

            app.persistence.commit_stem_selection(
                sample_id,
                previous,
                expected,
                partial(_selection_current, app, sample_id, previous),
            )
            current = expected.model_copy(deep=True)
            app.project.stem_cache[sample_id] = current
            app.stems._output_pair_replacements.pop(sample_id, None)
        current = current.model_copy(update={"available": False})
        app.project.stem_cache[sample_id] = current
        app.stems._restored_stem_candidates[sample_id] = current
    app.transport.apply_project_state_to_audio(
        restored_sample_ids=frozenset(snapshot.paths),
        source_initializers_in_loader=True,
        after_output_retirement=True,
    )
    app.loader.restore_samples_from_project_state(
        validated_paths=snapshot.paths, preserve_output_identity=True
    )


def sources_restored(app: AppController, snapshot: OutputRestoreSnapshot) -> bool:
    """Require complete control-side source success plus fresh current selection ACKs."""
    for sample_id in snapshot.paths:
        _raise_pad_restore_errors(app, sample_id)
    if (
        _loader_restoration_pending(app)
        or app.accepted_timing._waiting_refresh
        or app.accepted_timing._refreshes
        or app.transport.residency._pending
        or app.session.loading_sample_ids
    ):
        return False
    if not set(snapshot.paths).issubset(app.loader._output_restore_acknowledged):
        return False
    ready = True
    for sample_id in snapshot.paths:
        version = app.stems._loaded_source_version_for_pad(sample_id)
        if version is None:
            return False
        # A new source ticket validates current digest, request, source generation
        # and prepared epoch; an old waveform-ready projection is insufficient.
        ticket = app.audio.capture_prepared_source(sample_id, version)
        if ticket.is_current() is not True:
            return False
        entry = snapshot.stems[sample_id]
        if entry is not None and entry.pair is None:
            epoch = app.audio_output._epoch
            if epoch is None:
                message = "Legacy restoration lost its original output transaction"
                raise RuntimeError(message)
            ready &= legacy_restore_ready(app, epoch, sample_id, entry)
        else:
            replacement = app.stems._output_pair_replacements.get(sample_id)
            if (
                entry is not None
                and replacement is not None
                and replacement[0] is not None
                and (
                    replacement[0].model_dump(exclude={"available"})
                    == entry.model_dump(exclude={"available"})
                )
            ):
                entry = replacement[1]
            ready &= app.stems.recovered_copy_selection_ready(sample_id, entry)
    pool = app.stems._stem_worker_pool
    return ready and pool is not None and pool.idle


def cancel_restore_admissions(app: AppController) -> None:
    """Withdraw unsubmitted restoration only; retain admitted owners until real END."""
    snapshot = app._output_restore_snapshot
    ids = snapshot.paths if snapshot is not None else range(NUM_SAMPLES)
    app.loader._output_restore_cancelling = True
    for sample_id in ids:
        app.loader._cancel_deferred_restore(sample_id)
        app.loader._accepted_restore.cancel(sample_id)
        app.accepted_timing.cancel(sample_id)
        app.transport.residency.cancel_requested(sample_id)
        app.stems._pair_preparations.cancel(sample_id)
        pending = app.stems._requested_preparations.pop(sample_id, None)
        if pending is not None:
            pending.heavy_work.close()
        app.stems.cancel_verified_output_restore(sample_id)
        owner = app._output_legacy_restores.get(sample_id)
        if owner is not None:
            owner.cancel()
    for retry in tuple(app.stems._publication_retries.pending.values()):
        if retry.pending.source_ticket.publication_status() == "rejected":
            app.stems._settle_publication_retry(retry)


def startup_sources_restored(app: AppController) -> bool:
    """Validate the actual post-recovery image before first performer admission."""
    for sample_id, path in enumerate(app.project.sample_paths):
        if path is None:
            continue
        _raise_pad_restore_errors(app, sample_id)
        version = app.stems._loaded_source_version_for_pad(sample_id)
        if version is None:
            return False
        ticket = app.audio.capture_prepared_source(sample_id, version)
        if not ticket.is_current():
            return False
        entry = app.project.stem_cache[sample_id]
        if (
            entry is not None
            and entry.pair is not None
            and not app.stems.recovered_copy_selection_ready(sample_id, entry)
        ):
            return False
    return _startup_readers_settled(app)


class OutputRestoreRuntime:
    """Internal App operations for the original stopped output transaction."""

    def __init__(self, app: AppController) -> None:
        self._app = app

    def activate(self, *, after_output_retirement: bool = False) -> None:
        """Activate historical startup custody once, after output installation."""
        self._app._activate_startup_owners()
        self._app._restore_initial_runtime_state(after_output_retirement=after_output_retirement)

    def startup_restored(self, *, after_output_retirement: bool = False) -> bool:
        """Observe the post-recovery image while Native input remains disabled."""
        return startup_sources_restored(self._app) and (
            not after_output_retirement
            or self._app.transport.global_params._output_transport_restored()
        )

    def finish_startup(self, *, after_output_retirement: bool = False) -> None:
        """Enable saved input once, after initial source and selection own ACKs."""
        self._app._initialize_native_gui_selection(after_output_retirement=after_output_retirement)
        self._app.input_mapping.apply_project_state_to_input_runtime()

    def release_completed_restore(self) -> None:
        """Release consumed transaction observers after its real idle/finish barriers."""
        self._app._output_legacy_restores.clear()
        self._app.stems._output_pair_replacements.clear()
        self._app._output_restore_snapshot = None
        self._app.loader._output_restore = False
        self._app.loader._output_restore_cancelling = False

    def prepare_change(self) -> int:
        """Claim actual idle control/Native ownership and retain exact rollback intent."""
        epoch, snapshot = claim_output_change(self._app)
        self._app._output_restore_snapshot = snapshot
        return epoch

    def resolved_output(self, observed: dict[str, object]) -> OutputRollbackPlan:
        """Return the original installed request used for exact-device rollback."""
        return capture_output_rollback(observed)

    def retire_projections(self) -> None:
        """Retire idle control projections under the exclusive configuration worker."""
        retire_runtime_projections(self._app)

    def require_change_idle(self, epoch: int) -> None:
        """Retain an errored transaction until its original readers really settle."""
        require_frontend_idle(self._app)
        pool = self._app.stems._stem_worker_pool
        if pool is None or not pool.idle:
            message = "Original Python worker readers have not returned"
            raise RuntimeError(message)
        self._app._audio.ensure_audio_output_change_sources_settled(epoch)

    def finish_native_change(self, epoch: int, *, empty_startup: bool = False) -> None:
        """Fence Python worker tails while completing the original Native latch."""
        pool = self._app.stems._stem_worker_pool
        if pool is None:
            message = "Original stem worker-idle barrier is unavailable"
            raise RuntimeError(message)
        finish = (
            self._app._audio.finish_empty_audio_output_startup
            if empty_startup
            else self._app._audio.finish_audio_output_change
        )
        pool.with_idle(lambda: finish(epoch))

    def restore_original_grid(self) -> None:
        """Restore original integer offsets after abort, without repeated rounding."""
        if self._app._output_restore_snapshot is not None:
            self._app._project.pad_grid_offset_samples = list(
                self._app._output_restore_snapshot.grid_offsets
            )

    def cancel_admissions(self) -> None:
        """Withdraw only unclaimed work while original readers continue settling."""
        cancel_restore_admissions(self._app)

    def restore_sources(self) -> None:
        """Reload complete existing originals and selected stems without autoplay."""
        snapshot = self._app._output_restore_snapshot
        if snapshot is None:
            message = "Original output restoration snapshot is unavailable"
            raise RuntimeError(message)
        restore_sources(
            self._app, snapshot, rollback=self._app.audio_output.phase == "rolling_back"
        )

    def sources_restored(self) -> bool:
        """Require matching fresh full source and selected-stem own acknowledgements."""
        snapshot = self._app._output_restore_snapshot
        if snapshot is None:
            message = "Original output restoration snapshot is unavailable"
            raise RuntimeError(message)
        return (
            sources_restored(self._app, snapshot)
            and self._app.transport.global_params._output_transport_restored()
        )

    def finish_restore(self) -> None:
        """Publish complete input/MIDI state last, without any playback request."""
        self._app._initialize_native_gui_selection(after_output_retirement=True)
        self._app.input_mapping.apply_project_state_to_input_runtime()
        self._app.loader._output_restore = False


def _reset_output_activity_projections(app: AppController) -> None:
    app._gui_native_selection = None
    app.input_mapping._last_input_runtime_state_signature = None
    app.input_mapping._last_input_runtime_state_error = None
    app.session.active_sample_ids.clear()
    app.session.paused_sample_ids.clear()
    app.session.pad_peak = [0.0] * NUM_SAMPLES
    app.session.pad_peak_updated_at = [0.0] * NUM_SAMPLES
    app.session.pad_clip_hold_until = [0.0] * NUM_SAMPLES
    app.session.pad_playhead_s = [None] * NUM_SAMPLES
    app.session.pad_playhead_updated_at = [0.0] * NUM_SAMPLES
    app.session.master_output_peak = 0.0
    app.session.master_output_clip_hold_until = 0.0


def _raise_pad_restore_errors(app: AppController, sample_id: int) -> None:
    for error in (
        app.session.sample_load_errors.get(sample_id),
        app.session.stem_generation_errors.get(sample_id),
    ):
        if error and not resident_stem_admission_pressure(error):
            raise RuntimeError(error)


def _startup_readers_settled(app: AppController) -> bool:
    try:
        require_frontend_idle(app)
    except RuntimeError:
        return False
    pool = app.stems._stem_worker_pool
    if pool is None or not pool.idle:
        return False
    try:
        pool.with_idle(app.audio.ensure_audio_output_startup_sources_settled)
    except RuntimeError:
        return False
    return True


def _loader_restoration_pending(app: AppController) -> bool:
    return bool(
        app.loader._load_retirements
        or app.loader._deferred_restores
        or app.loader._accepted_restore.has_pending()
        or any(not future.done() for future in app.loader._accepted_restore._migration_futures)
    )
