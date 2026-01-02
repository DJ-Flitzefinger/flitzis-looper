import math
import time
from itertools import pairwise

from pydantic import ValidationError

from flitzis_looper.constants import SPEED_MAX, SPEED_MIN, VOLUME_MAX, VOLUME_MIN
from flitzis_looper.models import ProjectState, SampleAnalysis, SessionState, validate_sample_id
from flitzis_looper_audio import AudioEngine


class LooperController:  # noqa: PLR0904
    _TAP_BPM_WINDOW_SIZE = 5

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
        self._on_pad_bpm_changed(sample_id)

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
        self._recompute_master_bpm()

    def reset_speed(self) -> None:
        """Reset global speed back to 1.0x."""
        self.set_speed(1.0)

    @staticmethod
    def _normalize_bpm(bpm: float | None) -> float | None:
        if bpm is None:
            return None
        if not math.isfinite(bpm) or bpm <= 0:
            return None
        return float(bpm)

    def _recompute_master_bpm(self) -> None:
        if not self.project.bpm_lock:
            self._session.master_bpm = None
            return

        anchor_bpm = self._normalize_bpm(self._session.bpm_lock_anchor_bpm)
        if anchor_bpm is None:
            self._session.master_bpm = None
            return

        master_bpm = anchor_bpm * self.project.speed
        self._session.master_bpm = master_bpm
        self._audio.set_master_bpm(master_bpm)

    def _on_pad_bpm_changed(self, sample_id: int) -> None:
        bpm = self._normalize_bpm(self.effective_bpm(sample_id))
        self._audio.set_pad_bpm(sample_id, bpm)

        if self._session.bpm_lock_anchor_pad_id != sample_id:
            return

        self._session.bpm_lock_anchor_bpm = bpm
        self._recompute_master_bpm()

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
        if enabled == self.project.key_lock:
            return
        self.project.key_lock = enabled
        self._audio.set_key_lock(enabled)

    def set_bpm_lock(self, *, enabled: bool) -> None:
        """Enable or disable BPM Lock mode."""
        if enabled == self.project.bpm_lock:
            return

        self.project.bpm_lock = enabled

        if enabled:
            anchor_pad_id = self.project.selected_pad
            anchor_bpm = self._normalize_bpm(self.effective_bpm(anchor_pad_id))
            self._session.bpm_lock_anchor_pad_id = anchor_pad_id
            self._session.bpm_lock_anchor_bpm = anchor_bpm
        else:
            self._session.bpm_lock_anchor_pad_id = None
            self._session.bpm_lock_anchor_bpm = None

        self._audio.set_bpm_lock(enabled)
        self._recompute_master_bpm()

    def set_manual_bpm(self, sample_id: int, bpm: float) -> None:
        """Set a pad's manual BPM override.

        Args:
            sample_id: Sample slot identifier.
            bpm: Manual BPM value.
        """
        validate_sample_id(sample_id)
        self._ensure_finite(bpm)
        if bpm <= 0:
            msg = f"bpm must be > 0, got {bpm!r}"
            raise ValueError(msg)
        self._project.manual_bpm[sample_id] = float(bpm)
        self._on_pad_bpm_changed(sample_id)

    def clear_manual_bpm(self, sample_id: int) -> None:
        """Clear a pad's manual BPM override."""
        validate_sample_id(sample_id)
        self._project.manual_bpm[sample_id] = None
        self._on_pad_bpm_changed(sample_id)

    def tap_bpm(self, sample_id: int) -> float | None:
        """Register a Tap BPM event and update manual BPM.

        BPM is computed from the average interval between consecutive taps in the most recent
        window of taps.

        Args:
            sample_id: Sample slot identifier.

        Returns:
            The computed BPM when at least three taps are available, otherwise None.
        """
        validate_sample_id(sample_id)

        now = time.monotonic()
        if self._session.tap_bpm_pad_id != sample_id:
            self._session.tap_bpm_pad_id = sample_id
            self._session.tap_bpm_timestamps.clear()

        timestamps = self._session.tap_bpm_timestamps
        if timestamps and now <= timestamps[-1]:
            return None

        timestamps.append(now)
        if len(timestamps) > self._TAP_BPM_WINDOW_SIZE:
            del timestamps[: -self._TAP_BPM_WINDOW_SIZE]

        if len(timestamps) < 3:
            return None

        intervals = [b - a for a, b in pairwise(timestamps)]
        avg_interval = sum(intervals) / len(intervals)
        if avg_interval <= 0:
            return None

        bpm = 60.0 / avg_interval
        if not math.isfinite(bpm):
            return None

        self._project.manual_bpm[sample_id] = bpm
        return bpm

    def effective_bpm(self, sample_id: int) -> float | None:
        """Return the effective BPM for a pad.

        Manual BPM overrides detected BPM.
        """
        validate_sample_id(sample_id)

        manual = self._project.manual_bpm[sample_id]
        if manual is not None:
            return float(manual)

        analysis = self._project.sample_analysis[sample_id]
        return analysis.bpm if analysis is not None else None

    def set_manual_key(self, sample_id: int, key: str) -> None:
        """Set a pad's manual key override.

        Args:
            sample_id: Sample slot identifier.
            key: Manual key string (e.g., "C#m").
        """
        validate_sample_id(sample_id)
        if not key:
            msg = "key must be a non-empty string"
            raise ValueError(msg)
        self._project.manual_key[sample_id] = key

    def clear_manual_key(self, sample_id: int) -> None:
        """Clear a pad's manual key override."""
        validate_sample_id(sample_id)
        self._project.manual_key[sample_id] = None

    def effective_key(self, sample_id: int) -> str | None:
        """Return the effective key for a pad.

        Manual key overrides detected key.
        """
        validate_sample_id(sample_id)

        manual = self._project.manual_key[sample_id]
        if manual is not None:
            return manual

        analysis = self._project.sample_analysis[sample_id]
        return analysis.key if analysis is not None else None

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
        self._on_pad_bpm_changed(sample_id)

    # --- State Access ---

    @property
    def project(self) -> ProjectState:
        return self._project

    @property
    def session(self) -> SessionState:
        return self._session
