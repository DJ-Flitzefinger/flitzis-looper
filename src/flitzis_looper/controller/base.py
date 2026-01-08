from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.models import ProjectState, SessionState
    from flitzis_looper_audio import AudioEngine


class BaseController:
    def __init__(
        self,
        project: ProjectState,
        session: SessionState,
        audio: AudioEngine,
        on_project_changed: Callable[[], None] | None = None,
    ) -> None:
        self._project = project
        self._session = session
        self._audio = audio
        self._on_project_changed = on_project_changed
        self._on_frame_render_callbacks: list[Callable[[], None]] = []

    def on_frame_render(self) -> None:
        for cb in self._on_frame_render_callbacks:
            cb()

    def _output_sample_rate_hz(self) -> int | None:
        fn = getattr(self._audio, "output_sample_rate", None)
        if fn is None:
            return None
        try:
            return int(fn())
        except (RuntimeError, TypeError, ValueError) as _:
            return None

    def _mark_project_changed(self) -> None:
        if self._on_project_changed is not None:
            self._on_project_changed()
