from contextlib import suppress
from dataclasses import dataclass, field
from typing import TYPE_CHECKING, Literal, TypedDict, cast

from flitzis_looper.audio_gain import legacy_gain_value_to_db
from flitzis_looper.constants import (
    NUM_BANKS,
    PAD_GAIN_DB_MAX,
    PAD_GAIN_DB_MIN,
    SPEED_MAX,
    SPEED_MIN,
)
from flitzis_looper.controller.base import BaseController
from flitzis_looper.controller.current_binding import capture_current_pad_binding
from flitzis_looper.input_mapping.actions import (
    LooperAction,
    PadEqBand,
    describe_pitch_rejection,
    parse_pitch_action,
)
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
from flitzis_looper.input_timing import validate_input_timestamp_ns
from flitzis_looper.key_intent import recognize_key
from flitzis_looper.models import (
    STEM_MASK_DISPLAY_MODES,
    STEM_MIX_MODES,
    TRIGGER_QUANTIZATION_STEPS,
    StemMaskDisplayMode,
    StemMixMode,
    validate_sample_id,
)
from flitzis_looper.timing_policy import variable_source_timing

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.current_timing import CurrentPadTiming
    from flitzis_looper.key_intent import MusicalKey
    from flitzis_looper_audio import (
        CapturedInputPadTrigger,
        CapturedPitchAction,
        InputRuntimePadBinding,
        LoaderEventDelivery,
    )

type InputSource = Literal["midi", "keyboard"]


class _TimingPolicyOptions(TypedDict, total=False):
    timing_policy_json: str


@dataclass(frozen=True)
class _InputPitchProjection:
    source: MusicalKey | None = None
    correction: MusicalKey | None = None
    correction_unknown: bool = False
    base_shift: int = 0
    extra_shift: int = 0
    key_lock: bool = False
    quantize_step_64ths: int | None = None
    sync: bool = False
    request_sequence: int = 0

    def payload(self) -> dict[str, object]:
        return {
            "source_root": self.source.root if self.source is not None else None,
            "source_mode": self.source.mode if self.source is not None else None,
            "correction_root": self.correction.root if self.correction is not None else None,
            "correction_mode": self.correction.mode if self.correction is not None else None,
            "correction_unknown": self.correction_unknown,
            "base_shift": self.base_shift,
            "extra_shift": self.extra_shift,
            "key_lock": self.key_lock,
            "quantize_step_64ths": self.quantize_step_64ths,
            "sync": self.sync,
            "request_sequence": self.request_sequence,
        }


@dataclass(frozen=True)
class _InputSourceMapProjection:
    requested: bool = False
    revision: int = 0
    delivery: LoaderEventDelivery | None = None
    exact_quarters: float | None = None

    def payload(self) -> dict[str, object]:
        return {
            "sync_requested": self.requested,
            "intent_revision": self.revision,
            "delivery": self.delivery,
            "exact_quarters": self.exact_quarters,
        }


@dataclass(frozen=True)
class _InputRuntimePadSignature:
    sample_path: str | None = None
    duration_seconds: float | None = None
    start_seconds: float = 0.0
    end_seconds: float | None = None
    timing: CurrentPadTiming | None = None
    binding_timing: CurrentPadTiming | None = None
    source_signature: tuple[object, ...] | None = None
    ready: bool = False
    pitch: _InputPitchProjection = field(default_factory=_InputPitchProjection)
    source_map: _InputSourceMapProjection = field(default_factory=_InputSourceMapProjection)
    binding: InputRuntimePadBinding | None = field(default=None, compare=False)


type _InputRuntimeStateSignature = tuple[
    bool, int | None, tuple[_InputRuntimePadSignature, ...], str | None
]
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
        self._last_input_runtime_state_signature: _InputRuntimeStateSignature | None = None
        self._last_input_runtime_state_error: str | None = None
        self._input_action_error: tuple[str, str] | None = None
        app.transport.pitch.bind_runtime_refresh(self.refresh_pitch_runtime_state)
        app.transport.bind_input_runtime_refresh(self._refresh_rust_runtime_state)
        self._on_frame_render_callbacks.append(self._sync_rust_runtime_state)
        self._on_frame_render_callbacks.append(self._poll_rust_input_events)

    def apply_project_state_to_input_runtime(self) -> None:
        """Apply persisted mapping state to the Rust input runtime."""
        self._sync_rust_mapping_snapshot()
        self._sync_rust_runtime_state()
        self._sync_rust_learn_state()
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

        next_active = not self._session.input_learn_active
        if next_active:
            self._drain_rust_input_events()
        self._session.input_learn_active = next_active
        self._session.input_learn_pending_source = None
        self._session.input_learn_pending_binding_key = None
        self._sync_rust_learn_state()

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

    def capture_input_timestamp_ns(self) -> int:
        """Capture UI input time using the same Rust epoch as native MIDI."""
        return self._audio.capture_input_timestamp_ns()

    def capture_keyboard_input(
        self,
        binding: KeyboardBinding,
        *,
        text_input_focused: bool,
        received_at_ns: int | None = None,
    ) -> bool:
        """Capture or dispatch one normalized keyboard input.

        Args:
            binding: Normalized key and modifier combination.
            text_input_focused: Whether keyboard input belongs to a text editor.
            received_at_ns: Original observed input time from the Rust engine epoch.

        Returns:
            Whether Learn or an existing mapping accepted the input.
        """
        if not self._project.input_mapping_enabled:
            return False
        if self._keyboard.ignore_when_typing and text_input_focused:
            return False

        if self._session.input_learn_active:
            if (
                self._session.input_learn_pending_source is None
                or self._session.input_learn_pending_binding_key is None
            ):
                self._session.input_learn_pending_source = "keyboard"
                self._session.input_learn_pending_binding_key = binding.key
            return True

        action = self._keyboard_action_for(binding.key)
        if action is None:
            return False
        received_at_ns = validate_input_timestamp_ns(received_at_ns)
        self._execute_input_action_with_feedback(
            action.key, lambda: self.execute_action(action, received_at_ns=received_at_ns)
        )
        return True

    def _execute_input_action_with_feedback(
        self, action_key: str, execute: Callable[[], bool]
    ) -> bool:
        """Consume an expected refusal at the recognized performer-input boundary."""
        diagnostics = getattr(self._app, "diagnostics", None)
        if diagnostics is not None:
            diagnostics.begin_attempt("input_mapping")
        if (
            self._input_action_error is not None
            and self._input_action_error[1] != self._session.input_mapping_error
        ):
            self._input_action_error = None
        try:
            accepted = execute()
        except (RuntimeError, ValueError, TypeError) as error:
            self._record_input_action_error(action_key, error)
            return True
        if (
            accepted
            and self._input_action_error is not None
            and self._input_action_error == (action_key, self._session.input_mapping_error)
        ):
            self._session.input_mapping_error = None
            self._input_action_error = None
        return accepted

    def _record_input_action_error(self, action_key: str, error: Exception) -> None:
        message = str(error)
        self._session.input_mapping_error = message
        self._input_action_error = (action_key, message)
        diagnostics = getattr(self._app, "diagnostics", None)
        if diagnostics is not None:
            diagnostics.begin_attempt("input_mapping")
            diagnostics.observe(action_key, message, channel="input_mapping")

    def execute_action(self, action: LooperAction, *, received_at_ns: int | None = None) -> bool:
        """Execute a mapped action through controller semantics.

        Args:
            action: Stable performer action.
            received_at_ns: Original Rust input timestamp to retain for launch actions.

        Returns:
            Whether the action has an accepted controller handler.
        """
        received_at_ns = validate_input_timestamp_ns(received_at_ns)
        key = action.key
        if key.startswith("pitch."):
            return self._execute_pitch_action(key, received_at_ns=received_at_ns)
        exact_handler = self._exact_action_handlers(received_at_ns=received_at_ns).get(key)
        if exact_handler is not None:
            exact_handler()
            return True

        for prefix, handler in self._prefix_action_handlers(received_at_ns=received_at_ns):
            if key.startswith(prefix):
                return handler(key)
        return False

    def _exact_action_handlers(
        self, *, received_at_ns: int | None = None
    ) -> dict[str, Callable[[], None]]:
        return {
            "global.multi_loop.toggle": self._toggle_multi_loop,
            "global.key_lock.toggle": self._toggle_key_lock,
            "global.source_sync.toggle": lambda: self._app.transport.timing_policy.toggle("sync"),
            "global.realtime_bpm.toggle": lambda: self._app.transport.timing_policy.toggle(
                "realtime_bpm"
            ),
            "global.bpm_lock.toggle": self._toggle_bpm_lock,
            "global.trigger_quantization.toggle": self._toggle_trigger_quantization,
            "global.stop_all": lambda: self._app.transport.playback.stop_all_pads(
                received_at_ns=received_at_ns
            ),
            "global.start_stop": lambda: (
                self._app.transport.playback.start_or_restart_global_start_stop(
                    received_at_ns=received_at_ns
                )
            ),
            "global.speed.increase": self._increase_speed,
            "global.speed.decrease": self._decrease_speed,
            "global.speed.reset": self._app.transport.global_params.reset_speed,
            "pad.tap_bpm.selected": self._execute_selected_tap_bpm,
        }

    def _prefix_action_handlers(
        self, *, received_at_ns: int | None = None
    ) -> tuple[tuple[str, Callable[[str], bool]], ...]:
        return (
            (
                "pad.trigger:",
                lambda key: self._execute_trigger_pad(key, received_at_ns=received_at_ns),
            ),
            ("pad.stop:", self._execute_stop_pad),
            ("pad.unload:", self._execute_unload_pad),
            ("pad.analyze:", self._execute_analyze_pad),
            ("pad.adjust_loop:", self._execute_adjust_loop),
            ("pad.source_sync.toggle:", self._execute_source_sync_toggle),
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

    def _drain_rust_input_events(self) -> None:
        while True:
            try:
                event = self._audio.poll_input_events()
            except RuntimeError as err:
                self._session.input_mapping_error = str(err)
                return

            if event is None:
                return
            self._discard_captured_pitch(event)

    def _discard_captured_pitch(self, event: dict[str, object]) -> None:
        """Cancel this undelivered owner through Native's guarded shadow rollback."""
        captured = event.get("captured_pitch")
        if captured is None:
            return
        try:
            self._audio.cancel_pitch_action(cast("CapturedPitchAction", captured))
        except (RuntimeError, ValueError, TypeError) as err:
            self._session.input_mapping_error = f"Pitch choice cancellation failed: {err}"

    def _handle_rust_input_event(self, event: dict[str, object]) -> None:
        source = event.get("source")
        binding_key = event.get("binding_key")
        if source != "midi" or not isinstance(binding_key, str):
            return
        if not self._project.input_mapping_enabled:
            self._discard_captured_pitch(event)
            return
        if _midi_note_release(event, binding_key):
            return

        if self._session.input_learn_active:
            self._discard_captured_pitch(event)
            self._capture_midi_learn_input(event, binding_key)
            return

        action_key = event.get("action_key")
        if isinstance(action_key, str):
            self._execute_midi_action(event, binding_key, action_key)

    def _capture_midi_learn_input(self, event: dict[str, object], binding_key: str) -> None:
        if (
            self._session.input_learn_pending_source is not None
            and self._session.input_learn_pending_binding_key is not None
        ):
            return
        self._session.input_learn_pending_source = "midi"
        self._session.input_learn_pending_binding_key = binding_key
        self._record_midi_cc_value(binding_key, _midi_event_value(event))

    def _execute_midi_action(
        self, event: dict[str, object], binding_key: str, action_key: str
    ) -> None:
        if action_key.startswith("pitch."):
            self._accept_captured_pitch_event(event)
            return
        if self._consume_direct_stop_receipt(event, action_key) or (
            event.get("direct") is True and event.get("dispatched") is True
        ):
            if action_key.startswith("pad.trigger:"):
                self._observe_dispatched_midi_trigger(event)
            return
        if action_key.startswith("ui.select_pad:"):
            self._project_midi_selection(event, action_key)
            return
        if self._execute_bound_midi_playback(
            action_key, event.get("received_at_ns"), event.get("captured_trigger")
        ):
            return
        if _is_relative_action_key(action_key):
            self._execute_input_action_with_feedback(
                action_key,
                lambda: self._execute_relative_midi_action(
                    action_key, binding_key, _midi_event_value(event)
                ),
            )
            return
        try:
            received_at_ns = validate_input_timestamp_ns(event.get("received_at_ns"))
        except ValueError as err:
            self._record_input_action_error(action_key, err)
            return
        self._execute_input_action_with_feedback(
            action_key,
            lambda: self.execute_action(
                LooperAction.from_key(action_key), received_at_ns=received_at_ns
            ),
        )

    def _consume_direct_stop_receipt(self, event: dict[str, object], action_key: str) -> bool:
        if event.get("direct") is not True or not action_key.startswith("pad.stop:"):
            return False
        if event.get("dispatched") is True:
            self._execute_input_action_with_feedback(action_key, lambda: True)
        else:
            self._record_input_action_error(
                action_key, RuntimeError("MIDI stop was not accepted by Native")
            )
        return True

    def _observe_dispatched_midi_trigger(self, event: dict[str, object]) -> None:
        captured = event.get("captured_trigger")
        if captured is None:
            return
        trigger = cast("CapturedInputPadTrigger", captured)
        try:
            self._app.transport.residency.observe_captured_start(trigger.captured_start)
        except (RuntimeError, ValueError, TypeError) as err:
            self._session.input_mapping_error = str(err)

    def _project_midi_selection(self, event: dict[str, object], action_key: str) -> None:
        revision = event.get("selection_revision")
        expected_revision = (
            revision if not isinstance(revision, bool) and isinstance(revision, int) else None
        )
        self._execute_select_pad(
            action_key,
            native_selection_already_applied=True,
            expected_native_selection_revision=expected_revision,
        )

    def _accept_captured_pitch_event(self, event: dict[str, object]) -> None:
        diagnostics = getattr(self._app, "diagnostics", None)
        if diagnostics is not None:
            diagnostics.begin_attempt("input_mapping")
        rejection = event.get("pitch_rejection")
        if isinstance(rejection, str) and rejection:
            self._report_pitch_input_error(
                f"Pitch choice not accepted: {describe_pitch_rejection(rejection)}"
            )
            return
        captured = event.get("captured_pitch")
        if captured is None:
            self._report_pitch_input_error(
                "Pitch choice not accepted: original selected content capture is unavailable"
            )
            return
        self._clear_pitch_mapping_error()
        try:
            self._app.transport.pitch.accept_captured_event(cast("CapturedPitchAction", captured))
        except (RuntimeError, ValueError, TypeError) as err:
            self._report_pitch_input_error(f"Pitch choice not accepted: {err}")

    def _report_pitch_input_error(self, message: str) -> None:
        """Retain each original event's refusal before a later event can clear the snapshot."""
        self._session.input_mapping_error = message
        diagnostics = getattr(self._app, "diagnostics", None)
        if diagnostics is not None:
            diagnostics.observe("pitch_input", message, channel="input_mapping")

    def _execute_pitch_action(self, key: str, *, received_at_ns: int | None = None) -> bool:
        action = parse_pitch_action(key)
        if action is None:
            self._report_pitch_input_error(
                f"Pitch choice not accepted: {describe_pitch_rejection('invalid_action')}"
            )
            return False
        self._clear_pitch_mapping_error()
        try:
            self._app.transport.pitch.accept_selected_action(key, received_at_ns=received_at_ns)
        except (RuntimeError, ValueError, TypeError) as err:
            self._report_pitch_input_error(f"Pitch choice not accepted: {err}")
            return False
        return True

    def _clear_pitch_mapping_error(self) -> None:
        error = self._session.input_mapping_error
        if error is not None and error.startswith("Pitch choice"):
            self._session.input_mapping_error = None
            diagnostics = getattr(self._app, "diagnostics", None)
            if diagnostics is not None:
                diagnostics.observe("input_mapping", None)

    def _execute_bound_midi_playback(
        self, action_key: str, timestamp: object, captured_trigger: object = None
    ) -> bool:
        if action_key.startswith("pad.trigger:"):
            self._retry_guarded_midi_trigger(action_key, timestamp, captured_trigger)
            return True
        if action_key not in {"global.start_stop", "global.stop_all"}:
            return False
        try:
            received_at_ns = validate_input_timestamp_ns(timestamp)
        except ValueError as err:
            self._record_input_action_error(action_key, err)
            return True
        self._execute_input_action_with_feedback(
            action_key,
            lambda: self.execute_action(
                LooperAction.from_key(action_key), received_at_ns=received_at_ns
            ),
        )
        return True

    def _retry_guarded_midi_trigger(
        self, _action_key: str, _timestamp: object, captured_trigger: object = None
    ) -> None:
        if captured_trigger is None:
            self._session.input_mapping_error = "MIDI trigger has no original Native gesture owner"
            return
        trigger = cast("CapturedInputPadTrigger", captured_trigger)
        try:
            if self._app.transport.residency.owns_captured_trigger(trigger):
                return
            sample_id = trigger.diagnostic_slot
            if sample_id is None:
                self._app.transport.residency.prepare_captured_trigger(trigger)
                return
            self._app.transport.ensure_current_source_assignment_ready(sample_id)
            try:
                dispatched = self._audio.trigger_captured_input_pad(trigger)
            except RuntimeError:
                self._app.transport.residency.prepare_captured_trigger(trigger)
                raise
            if not dispatched:
                self._app.transport.residency.prepare_captured_trigger(trigger)
        except (RuntimeError, ValueError, TypeError) as err:
            self._session.input_mapping_error = str(err)

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
        """Return whether this recognized relative event executes a controller edit."""
        if (
            not _is_relative_action_key(action_key)
            or value is None
            or not _is_relative_midi_binding_key(binding_key)
        ):
            return False

        direction = self._midi_cc_step_direction(binding_key, value)
        if direction is None:
            return False

        if action_key == "global.volume.delta":
            self._app.transport.global_params.set_volume(
                self._project.volume + MIDI_RELATIVE_VOLUME_STEP * direction
            )
        elif action_key == "global.speed.delta":
            self._app.transport.global_params.nudge_speed_by_bpm_step(direction)
        elif action_key.startswith("pad.gain.delta:"):
            return self._execute_relative_pad_gain(action_key, direction)
        elif action_key.startswith("pad.eq.selected.delta:"):
            return self._execute_relative_selected_pad_eq(action_key, direction)
        else:
            return self._execute_relative_pad_eq(action_key, direction)
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
            return False
        self._app.transport.pad.set_pad_gain(
            pad_id,
            self._project.pad_gain_db[pad_id] + MIDI_RELATIVE_GAIN_STEP_DB * direction,
        )
        return True

    def _execute_relative_pad_eq(self, key: str, direction: int) -> bool:
        parts = key.split(":")
        if len(parts) != 3 or parts[0] != "pad.eq.delta":
            return False
        try:
            pad_id = int(parts[1])
            validate_sample_id(pad_id)
        except ValueError:
            return False
        band = parts[2]
        if band not in PAD_EQ_BANDS:
            return False

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

    def _execute_relative_selected_pad_eq(self, key: str, direction: int) -> bool:
        raw_band = key.removeprefix("pad.eq.selected.delta:")
        if raw_band == key or raw_band not in PAD_EQ_BANDS:
            return False

        pad_id = self._project.selected_pad
        band = cast("PadEqBand", raw_band)
        if band == "low":
            current = float(self._project.pad_eq_low_db[pad_id])
        elif band == "mid":
            current = float(self._project.pad_eq_mid_db[pad_id])
        else:
            current = float(self._project.pad_eq_high_db[pad_id])

        self._set_pad_eq_band(
            pad_id,
            band,
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
        self._refresh_rust_runtime_state()

    def refresh_pitch_runtime_state(self) -> bool:
        """Synchronize same-frame source and metadata edits before explicit GUI capture."""
        if not self._refresh_rust_runtime_state():
            message = "Audio control state is not ready for this pitch choice."
            if self._last_input_runtime_state_error:
                message += f" {self._last_input_runtime_state_error}"
            raise RuntimeError(message)
        return True

    def _refresh_rust_runtime_state(self) -> bool:
        try:
            signature = self._input_runtime_state_signature()
        except (RuntimeError, ValueError, TypeError) as error:
            self._last_input_runtime_state_error = str(error)
            return False
        if signature == self._last_input_runtime_state_signature:
            return self._request_runtime_policy_adoption()

        multi_loop, _sample_rate_hz, pad_signatures, policy_json = signature
        policy_options: _TimingPolicyOptions = (
            {"timing_policy_json": policy_json} if policy_json is not None else {}
        )
        loaded = [pad.ready for pad in pad_signatures]
        loop_starts = [pad.start_seconds for pad in pad_signatures]
        loop_ends = [pad.end_seconds for pad in pad_signatures]
        bindings = [pad.binding for pad in pad_signatures]

        try:
            if callable(getattr(self._audio, "capture_selected_pitch_action", None)):
                self._audio.set_input_runtime_state(
                    multi_loop,
                    loaded,
                    loop_starts,
                    loop_ends,
                    bindings,
                    pitch_rows=[pad.pitch.payload() for pad in pad_signatures],
                    source_map_rows=[pad.source_map.payload() for pad in pad_signatures],
                    **policy_options,
                )
            else:
                self._audio.set_input_runtime_state(
                    multi_loop,
                    loaded,
                    loop_starts,
                    loop_ends,
                    bindings,
                    **policy_options,
                )
        except (RuntimeError, ValueError, TypeError) as error:
            self._last_input_runtime_state_error = str(error)
            return False

        self._last_input_runtime_state_signature = signature
        return self._request_runtime_policy_adoption()

    def _request_runtime_policy_adoption(self) -> bool:
        """Retry the current Native-owned request without replacing published rows."""
        try:
            request = getattr(self._audio, "request_input_runtime_policy_adoption", None)
            if callable(request):
                # Native deduplicates before owning work. Ordinary pending/refusal
                # returns normally; only callback feedback can confirm adoption.
                request()
        except (RuntimeError, ValueError, TypeError) as error:
            self._last_input_runtime_state_error = str(error)
            return False
        self._last_input_runtime_state_error = None
        return True

    def _input_runtime_state_signature(self) -> _InputRuntimeStateSignature:
        policy_json = self._timing_policy_projection()
        pad_signatures: list[_InputRuntimePadSignature] = []
        deferred = self._app.transport._pair_deferred_sample_ids
        previous = self._last_input_runtime_state_signature
        for sample_id, sample_path in enumerate(self._project.sample_paths):
            if sample_id in deferred:
                if previous is None:
                    message = "Input runtime is waiting for reserved pad observers."
                    raise RuntimeError(message)
                # The arrangement owns these observers until its real ACK/mirror.
                # Keep the last successfully published row, including an empty row;
                # a fenced readiness lookup does not report removal of that content.
                pad_signatures.append(previous[2][sample_id])
                continue
            if sample_path is None:
                pad_signatures.append(_InputRuntimePadSignature())
                continue
            try:
                self._app.transport.ensure_current_source_assignment_ready(sample_id)
            except ValueError:
                pad_signatures.append(_InputRuntimePadSignature())
                continue

            timing = self._app.transport.bpm.current_timing(sample_id)
            snapshot = capture_current_pad_binding(self._audio, sample_id, timing=timing)
            sync_intent = self._project.pad_source_sync_intent[sample_id]
            requested_sync = self._project.resolve_pad_source_sync(sample_id)
            try:
                region = (
                    self._app.transport.loop.requested_region
                    if requested_sync
                    or sync_intent.revision > 0
                    or not self._project.global_timing_policy.is_neutral
                    else self._app.transport.loop.effective_region
                )
                start_s, end_s = region(sample_id, timing=timing)
            except RuntimeError, ValueError:
                if not requested_sync:
                    raise
                # An unavailable mapped extent must not block unrelated PAD rows.
                # The independent request below withholds delivery and refuses launch.
                start_s = self._project.pad_loop_start_s[sample_id]
                end_s = self._project.pad_loop_end_s[sample_id]
            pad_signatures.append(
                _InputRuntimePadSignature(
                    sample_path=sample_path,
                    duration_seconds=self._project.sample_durations[sample_id],
                    start_seconds=float(start_s),
                    end_seconds=float(end_s) if end_s is not None else None,
                    timing=timing,
                    binding_timing=snapshot.accepted_timing if snapshot else None,
                    source_signature=snapshot.source_signature if snapshot else None,
                    ready=snapshot is not None,
                    pitch=self._input_pitch_projection(sample_id),
                    source_map=self._input_source_map_projection(sample_id),
                    binding=snapshot.binding if snapshot else None,
                )
            )
        return (
            self._project.multi_loop,
            self._app.transport._output_sample_rate_hz(),
            tuple(pad_signatures),
            policy_json,
        )

    def _timing_policy_projection(self) -> str | None:
        """Fence an entire changed global request while any cohort row is reserved."""
        policy = self._project.global_timing_policy
        encoded = None if policy.is_neutral else policy.model_dump_json()
        previous = self._last_input_runtime_state_signature
        if self._app.transport._pair_deferred_sample_ids and (
            previous is None or previous[3] != encoded
        ):
            message = "Global timing policy is waiting for reserved pad observers."
            raise RuntimeError(message)
        return encoded

    def _input_pitch_projection(self, sample_id: int) -> _InputPitchProjection:
        intent = self._project.pad_key_intent[sample_id]
        source = intent.source.recognized if intent.source is not None else None
        correction = recognize_key(intent.correction)
        quantize_step = (
            {"1_64": 1, "1_32": 2, "1_16": 4}[self._project.trigger_quantization_step]
            if self._project.trigger_quantization_enabled
            else None
        )
        return _InputPitchProjection(
            source=source,
            correction=correction,
            correction_unknown=intent.correction is not None and correction is None,
            base_shift=intent.base_shift,
            extra_shift=intent.extra_shift,
            key_lock=self._project.resolve_pad_key_lock(sample_id),
            quantize_step_64ths=quantize_step,
            sync=self._project.bpm_lock,
            request_sequence=self._app.transport.pitch.projection_sequence(sample_id),
        )

    def _execute_source_sync_toggle(self, key: str) -> bool:
        target = key.removeprefix("pad.source_sync.toggle:")
        if not target.isascii() or not target.isdecimal():
            return False
        sample_id = int(target)
        if not 0 <= sample_id < len(self._project.sample_paths):
            return False
        self._app.transport.source_sync.toggle(sample_id)
        return True

    def _input_source_map_projection(self, sample_id: int) -> _InputSourceMapProjection:
        intent = self._project.pad_source_sync_intent[sample_id]
        requested = self._project.resolve_pad_source_sync(sample_id)
        variable = variable_source_timing(self._project.global_timing_policy)
        maps = self._app.loader.source_beatmaps
        delivery = maps.playback_delivery(sample_id) if variable and requested else None
        if requested and delivery is not None:
            try:
                self._app.transport.loop.requested_region(sample_id)
            except RuntimeError, ValueError:
                # Preserve requested SYNC while withholding inadmissible geometry.
                # Other valid PAD rows must still publish, without scalar fallback.
                delivery = None
        exact = (
            self._project.pad_loop_bars[sample_id] * 4.0
            if requested and variable and self._project.pad_loop_auto[sample_id]
            else None
        )
        return _InputSourceMapProjection(
            self._project.pad_source_sync_preference(sample_id), intent.revision, delivery, exact
        )

    def _set_rust_enabled(self, *, enabled: bool) -> None:
        with suppress(RuntimeError, TypeError):
            self._audio.set_input_mapping_enabled(enabled)

    def _sync_rust_learn_state(self) -> None:
        with suppress(RuntimeError, TypeError):
            self._audio.set_input_learn_active(self._session.input_learn_active)

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

    def _execute_trigger_pad(self, key: str, *, received_at_ns: int | None = None) -> bool:
        if (pad_id := _parse_prefixed_sample_id(key, "pad.trigger:")) is None:
            return False
        self._app.transport.playback.trigger_pad(pad_id, received_at_ns=received_at_ns)
        return True

    def _execute_stop_pad(self, key: str) -> bool:
        if (pad_id := _parse_prefixed_sample_id(key, "pad.stop:")) is None:
            return False
        self._app.transport.playback.stop_pad(pad_id)
        return True

    def _execute_unload_pad(self, key: str) -> bool:
        if (pad_id := _parse_prefixed_sample_id(key, "pad.unload:")) is None:
            return False
        self._app.performance_confirmation.request("unload", pad_id)
        return True

    def _execute_analyze_pad(self, key: str) -> bool:
        if (pad_id := _parse_prefixed_sample_id(key, "pad.analyze:")) is None:
            return False
        self._app.performance_confirmation.request("analyze", pad_id)
        return True

    def _execute_adjust_loop(self, key: str) -> bool:
        if (pad_id := _parse_prefixed_sample_id(key, "pad.adjust_loop:")) is None:
            return False
        self._app.transport.waveform.toggle_editor(pad_id)
        return True

    def _execute_tap_bpm(self, key: str) -> bool:
        if (pad_id := _parse_prefixed_sample_id(key, "pad.tap_bpm:")) is None:
            return False
        self._app.transport.bpm.tap_bpm(pad_id)
        return True

    def _execute_selected_tap_bpm(self) -> None:
        self._app.transport.bpm.tap_bpm(self._project.selected_pad)

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

    def _execute_select_pad(
        self,
        key: str,
        *,
        native_selection_already_applied: bool = False,
        expected_native_selection_revision: int | None = None,
    ) -> bool:
        if (pad_id := _parse_prefixed_sample_id(key, "ui.select_pad:")) is None:
            return False
        if native_selection_already_applied:
            self._app.select_pad(
                pad_id,
                native_selection_already_applied=True,
                expected_native_selection_revision=expected_native_selection_revision,
            )
        else:
            self._app.select_pad(pad_id)
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
        self._sync_rust_learn_state()


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


def _midi_note_release(event: dict[str, object], binding_key: str) -> bool:
    if not binding_key.startswith("midi:note:"):
        return False
    kind = event.get("kind")
    return (
        event.get("value") == 0
        or event.get("velocity") == 0
        or (isinstance(kind, str) and kind in {"note_off", "noteoff"})
    )


def _repeated_relative_midi_cc_direction(value: int) -> int | None:
    if value in {1, 65}:
        return 1
    if value in {63, 127}:
        return -1
    return None


def _is_relative_action_key(action_key: str) -> bool:
    return action_key in {"global.volume.delta", "global.speed.delta"} or action_key.startswith((
        "pad.eq.delta:",
        "pad.eq.selected.delta:",
        "pad.gain.delta:",
    ))


def _is_relative_midi_binding_key(binding_key: str) -> bool:
    return binding_key.startswith(("midi:cc:", "midi:nrpn:"))
