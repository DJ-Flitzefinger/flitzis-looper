from pathlib import Path, PureWindowsPath
from typing import TYPE_CHECKING, TypeVar, cast

from pydantic import BaseModel

from flitzis_looper.controller.current_timing import UNRESOLVED_TIMING, UnresolvedTiming
from flitzis_looper.input_mapping import (
    KeyboardBinding,
    LooperAction,
    PadEqBand,
    global_speed_action,
    global_speed_delta_action,
    master_volume_action,
    master_volume_delta_action,
    pad_eq_action,
    pad_gain_action,
    pad_gain_delta_action,
    selected_pad_eq_delta_action,
    selected_tap_bpm_action,
    start_stop_action,
)
from flitzis_looper.input_timing import validate_input_timestamp_ns
from flitzis_looper.ui.waveform_grid import waveform_view_start

if TYPE_CHECKING:
    from imgui_bundle import imgui

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.current_timing import CurrentPadTiming
    from flitzis_looper.models import (
        ProjectState,
        SampleAnalysis,
        SessionState,
        StemGridIndicatorState,
        StemMaskDisplayMode,
        StemMixMode,
        StemSeparator,
        TriggerQuantizationMode,
        TriggerQuantizationStep,
    )
    from flitzis_looper_audio import WaveFormRenderData

T = TypeVar("T", bound=BaseModel)
type _WaveformSourceIdentity = tuple[
    str | None,
    float | None,
    tuple[str, int, str, str, int, int] | None,
    tuple[int, str, int, int] | None,
]


class ReadOnlyStateProxy[T]:
    """Wraps a Pydantic model and prevents attribute assignment."""

    def __init__(self, model: T):
        super().__setattr__("_model", model)

    def __getattr__(self, name: str) -> object:
        return getattr(self._model, name)

    def __setattr__(self, name: str, value: object) -> None:
        msg = f"State is read-only. Use controller actions to mutate '{name}'."
        raise AttributeError(msg)


class PadSelectors:  # noqa: PLR0904 - selector facade intentionally mirrors pad UI needs.
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

    def current_timing(self, pad_id: int) -> CurrentPadTiming | None:
        """Return one coherent current source period, origin and accepted revision."""
        return self._controller.transport.bpm.current_timing(pad_id)

    def manual_key(self, pad_id: int) -> str | None:
        return self._project.manual_key[pad_id]

    def effective_key(self, pad_id: int) -> str | None:
        return self._controller.transport.pad.effective_key(pad_id)

    def key_lock(self, pad_id: int) -> bool:
        if self._project.sample_paths[pad_id] is None:
            return False
        return bool(self._project.pad_key_lock[pad_id])

    def effective_loop_region(
        self,
        pad_id: int,
        *,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
    ) -> tuple[float, float | None]:
        if isinstance(timing, UnresolvedTiming):
            return self._controller.transport.loop.effective_region(pad_id)
        return self._controller.transport.loop.effective_region(pad_id, timing=timing)

    def grid_anchor_s(self, pad_id: int) -> float:
        """Return the same signed source origin used by snapping and native timing."""
        return self._controller.transport.loop.grid_anchor_sec(pad_id)

    def max_auto_loop_bars(self, pad_id: int) -> float | None:
        return self._controller.transport.loop.max_auto_loop_bars(pad_id)

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

    def clip_active(self, pad_id: int) -> bool:
        return self._controller.metering.pad_clip_active(pad_id)

    def is_active(self, pad_id: int) -> bool:
        return pad_id in self._session.active_sample_ids

    def is_pressed(self, pad_id: int) -> bool:
        return self._session.pressed_pads[pad_id]

    def is_selected(self, pad_id: int) -> bool:
        return self._project.selected_pad == pad_id


class StemSelectors:
    def __init__(self, controller: AppController, session: SessionState):
        self._controller = controller
        self._session = session

    def stems_available(self, pad_id: int) -> bool:
        return self._controller.stems.stems_available(pad_id)

    def has_stem_cache(self, pad_id: int) -> bool:
        return self._controller.stems.has_stem_cache(pad_id)

    def stem_mix_mode(self, pad_id: int) -> StemMixMode:
        return self._controller.stems.stem_mix_mode(pad_id)

    def stem_enabled_mask(self, pad_id: int) -> int:
        return self._controller.stems.stem_enabled_mask(pad_id)

    def stem_last_custom_mask(self, pad_id: int) -> int:
        return int(self._session.pad_stem_last_custom_mask[pad_id])

    def stem_mask_display_mode(self, pad_id: int) -> StemMaskDisplayMode:
        return self._controller.stems.stem_mask_display_mode(pad_id)

    def stem_mask_controls_enabled(self, pad_id: int) -> bool:
        return self._controller.stems.stem_mask_controls_enabled(pad_id)

    def stem_grid_indicator_state(self, pad_id: int) -> StemGridIndicatorState | None:
        return self._controller.stems.stem_grid_indicator_state(pad_id)

    def stem_generation_error(self, pad_id: int) -> str | None:
        return self._controller.stems.stem_generation_error(pad_id)

    def is_stem_generation_running(self, pad_id: int) -> bool:
        return self._controller.stems.is_stem_generation_running(pad_id)

    def stem_generation_block_reason(self, pad_id: int) -> str | None:
        return self._controller.stems.stem_generation_block_reason(pad_id)

    def stem_generation_status(self, pad_id: int) -> tuple[str | None, float | None]:
        return (
            self._controller.stems.stem_generation_stage(pad_id),
            self._controller.stems.stem_generation_progress(pad_id),
        )


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
        return self._controller.transport.global_params.effective_display_bpm()

    def speed_reference_bpm(self) -> float | None:
        """Return the BPM represented by neutral 1.00x speed."""
        return self._controller.transport.global_params.speed_reference_bpm()

    def master_output_peak(self) -> float:
        """Return the current master output peak used by the bottom-bar meter."""
        return float(self._session.master_output_peak)

    def master_clip_active(self) -> bool:
        """Return whether the held master output clip indicator is active."""
        return self._controller.metering.master_clip_active()


class UiState:
    """Read-only proxy of app state for UI rendering."""

    pads: PadSelectors
    stems: StemSelectors
    banks: BankSelectors
    global_: GlobalSelectors

    def __init__(self, controller: AppController):
        self._controller = controller
        self._project_proxy = ReadOnlyStateProxy(controller.project)
        self._session_proxy = ReadOnlyStateProxy(controller.session)

        project = cast("ProjectState", self._project_proxy)
        session = cast("SessionState", self._session_proxy)
        self.pads = PadSelectors(controller, project, session)
        self.stems = StemSelectors(controller, session)
        self.banks = BankSelectors(project)
        self.global_ = GlobalSelectors(controller, project, session)

    @property
    def project(self) -> ProjectState:
        return cast("ProjectState", self._project_proxy)

    @property
    def session(self) -> SessionState:
        return cast("SessionState", self._session_proxy)


class PadAudioActions:  # noqa: PLR0904 - pad action facade mirrors selected-pad controls.
    def __init__(self, controller: AppController):
        self._controller = controller

    def residency_status(self, pad_id: int) -> tuple[str | None, str | None]:
        """Read requested control readiness separately from effective playback."""
        return self._controller.transport.residency.status(pad_id)

    def cancel_residency(self, pad_id: int) -> None:
        """Cancel a pending source context before native adoption claims it."""
        self._controller.transport.residency.cancel_requested(pad_id)

    def trigger_pad(self, pad_id: int, *, received_at_ns: int | None = None) -> None:
        """Trigger or learn a pad action, retaining its captured Rust input timestamp."""
        self._controller.input_mapping.perform_learnable_action(
            LooperAction.trigger_pad(pad_id),
            lambda: self._controller.transport.playback.trigger_pad(
                pad_id, received_at_ns=received_at_ns
            ),
        )

    def stop_pad(self, pad_id: int) -> None:
        self._controller.input_mapping.perform_learnable_action(
            LooperAction.stop_pad(pad_id),
            lambda: self._controller.transport.playback.stop_pad(pad_id),
        )

    def set_pad_full_track_loop_region(self, pad_id: int) -> None:
        self._controller.transport.loop.set_full_track_region(pad_id)

    def set_pad_loop_auto(self, pad_id: int, *, enabled: bool) -> None:
        self._controller.transport.loop.set_auto(pad_id, enabled=enabled)

    def set_pad_loop_bars(self, pad_id: int, *, bars: float) -> None:
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
        self._controller.input_mapping.perform_learnable_action(
            LooperAction.unload_pad(pad_id),
            lambda: self._controller.loader.unload_sample(pad_id),
        )

    def analyze_sample_async(self, pad_id: int) -> None:
        self._controller.input_mapping.perform_learnable_action(
            LooperAction.analyze_pad(pad_id),
            lambda: self._controller.loader.analyze_sample_async(pad_id),
        )

    def set_manual_bpm(self, pad_id: int, bpm: float) -> None:
        self._controller.transport.bpm.set_manual_bpm(pad_id, bpm)

    def clear_manual_bpm(self, pad_id: int) -> None:
        self._controller.transport.bpm.clear_manual_bpm(pad_id)

    def tap_bpm(self, pad_id: int) -> float | None:
        bpm: float | None = None

        def execute() -> None:
            nonlocal bpm
            bpm = self._controller.transport.bpm.tap_bpm(pad_id)

        self._controller.input_mapping.perform_learnable_action(
            selected_tap_bpm_action(),
            execute,
        )
        return bpm

    def set_manual_key(self, pad_id: int, key: str) -> None:
        self._controller.transport.pad.set_manual_key(pad_id, key)

    def clear_manual_key(self, pad_id: int) -> None:
        self._controller.transport.pad.clear_manual_key(pad_id)

    def set_pad_gain(self, pad_id: int, gain_db: float) -> None:
        action = (
            pad_gain_delta_action(pad_id)
            if _midi_cc_learn_input_pending(self._controller)
            else pad_gain_action(pad_id, gain_db)
        )
        self._controller.input_mapping.perform_learnable_action(
            action,
            lambda: self._controller.transport.pad.set_pad_gain(pad_id, gain_db),
        )

    def set_pad_eq(self, pad_id: int, low_db: float, mid_db: float, high_db: float) -> None:
        self._controller.transport.pad.set_pad_eq(pad_id, low_db, mid_db, high_db)

    def set_pad_eq_band(self, pad_id: int, band: PadEqBand, db: float) -> None:
        low_db = float(self._controller.project.pad_eq_low_db[pad_id])
        mid_db = float(self._controller.project.pad_eq_mid_db[pad_id])
        high_db = float(self._controller.project.pad_eq_high_db[pad_id])

        if band == "low":
            low_db = db
        elif band == "mid":
            mid_db = db
        else:
            high_db = db

        action = (
            selected_pad_eq_delta_action(band)
            if _midi_cc_learn_input_pending(self._controller)
            else pad_eq_action(pad_id, band, db)
        )
        self._controller.input_mapping.perform_learnable_action(
            action,
            lambda: self._controller.transport.pad.set_pad_eq(
                pad_id,
                low_db,
                mid_db,
                high_db,
            ),
        )

    def toggle_pad_key_lock(self, pad_id: int) -> None:
        self._controller.transport.pad.toggle_pad_key_lock(pad_id)


class StemAudioActions:
    def __init__(self, controller: AppController):
        self._controller = controller

    def generate_stems_async(self, pad_id: int) -> None:
        def execute() -> None:
            self._controller.stems.generate_stems_async(pad_id)

        self._controller.input_mapping.perform_learnable_action(
            LooperAction.generate_stems(pad_id),
            execute,
        )

    def delete_stems(self, pad_id: int) -> None:
        def execute() -> None:
            self._controller.stems.delete_stems(pad_id)

        self._controller.input_mapping.perform_learnable_action(
            LooperAction.delete_stems(pad_id),
            execute,
        )

    def set_stem_mix_mode(self, pad_id: int, mode: StemMixMode) -> None:
        def execute() -> None:
            self._controller.stems.set_stem_mix_mode(pad_id, mode)

        self._controller.input_mapping.perform_learnable_action(
            LooperAction.stem_mix(pad_id, mode),
            execute,
        )

    def set_stem_enabled_mask(
        self,
        pad_id: int,
        enabled_stem_mask: int,
        display_mode: StemMaskDisplayMode = "custom",
    ) -> None:
        def execute() -> None:
            self._controller.stems.set_stem_enabled_mask(pad_id, enabled_stem_mask, display_mode)

        self._controller.input_mapping.perform_learnable_action(
            LooperAction.stem_mask(pad_id, enabled_stem_mask, display_mode),
            execute,
        )


class GlobalAudioActions:
    def __init__(self, controller: AppController):
        self._controller = controller

    def set_volume(self, volume: float) -> None:
        action = (
            master_volume_delta_action()
            if _midi_cc_learn_input_pending(self._controller)
            else master_volume_action(volume)
        )
        self._controller.input_mapping.perform_learnable_action(
            action,
            lambda: self._controller.transport.global_params.set_volume(volume),
        )

    def set_momentary_output_mute(self, *, enabled: bool) -> None:
        self._controller.transport.global_params.set_momentary_output_mute(enabled=enabled)

    def start_or_restart_start_stop(self, *, received_at_ns: int | None = None) -> None:
        """Restart loops with one captured Rust input timestamp shared by all pads."""
        self._controller.input_mapping.perform_learnable_action(
            start_stop_action(),
            lambda: self._controller.transport.playback.start_or_restart_global_start_stop(
                received_at_ns=received_at_ns
            ),
        )

    def stop_start_stop(self) -> None:
        self._controller.transport.playback.stop_global_start_stop()

    def set_speed(self, speed: float) -> None:
        action = (
            global_speed_delta_action()
            if _midi_cc_learn_input_pending(self._controller)
            else global_speed_action(speed)
        )
        self._controller.input_mapping.perform_learnable_action(
            action,
            lambda: self._controller.transport.global_params.set_speed(speed),
        )

    def set_effective_bpm(self, bpm: float) -> None:
        self._controller.transport.global_params.set_effective_display_bpm(bpm)

    def nudge_speed_by_bpm_steps(self, steps: int) -> None:
        self._controller.transport.global_params.nudge_speed_by_bpm_steps(steps)

    def reset_speed(self) -> None:
        self._controller.input_mapping.perform_learnable_action(
            LooperAction.reset_speed(),
            self._controller.transport.global_params.reset_speed,
        )

    def increase_speed(self) -> None:
        self._controller.input_mapping.perform_learnable_action(
            LooperAction.speed_delta("increase"),
            lambda: self._controller.transport.global_params.nudge_speed_by_bpm_step(1),
        )

    def decrease_speed(self) -> None:
        self._controller.input_mapping.perform_learnable_action(
            LooperAction.speed_delta("decrease"),
            lambda: self._controller.transport.global_params.nudge_speed_by_bpm_step(-1),
        )

    def toggle_multi_loop(self) -> None:
        self._controller.input_mapping.perform_learnable_action(
            LooperAction.toggle_multi_loop(),
            lambda: self._controller.transport.global_params.set_multi_loop(
                enabled=not self._controller.project.multi_loop
            ),
        )

    def toggle_key_lock(self) -> None:
        self._controller.input_mapping.perform_learnable_action(
            LooperAction.toggle_key_lock(),
            lambda: self._controller.transport.global_params.set_key_lock(
                enabled=not self._controller.project.key_lock
            ),
        )

    def toggle_bpm_lock(self) -> None:
        self._controller.input_mapping.perform_learnable_action(
            LooperAction.toggle_bpm_lock(),
            lambda: self._controller.transport.global_params.set_bpm_lock(
                enabled=not self._controller.project.bpm_lock
            ),
        )

    def set_trigger_quantization(self, mode: TriggerQuantizationMode) -> None:
        self._controller.input_mapping.perform_learnable_action(
            LooperAction.trigger_quantization(mode),
            lambda: self._controller.transport.global_params.set_trigger_quantization(mode),
        )

    def toggle_trigger_quantization(self) -> None:
        self._controller.input_mapping.perform_learnable_action(
            LooperAction.toggle_trigger_quantization(),
            self._controller.transport.global_params.toggle_trigger_quantization,
        )


class PollActions:
    def __init__(self, controller: AppController):
        self._controller = controller

    def poll(self) -> None:
        self._controller.poll_runtime_events()


class AudioActions:
    """Audio-related UI actions."""

    pads: PadAudioActions
    stems: StemAudioActions
    global_: GlobalAudioActions
    poll: PollActions

    def __init__(self, controller: AppController):
        self.pads = PadAudioActions(controller)
        self.stems = StemAudioActions(controller)
        self.global_ = GlobalAudioActions(controller)
        self.poll = PollActions(controller)


class WaveformEditorActions:
    """Waveform editor UI state/actions."""

    def __init__(self, controller: AppController) -> None:
        self._controller = controller
        self._last_pad_id: int | None = None
        self._last_width_px: int | None = None
        self._last_start_s: float | None = None
        self._last_end_s: float | None = None
        self._last_source_identity: _WaveformSourceIdentity | None = None
        self._last_waveform_value: WaveFormRenderData | None = None

        # Per-pad view state for the waveform editor plot (seconds).
        self._pad_view_ranges: dict[int, tuple[float, float]] = {}
        self._pad_view_sources: dict[int, _WaveformSourceIdentity] = {}

    def _selected_pad_id(self) -> int | None:
        return self._controller.session.waveform_editor_pad_id

    def open(self, pad_id: int) -> None:
        session = self._controller.session
        if session.waveform_editor_pad_id is not None and session.waveform_editor_pad_id != pad_id:
            self._controller.transport.waveform.release_view(session.waveform_editor_pad_id)
            self._last_waveform_value = None
        session.waveform_editor_open = True
        session.waveform_editor_pad_id = pad_id

    def close(self) -> None:
        session = self._controller.session
        if session.waveform_editor_pad_id is not None:
            self._controller.transport.waveform.release_view(session.waveform_editor_pad_id)
        self._last_waveform_value = None
        session.waveform_editor_open = False
        session.waveform_editor_pad_id = None

    def toggle_for_pad(self, pad_id: int) -> None:
        """Open, close, or retarget the waveform editor for a loaded pad."""
        if self._controller.project.sample_paths[pad_id] is None:
            return

        session = self._controller.session
        if session.waveform_editor_open and session.waveform_editor_pad_id == pad_id:
            self.close()
            return

        self.open(pad_id)

    def play_restart_selected_pad_on_press(self, *, received_at_ns: int | None = None) -> None:
        """Restart the selected pad, retaining its captured Rust input timestamp."""
        received_at_ns = validate_input_timestamp_ns(received_at_ns)
        pad_id = self._selected_pad_id()
        if pad_id is None:
            return

        if self._controller.project.sample_paths[pad_id] is None:
            return

        loop_start_s, _ = self._controller.transport.loop.effective_region(pad_id)

        # Update playhead immediately; audio messages will refine it.
        self._controller.session.pad_playhead_s[pad_id] = loop_start_s

        # Trigger without stopping other pads (ignores multi_loop setting).
        self._controller.transport.playback.trigger_pad_keep_others(
            pad_id, received_at_ns=received_at_ns
        )
        self._controller.session.paused_sample_ids.discard(pad_id)

    def pause_selected_pad_on_press(self) -> None:
        """Toggle pause/resume for the selected pad.

        If the pad is playing, pause it (stop mixing but keep position).
        If the pad is paused, resume it.
        """
        pad_id = self._selected_pad_id()
        if pad_id is None:
            return

        if pad_id not in self._controller.session.active_sample_ids:
            return

        if pad_id in self._controller.session.paused_sample_ids:
            self._controller.transport.playback.resume_pad(pad_id)
        else:
            self._controller.transport.playback.pause_pad(pad_id)

    def pause_selected_pad_hold_on_press(self) -> None:
        """Pause the selected pad until the matching right mouse hold is released."""
        pad_id = self._selected_pad_id()
        session = self._controller.session
        session.waveform_pause_hold_pad_id = None
        if pad_id is None:
            return

        if pad_id not in session.active_sample_ids or pad_id in session.paused_sample_ids:
            return

        self._controller.transport.playback.pause_pad(pad_id)
        if pad_id in session.paused_sample_ids:
            session.waveform_pause_hold_pad_id = pad_id

    def pause_selected_pad_hold_on_release(self) -> None:
        """Resume the pad paused by a waveform Pause right-button hold."""
        session = self._controller.session
        pad_id = session.waveform_pause_hold_pad_id
        if pad_id is None:
            return

        session.waveform_pause_hold_pad_id = None
        self._controller.transport.playback.resume_pad(pad_id)

    def stop_selected_pad_on_press(self) -> None:
        """Stop only the selected pad (waveform editor Play right mouse down)."""
        pad_id = self._selected_pad_id()
        if pad_id is None:
            return

        self._controller.transport.playback.stop_pad(pad_id)

    def stop_and_reset_selected_pad_on_press(self) -> None:
        """Stop playback and reset playhead to loop start (selected pad)."""
        pad_id = self._selected_pad_id()
        if pad_id is None:
            return

        loop_start_s, _ = self._controller.transport.loop.effective_region(pad_id)
        self._controller.transport.playback.stop_pad(pad_id)
        self._controller.session.pad_playhead_s[pad_id] = loop_start_s

    def seek_selected_pad_to_position(self, position_s: float) -> None:
        """Seek the selected pad's active or paused voice to a source position."""
        pad_id = self._selected_pad_id()
        if pad_id is None:
            return

        self._controller.transport.playback.seek_pad(pad_id, position_s)

    def set_loop_start_and_play_selected_pad(
        self, start_s: float, *, received_at_ns: int | None = None
    ) -> None:
        """Edit loop start and restart using the Rust timestamp captured before the edit."""
        received_at_ns = validate_input_timestamp_ns(received_at_ns)
        pad_id = self._selected_pad_id()
        if pad_id is None:
            return

        self._controller.transport.loop.set_start(pad_id, start_s)
        self.play_restart_selected_pad_on_press(received_at_ns=received_at_ns)

    def record_view_range(self, pad_id: int, start_s: float, end_s: float) -> None:
        """Record the plot's current visible X-range for a pad."""
        self._pad_view_ranges[int(pad_id)] = (float(start_s), float(end_s))

    def consume_source_view_reset(
        self,
        pad_id: int,
        *,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
    ) -> bool:
        """Reset the first view of a newly assigned source, preserving later navigation."""
        identity = self._waveform_source_identity(pad_id, timing=timing)
        previous = self._pad_view_sources.get(pad_id)
        if (
            previous is not None
            and previous[:2] == identity[:2]
            and (identity[3] is None or previous[3] == identity[3])
        ):
            # A transient unavailable timing lookup says nothing about source replacement.
            # Retain only observed source ownership for navigation, never timing authority.
            if identity[2] is None or previous[2] == identity[2]:
                return False
            if previous[2] is None:
                self._pad_view_sources[pad_id] = identity
                return False
        self._pad_view_sources[pad_id] = identity
        self._pad_view_ranges.pop(pad_id, None)
        return True

    def view_limits(
        self,
        pad_id: int,
        *,
        loop_only: bool = False,
        include_source_start: bool = False,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
    ) -> tuple[float, float] | None:
        """Project source bounds with virtual room for the beat preceding the loop."""
        if isinstance(timing, UnresolvedTiming):
            timing = self._controller.transport.bpm.current_timing(pad_id)
        duration_s = self._source_duration_s(pad_id, timing=timing)
        if duration_s is None or duration_s <= 0.0:
            return None
        loop_start_s = self._controller.project.pad_loop_start_s[pad_id]
        start_s = waveform_view_start(
            loop_start_s, period_seconds=timing.period_seconds if timing is not None else None
        )
        if loop_only:
            _, loop_end_s = self._controller.transport.loop.effective_region(pad_id, timing=timing)
            end_s = min(duration_s, loop_end_s) if loop_end_s is not None else duration_s
            return (start_s, end_s)
        if include_source_start or timing is None:
            start_s = min(0.0, start_s)
        return (start_s, duration_s)

    def _source_duration_s(self, pad_id: int, *, timing: CurrentPadTiming | None) -> float | None:
        if timing is not None and timing.source_duration_seconds is not None:
            return timing.source_duration_seconds
        return self._controller.project.sample_durations[pad_id]

    def _current_view_width_s(
        self, pad_id: int, *, sample_duration_s: float, timing: CurrentPadTiming | None
    ) -> float:
        if sample_duration_s <= 0.0:
            return 0.0

        limits = self.view_limits(pad_id, include_source_start=True, timing=timing)
        total_width_s = limits[1] - limits[0] if limits is not None else sample_duration_s
        current = self._pad_view_ranges.get(pad_id)
        if current is None:
            return total_width_s

        start_s, end_s = current
        width = max(0.0, float(end_s) - float(start_s))
        if width <= 0.0:
            return total_width_s

        return min(width, total_width_s)

    def view_jump_start_selected_pad_on_press(self) -> tuple[float, float] | None:
        """Jump the waveform editor view to the start (selected pad)."""
        pad_id = self._selected_pad_id()
        if pad_id is None:
            return None

        timing = self._controller.transport.bpm.current_timing(pad_id)
        dur_s = self._source_duration_s(pad_id, timing=timing)
        if dur_s is None:
            return None

        dur_s = float(dur_s)
        width_s = self._current_view_width_s(pad_id, sample_duration_s=dur_s, timing=timing)
        limits = self.view_limits(pad_id, include_source_start=True, timing=timing)
        start_s = limits[0] if limits is not None else 0.0
        end_s = min(dur_s, start_s + width_s)

        self._pad_view_ranges[pad_id] = (start_s, end_s)
        return (start_s, end_s)

    def view_jump_end_selected_pad_on_press(self) -> tuple[float, float] | None:
        """Jump the waveform editor view to the end (selected pad)."""
        pad_id = self._selected_pad_id()
        if pad_id is None:
            return None

        timing = self._controller.transport.bpm.current_timing(pad_id)
        dur_s = self._source_duration_s(pad_id, timing=timing)
        if dur_s is None:
            return None

        dur_s = float(dur_s)
        width_s = self._current_view_width_s(pad_id, sample_duration_s=dur_s, timing=timing)
        end_s = dur_s
        limits = self.view_limits(pad_id, include_source_start=True, timing=timing)
        minimum_s = limits[0] if limits is not None else 0.0
        start_s = max(minimum_s, end_s - width_s)

        self._pad_view_ranges[pad_id] = (start_s, end_s)
        return (start_s, end_s)

    def get_render_data(
        self,
        pad_id: int,
        width_px: int,
        start_s: float,
        end_s: float,
        *,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
    ) -> WaveFormRenderData | None:
        if isinstance(timing, UnresolvedTiming):
            timing = self._controller.transport.bpm.current_timing(pad_id)
        source_identity = self._waveform_source_identity(pad_id, timing=timing)
        view = (pad_id, width_px, start_s, end_s, source_identity)
        previous = (
            self._last_pad_id,
            self._last_width_px,
            self._last_start_s,
            self._last_end_s,
            self._last_source_identity,
        )
        if self._last_waveform_value is None or previous != view:
            self._last_pad_id = pad_id
            self._last_width_px = width_px
            self._last_start_s = start_s
            self._last_end_s = end_s
            self._last_source_identity = source_identity
            self._last_waveform_value = self._controller.transport.waveform.get_render_data(
                pad_id, width_px, start_s, end_s, timing=timing
            )
        return self._last_waveform_value

    def readiness(self, pad_id: int) -> tuple[str, str | None]:
        """Expose pending/error full-source reads while the editor stays navigable."""
        return self._controller.transport.waveform.readiness(pad_id)

    def retry(self, pad_id: int) -> None:
        """Retry the requested view while retaining absolute source coordinates."""
        self._controller.transport.waveform.retry(pad_id)
        self._last_waveform_value = None

    def _waveform_source_identity(
        self,
        pad_id: int,
        *,
        timing: CurrentPadTiming | UnresolvedTiming | None = UNRESOLVED_TIMING,
    ) -> _WaveformSourceIdentity:
        if not 0 <= pad_id < len(self._controller.project.sample_paths):
            return (None, None, None, None)
        if isinstance(timing, UnresolvedTiming):
            timing = self._controller.transport.bpm.current_timing(pad_id)
        source_identity = None
        if (
            timing is not None
            and timing.accepted_identity is not None
            and timing.sample_rate_hz is not None
        ):
            accepted = timing.accepted_identity
            source_identity = (
                accepted.source_id,
                accepted.source_generation,
                accepted.source_sha256,
                accepted.pcm_sha256,
                accepted.frame_count,
                timing.sample_rate_hz,
            )

        duration_s = self._controller.project.sample_durations[pad_id]
        return (
            self._controller.project.sample_paths[pad_id],
            float(duration_s) if duration_s is not None else None,
            source_identity,
            self._controller.transport.waveform.source_identity(pad_id),
        )


class UiActions:
    """UI-related actions."""

    settings: SettingsActions
    waveform: WaveformEditorActions

    def __init__(self, controller: AppController):
        self._controller = controller
        self.settings = SettingsActions(controller)
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

    def open_waveform_editor(self, pad_id: int) -> None:
        self._controller.input_mapping.perform_learnable_action(
            LooperAction.adjust_loop(pad_id),
            lambda: self.waveform.toggle_for_pad(pad_id),
        )

    def select_pad(self, pad_id: int) -> None:
        self._controller.project.selected_pad = pad_id
        self._controller.persistence.mark_dirty()

    def select_bank(self, bank_id: int) -> None:
        self._controller.input_mapping.perform_learnable_action(
            LooperAction.select_bank(bank_id),
            lambda: self._select_bank(bank_id),
        )

    def start_global_bpm_edit(self, text: str) -> None:
        self._controller.session.global_bpm_edit_active = True
        self._controller.session.global_bpm_edit_text = text
        self._controller.session.global_bpm_edit_initial_text = text
        self._controller.session.global_bpm_edit_focus_requested = True

    def set_global_bpm_edit_text(self, text: str) -> None:
        self._controller.session.global_bpm_edit_text = text

    def clear_global_bpm_edit_focus_request(self) -> None:
        self._controller.session.global_bpm_edit_focus_requested = False

    def finish_global_bpm_edit(self) -> None:
        self._controller.session.global_bpm_edit_active = False
        self._controller.session.global_bpm_edit_text = ""
        self._controller.session.global_bpm_edit_initial_text = ""
        self._controller.session.global_bpm_edit_focus_requested = False

    def store_pressed_pad_state(self, pad_id: int, *, pressed: bool) -> None:
        self._controller.session.pressed_pads[pad_id] = pressed

    def store_global_start_stop_pressed(self, *, pressed: bool) -> None:
        self._controller.session.global_start_stop_left_pressed = pressed

    def _select_bank(self, bank_id: int) -> None:
        self._controller.project.selected_bank = bank_id
        self._controller.persistence.mark_dirty()


class SettingsActions:
    """Settings overlay UI state/actions."""

    def __init__(self, controller: AppController) -> None:
        self._controller = controller

    def open(self) -> None:
        self._controller.session.settings_open = True

    def close(self) -> None:
        self._controller.session.settings_open = False

    def toggle(self) -> None:
        self._controller.session.settings_open = not self._controller.session.settings_open

    def set_demucs_quality(self, *, shifts: int, overlap: float) -> None:
        self._controller.settings.set_demucs_quality(shifts=shifts, overlap=overlap)

    def set_stem_separator(self, separator: StemSeparator) -> None:
        self._controller.settings.set_stem_separator(separator)

    def set_trigger_quantization_step(self, step: TriggerQuantizationStep) -> None:
        self._controller.transport.global_params.set_trigger_quantization_step(step)

    def set_input_mapping_enabled(self, *, enabled: bool) -> None:
        self._controller.input_mapping.set_enabled(enabled=enabled)

    def delete_all_keyboard_mappings(self) -> None:
        self._controller.input_mapping.delete_all_keyboard_mappings()

    def delete_all_midi_mappings(self) -> None:
        self._controller.input_mapping.delete_all_midi_mappings()


class InputMappingActions:
    """Input mapping UI actions."""

    def __init__(self, controller: AppController) -> None:
        self._controller = controller

    def toggle_learn(self) -> None:
        self._controller.input_mapping.toggle_learn()

    def capture_timestamp_ns(self) -> int:
        """Capture an observed UI input in the shared Rust input clock domain."""
        return self._controller.input_mapping.capture_input_timestamp_ns()

    def capture_keyboard(
        self,
        key_name: str,
        *,
        ctrl: bool = False,
        alt: bool = False,
        shift: bool = False,
        super_: bool = False,
        text_input_focused: bool = False,
        received_at_ns: int | None = None,
    ) -> bool:
        """Dispatch an observed key while retaining its captured Rust input timestamp."""
        binding = KeyboardBinding(
            key_name=key_name,
            ctrl=ctrl,
            alt=alt,
            shift=shift,
            super=super_,
        )
        return self._controller.input_mapping.capture_keyboard_input(
            binding,
            text_input_focused=text_input_focused,
            received_at_ns=received_at_ns,
        )


class UiContext:
    """The public interface for the UI layer."""

    state: UiState
    audio: AudioActions
    ui: UiActions
    input: InputMappingActions

    bold_font: imgui.ImFont

    def __init__(self, controller: AppController):
        self._controller = controller
        self.state = UiState(controller)
        self.audio = AudioActions(controller)
        self.ui = UiActions(controller)
        self.input = InputMappingActions(controller)
        self.persistence = controller.persistence

    def on_frame_render(self) -> None:
        self._controller.on_frame_render()


def _midi_cc_learn_input_pending(controller: AppController) -> bool:
    binding_key = controller.session.input_learn_pending_binding_key
    return (
        controller.session.input_learn_pending_source == "midi"
        and binding_key is not None
        and binding_key.startswith(("midi:cc:", "midi:nrpn:"))
    )
