from flitzis_looper.controller.loader import LoaderController
from flitzis_looper.controller.metering import MeteringController
from flitzis_looper.controller.transport import TransportController
from flitzis_looper.models import ProjectState, SessionState
from flitzis_looper_audio import AudioEngine


class LooperController:
    def __init__(self) -> None:
        self._project = ProjectState()
        self._session = SessionState()

        self._audio = AudioEngine()
        self._audio.run()

        self.transport = TransportController(self._project, self._session, self._audio)
        self.loader = LoaderController(
            self._project,
            self._session,
            self._audio,
            self.transport._on_pad_bpm_changed,
        )
        self.metering = MeteringController(self._session, self._audio)

    def shut_down(self) -> None:
        self._audio.stop_all()
        self._audio.shut_down()

    @property
    def project(self) -> ProjectState:
        return self._project

    @property
    def session(self) -> SessionState:
        return self._session
