from contextlib import suppress
from pathlib import Path
from typing import TYPE_CHECKING

from pydantic import ValidationError

from flitzis_looper.controller.base import BaseController
from flitzis_looper.controller.stems import source_version_for_sample_path
from flitzis_looper.models import (
    STEM_KINDS,
    ProjectState,
    SampleAnalysis,
    SessionState,
    validate_sample_id,
)

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.models import StemCacheEntry
    from flitzis_looper_audio import AudioEngine


class LoaderController(BaseController):
    def __init__(
        self,
        project: ProjectState,
        session: SessionState,
        audio: AudioEngine,
        on_pad_bpm_changed: Callable[[int], None],
        on_project_changed: Callable[[], None] | None = None,
        on_stem_generation_started: Callable[[int], None] | None = None,
        on_stem_generation_progress: Callable[[int, float | None, str | None], None]
        | None = None,
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

        self._audio.load_sample_async(sample_id, path, run_analysis=True)

    def unload_sample(self, sample_id: int) -> None:
        """Stop playback and unload a sample slot."""
        validate_sample_id(sample_id)
        self._session.active_sample_ids.discard(sample_id)
        self._session.loading_sample_ids.discard(sample_id)
        self._session.pending_sample_paths.pop(sample_id, None)
        self._session.sample_load_progress.pop(sample_id, None)
        self._session.sample_load_stage.pop(sample_id, None)
        self._session.sample_load_errors.pop(sample_id, None)

        self._clear_analysis_task_state(sample_id)
        self._clear_stem_generation_state(sample_id)

        old_path = self._project.sample_paths[sample_id]
        if self._on_stems_deleted is not None:
            self._on_stems_deleted(sample_id)
        else:
            self._clear_stem_cache(sample_id)

        self._audio.unload_sample(sample_id)
        self._project.sample_paths[sample_id] = None
        self._project.sample_durations[sample_id] = None
        self._project.sample_analysis[sample_id] = None
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
        self._session.analyzing_sample_ids.add(sample_id)

        try:
            self._audio.analyze_sample_async(sample_id)
        except RuntimeError as err:
            self._session.analyzing_sample_ids.discard(sample_id)
            self._session.sample_analysis_errors[sample_id] = str(err)

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

    def _handle_loader_started(self, sample_id: int, _event: dict[str, object]) -> None:
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
        cached_path = event.get("cached_path")

        target_path: str | None = cached_path if isinstance(cached_path, str) else pending
        if isinstance(target_path, str):
            target_path = self._normalize_project_path(target_path)

        if target_path is not None and self._project.sample_paths[sample_id] != target_path:
            self._project.sample_paths[sample_id] = target_path
            self._clear_stem_cache(sample_id)
            self._mark_project_changed()

        duration_s = event.get("duration_s")
        if isinstance(duration_s, float):
            self._project.sample_durations[sample_id] = duration_s

        # If analysis is provided in the event (from normal loading), store it
        analysis = event.get("analysis")

        if analysis is not None:
            self._store_sample_analysis(sample_id, analysis)
        # If no analysis in event (from restoration), keep existing analysis from project state
        elif self._project.sample_analysis[sample_id] is not None:
            # Analysis was already restored from project state, just trigger BPM update
            self._on_pad_bpm_changed(sample_id)

        self._clear_analysis_task_state(sample_id)

    def _handle_loader_error(self, sample_id: int, event: dict[str, object]) -> None:
        self._session.loading_sample_ids.discard(sample_id)
        self._session.sample_load_progress.pop(sample_id, None)
        self._session.sample_load_stage.pop(sample_id, None)
        self._session.pending_sample_paths.pop(sample_id, None)
        self._clear_analysis_task_state(sample_id)

        if self._project.sample_paths[sample_id] is not None:
            self._project.sample_paths[sample_id] = None
            self._project.sample_durations[sample_id] = None
            self._project.sample_analysis[sample_id] = None
            self._clear_stem_cache(sample_id)
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

        self._store_sample_analysis(sample_id, event.get("analysis"))
        self._clear_analysis_task_state(sample_id)

    def _handle_stem_generation_success(self, sample_id: int) -> None:
        if sample_id not in self._session.stem_generating_sample_ids:
            return

        source_version = self._session.stem_generation_source_versions.get(sample_id)
        self._clear_stem_generation_state(sample_id)
        if source_version is None:
            return

        entry = self._project.stem_cache[sample_id]
        if entry is None or entry.source_version != source_version:
            return

        current_source_version = self._source_version_for_pad(sample_id)
        if current_source_version != source_version:
            self._project.stem_cache[sample_id] = None
            self._mark_project_changed()
            return

        if sample_id in self._session.active_sample_ids:
            return

        if not self._stem_cache_files_available(entry):
            self._session.stem_generation_errors[sample_id] = (
                "Stem generation completed but cache files are incomplete"
            )
            return

        try:
            self._audio.publish_prepared_stems(sample_id, source_version, entry.cache_dir)
        except (RuntimeError, ValueError) as err:
            self._session.stem_generation_errors[sample_id] = (
                f"Stem generation completed but publication failed: {err}"
            )
        else:
            if not entry.available:
                self._project.stem_cache[sample_id] = entry.model_copy(update={"available": True})
                self._mark_project_changed()
            self._publish_all_stems_mode_if_preferred(sample_id, source_version)

    def _publish_all_stems_mode_if_preferred(self, sample_id: int, source_version: str) -> None:
        if self._project.pad_stem_mix_mode[sample_id] != "all_stems":
            return

        try:
            self._audio.set_stem_mix_mode(sample_id, "all_stems", source_version)
            self._audio.set_stem_enabled_mask(
                sample_id,
                self._session.pad_stem_enabled_mask[sample_id],
                source_version,
            )
        except (RuntimeError, ValueError) as err:
            self._session.stem_generation_errors[sample_id] = (
                f"Stem generation completed but mix update failed: {err}"
            )

    def _source_version_for_pad(self, sample_id: int) -> str | None:
        sample_path = self._project.sample_paths[sample_id]
        if sample_path is None:
            return None
        return source_version_for_sample_path(sample_path)

    def _stem_cache_files_available(self, entry: StemCacheEntry) -> bool:
        for kind in STEM_KINDS:
            path = entry.stems.path_for(kind)
            if path is None:
                return False
            if not (Path.cwd() / Path(path)).is_file():
                return False
        return True

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

        self._session.analyzing_sample_ids.discard(sample_id)
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

        self._project.sample_analysis[sample_id] = parsed
        self._on_pad_bpm_changed(sample_id)
        self._mark_project_changed()

    def _clear_restored_pad(self, sample_id: int) -> None:
        self._project.sample_paths[sample_id] = None
        self._project.sample_durations[sample_id] = None
        self._project.sample_analysis[sample_id] = None
        self._clear_stem_cache(sample_id)
        self._on_pad_bpm_changed(sample_id)

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

        try:
            self._audio.load_sample_async(sample_id, rel.as_posix(), run_analysis=run_analysis)
        except RuntimeError:
            self._session.loading_sample_ids.discard(sample_id)
            self._session.pending_sample_paths.pop(sample_id, None)
            return False

        return True
