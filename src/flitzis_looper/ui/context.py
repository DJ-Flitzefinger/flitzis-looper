from pathlib import Path, PureWindowsPath
from typing import TYPE_CHECKING, Any, cast

from flitzis_looper.constants import SPEED_STEP

if TYPE_CHECKING:
    from pydantic import BaseModel

    from flitzis_looper.controller import AppController
    from flitzis_looper.models import ProjectState, SampleAnalysis, SessionState


class ReadOnlyStateProxy:
    """Wraps a Pydantic model and prevents attribute assignment."""

    def __init__(self, model: BaseModel):
        self._model = model

    def __getattr__(self, name: str) -> Any:
        return getattr(self._model, name)

    def __setattr__(self, name: str, value: Any) -> None:
        if name == "_model":
            super().__setattr__(name, value)
        else:
            msg = f"State is read-only. Use controller actions to mutate '{name}'."
            raise AttributeError(msg)


class PadSelectors:
    def __init__(self, controller: AppController, project: ProjectState, session: SessionState):
        self._controller = controller
        self._project = project
        self._session = session

    def label(self, pad_id: int) -> str:
        """Derive the UI label for a performance pad."""
        path = self._project.sample_paths[pad_id]
        if path is None:
            pending = self._controller.loader.pending_sample_path(pad_id)
            if pending is None:
                return ""
            path = pending

        if "\\" in path:
            return PureWindowsPath(path).name

        return Path(path).name

    def is_loaded(self, pad_id: int) -> bool:
        return self._controller.loader.is_sample_loaded(pad_id)

    def is_loading(self, pad_id: int) -> bool:
        return self._controller.loader.is_sample_loading(pad_id)

    def load_error(self, pad_id: int) -> str | None:
        return self._controller.loader.sample_load_error(pad_id)

    def load_progress(self, pad_id: int) -> float | None:
        return self._controller.loader.sample_load_progress(pad_id)

    def load_stage(self, pad_id: int) -> str | None:
        return self._controller.loader.sample_load_stage(pad_id)

    def analysis(self, pad_id: int) -> SampleAnalysis | None:
        return self._project.sample_analysis[pad_id]

    def manual_bpm(self, pad_id: int) -> float | None:
        return self._project.manual_bpm[pad_id]

    def effective_bpm(self, pad_id: int) -> float | None:
        return self._controller.transport.bpm.effective_bpm(pad_id)

    def manual_key(self, pad_id: int) -> str | None:
        return self._project.manual_key[pad_id]

    def effective_key(self, pad_id: int) -> str | None:
        return self._controller.transport.pad.effective_key(pad_id)

    def effective_loop_region(self, pad_id: int) -> tuple[float, float | None]:
        return self._controller.transport.loop.effective_region(pad_id)

    def is_analyzing(self, pad_id: int) -> bool:
        return pad_id in self._session.analyzing_sample_ids

    def analysis_status(self, pad_id: int) -> tuple[str | None, float | None]:
        stage = self._session.sample_analysis_stage.get(pad_id)
        value = self._session.sample_analysis_progress.get(pad_id)
        progress = float(value) if value is not None else None
        return (stage, progress)

    def analysis_error(self, pad_id: int) -> str | None:
        return self._session.sample_analysis_errors.get(pad_id)

    def peak(self, pad_id: int) -> float:
        return float(self._session.pad_peak[pad_id])

    def is_active(self, pad_id: int) -> bool:
        return pad_id in self._session.active_sample_ids

    def is_pressed(self, pad_id: int) -> bool:
        return self._session.pressed_pads[pad_id]

    def is_selected(self, pad_id: int) -> bool:
        return self._project.selected_pad == pad_id


class BankSelectors:
    def __init__(self, project: ProjectState):
        self._project = project

    def is_selected(self, bank_id: int) -> bool:
        return self._project.selected_bank == bank_id


class GlobalSelectors:
    def __init__(self, controller: AppController, project: ProjectState, session: SessionState):
        self._controller = controller
        self._project = project
        self._session = session

    def effective_bpm(self) -> float | None:
        """Return the current effective global BPM."""
        if self._project.bpm_lock and self._session.master_bpm is not None:
            return float(self._session.master_bpm)

        selected = self._project.selected_pad
        bpm = self._controller.transport.bpm.effective_bpm(selected)
        if bpm is None:
            return None

        return float(bpm) * float(self._project.speed)


class UiState:
    """Read-only proxy of app state for UI rendering."""

    pads: PadSelectors
    banks: BankSelectors
    global_: GlobalSelectors

    def __init__(self, controller: AppController):
        self._controller = controller
        self._project_proxy = ReadOnlyStateProxy(controller.project)
        self._session_proxy = ReadOnlyStateProxy(controller.session)

        project = cast("ProjectState", self._project_proxy)
        session = cast("SessionState", self._session_proxy)
        self.pads = PadSelectors(controller, project, session)
        self.banks = BankSelectors(project)
        self.global_ = GlobalSelectors(controller, project, session)

    @property
    def project(self) -> ProjectState:
        return cast("ProjectState", self._project_proxy)

    @property
    def session(self) -> SessionState:
        return cast("SessionState", self._session_proxy)


class PadAudioActions:
    def __init__(self, controller: AppController):
        self._controller = controller

    def trigger_pad(self, pad_id: int) -> None:
        self._controller.transport.playback.trigger_pad(pad_id)

    def stop_pad(self, pad_id: int) -> None:
        self._controller.transport.playback.stop_pad(pad_id)

    def play_pad(self, pad_id: int) -> None:
        self._controller.transport.playback.play(pad_id)

    def toggle_pad_playback(self, pad_id: int) -> None:
        self._controller.transport.playback.toggle(pad_id)

    def reset_pad_loop_region(self, pad_id: int) -> None:
        self._controller.transport.loop.reset(pad_id)

    def set_pad_loop_auto(self, pad_id: int, *, enabled: bool) -> None:
        self._controller.transport.loop.set_auto(pad_id, enabled=enabled)

    def set_pad_loop_bars(self, pad_id: int, *, bars: int) -> None:
        self._controller.transport.loop.set_bars(pad_id, bars=bars)

    def set_pad_loop_start(self, pad_id: int, start_s: float) -> None:
        self._controller.transport.loop.set_start(pad_id, start_s)

    def set_pad_loop_end(self, pad_id: int, end_s: float | None) -> None:
        self._controller.transport.loop.set_end(pad_id, end_s)

    def load_sample_async(self, pad_id: int, path: str) -> None:
        self._controller.loader.load_sample_async(pad_id, path)

    def unload_sample(self, pad_id: int) -> None:
        self._controller.loader.unload_sample(pad_id)

    def analyze_sample_async(self, pad_id: int) -> None:
        self._controller.loader.analyze_sample_async(pad_id)

    def set_manual_bpm(self, pad_id: int, bpm: float) -> None:
        self._controller.transport.bpm.set_manual_bpm(pad_id, bpm)

    def clear_manual_bpm(self, pad_id: int) -> None:
        self._controller.transport.bpm.clear_manual_bpm(pad_id)

    def tap_bpm(self, pad_id: int) -> float | None:
        return self._controller.transport.bpm.tap_bpm(pad_id)

    def set_manual_key(self, pad_id: int, key: str) -> None:
        self._controller.transport.pad.set_manual_key(pad_id, key)

    def clear_manual_key(self, pad_id: int) -> None:
        self._controller.transport.pad.clear_manual_key(pad_id)

    def set_pad_gain(self, pad_id: int, gain: float) -> None:
        self._controller.transport.pad.set_pad_gain(pad_id, gain)

    def set_pad_eq(self, pad_id: int, low_db: float, mid_db: float, high_db: float) -> None:
        self._controller.transport.pad.set_pad_eq(pad_id, low_db, mid_db, high_db)


class GlobalAudioActions:
    def __init__(self, controller: AppController):
        self._controller = controller

    def set_volume(self, volume: float) -> None:
        self._controller.transport.global_params.set_volume(volume)

    def set_speed(self, speed: float) -> None:
        self._controller.transport.global_params.set_speed(speed)

    def reset_speed(self) -> None:
        self._controller.transport.global_params.reset_speed()

    def increase_speed(self) -> None:
        self._controller.transport.global_params.set_speed(
            self._controller.project.speed + SPEED_STEP
        )

    def decrease_speed(self) -> None:
        self._controller.transport.global_params.set_speed(
            self._controller.project.speed - SPEED_STEP
        )

    def toggle_multi_loop(self) -> None:
        self._controller.transport.global_params.set_multi_loop(
            enabled=not self._controller.project.multi_loop
        )

    def toggle_key_lock(self) -> None:
        self._controller.transport.global_params.set_key_lock(
            enabled=not self._controller.project.key_lock
        )

    def toggle_bpm_lock(self) -> None:
        self._controller.transport.global_params.set_bpm_lock(
            enabled=not self._controller.project.bpm_lock
        )


class PollActions:
    def __init__(self, controller: AppController):
        self._controller = controller

    def poll_loader_events(self) -> None:
        self._controller.loader.poll_loader_events()

    def poll_audio_messages(self) -> None:
        self._controller.metering.poll_audio_messages()


class AudioActions:
    """Audio-related UI actions."""

    pads: PadAudioActions
    global_: GlobalAudioActions
    poll: PollActions

    def __init__(self, controller: AppController):
        self.pads = PadAudioActions(controller)
        self.global_ = GlobalAudioActions(controller)
        self.poll = PollActions(controller)


class WaveformEditorActions:
    """Waveform editor UI state/actions."""

    def __init__(self, controller: AppController) -> None:
        self._controller = controller

    def open(self, pad_id: int) -> None:
        session = self._controller.session
        session.waveform_editor_open = True
        session.waveform_editor_pad_id = pad_id

        start_s, end_s = self._controller.transport.loop.effective_region(pad_id)
        xmin = max(0.0, float(start_s) - 1.0)
        xmax = float(start_s) + 9.0 if end_s is None else float(end_s) + 1.0
        if xmax <= xmin:
            xmax = xmin + 1.0

        session.waveform_editor_view_xmin_s = xmin
        session.waveform_editor_view_xmax_s = xmax

    def close(self) -> None:
        session = self._controller.session
        session.waveform_editor_open = False
        session.waveform_editor_pad_id = None

    def set_view(self, xmin_s: float, xmax_s: float) -> None:
        session = self._controller.session
        session.waveform_editor_view_xmin_s = float(xmin_s)
        session.waveform_editor_view_xmax_s = float(xmax_s)

    def zoom(self, *, zoom_in: bool) -> None:
        session = self._controller.session
        xmin = float(session.waveform_editor_view_xmin_s)
        xmax = float(session.waveform_editor_view_xmax_s)
        if not (xmax > xmin):
            xmin = 0.0
            xmax = 8.0

        center = (xmin + xmax) * 0.5
        duration = (xmax - xmin) * (0.5 if zoom_in else 2.0)
        duration = max(0.001, duration)

        self.set_view(center - duration * 0.5, center + duration * 0.5)

    def pan(self, *, left: bool) -> None:
        session = self._controller.session
        xmin = float(session.waveform_editor_view_xmin_s)
        xmax = float(session.waveform_editor_view_xmax_s)
        if not (xmax > xmin):
            xmin = 0.0
            xmax = 8.0

        delta = (xmax - xmin) * 0.2
        if left:
            delta = -delta

        self.set_view(xmin + delta, xmax + delta)

    def zoom_wheel(self, wheel: float, mouse_x_s: float | None) -> None:
        session = self._controller.session
        xmin = float(session.waveform_editor_view_xmin_s)
        xmax = float(session.waveform_editor_view_xmax_s)
        if not (xmax > xmin):
            xmin = 0.0
            xmax = 8.0

        zoom = 0.9**wheel
        new_duration = max(0.001, (xmax - xmin) * zoom)
        anchor = (xmin + xmax) * 0.5 if mouse_x_s is None else float(mouse_x_s)
        left_ratio = 0.5 if mouse_x_s is None else (anchor - xmin) / (xmax - xmin)
        left_ratio = min(max(left_ratio, 0.0), 1.0)

        new_xmin = anchor - new_duration * left_ratio
        new_xmax = new_xmin + new_duration
        self.set_view(new_xmin, new_xmax)

    def pan_drag(self, dx_px: float, width_px: float) -> None:
        session = self._controller.session
        xmin = float(session.waveform_editor_view_xmin_s)
        xmax = float(session.waveform_editor_view_xmax_s)
        if not (xmax > xmin) or width_px <= 0:
            return

        delta_s = -(dx_px / width_px) * (xmax - xmin)
        self.set_view(xmin + delta_s, xmax + delta_s)


class UiActions:
    """UI-related actions."""

    waveform: WaveformEditorActions

    def __init__(self, controller: AppController):
        self._controller = controller
        self.waveform = WaveformEditorActions(controller)

    def toggle_left_sidebar(self) -> None:
        new_val = not self._controller.project.sidebar_left_expanded
        self._controller.project.sidebar_left_expanded = new_val
        self._controller.persistence.mark_dirty()

    def toggle_right_sidebar(self) -> None:
        new_val = not self._controller.project.sidebar_right_expanded
        self._controller.project.sidebar_right_expanded = new_val
        self._controller.persistence.mark_dirty()

    def open_file_dialog(self, pad_id: int) -> None:
        self._controller.session.file_dialog_pad_id = pad_id

    def close_file_dialog(self) -> None:
        self._controller.session.file_dialog_pad_id = None

    def select_pad(self, pad_id: int) -> None:
        self._controller.project.selected_pad = pad_id
        self._controller.persistence.mark_dirty()

    def select_bank(self, bank_id: int) -> None:
        self._controller.project.selected_bank = bank_id
        self._controller.persistence.mark_dirty()

    def store_pressed_pad_state(self, pad_id: int, *, pressed: bool) -> None:
        self._controller.session.pressed_pads[pad_id] = pressed


class UiContext:
    """The public interface for the UI layer."""

    state: UiState
    audio: AudioActions
    ui: UiActions

    def __init__(self, controller: AppController):
        self._controller = controller
        self.state = UiState(controller)
        self.audio = AudioActions(controller)
        self.ui = UiActions(controller)
        self.persistence = controller.persistence
