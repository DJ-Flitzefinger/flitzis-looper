from pathlib import Path, PureWindowsPath
from typing import TYPE_CHECKING, TypeVar, cast

from pydantic import BaseModel

from flitzis_looper.constants import SPEED_STEP
from flitzis_looper_audio import AudioMessage

if TYPE_CHECKING:
    from flitzis_looper.controller import AppController
    from flitzis_looper.models import ProjectState, SampleAnalysis, SessionState
    from flitzis_looper_audio import WaveFormRenderData

T = TypeVar("T", bound=BaseModel)


class ReadOnlyStateProxy[T]:
    """Wraps a Pydantic model and prevents attribute assignment."""

    def __init__(self, model: T):
        super().__setattr__("_model", model)

    def __getattr__(self, name: str) -> object:
        return getattr(self._model, name)

    def __setattr__(self, name: str, value: object) -> None:
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

    def set_pad_grid_offset_samples(self, pad_id: int, grid_offset_samples: int) -> None:
        self._controller.transport.loop.set_grid_offset_samples(pad_id, grid_offset_samples)

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

    def poll(self) -> None:
        self._poll_loader_events()
        self._poll_audio_messages()

    def _poll_loader_events(self) -> None:
        self._controller.loader.poll_loader_events()

    def _poll_audio_messages(self) -> None:
        while True:
            msg = self._controller._audio.receive_msg()
            if msg is None:
                return

            if isinstance(msg, AudioMessage.PadPeak):
                self._controller.metering.handle_pad_peak_message(msg)

            if isinstance(msg, AudioMessage.PadPlayhead):
                self._controller.metering.handle_pad_playhead_message(msg)

            if isinstance(msg, AudioMessage.SampleStarted):
                self._controller.transport.playback.handle_sample_started_message(msg)

            if isinstance(msg, AudioMessage.SampleStopped):
                self._controller.transport.playback.handle_sample_stopped_message(msg)


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

    # Cache render data
    _last_pad_id: int | None = None
    _last_width_px: int | None = None
    _last_start_s: float | None = None
    _last_end_s: float | None = None
    _last_waveform_value: WaveFormRenderData | None = None

    def __init__(self, controller: AppController) -> None:
        self._controller = controller

    def open(self, pad_id: int) -> None:
        session = self._controller.session
        session.waveform_editor_open = True
        session.waveform_editor_pad_id = pad_id

    def close(self) -> None:
        session = self._controller.session
        session.waveform_editor_open = False
        session.waveform_editor_pad_id = None

    def get_render_data(
        self, pad_id: int, width_px: int, start_s: float, end_s: float
    ) -> WaveFormRenderData | None:
        if (
            self._last_pad_id != pad_id
            or self._last_width_px != width_px
            or self._last_start_s != start_s
            or self._last_end_s != end_s
        ):
            self._last_pad_id = pad_id
            self._last_width_px = width_px
            self._last_start_s = start_s
            self._last_end_s = end_s
            self._last_waveform_value = self._controller.transport.waveform.get_render_data(
                pad_id, width_px, start_s, end_s
            )
        return self._last_waveform_value


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

    def on_frame_render(self) -> None:
        self._controller.on_frame_render()
