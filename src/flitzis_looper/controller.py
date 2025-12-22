import math

from flitzis_looper.constants import SPEED_MAX, SPEED_MIN, VOLUME_MAX, VOLUME_MIN
from flitzis_looper.models import ProjectState, SessionState, validate_sample_id
from flitzis_looper_audio import AudioEngine


class LooperController:
    def __init__(self) -> None:
        self._project = ProjectState()
        self._session = SessionState()
        self._audio = AudioEngine()
        self._audio.run()

    # --- Actions ---

    def load_sample(self, sample_id: int, path: str) -> None:
        """Load an audio file into a sample slot.

        Args:
            sample_id: Sample slot identifier.
            path: Path to an audio file on disk.
        """
        validate_sample_id(sample_id)
        if self.is_sample_loaded(sample_id):
            self.unload_sample(sample_id)
        self._audio.load_sample(sample_id, path)
        self._project.sample_paths[sample_id] = path

    def unload_sample(self, sample_id: int) -> None:
        """Stop playback and unload a sample slot.

        Args:
            sample_id: Sample slot identifier.
        """
        validate_sample_id(sample_id)
        self._session.active_sample_ids.discard(sample_id)
        self._audio.unload_sample(sample_id)
        self.project.sample_paths[sample_id] = None

    def set_volume(self, volume: float) -> None:
        """Set global volume.

        Args:
            volume: Desired volume.
        """
        self._ensure_finite(volume)
        clamped = min(max(volume, VOLUME_MIN), VOLUME_MAX)
        self._audio.set_volume(clamped)
        self._project.volume = clamped

    def set_speed(self, speed: float) -> None:
        """Set global playback speed multiplier.

        Args:
            speed: Desired speed multiplier. Values are clamped to SPEED_MIN..SPEED_MAX.
        """
        self._ensure_finite(speed)
        clamped = min(max(speed, SPEED_MIN), SPEED_MAX)
        self._audio.set_speed(clamped)
        self._project.speed = clamped

    def reset_speed(self) -> None:
        """Reset global speed back to 1.0x."""
        self.set_speed(1.0)

    def trigger_pad(self, sample_id: int) -> None:
        """Trigger or retrigger a pad's loop.

        When Multi Loop is disabled, all other active pads are stopped first.

        Args:
            sample_id: Sample slot identifier.
        """
        validate_sample_id(sample_id)

        if not self.is_sample_loaded(sample_id):
            return

        if self.project.multi_loop:
            self.stop_pad(sample_id)
        else:
            self.stop_all_pads()

        self._audio.play_sample(sample_id, 1.0)
        self.session.active_sample_ids.add(sample_id)

    def stop_pad(self, sample_id: int) -> None:
        """Stop a pad if it is currently active.

        Args:
            sample_id: Sample slot identifier.
        """
        if not self.is_sample_active(sample_id):
            return

        self._audio.stop_sample(sample_id)
        self.session.active_sample_ids.discard(sample_id)

    def stop_all_pads(self) -> None:
        """Stop all currently active pads."""
        self._audio.stop_all()
        self.session.active_sample_ids.clear()

    def set_multi_loop(self, *, enabled: bool) -> None:
        """Enable or disable Multi Loop mode."""
        self.project.multi_loop = enabled

    def set_key_lock(self, *, enabled: bool) -> None:
        """Enable or disable Key Lock mode."""
        self.project.key_lock = enabled

    def set_bpm_lock(self, *, enabled: bool) -> None:
        """Enable or disable BPM Lock mode."""
        self.project.bpm_lock = enabled

    def shut_down(self) -> None:
        self._audio.stop_all()
        self._audio.shut_down()

    # --- Helpers ---

    def is_sample_loaded(self, sample_id: int) -> bool:
        """Return whether a sample slot has audio loaded.

        Args:
            sample_id: Sample slot identifier.

        Returns:
            True when the slot is loaded.
        """
        return self.project.sample_paths[sample_id] is not None

    def is_sample_active(self, sample_id: int) -> bool:
        """Return whether a sample slot is currently playing audio.

        Args:
            sample_id: Sample slot identifier.

        Returns:
            True when the slot is playing audio.
        """
        return sample_id in self.session.active_sample_ids

    @staticmethod
    def _ensure_finite(value: float) -> None:
        """Ensure value is finite.

        Args:
            value: Value to check.

        Raises:
            ValueError: If value is not finite.
        """
        if not math.isfinite(value):
            msg = f"value must be finite, got {value!r}"
            raise ValueError(msg)

    # --- State Access ---

    @property
    def project(self) -> ProjectState:
        return self._project

    @property
    def session(self) -> SessionState:
        return self._session
