from contextlib import suppress
from typing import TYPE_CHECKING

from flitzis_looper.controller.accepted_publication import AcceptedTimingController
from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.metering import MeteringController
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.settings import SettingsController
from flitzis_looper.controller.stems import StemController, StemTaskRunner
from flitzis_looper.controller.transport import TransportController
from flitzis_looper.input_mapping import InputMappingController
from flitzis_looper.models import ProjectState, SessionState
from flitzis_looper_audio import AudioEngine, AudioMessage

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller.base import BaseController
    from flitzis_looper.controller.stem_generation import StemGenerationBackend


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

        self._audio = AudioEngine()
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
        self.loader.set_stems_invalidated_callback(self.stems.invalidate_stem_cache)
        self.loader.set_restored_sample_loaded_callback(
            self.stems.publish_restored_stem_cache_if_available
        )
        self.loader.set_new_sample_loaded_callback(
            self.transport.loop.initialize_loaded_pad_defaults
        )
        self.loader.set_sample_unloaded_callback(self._on_sample_unloaded)
        self.loader.set_accepted_timing_refresh_callback(self._refresh_restored_accepted_timing)
        self.metering = MeteringController(self._project, self._session, self._audio)
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
        self.transport.apply_project_state_to_audio()
        self.input_mapping.apply_project_state_to_input_runtime()

    def shut_down(self) -> None:
        self._audio.set_input_mapping_enabled(False)
        self.transport.residency.shut_down()
        self.accepted_timing.shut_down()
        self.loader.shut_down()
        self.stems.shut_down()
        with suppress(OSError):
            self._persistence.flush()

        self._audio.stop_all()
        self._assets.release_saved_assignments()
        self._audio.shut_down()

    def _report_timing_save_error(self, sample_id: int, message: str) -> None:
        self._session.sample_analysis_errors[sample_id] = message

    def on_frame_render(self) -> None:
        for controller in self._controllers:
            controller.on_frame_render()

    def poll_runtime_events(self) -> None:
        """Poll runtime event sources and update controller-owned state projections."""
        self.loader.poll_loader_events()
        self._poll_audio_messages()
        self.accepted_timing.poll()
        self.transport.residency.poll()

    def _refresh_restored_accepted_timing(self, sample_id: int) -> None:
        self.accepted_timing.refresh_current(
            sample_id, on_refreshed=self.loader.finish_accepted_timing_refresh
        )

    def _on_sample_unloaded(self, sample_id: int) -> None:
        self.transport.residency.cancel(sample_id)
        self.accepted_timing.cancel(sample_id)
        self.transport.playback.discard_global_restore_for_unloaded_pad(sample_id)

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
