import math
from pathlib import Path, PureWindowsPath

from flitzis_looper.constants import NUM_BANKS, NUM_PADS, SPEED_MAX, SPEED_MIN
from flitzis_looper.state import AppState
from flitzis_looper_rs import AudioEngine


class FlitzisLooperApp:
    """Application logic for Flitzis Looper.

    Constructs the Rust-backed `AudioEngine` and holds the UI state.
    """

    def __init__(self) -> None:
        self.audio_engine: AudioEngine = AudioEngine()
        self.state = AppState()

    def select_bank(self, bank_id: int) -> None:
        """Select the active performance bank.

        Args:
            bank_id: Bank number (0..n).

        Raises:
            ValueError: If bank_id is out of range.
        """
        if not 0 <= bank_id <= NUM_BANKS - 1:
            msg = f"bank_id must be in 1..{NUM_BANKS}, got {bank_id}"
            raise ValueError(msg)

        self.state.selected_bank = bank_id

    def is_sample_loaded(self, sample_id: int) -> bool:
        """Return whether a sample slot has audio loaded.

        Args:
            sample_id: Sample slot identifier (0..35).

        Returns:
            True when the slot is loaded.
        """
        self._validate_sample_id(sample_id)
        return self.state.sample_paths[sample_id] is not None

    def load_sample(self, sample_id: int, path: str) -> None:
        """Load an audio file into a sample slot.

        Args:
            sample_id: Sample slot identifier (0..35).
            path: Path to an audio file on disk.
        """
        self._validate_sample_id(sample_id)
        self.audio_engine.load_sample(sample_id, path)
        self.state.sample_paths[sample_id] = path

    def unload_sample(self, sample_id: int) -> None:
        """Stop playback and unload a sample slot.

        Args:
            sample_id: Sample slot identifier (0..35).
        """
        self._validate_sample_id(sample_id)
        self.audio_engine.stop_sample(sample_id)
        self.state.active_sample_ids.discard(sample_id)
        self.audio_engine.unload_sample(sample_id)
        self.state.sample_paths[sample_id] = None

    def set_multi_loop(self, *, enabled: bool) -> None:
        """Enable or disable Multi Loop mode."""
        self.state.multi_loop = enabled

    def toggle_multi_loop(self) -> None:
        """Toggle Multi Loop mode."""
        self.set_multi_loop(enabled=not self.state.multi_loop)

    def set_key_lock(self, *, enabled: bool) -> None:
        """Enable or disable Key Lock mode."""
        self.state.key_lock = enabled

    def toggle_key_lock(self) -> None:
        """Toggle Key Lock mode."""
        self.set_key_lock(enabled=not self.state.key_lock)

    def set_bpm_lock(self, *, enabled: bool) -> None:
        """Enable or disable BPM Lock mode."""
        self.state.bpm_lock = enabled

    def toggle_bpm_lock(self) -> None:
        """Toggle BPM Lock mode."""
        self.set_bpm_lock(enabled=not self.state.bpm_lock)

    def set_volume(self, volume: float) -> None:
        """Set global volume.

        Args:
            volume: Desired volume.
        """
        clamped = min(max(volume, 0.0), 1.0)
        # self.audio_engine.set_volume(clamped)
        self.state.volume = clamped

    def set_speed(self, speed: float) -> None:
        """Set global playback speed multiplier.

        Args:
            speed: Desired speed multiplier. Values are clamped to SPEED_MIN..SPEED_MAX.

        Raises:
            ValueError: If speed is not finite.
        """
        if not math.isfinite(speed):
            msg = f"speed must be finite, got {speed!r}"
            raise ValueError(msg)

        clamped = min(max(speed, SPEED_MIN), SPEED_MAX)
        self.audio_engine.set_speed(clamped)
        self.state.speed = clamped

    def reset_speed(self) -> None:
        """Reset global speed back to 1.0x."""
        self.set_speed(1.0)

    def trigger_pad(self, sample_id: int, velocity: float = 1.0) -> None:
        """Trigger or retrigger a pad's loop.

        When Multi Loop is disabled, all other active pads are stopped first.

        Args:
            sample_id: Sample slot identifier (0..35).
            velocity: Playback velocity (0.0..=1.0).
        """
        self._validate_sample_id(sample_id)

        if not self.is_sample_loaded(sample_id):
            return

        if self.state.multi_loop:
            self.audio_engine.stop_sample(sample_id)
            self.state.active_sample_ids.discard(sample_id)
            self.audio_engine.play_sample(sample_id, velocity)
            self.state.active_sample_ids.add(sample_id)
            return

        self.audio_engine.stop_all()
        self.state.active_sample_ids.clear()
        self.audio_engine.play_sample(sample_id, velocity)
        self.state.active_sample_ids.add(sample_id)

    def stop_pad(self, sample_id: int) -> None:
        """Stop a pad if it is currently active.

        Args:
            sample_id: Sample slot identifier (0..35).
        """
        self._validate_sample_id(sample_id)

        if sample_id not in self.state.active_sample_ids:
            return

        self.audio_engine.stop_sample(sample_id)
        self.state.active_sample_ids.discard(sample_id)

    def stop_all_pads(self) -> None:
        """Stop all currently active pads."""
        self.audio_engine.stop_all()
        self.state.active_sample_ids.clear()

    def _validate_sample_id(self, sample_id: int) -> None:
        if not 0 <= sample_id < NUM_PADS:
            msg = f"sample_id must be in 0..{NUM_PADS - 1}, got {sample_id}"
            raise ValueError(msg)

    # UI

    def select_pad(self, pad_id: int) -> None:
        self.state.selected_pad = pad_id

    def toggle_left_sidebar(self) -> None:
        self.state.sidebar_left_expanded = not self.state.sidebar_left_expanded

    def toggle_right_sidebar(self) -> None:
        self.state.sidebar_right_expanded = not self.state.sidebar_right_expanded

    def open_file_dialog(self, pad_id: int) -> None:
        self.state.pending_file_dialog = pad_id

    def close_file_dialog(self) -> None:
        self.state.pending_file_dialog = None

    def pad_label(self, pad_id: int) -> str:
        """Derive the UI label for a performance pad.

        Args:
            pad_id: Pad number

        Returns:
            The basename of the loaded audio file, otherwise the pad number.
        """
        path = self.state.sample_paths[pad_id]
        if path is None:
            return ""

        if "\\" in path:
            return PureWindowsPath(path).name

        return Path(path).name
