from flitzis_looper_rs import AudioEngine

NUM_BANKS = 6


class FlitzisLooperApp:
    """Application logic stub for Flitzis Looper.

    This is currently a minimal shell that constructs the Rust-backed `AudioEngine`
    and holds small bits of UI state.
    """

    def __init__(self) -> None:
        self.audio_engine: AudioEngine = AudioEngine()
        self.selected_bank: int = 1

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
