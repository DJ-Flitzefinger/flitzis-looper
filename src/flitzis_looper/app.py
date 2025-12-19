from flitzis_looper_rs import AudioEngine

NUM_BANKS = 6
NUM_PADS = 36


class FlitzisLooperApp:
    """Application logic stub for Flitzis Looper.

    This is currently a minimal shell that constructs the Rust-backed `AudioEngine`
    and holds small bits of UI state.
    """

    def __init__(self) -> None:
        self.audio_engine: AudioEngine = AudioEngine()
        self.selected_bank: int = 1
        self.sample_paths: list[str | None] = [None] * NUM_PADS

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
        self.audio_engine.unload_sample(sample_id)
        self.sample_paths[sample_id] = None

    def _validate_sample_id(self, sample_id: int) -> None:
        if not 0 <= sample_id < NUM_PADS:
            msg = f"sample_id must be in 0..{NUM_PADS - 1}, got {sample_id}"
            raise ValueError(msg)
