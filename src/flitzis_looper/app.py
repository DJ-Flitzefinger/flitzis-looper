import math
from pathlib import Path, PureWindowsPath

from flitzis_looper_rs import AudioEngine

NUM_BANKS = 6
NUM_PADS = 36


def pad_label_from_sample_path(path: str | None, pad_id: int) -> str:
    """Derive the UI label for a performance pad.

    Args:
        path: Loaded sample path, or None for empty pads.
        pad_id: Pad number (1..36).

    Returns:
        The basename of the loaded audio file, otherwise the pad number.
    """
    if not path:
        return str(pad_id)

    if "\\" in path:
        return PureWindowsPath(path).name

    return Path(path).name


class FlitzisLooperApp:
    """Application logic stub for Flitzis Looper.

    This is currently a minimal shell that constructs the Rust-backed `AudioEngine`
    and holds small bits of UI state.
    """

    def __init__(self) -> None:
        self.audio_engine: AudioEngine = AudioEngine()
        self.selected_bank: int = 1
        self.sample_paths: list[str | None] = [None] * NUM_PADS
        self.multi_loop_enabled: bool = False
        self.speed: float = 1.0
        self.active_sample_ids: set[int] = set()

    def select_bank(self, bank_id: int) -> None:
        """Select the active performance bank.

        Args:
            bank_id: Bank number (1..6).

        Raises:
            ValueError: If bank_id is out of range.
        """
        if not 1 <= bank_id <= NUM_BANKS:
            msg = f"bank_id must be in 1..{NUM_BANKS}, got {bank_id}"
            raise ValueError(msg)

        self.selected_bank = bank_id

    def is_sample_loaded(self, sample_id: int) -> bool:
        """Return whether a sample slot has audio loaded.

        Args:
            sample_id: Sample slot identifier (0..35).

        Returns:
            True when the slot is loaded.
        """
        self._validate_sample_id(sample_id)
        return self.sample_paths[sample_id] is not None

    def load_sample(self, sample_id: int, path: str) -> None:
        """Load an audio file into a sample slot.

        Args:
            sample_id: Sample slot identifier (0..35).
            path: Path to an audio file on disk.
        """
        self._validate_sample_id(sample_id)
        self.audio_engine.load_sample(sample_id, path)
        self.sample_paths[sample_id] = path

    def unload_sample(self, sample_id: int) -> None:
        """Stop playback and unload a sample slot.

        Args:
            sample_id: Sample slot identifier (0..35).
        """
        self._validate_sample_id(sample_id)
        self.audio_engine.stop_sample(sample_id)
        self.active_sample_ids.discard(sample_id)
        self.audio_engine.unload_sample(sample_id)
        self.sample_paths[sample_id] = None

    def set_multi_loop_enabled(self, *, enabled: bool) -> None:
        """Enable or disable MultiLoop mode."""
        self.multi_loop_enabled = enabled

    def set_speed(self, speed: float) -> None:
        """Set global playback speed multiplier.

        Args:
            speed: Desired speed multiplier. Values are clamped to 0.5..2.0.

        Raises:
            ValueError: If speed is not finite.
        """
        if not math.isfinite(speed):
            msg = f"speed must be finite, got {speed!r}"
            raise ValueError(msg)

        clamped = min(max(speed, 0.5), 2.0)
        self.audio_engine.set_speed(clamped)
        self.speed = clamped

    def reset_speed(self) -> None:
        """Reset global speed back to 1.0x."""
        self.set_speed(1.0)

    def trigger_pad(self, sample_id: int, velocity: float = 1.0) -> None:
        """Trigger or retrigger a pad's loop.

        When MultiLoop is disabled, all other active pads are stopped first.

        Args:
            sample_id: Sample slot identifier (0..35).
            velocity: Playback velocity (0.0..=1.0).
        """
        self._validate_sample_id(sample_id)

        if not self.is_sample_loaded(sample_id):
            return

        if self.multi_loop_enabled:
            self.audio_engine.stop_sample(sample_id)
            self.active_sample_ids.discard(sample_id)
            self.audio_engine.play_sample(sample_id, velocity)
            self.active_sample_ids.add(sample_id)
            return

        self.audio_engine.stop_all()
        self.active_sample_ids.clear()
        self.audio_engine.play_sample(sample_id, velocity)
        self.active_sample_ids.add(sample_id)

    def stop_pad(self, sample_id: int) -> None:
        """Stop a pad if it is currently active.

        Args:
            sample_id: Sample slot identifier (0..35).
        """
        self._validate_sample_id(sample_id)

        if sample_id not in self.active_sample_ids:
            return

        self.audio_engine.stop_sample(sample_id)
        self.active_sample_ids.discard(sample_id)

    def stop_all_pads(self) -> None:
        """Stop all currently active pads."""
        self.audio_engine.stop_all()
        self.active_sample_ids.clear()

    def _validate_sample_id(self, sample_id: int) -> None:
        if not 0 <= sample_id < NUM_PADS:
            msg = f"sample_id must be in 0..{NUM_PADS - 1}, got {sample_id}"
            raise ValueError(msg)
