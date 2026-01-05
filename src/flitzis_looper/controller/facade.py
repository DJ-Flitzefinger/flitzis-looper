from contextlib import suppress

from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.metering import MeteringController
from flitzis_looper.controller.transport import TransportController
from flitzis_looper.models import ProjectState, SessionState
from flitzis_looper.persistence import ProjectPersistence, load_project_state
from flitzis_looper_audio import AudioEngine


class LooperController:
    def __init__(self) -> None:
        self._project = load_project_state()
        self._session = SessionState()

        self._audio = AudioEngine()
        self._audio.run()

        self._persistence = ProjectPersistence(self._project)

        self.transport = TransportController(
            self._project,
            self._session,
            self._audio,
            on_project_changed=self._persistence.mark_dirty,
        )
        self.loader = LoaderController(
            self._project,
            self._session,
            self._audio,
            self.transport._on_pad_bpm_changed,
            on_project_changed=self._persistence.mark_dirty,
        )
        self.metering = MeteringController(self._session, self._audio)

        self.transport._apply_project_state_to_audio()
        self.loader.restore_samples_from_project_state()

    def shut_down(self) -> None:
        with suppress(OSError):
            self._persistence.flush()

        self._audio.stop_all()
        self._audio.shut_down()

    @property
    def project(self) -> ProjectState:
        return self._project

    @property
    def session(self) -> SessionState:
        return self._session

    @property
    def persistence(self) -> ProjectPersistence:
        return self._persistence
