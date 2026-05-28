from contextlib import suppress
from typing import TYPE_CHECKING, Literal, cast

from flitzis_looper.audio_gain import legacy_gain_value_to_db
from flitzis_looper.constants import (
    NUM_BANKS,
    NUM_SAMPLES,
    PAD_GAIN_DB_MAX,
    PAD_GAIN_DB_MIN,
    SPEED_MAX,
    SPEED_MIN,
)
from flitzis_looper.controller.base import BaseController
from flitzis_looper.input_mapping.actions import LooperAction, PadEqBand
from flitzis_looper.input_mapping.bindings import KeyboardBinding, MidiBinding
from flitzis_looper.input_mapping.storage import (
    KeyboardMappingEntry,
    MidiMappingEntry,
    clear_keyboard_mappings,
    clear_midi_mappings,
    load_keyboard_mapping_file,
    load_midi_mapping_file,
    save_keyboard_mapping_file,
    save_midi_mapping_file,
)
from flitzis_looper.models import (
    STEM_MASK_DISPLAY_MODES,
    STEM_MIX_MODES,
    TRIGGER_QUANTIZATION_STEPS,
    StemMaskDisplayMode,
    StemMixMode,
    validate_sample_id,
)

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller import AppController

type InputSource = Literal["midi", "keyboard"]
PAD_EQ_BANDS: tuple[PadEqBand, ...] = ("low", "mid", "high")
MIDI_RELATIVE_VOLUME_STEP = 0.01
MIDI_RELATIVE_GAIN_STEP_DB = 0.1
MIDI_RELATIVE_EQ_STEP_DB = 0.5


class InputMappingController(BaseController):
    """Manage Learn UX, local mapping files, and Rust input-runtime snapshots."""

    def __init__(
        self,
        app: AppController,
        on_project_changed: Callable[[], None] | None = None,
    ) -> None:
        super().__init__(app.project, app.session, app._audio, on_project_changed)
        self._app = app
        self._midi = load_midi_mapping_file()
        self._keyboard = load_keyboard_mapping_file()
        self._midi_cc_values: dict[str, int] = {}
        self._midi_cc_directions: dict[str, int] = {}
        self._on_frame_render_callbacks.append(self._sync_rust_runtime_state)
        self._on_frame_render_callbacks.append(self._poll_rust_input_events)

    def apply_project_state_to_input_runtime(self) -> None:
        """Apply persisted mapping state to the Rust input runtime."""
        self._sync_rust_mapping_snapshot()
        self._sync_rust_runtime_state()
        self._set_rust_enabled(enabled=self._project.input_mapping_enabled)
        self._sync_midi_ports()

    def set_enabled(self, *, enabled: bool) -> None:
        """Enable or disable performer input mapping."""
        if enabled == self._project.input_mapping_enabled:
            return

        self._project.input_mapping_enabled = enabled
        if not enabled:
            self._clear_learn_state()
        self._mark_project_changed()
        self._set_rust_enabled(enabled=enabled)
        self._sync_midi_ports()

    def toggle_learn(self) -> None:
        """Toggle Learn mode or delete the pending input mapping."""
        if not self._project.input_mapping_enabled:
            return

        if (
            self._session.input_learn_pending_source is not None
            and self._session.input_learn_pending_binding_key is not None
        ):
            self.delete_mapping(
                self._session.input_learn_pending_source,
                self._session.input_learn_pending_binding_key,
            )
            self._clear_learn_state()
            return

        self._session.input_learn_active = not self._session.input_learn_active
        self._session.input_learn_pending_source = None
        self._session.input_learn_pending_binding_key = None

    def perform_learnable_action(self, action: LooperAction, execute: Callable[[], None]) -> None:
        """Execute or learn-map a UI action depending on current Learn state."""
        if not self._project.input_mapping_enabled:
            execute()
            return

        pending_source = self._session.input_learn_pending_source
        pending_key = self._session.input_learn_pending_binding_key
        if pending_source is not None and pending_key is not None:
            self.save_mapping(pending_source, pending_key, action)
            self._clear_learn_state()
            return

        if self._session.input_learn_active:
            return

        execute()

    def save_mapping(
        self,
        source: InputSource | str,
        binding_key: str,
        action: LooperAction,
    ) -> None:
        """Save or replace one input mapping."""
        if source == "midi":
            midi_binding = MidiBinding.from_key(binding_key)
            self._midi.mappings = [
                mapping for mapping in self._midi.mappings if mapping.input.key != midi_binding.key
            ]
            self._midi.mappings.append(MidiMappingEntry(input=midi_binding, action=action))
            save_midi_mapping_file(self._midi)
            self._sync_rust_mapping_snapshot()
            return

        if source == "keyboard":
            keyboard_binding = KeyboardBinding.from_key(binding_key)
            self._keyboard.mappings = [
                mapping
                for mapping in self._keyboard.mappings
                if mapping.input.key != keyboard_binding.key
            ]
            self._keyboard.mappings.append(
                KeyboardMappingEntry(input=keyboard_binding, action=action)
            )
            save_keyboard_mapping_file(self._keyboard)
            return

        msg = f"unsupported input source: {source}"
        raise ValueError(msg)

    def delete_mapping(self, source: InputSource | str, binding_key: str) -> bool:
        """Delete one input mapping if present."""
        if source == "midi":
            before = len(self._midi.mappings)
            self._midi.mappings = [
                mapping for mapping in self._midi.mappings if mapping.input.key != binding_key
            ]
            changed = len(self._midi.mappings) != before
            if changed:
                save_midi_mapping_file(self._midi)
                self._sync_rust_mapping_snapshot()
            return changed

        if source == "keyboard":
            before = len(self._keyboard.mappings)
            self._keyboard.mappings = [
                mapping for mapping in self._keyboard.mappings if mapping.input.key != binding_key
            ]
            changed = len(self._keyboard.mappings) != before
            if changed:
                save_keyboard_mapping_file(self._keyboard)
            return changed

        msg = f"unsupported input source: {source}"
        raise ValueError(msg)

    def delete_all_keyboard_mappings(self) -> None:
        """Clear all keyboard mappings and keep keyboard defaults."""
        self._keyboard = clear_keyboard_mappings()

    def delete_all_midi_mappings(self) -> None:
        """Clear all MIDI mappings and keep MIDI defaults."""
        self._midi = clear_midi_mappings()
        self._sync_rust_mapping_snapshot()

    def capture_keyboard_input(self, binding: KeyboardBinding, *, text_input_focused: bool) -> bool:
        """Capture or dispatch one normalized keyboard input."""
        if not self._project.input_mapping_enabled:
            return False
        if self._keyboard.ignore_when_typing and text_input_focused:
            return False

        if self._session.input_learn_active:
            self._session.input_learn_pending_source = "keyboard"
            self._session.input_learn_pending_binding_key = binding.key
            return True

        action = self._keyboard_action_for(binding.key)
        if action is None:
            return False
        self.execute_action(action)
        return True

    def execute_action(self, action: LooperAction) -> bool:
        """Execute a mapped action through controller semantics."""
        key = action.key
        exact_handler = self._exact_action_handlers().get(key)
        if exact_handler is not None:
            exact_handler()
            return True

        for prefix, handler in self._prefix_action_handlers():
            if key.startswith(prefix):
                return handler(key)
        return False

    def _exact_action_handlers(self) -> dict[str, Callable[[], None]]:
        return {
            "global.multi_loop.toggle": self._toggle_multi_loop,
            "global.key_lock.toggle": self._toggle_key_lock,
            "global.bpm_lock.toggle": self._toggle_bpm_lock,
            "global.trigger_quantization.toggle": self._toggle_trigger_quantization,
            "global.stop_all": self._app.transport.playback.stop_all_pads,
            "global.speed.increase": self._increase_speed,
            "global.speed.decrease": self._decrease_speed,
            "global.speed.reset": self._app.transport.global_params.reset_speed,
        }

    def _prefix_action_handlers(self) -> tuple[tuple[str, Callable[[str], bool]], ...]:
        return (
            ("pad.trigger:", self._execute_trigger_pad),
            ("pad.stop:", self._execute_stop_pad),
            ("pad.unload:", self._execute_unload_pad),
            ("pad.analyze:", self._execute_analyze_pad),
            ("pad.adjust_loop:", self._execute_adjust_loop),
            ("pad.tap_bpm:", self._execute_tap_bpm),
            ("pad.gain_db:", self._execute_pad_gain_db),
            ("pad.gain:", self._execute_legacy_pad_gain),
            ("pad.eq:", self._execute_pad_eq),
            ("ui.select_pad:", self._execute_select_pad),
            ("ui.select_bank:", self._execute_select_bank),
            ("global.volume:", self._execute_master_volume),
            ("global.speed:", self._execute_global_speed),
            ("global.trigger_quantization:", self._execute_trigger_quantization),
            ("stem.generate:", self._execute_generate_stems),
            ("stem.delete:", self._execute_delete_stems),
            ("stem.mix:", self._execute_stem_mix),
            ("stem.mask:", self._execute_stem_mask),
        )

    def _poll_rust_input_events(self) -> None:
        while True:
            try:
                event = self._audio.poll_input_events()
            except RuntimeError as err:
                self._session.input_mapping_error = str(err)
                return

            if event is None:
                return
            self._handle_rust_input_event(event)

    def _handle_rust_input_event(self, event: dict[str, object]) -> None:
        source = event.get("source")
        binding_key = event.get("binding_key")
        if source != "midi" or not isinstance(binding_key, str):
            return
        if not self._project.input_mapping_enabled:
            return

        if self._session.input_learn_active:
            self._session.input_learn_pending_source = "midi"
            self._session.input_learn_pending_binding_key = binding_key
            self._record_midi_cc_value(binding_key, _midi_event_value(event))
            return

        if event.get("direct") is True:
            return

        action_key = event.get("action_key")
        if isinstance(action_key, str):
            if self._execute_relative_midi_action(
                action_key,
                binding_key,
                _midi_event_value(event),
            ):
                return
            self.execute_action(LooperAction.from_key(action_key))

    def _record_midi_cc_value(self, binding_key: str, value: int | None) -> None:
        if value is None or not _is_relative_midi_binding_key(binding_key):
            return
        self._midi_cc_values[binding_key] = value
        self._midi_cc_directions.pop(binding_key, None)

    def _execute_relative_midi_action(
        self,
        action_key: str,
        binding_key: str,
        value: int | None,
    ) -> bool:
        if not _is_relative_action_key(action_key):
            return False
        if value is None or not _is_relative_midi_binding_key(binding_key):
            return True

        direction = self._midi_cc_step_direction(binding_key, value)
        if direction is None:
            return True

        if action_key == "global.volume.delta":
            self._app.transport.global_params.set_volume(
                self._project.volume + MIDI_RELATIVE_VOLUME_STEP * direction
            )
        elif action_key == "global.speed.delta":
            self._app.transport.global_params.nudge_speed_by_bpm_step(direction)
        elif action_key.startswith("pad.gain.delta:"):
            self._execute_relative_pad_gain(action_key, direction)
        else:
            self._execute_relative_pad_eq(action_key, direction)
        return True

    def _midi_cc_step_direction(self, binding_key: str, value: int) -> int | None:
        previous = self._midi_cc_values.get(binding_key)
        self._midi_cc_values[binding_key] = value
        if previous is None:
            return None
        if previous == value:
            return self._midi_cc_directions.get(
                binding_key
            ) or _repeated_relative_midi_cc_direction(value)

        delta = value - previous
        if delta > 64:
            delta -= 128
        elif delta < -64:
            delta += 128

        if delta == 0:
            return None
        direction = 1 if delta > 0 else -1
        self._midi_cc_directions[binding_key] = direction
        return direction

    def _execute_relative_pad_gain(self, key: str, direction: int) -> bool:
        if (pad_id := _parse_prefixed_sample_id(key, "pad.gain.delta:")) is None:
            return True
        self._app.transport.pad.set_pad_gain(
            pad_id,
            self._project.pad_gain_db[pad_id] + MIDI_RELATIVE_GAIN_STEP_DB * direction,
        )
        return True

    def _execute_relative_pad_eq(self, key: str, direction: int) -> bool:
        parts = key.split(":")
        if len(parts) != 3 or parts[0] != "pad.eq.delta":
            return True
        try:
            pad_id = int(parts[1])
            validate_sample_id(pad_id)
        except ValueError:
            return True
        band = parts[2]
        if band not in PAD_EQ_BANDS:
            return True

        if band == "low":
            current = float(self._project.pad_eq_low_db[pad_id])
        elif band == "mid":
            current = float(self._project.pad_eq_mid_db[pad_id])
        else:
            current = float(self._project.pad_eq_high_db[pad_id])

        self._set_pad_eq_band(
            pad_id,
            cast("PadEqBand", band),
            current + MIDI_RELATIVE_EQ_STEP_DB * direction,
        )
        return True

    def _keyboard_action_for(self, binding_key: str) -> LooperAction | None:
        for mapping in self._keyboard.mappings:
            if mapping.input.key == binding_key:
                return mapping.action
        return None

    def _sync_rust_mapping_snapshot(self) -> None:
        mappings = [(mapping.input.key, mapping.action.key) for mapping in self._midi.mappings]
        with suppress(RuntimeError, ValueError, TypeError):
            self._audio.set_input_mapping_snapshot(mappings)

    def _sync_rust_runtime_state(self) -> None:
        loaded = [path is not None for path in self._project.sample_paths]
        loop_starts: list[float] = []
        loop_ends: list[float | None] = []
        for sample_id in range(NUM_SAMPLES):
            if self._project.sample_paths[sample_id] is None:
                loop_starts.append(0.0)
                loop_ends.append(None)
                continue
            start_s, end_s = self._app.transport.loop.effective_region(sample_id)
            loop_starts.append(float(start_s))
            loop_ends.append(float(end_s) if end_s is not None else None)

        with suppress(RuntimeError, ValueError, TypeError):
            self._audio.set_input_runtime_state(
                self._project.multi_loop,
                loaded,
                loop_starts,
                loop_ends,
            )

    def _set_rust_enabled(self, *, enabled: bool) -> None:
        with suppress(RuntimeError, TypeError):
            self._audio.set_input_mapping_enabled(enabled)

    def _toggle_multi_loop(self) -> None:
        self._app.transport.global_params.set_multi_loop(enabled=not self._app.project.multi_loop)

    def _toggle_key_lock(self) -> None:
        self._app.transport.global_params.set_key_lock(enabled=not self._app.project.key_lock)

    def _toggle_bpm_lock(self) -> None:
        self._app.transport.global_params.set_bpm_lock(enabled=not self._app.project.bpm_lock)

    def _toggle_trigger_quantization(self) -> None:
        self._app.transport.global_params.toggle_trigger_quantization()

    def _increase_speed(self) -> None:
        self._app.transport.global_params.nudge_speed_by_bpm_step(1)

    def _decrease_speed(self) -> None:
        self._app.transport.global_params.nudge_speed_by_bpm_step(-1)

    def _execute_trigger_pad(self, key: str) -> bool:
        if (pad_id := _parse_prefixed_sample_id(key, "pad.trigger:")) is None:
            return False
        self._app.transport.playback.trigger_pad(pad_id)
        return True

    def _execute_stop_pad(self, key: str) -> bool:
        if (pad_id := _parse_prefixed_sample_id(key, "pad.stop:")) is None:
            return False
        self._app.transport.playback.stop_pad(pad_id)
        return True

    def _execute_unload_pad(self, key: str) -> bool:
        if (pad_id := _parse_prefixed_sample_id(key, "pad.unload:")) is None:
            return False
        self._app.loader.unload_sample(pad_id)
        return True

    def _execute_analyze_pad(self, key: str) -> bool:
        if (pad_id := _parse_prefixed_sample_id(key, "pad.analyze:")) is None:
            return False
        self._app.loader.analyze_sample_async(pad_id)
        return True

    def _execute_adjust_loop(self, key: str) -> bool:
        if (pad_id := _parse_prefixed_sample_id(key, "pad.adjust_loop:")) is None:
            return False
        if self._project.sample_paths[pad_id] is None:
            return True

        session = self._app.session
        if session.waveform_editor_open and session.waveform_editor_pad_id == pad_id:
            session.waveform_editor_open = False
            session.waveform_editor_pad_id = None
            return True

        session.waveform_editor_open = True
        session.waveform_editor_pad_id = pad_id
        return True

    def _execute_tap_bpm(self, key: str) -> bool:
        if (pad_id := _parse_prefixed_sample_id(key, "pad.tap_bpm:")) is None:
            return False
        self._app.transport.bpm.tap_bpm(pad_id)
        return True

    def _execute_pad_gain_db(self, key: str) -> bool:
        parts = key.split(":")
        if len(parts) != 3 or parts[0] != "pad.gain_db":
            return False
        try:
            pad_id = int(parts[1])
            tenths_db = int(parts[2])
            validate_sample_id(pad_id)
        except ValueError:
            return False
        min_tenths = round(PAD_GAIN_DB_MIN * 10)
        max_tenths = round(PAD_GAIN_DB_MAX * 10)
        if not min_tenths <= tenths_db <= max_tenths:
            return False
        self._app.transport.pad.set_pad_gain(pad_id, tenths_db / 10.0)
        return True

    def _execute_legacy_pad_gain(self, key: str) -> bool:
        parts = key.split(":")
        if len(parts) != 3 or parts[0] != "pad.gain":
            return False
        try:
            pad_id = int(parts[1])
            percent = int(parts[2])
            validate_sample_id(pad_id)
        except ValueError:
            return False
        if not 0 <= percent <= 100:
            return False
        self._app.transport.pad.set_pad_gain(pad_id, legacy_gain_value_to_db(percent))
        return True

    def _execute_pad_eq(self, key: str) -> bool:
        parts = key.split(":")
        if len(parts) != 4 or parts[0] != "pad.eq":
            return False
        try:
            pad_id = int(parts[1])
            db = float(parts[3])
            validate_sample_id(pad_id)
        except ValueError:
            return False
        band = parts[2]
        if band not in PAD_EQ_BANDS:
            return False
        self._set_pad_eq_band(pad_id, cast("PadEqBand", band), db)
        return True

    def _set_pad_eq_band(self, pad_id: int, band: PadEqBand, db: float) -> None:
        low_db = float(self._project.pad_eq_low_db[pad_id])
        mid_db = float(self._project.pad_eq_mid_db[pad_id])
        high_db = float(self._project.pad_eq_high_db[pad_id])

        if band == "low":
            low_db = db
        elif band == "mid":
            mid_db = db
        else:
            high_db = db

        self._app.transport.pad.set_pad_eq(pad_id, low_db, mid_db, high_db)

    def _execute_select_pad(self, key: str) -> bool:
        if (pad_id := _parse_prefixed_sample_id(key, "ui.select_pad:")) is None:
            return False
        self._app.project.selected_pad = pad_id
        self._app.persistence.mark_dirty()
        return True

    def _execute_select_bank(self, key: str) -> bool:
        if (bank_id := _parse_prefixed_bank_id(key, "ui.select_bank:")) is None:
            return False
        self._app.project.selected_bank = bank_id
        self._app.persistence.mark_dirty()
        return True

    def _execute_generate_stems(self, key: str) -> bool:
        if (pad_id := _parse_prefixed_sample_id(key, "stem.generate:")) is None:
            return False
        self._app.stems.generate_stems_async(pad_id)
        return True

    def _execute_delete_stems(self, key: str) -> bool:
        if (pad_id := _parse_prefixed_sample_id(key, "stem.delete:")) is None:
            return False
        self._app.stems.delete_stems(pad_id)
        return True

    def _sync_midi_ports(self) -> None:
        try:
            if self._project.input_mapping_enabled:
                self._audio.start_midi_input()
            else:
                self._audio.stop_midi_input()
        except (RuntimeError, TypeError) as err:
            self._session.input_mapping_error = str(err)

    def _execute_master_volume(self, key: str) -> bool:
        raw = key.removeprefix("global.volume:")
        if raw == key:
            return False
        try:
            percent = int(raw)
        except ValueError:
            return False
        if not 0 <= percent <= 100:
            return False
        self._app.transport.global_params.set_volume(percent / 100.0)
        return True

    def _execute_global_speed(self, key: str) -> bool:
        raw = key.removeprefix("global.speed:")
        if raw == key:
            return False
        try:
            percent = int(raw)
        except ValueError:
            return False
        min_percent = round(SPEED_MIN * 100)
        max_percent = round(SPEED_MAX * 100)
        if not min_percent <= percent <= max_percent:
            return False
        self._app.transport.global_params.set_speed(percent / 100.0)
        return True

    def _execute_trigger_quantization(self, key: str) -> bool:
        mode = key.removeprefix("global.trigger_quantization:")
        valid_modes = {"immediate", "next_beat", "next_bar", *TRIGGER_QUANTIZATION_STEPS}
        if mode not in valid_modes:
            return False
        self._app.transport.global_params.set_trigger_quantization(mode)
        return True

    def _execute_stem_mix(self, key: str) -> bool:
        parts = key.split(":")
        if len(parts) != 3 or parts[0] != "stem.mix":
            return False
        try:
            pad_id = int(parts[1])
        except ValueError:
            return False
        mode = parts[2]
        if mode not in STEM_MIX_MODES:
            return False
        self._app.stems.set_stem_mix_mode(pad_id, cast("StemMixMode", mode))
        return True

    def _execute_stem_mask(self, key: str) -> bool:
        parts = key.split(":")
        if len(parts) != 4 or parts[0] != "stem.mask":
            return False
        try:
            pad_id = int(parts[1])
            mask = int(parts[2])
        except ValueError:
            return False
        display_mode = parts[3]
        if display_mode not in STEM_MASK_DISPLAY_MODES:
            return False
        self._app.stems.set_stem_enabled_mask(
            pad_id,
            mask,
            cast("StemMaskDisplayMode", display_mode),
        )
        return True

    def _clear_learn_state(self) -> None:
        self._session.input_learn_active = False
        self._session.input_learn_pending_source = None
        self._session.input_learn_pending_binding_key = None


def _parse_prefixed_sample_id(key: str, prefix: str) -> int | None:
    raw = key.removeprefix(prefix)
    if raw == key:
        return None
    try:
        value = int(raw)
    except ValueError:
        return None
    try:
        validate_sample_id(value)
    except ValueError:
        return None
    return value


def _parse_prefixed_bank_id(key: str, prefix: str) -> int | None:
    raw = key.removeprefix(prefix)
    if raw == key:
        return None
    try:
        value = int(raw)
    except ValueError:
        return None
    if not 0 <= value < NUM_BANKS:
        return None
    return value


def _midi_event_value(event: dict[str, object]) -> int | None:
    value = event.get("value")
    if not isinstance(value, bool) and isinstance(value, int) and 0 <= value <= 127:
        return value
    return None


def _repeated_relative_midi_cc_direction(value: int) -> int | None:
    if value in {1, 65}:
        return 1
    if value in {63, 127}:
        return -1
    return None


def _is_relative_action_key(action_key: str) -> bool:
    return action_key in {"global.volume.delta", "global.speed.delta"} or action_key.startswith((
        "pad.eq.delta:",
        "pad.gain.delta:",
    ))


def _is_relative_midi_binding_key(binding_key: str) -> bool:
    return binding_key.startswith(("midi:cc:", "midi:nrpn:"))
