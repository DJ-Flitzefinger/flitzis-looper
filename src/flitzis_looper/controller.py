import math

from pydantic import ValidationError

from flitzis_looper.constants import SPEED_MAX, SPEED_MIN, VOLUME_MAX, VOLUME_MIN
from flitzis_looper.models import ProjectState, SampleAnalysis, SessionState, validate_sample_id
from flitzis_looper_audio import AudioEngine


class LooperController:  # noqa: PLR0904
    def __init__(self) -> None:
        self._project = ProjectState()
        self._session = SessionState()
        self._audio = AudioEngine()
        self._audio.run()

    # --- Actions ---

    def load_sample_async(self, sample_id: int, path: str) -> None:
        """Load an audio file into a sample slot asynchronously.

        The load work happens on a Rust background thread. UI code should call
        `poll_loader_events()` each frame to apply completion/error updates.

        Args:
            sample_id: Sample slot identifier.
            path: Path to an audio file on disk.
        """
        validate_sample_id(sample_id)
        if self.is_sample_loaded(sample_id):
            self.unload_sample(sample_id)

        self._project.sample_analysis[sample_id] = None

        self._session.sample_load_errors.pop(sample_id, None)
        self._session.sample_load_progress.pop(sample_id, None)
        self._session.sample_load_stage.pop(sample_id, None)

        self._session.analyzing_sample_ids.discard(sample_id)
        self._session.sample_analysis_errors.pop(sample_id, None)
        self._session.sample_analysis_progress.pop(sample_id, None)
        self._session.sample_analysis_stage.pop(sample_id, None)

        self._session.pending_sample_paths[sample_id] = path
        self._session.loading_sample_ids.add(sample_id)

        self._audio.load_sample_async(sample_id, path)

    def analyze_sample_async(self, sample_id: int) -> None:
        """Analyze a previously loaded sample asynchronously."""
        validate_sample_id(sample_id)
        if self.is_sample_loading(sample_id):
            return

        self._clear_analysis_task_messages(sample_id)
        self._session.analyzing_sample_ids.add(sample_id)

        try:
            self._audio.analyze_sample_async(sample_id)
        except Exception as err:  # noqa: BLE001
            self._session.analyzing_sample_ids.discard(sample_id)
            self._session.sample_analysis_errors[sample_id] = str(err)

    def _clear_analysis_task_messages(self, sample_id: int) -> None:
        self._session.sample_analysis_errors.pop(sample_id, None)
        self._session.sample_analysis_progress.pop(sample_id, None)
        self._session.sample_analysis_stage.pop(sample_id, None)

    def _clear_analysis_task_state(self, sample_id: int) -> None:
        self._session.analyzing_sample_ids.discard(sample_id)
        self._clear_analysis_task_messages(sample_id)

    def _handle_loader_started(self, sample_id: int, _event: dict[str, object]) -> None:
        self._project.sample_analysis[sample_id] = None

        self._session.loading_sample_ids.add(sample_id)
        self._session.sample_load_errors.pop(sample_id, None)
        self._session.sample_load_progress.pop(sample_id, None)
        self._session.sample_load_stage.pop(sample_id, None)

        self._clear_analysis_task_state(sample_id)

    def _handle_loader_progress(self, sample_id: int, event: dict[str, object]) -> None:
        stage = event.get("stage")
        if isinstance(stage, str):
            self._session.sample_load_stage[sample_id] = stage

        percent = event.get("percent")
        if isinstance(percent, (int, float)):
            self._session.sample_load_progress[sample_id] = float(percent)

    def _handle_loader_success(self, sample_id: int, event: dict[str, object]) -> None:
        self._session.loading_sample_ids.discard(sample_id)
        self._session.sample_load_errors.pop(sample_id, None)
        self._session.sample_load_progress.pop(sample_id, None)
        self._session.sample_load_stage.pop(sample_id, None)

        pending = self._session.pending_sample_paths.pop(sample_id, None)
        if pending is not None:
            self._project.sample_paths[sample_id] = pending

        self._store_sample_analysis(sample_id, event.get("analysis"))
        self._clear_analysis_task_state(sample_id)

    def _handle_loader_error(self, sample_id: int, event: dict[str, object]) -> None:
        self._session.loading_sample_ids.discard(sample_id)
        self._session.sample_load_progress.pop(sample_id, None)
        self._session.sample_load_stage.pop(sample_id, None)
        self._session.pending_sample_paths.pop(sample_id, None)
        self._clear_analysis_task_state(sample_id)

        msg = event.get("msg")
        if isinstance(msg, str):
            self._session.sample_load_errors[sample_id] = msg

    def _handle_task_started(self, sample_id: int, event: dict[str, object]) -> None:
        if event.get("task") != "analysis":
            return

        self._session.analyzing_sample_ids.add(sample_id)
        self._clear_analysis_task_messages(sample_id)

    def _handle_task_progress(self, sample_id: int, event: dict[str, object]) -> None:
        if event.get("task") != "analysis":
            return

        stage = event.get("stage")
        if isinstance(stage, str):
            self._session.sample_analysis_stage[sample_id] = stage

        percent = event.get("percent")
        if isinstance(percent, (int, float)):
            self._session.sample_analysis_progress[sample_id] = float(percent)

    def _handle_task_success(self, sample_id: int, event: dict[str, object]) -> None:
        if event.get("task") != "analysis":
            return

        self._store_sample_analysis(sample_id, event.get("analysis"))
        self._clear_analysis_task_state(sample_id)

    def _handle_task_error(self, sample_id: int, event: dict[str, object]) -> None:
        if event.get("task") != "analysis":
            return

        self._session.analyzing_sample_ids.discard(sample_id)
        self._session.sample_analysis_progress.pop(sample_id, None)
        self._session.sample_analysis_stage.pop(sample_id, None)

        msg = event.get("msg")
        if isinstance(msg, str):
            self._session.sample_analysis_errors[sample_id] = msg

    def poll_loader_events(self) -> None:
        """Drain pending loader events from the Rust audio engine."""
        handlers = {
            "started": self._handle_loader_started,
            "progress": self._handle_loader_progress,
            "success": self._handle_loader_success,
            "error": self._handle_loader_error,
            "task_started": self._handle_task_started,
            "task_progress": self._handle_task_progress,
            "task_success": self._handle_task_success,
            "task_error": self._handle_task_error,
        }

        while True:
            event = self._audio.poll_loader_events()
            if event is None:
                return

            event_type = event.get("type")
            sample_id = event.get("id")
            if not isinstance(event_type, str) or not isinstance(sample_id, int):
                continue

            handler = handlers.get(event_type)
            if handler is None:
                continue

            handler(sample_id, event)

    def is_sample_loading(self, sample_id: int) -> bool:
        """Return whether a sample slot is currently being loaded."""
        validate_sample_id(sample_id)
        return sample_id in self._session.loading_sample_ids

    def pending_sample_path(self, sample_id: int) -> str | None:
        """Return the pending path for an in-flight async load."""
        validate_sample_id(sample_id)
        return self._session.pending_sample_paths.get(sample_id)

    def sample_load_error(self, sample_id: int) -> str | None:
        """Return the last async load error message for a pad."""
        validate_sample_id(sample_id)
        return self._session.sample_load_errors.get(sample_id)

    def sample_load_progress(self, sample_id: int) -> float | None:
        """Return best-effort async load progress for a pad."""
        validate_sample_id(sample_id)
        value = self._session.sample_load_progress.get(sample_id)
        return float(value) if value is not None else None

    def sample_load_stage(self, sample_id: int) -> str | None:
        """Return the last reported async load stage for a pad."""
        validate_sample_id(sample_id)
        return self._session.sample_load_stage.get(sample_id)

    def unload_sample(self, sample_id: int) -> None:
        """Stop playback and unload a sample slot.

        Args:
            sample_id: Sample slot identifier.
        """
        validate_sample_id(sample_id)
        self._session.active_sample_ids.discard(sample_id)
        self._session.loading_sample_ids.discard(sample_id)
        self._session.pending_sample_paths.pop(sample_id, None)
        self._session.sample_load_progress.pop(sample_id, None)
        self._session.sample_load_stage.pop(sample_id, None)
        self._session.sample_load_errors.pop(sample_id, None)

        self._session.analyzing_sample_ids.discard(sample_id)
        self._session.sample_analysis_errors.pop(sample_id, None)
        self._session.sample_analysis_progress.pop(sample_id, None)
        self._session.sample_analysis_stage.pop(sample_id, None)

        self._audio.unload_sample(sample_id)
        self.project.sample_paths[sample_id] = None
        self.project.sample_analysis[sample_id] = None

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

    def _store_sample_analysis(self, sample_id: int, analysis: object) -> None:
        if not isinstance(analysis, dict):
            return

        try:
            parsed = SampleAnalysis.model_validate(analysis)
        except ValidationError:
            return

        self._project.sample_analysis[sample_id] = parsed

    # --- State Access ---

    @property
    def project(self) -> ProjectState:
        return self._project

    @property
    def session(self) -> SessionState:
        return self._session
