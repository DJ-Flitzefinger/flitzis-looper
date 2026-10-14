import json
from contextlib import suppress
from typing import TYPE_CHECKING

from flitzis_looper.controller.accepted_publication import AcceptedTimingController
from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.material_migration import MaterialMigrationController
from flitzis_looper.controller.metering import MeteringController
from flitzis_looper.controller.pad_arrangement import PadArrangementController
from flitzis_looper.controller.performance_confirmation import PerformanceConfirmationController
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.settings import SettingsController
from flitzis_looper.controller.stems import StemController, StemTaskRunner
from flitzis_looper.controller.transport import TransportController
from flitzis_looper.input_mapping import InputMappingController
from flitzis_looper.models import ProjectState, SessionState, validate_sample_id
from flitzis_looper_audio import AudioEngine, AudioMessage

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller.base import BaseController
    from flitzis_looper.controller.stem_generation import StemGenerationBackend
    from flitzis_looper_audio import PadCopyTicket


def _copied_components_available(sample_id: int, ticket: PadCopyTicket) -> bool:
    try:
        metadata = json.loads(ticket.metadata_json())
    except (AttributeError, TypeError, ValueError) as error:
        message = "Complete Copy target acknowledgement metadata is unavailable"
        raise RuntimeError(message) from error
    if not isinstance(metadata, dict):
        message = "Complete Copy target acknowledgement metadata is invalid"
        raise TypeError(message)
    target = metadata.get("target_id")
    generation = metadata.get("source_generation")
    lifetime = metadata.get("content_lifetime")
    components = metadata.get("components_available")
    target_matches = type(target) is int and target == sample_id
    counters_are_current = all(
        type(counter) is int and counter > 0 for counter in (generation, lifetime)
    )
    if (
        not target_matches
        or not counters_are_current
        or metadata.get("status") != "accepted"
        or metadata.get("control_mirrored") is not True
        or not isinstance(components, bool)
    ):
        message = "Complete Copy target has no matching acknowledged Native image"
        raise RuntimeError(message)
    return components


class AppController:
    def __init__(
        self,
        stem_backend: StemGenerationBackend | None = None,
        stem_task_runner: StemTaskRunner | None = None,
        *,
        project_config_path: Path | None = None,
    ) -> None:
        self._persistence = (
            ProjectPersistence.from_config_path()
            if project_config_path is None
            else ProjectPersistence.from_config_path(project_config_path)
        )
        if project_config_path is not None:
            self._persistence.config_path = project_config_path
        self._project = self._persistence.project
        self._session = SessionState()
        self._close_requested = False
        self._shutdown_preparing = False
        self._shutdown_running = False
        self._shutdown_complete = False

        self._audio = AudioEngine()
        self._audio.set_resident_pcm_budget_mib(self._project.resident_pcm_budget_mib)
        self._audio.run()
        self._assets = ProjectAssetLifecycle(self._project, self._audio)
        self._assets.sync_assignments()
        self._persistence.bind_audio(self._audio, self._report_timing_save_error)

        self.settings = SettingsController(
            self._project,
            self._session,
            self._audio,
            on_project_changed=self._persistence.mark_dirty,
        )
        self.transport = TransportController(
            self._project,
            self._session,
            self._audio,
            on_project_changed=self._persistence.mark_dirty,
        )
        self.accepted_timing = AcceptedTimingController(self.transport)
        self.transport.bpm.set_accepted_refresh_callback(self.accepted_timing.refresh_current)
        self.stems = StemController(
            self._project,
            self._session,
            self._audio,
            on_project_changed=self._persistence.mark_dirty,
            stem_backend=stem_backend,
            stem_task_runner=stem_task_runner,
            asset_lifecycle=self._assets,
        )
        self.stems.fence_pair_metadata(self._persistence.invalid_stem_pair_ids)
        self.stems.bind_disk_selection_commit(
            self._persistence.commit_stem_selection, self._persistence.flush
        )
        self.arrangement = PadArrangementController(
            self._persistence,
            self._session,
            self._audio,
            self._assets,
            stem_task_runner=self.stems._stem_task_runner,
            on_copy_committed=self._on_copy_committed,
        )
        self.transport.bind_pad_edit_guard(self.ensure_not_copy_target)
        self.stems.bind_pad_edit_guard(self.ensure_not_copy_target)
        self.transport.bind_global_key_lock_edit_guard(
            self.arrangement.ensure_global_key_lock_edit_allowed
        )
        self.settings.bind_residency_policy(self.stems.refresh_residency_policy)
        self.loader = LoaderController(
            self._project,
            self._session,
            self._audio,
            on_pad_bpm_changed=self.transport.bpm.on_pad_bpm_changed,
            on_project_changed=self._persistence.mark_dirty,
            on_stem_generation_started=self.stems._handle_stem_generation_started,
            on_stem_generation_progress=self.stems._handle_stem_generation_progress,
            on_stem_generation_success=self.stems._handle_stem_generation_success,
            on_stem_generation_error=self.stems._handle_stem_generation_error,
            on_stems_deleted=self.stems.delete_stems,
        )
        self.loader.bind_asset_lifecycle(self._assets)
        self.transport.bind_incoming_global_key_lock(self.loader._refresh_incoming_global_intent)
        self.transport.bind_current_pad_edit(
            self.loader.record_current_pad_edit,
            incoming_request=self.loader.incoming_load_request_id,
        )
        self.loader.set_stems_invalidated_callback(self.stems.invalidate_stem_cache)
        self.loader.set_restored_sample_loaded_callback(
            self.stems.publish_restored_stem_cache_if_available
        )
        self.loader.set_new_sample_loaded_callback(
            self._on_new_sample_loaded,
            key_lock_callback=self._refresh_loaded_key_lock,
            incoming_callback=self._on_incoming_sample_loaded,
        )
        self.loader.set_sample_unloaded_callback(self._on_sample_unloaded)
        self.loader.set_accepted_timing_refresh_callback(self._refresh_restored_accepted_timing)
        self.material_migration = MaterialMigrationController(
            self._persistence,
            self._session,
            self._audio,
            self._assets,
            self.loader,
            stem_task_runner=self.stems._stem_task_runner,
        )
        self.metering = MeteringController(self._project, self._session, self._audio)
        self.performance_confirmation = PerformanceConfirmationController(self)
        self.input_mapping = InputMappingController(
            self,
            on_project_changed=self._persistence.mark_dirty,
        )

        self._controllers: set[BaseController] = {
            self.transport,
            self.loader,
            self.metering,
            self.stems,
            self.input_mapping,
        }
        self.transport.playback.set_global_playback_feedback_poll(self._poll_audio_messages)

        self.loader.restore_samples_from_project_state()
        self.stems.restore_stem_cache_from_project_state()
        self.material_migration.schedule_after_restore()
        self.transport.apply_project_state_to_audio()
        self.input_mapping.apply_project_state_to_input_runtime()

    def prepare_shutdown(self) -> bool:
        """Settle complete Copy ownership while its workers and callback remain alive."""
        if self._shutdown_complete:
            return True
        if self._shutdown_preparing:
            message = "Application shutdown preparation is already running"
            raise RuntimeError(message)
        self._shutdown_preparing = True
        try:
            if not self._close_requested:
                self._audio.set_input_mapping_enabled(False)
                self._close_requested = True
            return self.arrangement.prepare_shutdown()
        finally:
            self._shutdown_preparing = False

    def shut_down(self) -> None:
        """Release dependent owners only after actual Copy settlement permits exit."""
        if self._shutdown_complete:
            return
        if self._shutdown_running:
            message = "Application shutdown is already running"
            raise RuntimeError(message)
        self._shutdown_running = True
        try:
            if not self.prepare_shutdown():
                message = "Application close is waiting for complete Copy settlement"
                raise RuntimeError(message)
            self.material_migration.shut_down()
            self.transport.residency.shut_down()
            self.accepted_timing.shut_down()
            self.loader.shut_down()
            self.stems.shut_down()
            with suppress(OSError):
                self._persistence.flush()

            self._audio.stop_all()
            self._assets.release_saved_assignments()
            self._audio.shut_down()
            self._shutdown_complete = True
        finally:
            self._shutdown_running = False

    def _report_timing_save_error(self, sample_id: int, message: str) -> None:
        self._session.sample_analysis_errors[sample_id] = message

    def on_frame_render(self) -> None:
        self.arrangement.poll()
        for controller in self._controllers:
            controller.on_frame_render()

    def poll_runtime_events(self) -> None:
        """Poll runtime event sources and update controller-owned state projections."""
        self.loader.poll_loader_events()
        self.material_migration.poll()
        self._poll_audio_messages()
        self.accepted_timing.poll()
        self.transport.residency.poll()
        self.arrangement.poll()

    def ensure_not_copy_target(self, sample_id: int) -> None:
        """Reject a target gesture while its complete Copy transaction remains fenced."""
        validate_sample_id(sample_id)
        if sample_id in self.arrangement.fenced_sample_ids:
            message = "Complete Copy target receipt is pending; current-pad gesture rejected"
            raise ValueError(message)

    def _on_copy_committed(self, sample_id: int, ticket: PadCopyTicket) -> None:
        """Project the durable stopped target before its Native reservation is released."""
        validate_sample_id(sample_id)
        components = _copied_components_available(sample_id, ticket)
        try:
            selection = ticket.committed_stem_ticket()
        except AttributeError as error:
            message = "Complete Copy target registration metadata is unavailable"
            raise RuntimeError(message) from error
        self.stems.project_copied_native_state(
            sample_id, components_available=components, selection=selection
        )
        self._session.active_sample_ids.discard(sample_id)
        self._session.paused_sample_ids.discard(sample_id)
        self._session.pressed_pads[sample_id] = False
        self._session.pad_peak[sample_id] = 0.0
        self._session.pad_peak_updated_at[sample_id] = 0.0
        self._session.pad_clip_hold_until[sample_id] = 0.0
        self._session.pad_playhead_s[sample_id] = None
        self._session.pad_playhead_updated_at[sample_id] = 0.0
        self.metering._active_peak_sample_ids.discard(sample_id)
        self.transport.playback.discard_global_restore_for_unloaded_pad(sample_id)

    def _refresh_restored_accepted_timing(self, sample_id: int) -> None:
        self.accepted_timing.refresh_current(
            sample_id, on_refreshed=self.loader.finish_accepted_timing_refresh
        )

    def _on_sample_unloaded(self, sample_id: int) -> None:
        if self._session.waveform_editor_pad_id == sample_id:
            self.transport.waveform.close_editor()
        self.transport.residency.cancel(sample_id)
        self.accepted_timing.cancel(sample_id)
        self.transport.playback.discard_global_restore_for_unloaded_pad(sample_id)

    def _on_new_sample_loaded(self, sample_id: int, detected_start: float | None) -> None:
        self.transport.loop.initialize_loaded_pad_defaults(sample_id, detected_start)
        self.stems.reuse_compatible_stems_on_load(sample_id)

    def _on_incoming_sample_loaded(self, sample_id: int) -> None:
        """Refresh derived master and stem work after Native has adopted incoming defaults."""
        self.transport.residency.cancel(sample_id)
        self.transport.residency.remember(sample_id)
        if self._session.bpm_lock_anchor_pad_id == sample_id:
            self.transport.bpm.recompute_master_bpm()
        self.stems.reuse_compatible_stems_on_load(sample_id)

    def _refresh_loaded_key_lock(self, sample_id: int) -> None:
        """Retire prior-source control baselines before resolving the adopted source."""
        self.transport.residency.cancel(sample_id)
        # A settled snapshot cannot retire scalar defaults already queued by load/startup.
        self.transport.residency.apply_key_lock(sample_id, force=True)

    def _poll_audio_messages(self) -> None:
        while True:
            msg = self._audio.receive_msg()
            if msg is None:
                return

            self._handle_audio_message(msg)

    def _handle_audio_message(self, msg: object) -> None:
        if isinstance(msg, AudioMessage.PadPeak):
            self.metering.handle_pad_peak_message(msg)

        if isinstance(msg, AudioMessage.MasterPeak):
            self.metering.handle_master_peak_message(msg)

        if isinstance(msg, AudioMessage.PadPlayhead):
            self.metering.handle_pad_playhead_message(msg)

        if isinstance(msg, AudioMessage.SampleStarted):
            self.transport.playback.handle_sample_started_message(msg)

        if isinstance(msg, AudioMessage.SampleStopped):
            self.transport.playback.handle_sample_stopped_message(msg)

    @property
    def project(self) -> ProjectState:
        return self._project

    @property
    def session(self) -> SessionState:
        return self._session

    @property
    def persistence(self) -> ProjectPersistence:
        return self._persistence
