from contextlib import suppress
from typing import TYPE_CHECKING

from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.metering import MeteringController
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.stems import StemController, StemTaskRunner
from flitzis_looper.controller.transport import TransportController
from flitzis_looper.models import ProjectState, SessionState
from flitzis_looper_audio import AudioEngine

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
        )
        self.metering = MeteringController(self._project, self._session, self._audio)

        self._controllers: set[BaseController] = {
            self.transport,
            self.loader,
            self.metering,
            self.stems,
        }

        self.transport.apply_project_state_to_audio()
        self.stems.restore_stem_cache_from_project_state()
        self.loader.restore_samples_from_project_state()

    def shut_down(self) -> None:
        with suppress(OSError):
            self._persistence.flush()

        self._audio.stop_all()
        self._audio.shut_down()

    def on_frame_render(self) -> None:
        for controller in self._controllers:
            controller.on_frame_render()

    @property
    def project(self) -> ProjectState:
        return self._project

    @property
    def session(self) -> SessionState:
        return self._session

    @property
    def persistence(self) -> ProjectPersistence:
        return self._persistence
