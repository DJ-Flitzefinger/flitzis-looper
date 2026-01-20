from typing import TYPE_CHECKING

from flitzis_looper.controller.base import BaseController
from flitzis_looper.controller.transport.bpm import BpmController
from flitzis_looper.controller.transport.global_params import GlobalParametersController
from flitzis_looper.controller.transport.loop import PadLoopController
from flitzis_looper.controller.transport.pad import PadController
from flitzis_looper.controller.transport.playback import PadPlaybackController
from flitzis_looper.controller.transport.state import ApplyProjectState
from flitzis_looper.controller.transport.waveform import WaveformController

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.models import ProjectState, SessionState
    from flitzis_looper_audio import AudioEngine


class TransportController(BaseController):
    def __init__(
        self,
        project: ProjectState,
        session: SessionState,
        audio: AudioEngine,
        on_project_changed: Callable[[], None] | None = None,
    ) -> None:
        super().__init__(project, session, audio, on_project_changed)

        self.bpm = BpmController(self)
        self.global_params = GlobalParametersController(self)
        self.loop = PadLoopController(self)
        self.playback = PadPlaybackController(self)
        self.pad = PadController(self)
        self.waveform = WaveformController(self)

    def apply_project_state_to_audio(self) -> None:
        ApplyProjectState(self).apply_project_state_to_audio()
