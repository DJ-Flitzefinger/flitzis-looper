from contextlib import suppress
from pathlib import Path
from typing import TYPE_CHECKING, TypeVar

from pydantic import ValidationError

from flitzis_looper.controller.accepted_restore import AcceptedTimingRestore
from flitzis_looper.controller.base import BaseController
from flitzis_looper.controller.validation import normalize_bpm
from flitzis_looper.models import (
    ProjectState,
    SampleAnalysis,
    SessionState,
    validate_sample_id,
)

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper_audio import AudioEngine


_PadValue = TypeVar("_PadValue")


def _reset_pad_value(values: list[_PadValue], sample_id: int, default: _PadValue) -> bool:
    if values[sample_id] == default:
        return False

    values[sample_id] = default
    return True


class LoaderController(BaseController):
    def __init__(
        self,
        project: ProjectState,
        session: SessionState,
        audio: AudioEngine,
        on_pad_bpm_changed: Callable[[int], None],
        on_project_changed: Callable[[], None] | None = None,
        on_stem_generation_started: Callable[[int], None] | None = None,
        on_stem_generation_progress: Callable[[int, float | None, str | None], None] | None = None,
        on_stem_generation_success: Callable[[int], None] | None = None,
        on_stem_generation_error: Callable[[int, str], None] | None = None,
        on_stems_deleted: Callable[[int], bool] | None = None,
    ) -> None:
        super().__init__(project, session, audio, on_project_changed)

        self._on_pad_bpm_changed = on_pad_bpm_changed
        self._on_stem_generation_started = on_stem_generation_started
        self._on_stem_generation_progress = on_stem_generation_progress
        self._on_stem_generation_success = on_stem_generation_success
        self._on_stem_generation_error = on_stem_generation_error
        self._on_stems_deleted = on_stems_deleted
        self._on_restored_sample_loaded: Callable[[int], bool] | None = None
        self._on_new_sample_loaded: Callable[[int, float | None], None] | None = None
        self._on_sample_unloaded: Callable[[int], None] | None = None
        self._on_accepted_timing_refresh: Callable[[int], None] | None = None
        self._load_request_ids: dict[int, int] = {}
        self._analysis_request_ids: dict[int, int] = {}
        self._accepted_restore = AcceptedTimingRestore(
            project, session, audio, self._finish_accepted_restore
        )

    def _finish_accepted_restore(self, sample_id: int) -> None:
        if self._on_accepted_timing_refresh is not None:
            self._on_accepted_timing_refresh(sample_id)
            return
        self._on_pad_bpm_changed(sample_id)
        self.finish_accepted_timing_refresh(sample_id)
        self._mark_project_changed()

    def finish_accepted_timing_refresh(self, sample_id: int) -> None:
        """Refresh restored stem intent after acknowledged derived timing completion."""
        if self._on_restored_sample_loaded is not None:
            self._on_restored_sample_loaded(sample_id)

    def set_accepted_timing_refresh_callback(self, callback: Callable[[int], None]) -> None:
        """Route fresh accepted restore through the application's guarded completion."""
        self._on_accepted_timing_refresh = callback

    def shut_down(self) -> None:
        """Drain owned timing restoration before native stream teardown."""
        self._accepted_restore.shut_down()

    def set_new_sample_loaded_callback(self, callback: Callable[[int, float | None], None]) -> None:
        """Register behavior that runs after a newly assigned sample finishes loading."""
        self._on_new_sample_loaded = callback

    def set_restored_sample_loaded_callback(self, callback: Callable[[int], bool]) -> None:
        """Register behavior that runs after a restored sample finishes loading."""
        self._on_restored_sample_loaded = callback

    def set_sample_unloaded_callback(self, callback: Callable[[int], None]) -> None:
        """Register control-intent cleanup after a native unload is admitted."""
        self._on_sample_unloaded = callback

    def restore_samples_from_project_state(self) -> None:
        """Schedule async loads for cached samples referenced by `ProjectState`.

        Invalid/missing cached files are ignored by clearing the pad assignment.
        """
        output_sample_rate = self._output_sample_rate_hz()
        if output_sample_rate is None:
            return

        changed = False
        for sample_id, path in enumerate(self._project.sample_paths):
            if path is None:
                continue

            rel = self._parse_cached_sample_path(path)
            if rel is None:
                self._clear_restored_pad(sample_id)
                changed = True
                continue

            abs_path = Path.cwd() / rel
            if not abs_path.is_file():
                self._clear_restored_pad(sample_id)
                changed = True
                continue

            if not self._schedule_restored_load(sample_id, rel, run_analysis=False):
                self._clear_restored_pad(sample_id)
                changed = True

        if changed:
            self._mark_project_changed()

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
        else:
            self._reset_unloaded_pad_defaults(sample_id)
            self._on_pad_bpm_changed(sample_id)

        self._project.sample_analysis[sample_id] = None
        self._clear_stem_cache(sample_id)
        self._mark_project_changed()

        self._session.sample_load_errors.pop(sample_id, None)
        self._session.sample_load_progress.pop(sample_id, None)
        self._session.sample_load_stage.pop(sample_id, None)

        self._clear_analysis_task_state(sample_id)
        self._clear_stem_generation_state(sample_id)

        self._session.pending_sample_paths[sample_id] = path
        self._session.loading_sample_ids.add(sample_id)
        self._load_request_ids.pop(sample_id, None)

        request_id = self._audio.load_sample_async(sample_id, path, run_analysis=True)
        self._record_load_request_id(sample_id, request_id)

    def unload_sample(self, sample_id: int) -> None:
        """Stop playback and unload a sample slot."""
        validate_sample_id(sample_id)
        self._accepted_restore.cancel(sample_id)
        self._session.active_sample_ids.discard(sample_id)
        self._session.paused_sample_ids.discard(sample_id)
        self._session.global_stop_restore_sample_ids.discard(sample_id)
        self._session.loading_sample_ids.discard(sample_id)
        self._session.pending_sample_paths.pop(sample_id, None)
        self._load_request_ids.pop(sample_id, None)
        self._session.sample_load_progress.pop(sample_id, None)
        self._session.sample_load_stage.pop(sample_id, None)
        self._session.sample_load_errors.pop(sample_id, None)
        self._session.pressed_pads[sample_id] = False
        self._session.pad_peak[sample_id] = 0.0
        self._session.pad_peak_updated_at[sample_id] = 0.0
        self._session.pad_clip_hold_until[sample_id] = 0.0
        self._session.pad_playhead_s[sample_id] = None
        self._session.pad_playhead_updated_at[sample_id] = 0.0
        if self._session.waveform_editor_pad_id == sample_id:
            self._session.waveform_editor_open = False
            self._session.waveform_editor_pad_id = None
        if self._session.waveform_pause_hold_pad_id == sample_id:
            self._session.waveform_pause_hold_pad_id = None
        if self._session.tap_bpm_pad_id == sample_id:
            self._session.tap_bpm_pad_id = None
            self._session.tap_bpm_timestamps.clear()

        self._clear_analysis_task_state(sample_id)
        self._clear_stem_generation_state(sample_id)

        old_path = self._project.sample_paths[sample_id]
        if self._on_stems_deleted is not None:
            self._on_stems_deleted(sample_id)
        else:
            self._clear_stem_cache(sample_id)

        self._audio.unload_sample(sample_id)
        if self._on_sample_unloaded is not None:
            self._on_sample_unloaded(sample_id)
        self._reset_unloaded_pad_defaults(sample_id)
        self._on_pad_bpm_changed(sample_id)
        self._mark_project_changed()

        if old_path is None or "\\" in old_path:
            return

        rel = Path(old_path)
        if rel.is_absolute() or not rel.parts or rel.parts[0] != "samples":
            return

        with suppress(OSError):
            (Path.cwd() / rel).unlink(missing_ok=True)

    def analyze_sample_async(self, sample_id: int) -> None:
        """Analyze a previously loaded sample asynchronously."""
        validate_sample_id(sample_id)
        if self.is_sample_loading(sample_id):
            return

        self._clear_analysis_task_messages(sample_id)
        if not self.is_sample_loaded(sample_id):
            self._session.analyzing_sample_ids.discard(sample_id)
            self._analysis_request_ids.pop(sample_id, None)
            self._session.sample_analysis_errors[sample_id] = "sample is not loaded"
            return

        self._session.analyzing_sample_ids.add(sample_id)
        self._analysis_request_ids.pop(sample_id, None)

        try:
            request_id = self._audio.analyze_sample_async(sample_id)
        except (RuntimeError, ValueError) as err:
            self._session.analyzing_sample_ids.discard(sample_id)
            self._session.sample_analysis_errors[sample_id] = str(err)
        else:
            self._record_analysis_request_id(sample_id, request_id)

    def poll_loader_events(self) -> None:
        """Drain pending loader events from the Rust audio engine."""
        self._accepted_restore.poll()
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

    def is_sample_loaded(self, sample_id: int) -> bool:
        """Return whether a sample slot has audio loaded."""
        validate_sample_id(sample_id)
        return self._project.sample_paths[sample_id] is not None

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

    def _clear_analysis_task_state(self, sample_id: int) -> None:
        self._session.analyzing_sample_ids.discard(sample_id)
        self._analysis_request_ids.pop(sample_id, None)
        self._clear_analysis_task_messages(sample_id)

    def _clear_analysis_task_messages(self, sample_id: int) -> None:
        self._session.sample_analysis_errors.pop(sample_id, None)
        self._session.sample_analysis_progress.pop(sample_id, None)
        self._session.sample_analysis_stage.pop(sample_id, None)

    def _clear_stem_generation_state(self, sample_id: int) -> None:
        self._session.stem_generating_sample_ids.discard(sample_id)
        self._session.stem_generation_source_versions.pop(sample_id, None)
        self._clear_stem_generation_messages(sample_id)

    def _clear_stem_generation_messages(self, sample_id: int) -> None:
        self._session.stem_generation_errors.pop(sample_id, None)
        self._session.stem_generation_diagnostics.pop(sample_id, None)
        self._session.stem_generation_progress.pop(sample_id, None)
        self._session.stem_generation_stage.pop(sample_id, None)

    def _clear_stem_cache(self, sample_id: int) -> None:
        self._project.stem_cache[sample_id] = None

    def _reset_unloaded_pad_defaults(self, sample_id: int) -> None:
        defaults = ProjectState()
        self._reset_unloaded_pad_project_defaults(sample_id, defaults)
        self._publish_unloaded_pad_audio_defaults(sample_id, defaults)

    def _reset_unloaded_pad_project_defaults(self, sample_id: int, defaults: ProjectState) -> bool:
        source_changed = self._reset_unloaded_pad_source_defaults(sample_id, defaults)
        mixing_changed = self._reset_unloaded_pad_mixing_defaults(sample_id, defaults)
        loop_changed = self._reset_unloaded_pad_loop_defaults(sample_id, defaults)
        return source_changed or mixing_changed or loop_changed

    def _reset_unloaded_pad_source_defaults(self, sample_id: int, defaults: ProjectState) -> bool:
        return any((
            _reset_pad_value(
                self._project.sample_paths, sample_id, defaults.sample_paths[sample_id]
            ),
            _reset_pad_value(
                self._project.sample_durations,
                sample_id,
                defaults.sample_durations[sample_id],
            ),
            _reset_pad_value(
                self._project.sample_analysis,
                sample_id,
                defaults.sample_analysis[sample_id],
            ),
            _reset_pad_value(self._project.stem_cache, sample_id, defaults.stem_cache[sample_id]),
            _reset_pad_value(
                self._project.pad_stem_mix_mode,
                sample_id,
                defaults.pad_stem_mix_mode[sample_id],
            ),
            _reset_pad_value(
                self._project.pad_key_lock,
                sample_id,
                defaults.pad_key_lock[sample_id],
            ),
            _reset_pad_value(self._project.manual_bpm, sample_id, defaults.manual_bpm[sample_id]),
            _reset_pad_value(self._project.manual_key, sample_id, defaults.manual_key[sample_id]),
            _reset_pad_value(
                self._project.pad_timing_intent, sample_id, defaults.pad_timing_intent[sample_id]
            ),
        ))

    def _reset_unloaded_pad_mixing_defaults(self, sample_id: int, defaults: ProjectState) -> bool:
        return any((
            _reset_pad_value(self._project.pad_gain_db, sample_id, defaults.pad_gain_db[sample_id]),
            _reset_pad_value(
                self._project.pad_eq_low_db, sample_id, defaults.pad_eq_low_db[sample_id]
            ),
            _reset_pad_value(
                self._project.pad_eq_mid_db, sample_id, defaults.pad_eq_mid_db[sample_id]
            ),
            _reset_pad_value(
                self._project.pad_eq_high_db, sample_id, defaults.pad_eq_high_db[sample_id]
            ),
        ))

    def _reset_unloaded_pad_loop_defaults(self, sample_id: int, defaults: ProjectState) -> bool:
        return any((
            _reset_pad_value(
                self._project.pad_loop_start_s,
                sample_id,
                defaults.pad_loop_start_s[sample_id],
            ),
            _reset_pad_value(
                self._project.pad_loop_end_s, sample_id, defaults.pad_loop_end_s[sample_id]
            ),
            _reset_pad_value(
                self._project.pad_loop_auto, sample_id, defaults.pad_loop_auto[sample_id]
            ),
            _reset_pad_value(
                self._project.pad_loop_bars, sample_id, defaults.pad_loop_bars[sample_id]
            ),
            _reset_pad_value(
                self._project.pad_grid_offset_samples,
                sample_id,
                defaults.pad_grid_offset_samples[sample_id],
            ),
            _reset_pad_value(
                self._project.pad_grid_anchor_s,
                sample_id,
                defaults.pad_grid_anchor_s[sample_id],
            ),
        ))

    def _publish_unloaded_pad_audio_defaults(self, sample_id: int, defaults: ProjectState) -> None:
        self._restore_legacy_timing_authority(sample_id, None)
        self._audio.set_pad_gain(sample_id, defaults.pad_gain_db[sample_id])
        self._audio.set_pad_eq(
            sample_id,
            defaults.pad_eq_low_db[sample_id],
            defaults.pad_eq_mid_db[sample_id],
            defaults.pad_eq_high_db[sample_id],
        )
        self._audio.set_pad_loop_region(
            sample_id,
            defaults.pad_loop_start_s[sample_id],
            defaults.pad_loop_end_s[sample_id],
        )
        self._audio.set_pad_key_lock(sample_id, defaults.pad_key_lock[sample_id])

    def _handle_loader_started(self, sample_id: int, _event: dict[str, object]) -> None:
        if not self._matches_load_request(sample_id, _event):
            return

        if self._project.sample_paths[sample_id] is None:
            self._project.sample_analysis[sample_id] = None
            self._clear_stem_cache(sample_id)
            self._mark_project_changed()

        self._session.loading_sample_ids.add(sample_id)
        self._session.sample_load_errors.pop(sample_id, None)
        self._session.sample_load_progress.pop(sample_id, None)
        self._session.sample_load_stage.pop(sample_id, None)

        self._clear_analysis_task_state(sample_id)

    def _handle_loader_progress(self, sample_id: int, event: dict[str, object]) -> None:
        if not self._matches_load_request(sample_id, event):
            return

        stage = event.get("stage")
        if isinstance(stage, str):
            self._session.sample_load_stage[sample_id] = stage

        percent = event.get("percent")
        if isinstance(percent, (int, float)):
            self._session.sample_load_progress[sample_id] = float(percent)

    def _handle_loader_success(self, sample_id: int, event: dict[str, object]) -> None:
        if not self._matches_load_request(sample_id, event):
            return

        self._session.loading_sample_ids.discard(sample_id)
        self._session.sample_load_errors.pop(sample_id, None)
        self._session.sample_load_progress.pop(sample_id, None)
        self._session.sample_load_stage.pop(sample_id, None)
        self._load_request_ids.pop(sample_id, None)

        pending = self._session.pending_sample_paths.pop(sample_id, None)
        cached_path = event.get("cached_path")

        target_path: str | None = cached_path if isinstance(cached_path, str) else pending
        if isinstance(target_path, str):
            target_path = self._normalize_project_path(target_path)

        previous_path = self._project.sample_paths[sample_id]
        new_assignment = target_path is not None and previous_path != target_path
        restored_assignment = target_path is not None and previous_path == target_path
        if new_assignment:
            self._project.sample_paths[sample_id] = target_path
            self._clear_stem_cache(sample_id)
            self._mark_project_changed()

        duration_s = event.get("duration_s")
        if isinstance(duration_s, float):
            self._project.sample_durations[sample_id] = duration_s

        timing_stale = event.get("timing_stale") is True
        if self._begin_accepted_restore(
            sample_id, restored_assignment=restored_assignment, timing_stale=timing_stale
        ):
            return
        # A successful ordinary load owns its timing again. Passive refresh
        # cannot clear Automatic authority while acceptance is still pending.
        self._restore_legacy_timing_authority(sample_id, None, timing_stale=timing_stale)
        # The source still belongs to this load, but a newer native request owns timing.
        # Settle source bookkeeping without replaying automatic or restored grid intent.
        if not timing_stale and new_assignment and self._on_new_sample_loaded is not None:
            detected_start = event.get("detected_loop_start_s")
            self._on_new_sample_loaded(
                sample_id, detected_start if isinstance(detected_start, float) else None
            )

        if not timing_stale:
            self._apply_loaded_analysis(sample_id, event.get("analysis"))
            self._clear_analysis_task_state(sample_id)

        if restored_assignment and self._on_restored_sample_loaded is not None:
            self._on_restored_sample_loaded(sample_id)

    def _apply_loaded_analysis(self, sample_id: int, analysis: object) -> None:
        if analysis is not None:
            self._store_sample_analysis(sample_id, analysis)
        elif (
            self._project.sample_analysis[sample_id] is not None
            or self._project.manual_bpm[sample_id] is not None
            or self._project.pad_grid_offset_samples[sample_id] != 0
            or self._project.pad_grid_anchor_s[sample_id] is not None
            or self._project.pad_timing_intent[sample_id] in {"manual", "tap"}
        ):
            self._on_pad_bpm_changed(sample_id)

    def _handle_loader_error(self, sample_id: int, event: dict[str, object]) -> None:
        if not self._matches_load_request(sample_id, event):
            return

        self._session.loading_sample_ids.discard(sample_id)
        self._session.sample_load_progress.pop(sample_id, None)
        self._session.sample_load_stage.pop(sample_id, None)
        self._session.pending_sample_paths.pop(sample_id, None)
        self._load_request_ids.pop(sample_id, None)
        self._clear_analysis_task_state(sample_id)

        if self._project.sample_paths[sample_id] is not None:
            self._reset_unloaded_pad_defaults(sample_id)
            self._on_pad_bpm_changed(sample_id)
            self._mark_project_changed()

        msg = event.get("msg")
        if isinstance(msg, str):
            self._session.sample_load_errors[sample_id] = msg

    def _handle_task_started(self, sample_id: int, event: dict[str, object]) -> None:
        task = event.get("task")
        if task == "stem_generation":
            if self._on_stem_generation_started is not None:
                self._on_stem_generation_started(sample_id)
                return

            self._session.stem_generating_sample_ids.add(sample_id)
            self._clear_stem_generation_messages(sample_id)
            return

        if task != "analysis":
            return

        if not self._matches_analysis_request(sample_id, event):
            return

        self._session.analyzing_sample_ids.add(sample_id)
        self._clear_analysis_task_messages(sample_id)

    def _handle_task_progress(self, sample_id: int, event: dict[str, object]) -> None:
        task = event.get("task")
        if task == "stem_generation":
            if self._on_stem_generation_progress is not None:
                percent = event.get("percent")
                stage = event.get("stage")
                self._on_stem_generation_progress(
                    sample_id,
                    float(percent) if isinstance(percent, (int, float)) else None,
                    stage if isinstance(stage, str) else None,
                )
                return

            if sample_id not in self._session.stem_generating_sample_ids:
                return

            stage = event.get("stage")
            if isinstance(stage, str):
                self._session.stem_generation_stage[sample_id] = stage

            percent = event.get("percent")
            if isinstance(percent, (int, float)):
                self._session.stem_generation_progress[sample_id] = float(percent)
            return

        if task != "analysis":
            return

        if not self._matches_analysis_request(sample_id, event):
            return

        stage = event.get("stage")
        if isinstance(stage, str):
            self._session.sample_analysis_stage[sample_id] = stage

        percent = event.get("percent")
        if isinstance(percent, (int, float)):
            self._session.sample_analysis_progress[sample_id] = float(percent)

    def _handle_task_success(self, sample_id: int, event: dict[str, object]) -> None:
        task = event.get("task")
        if task == "stem_generation":
            if self._on_stem_generation_success is not None:
                self._on_stem_generation_success(sample_id)
                return

            self._handle_stem_generation_success(sample_id)
            return

        if task != "analysis":
            return

        if not self._matches_analysis_request(sample_id, event):
            return

        if event.get("timing_stale") is not True:
            self._store_sample_analysis(sample_id, event.get("analysis"))
        self._clear_analysis_task_state(sample_id)

    def _handle_stem_generation_success(self, sample_id: int) -> None:
        if sample_id not in self._session.stem_generating_sample_ids:
            return

        # This legacy native event stream has no source ticket captured at admission.
        # Only the Python backend's ticket-bound completion can publish prepared stems.
        self._clear_stem_generation_state(sample_id)
        self._session.stem_generation_errors[sample_id] = (
            "Stem completion rejected because its admission ticket is missing"
        )

    def _handle_task_error(self, sample_id: int, event: dict[str, object]) -> None:
        task = event.get("task")
        if task == "stem_generation":
            if self._on_stem_generation_error is not None:
                msg = event.get("msg")
                if isinstance(msg, str):
                    self._on_stem_generation_error(sample_id, msg)
                return

            if sample_id not in self._session.stem_generating_sample_ids:
                return

            self._session.stem_generating_sample_ids.discard(sample_id)
            self._session.stem_generation_source_versions.pop(sample_id, None)
            self._session.stem_generation_progress.pop(sample_id, None)
            self._session.stem_generation_stage.pop(sample_id, None)

            msg = event.get("msg")
            if isinstance(msg, str):
                self._session.stem_generation_errors[sample_id] = msg
            return

        if task != "analysis":
            return

        if not self._matches_analysis_request(sample_id, event):
            return

        self._session.analyzing_sample_ids.discard(sample_id)
        self._analysis_request_ids.pop(sample_id, None)
        self._session.sample_analysis_progress.pop(sample_id, None)
        self._session.sample_analysis_stage.pop(sample_id, None)

        msg = event.get("msg")
        if isinstance(msg, str):
            self._session.sample_analysis_errors[sample_id] = msg

    def _store_sample_analysis(self, sample_id: int, analysis: object) -> None:
        if not isinstance(analysis, dict):
            return

        try:
            parsed = SampleAnalysis.model_validate(analysis)
        except ValidationError:
            return

        manual = self._project.manual_bpm[sample_id]
        self._restore_legacy_timing_authority(
            sample_id, normalize_bpm(manual if manual is not None else parsed.bpm)
        )
        self._project.sample_analysis[sample_id] = parsed
        self._project.pad_timing_intent[sample_id] = (
            "tap"
            if manual is not None and self._project.pad_timing_intent[sample_id] == "tap"
            else "manual"
            if manual is not None
            else "legacy"
        )
        self._on_pad_bpm_changed(sample_id)
        self._mark_project_changed()

    def _restore_legacy_timing_authority(
        self, sample_id: int, bpm: float | None, *, timing_stale: bool = False
    ) -> None:
        """Resume ordinary timing only at a successful or explicitly cleared lifecycle."""
        if not timing_stale and self._audio.pad_timing_intent(sample_id) == "automatic":
            self._audio.set_pad_bpm(sample_id, bpm)

    def _clear_restored_pad(self, sample_id: int) -> None:
        self._reset_unloaded_pad_defaults(sample_id)
        self._on_pad_bpm_changed(sample_id)

    def _wants_accepted_restore(self, sample_id: int) -> bool:
        return (
            self._project.pad_timing_intent[sample_id] == "automatic"
            and self._project.manual_bpm[sample_id] is None
        )

    def _begin_accepted_restore(
        self, sample_id: int, *, restored_assignment: bool, timing_stale: bool
    ) -> bool:
        if timing_stale or not restored_assignment or not self._wants_accepted_restore(sample_id):
            return False
        self._clear_analysis_task_state(sample_id)
        self._accepted_restore.begin(sample_id)
        return True

    @staticmethod
    def _normalize_project_path(value: str) -> str:
        cwd = Path.cwd().resolve()

        path = Path(value)
        try:
            abs_path = path if path.is_absolute() else (cwd / path)
            rel = abs_path.resolve().relative_to(cwd)
        except OSError:
            return value
        except ValueError:
            return value

        return rel.as_posix()

    def _parse_cached_sample_path(self, path: str) -> Path | None:
        # Accept both separators in persisted configs (Windows may emit backslashes).
        path = path.replace("\\", "/")

        rel = Path(path)
        if rel.is_absolute() or not rel.parts or rel.parts[0] != "samples":
            return None

        return rel

    def _schedule_restored_load(self, sample_id: int, rel: Path, *, run_analysis: bool) -> bool:
        self._session.pending_sample_paths[sample_id] = rel.as_posix()
        self._session.loading_sample_ids.add(sample_id)
        self._load_request_ids.pop(sample_id, None)

        try:
            if self._wants_accepted_restore(sample_id):
                # Reserve Automatic before startup projects BPM/grid/loop settings.
                self._audio.set_pad_timing_intent(sample_id, "automatic")
            request_id = self._audio.load_sample_async(
                sample_id,
                rel.as_posix(),
                run_analysis=run_analysis,
            )
        except RuntimeError:
            self._session.loading_sample_ids.discard(sample_id)
            self._session.pending_sample_paths.pop(sample_id, None)
            self._load_request_ids.pop(sample_id, None)
            return False

        self._record_load_request_id(sample_id, request_id)
        return True

    def _record_load_request_id(self, sample_id: int, request_id: object) -> None:
        if isinstance(request_id, bool) or not isinstance(request_id, int):
            return
        self._load_request_ids[sample_id] = request_id

    def _record_analysis_request_id(self, sample_id: int, request_id: object) -> None:
        if isinstance(request_id, bool) or not isinstance(request_id, int):
            return
        self._analysis_request_ids[sample_id] = request_id

    def _matches_load_request(self, sample_id: int, event: dict[str, object]) -> bool:
        request_id = event.get("request_id")
        if isinstance(request_id, bool) or not isinstance(request_id, int):
            return True
        return self._load_request_ids.get(sample_id) == request_id

    def _matches_analysis_request(self, sample_id: int, event: dict[str, object]) -> bool:
        request_id = event.get("request_id")
        if isinstance(request_id, bool) or not isinstance(request_id, int):
            return True
        return self._analysis_request_ids.get(sample_id) == request_id
