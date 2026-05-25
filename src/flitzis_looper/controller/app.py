from contextlib import suppress
from typing import TYPE_CHECKING

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
    from flitzis_looper.controller.base import BaseController
    from flitzis_looper.controller.stem_generation import StemGenerationBackend


class AppController:
    def __init__(
        self,
        stem_backend: StemGenerationBackend | None = None,
        stem_task_runner: StemTaskRunner | None = None,
    ) -> None:
        self._persistence = ProjectPersistence.from_config_path()
        self._project = self._persistence.project
        self._session = SessionState()

        self._audio = AudioEngine()
        self._audio.run()

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
        self.stems = StemController(
            self._project,
            self._session,
            self._audio,
            on_project_changed=self._persistence.mark_dirty,
            stem_backend=stem_backend,
            stem_task_runner=stem_task_runner,
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

        self.transport.apply_project_state_to_audio()
        self.stems.restore_stem_cache_from_project_state()
        self.loader.restore_samples_from_project_state()
        self.input_mapping.apply_project_state_to_input_runtime()

    def shut_down(self) -> None:
        with suppress(OSError):
            self._persistence.flush()

        self._audio.stop_all()
        self._audio.shut_down()

    def on_frame_render(self) -> None:
        for controller in self._controllers:
            controller.on_frame_render()

    def poll_runtime_events(self) -> None:
        """Poll runtime event sources and update controller-owned state projections."""
        self.loader.poll_loader_events()
        self._poll_audio_messages()

    def _poll_audio_messages(self) -> None:
        while True:
            msg = self._audio.receive_msg()
            if msg is None:
                return

            self._handle_audio_message(msg)

    def _handle_audio_message(self, msg: object) -> None:
        if isinstance(msg, AudioMessage.PadPeak):
            self.metering.handle_pad_peak_message(msg)

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
