from contextlib import suppress
from pathlib import Path
from typing import TYPE_CHECKING

from pydantic import ValidationError

from flitzis_looper.models import ProjectState, SampleAnalysis, SessionState, validate_sample_id
from flitzis_looper.persistence import probe_wav_sample_rate

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper_audio import AudioEngine


class LoaderController:
    def __init__(
        self,
        project: ProjectState,
        session: SessionState,
        audio: AudioEngine,
        on_pad_bpm_changed: Callable[[int], None],
        *,
        on_project_changed: Callable[[], None] | None = None,
    ) -> None:
        self._project = project
        self._session = session
        self._audio = audio
        self._on_pad_bpm_changed = on_pad_bpm_changed
        self._on_project_changed = on_project_changed

    def _mark_project_changed(self) -> None:
        if self._on_project_changed is not None:
            self._on_project_changed()

    def _get_output_sample_rate(self) -> int | None:
        output_sample_rate_fn = getattr(self._audio, "output_sample_rate", None)
        if output_sample_rate_fn is None:
            return None

        try:
            return int(output_sample_rate_fn())
        except RuntimeError:
            return None

    def _parse_cached_sample_path(self, path: str) -> Path | None:
        if "\\" in path:
            return None

        rel = Path(path)
        if rel.is_absolute() or not rel.parts or rel.parts[0] != "samples":
            return None

        return rel

    def _is_cached_wav_usable(self, abs_path: Path, *, output_sample_rate: int) -> bool:
        if not abs_path.is_file():
            return False

        file_sample_rate = probe_wav_sample_rate(abs_path)
        return file_sample_rate is not None and file_sample_rate == output_sample_rate

    def _schedule_restored_load(self, sample_id: int, rel: Path) -> bool:
        self._session.pending_sample_paths[sample_id] = rel.as_posix()
        self._session.loading_sample_ids.add(sample_id)

        try:
            self._audio.load_sample_async(sample_id, rel.as_posix())
        except RuntimeError:
            self._session.loading_sample_ids.discard(sample_id)
            self._session.pending_sample_paths.pop(sample_id, None)
            return False

        return True

    def restore_samples_from_project_state(self) -> None:
        """Schedule async loads for cached samples referenced by `ProjectState`.

        Invalid/missing/mismatched cached WAVs are ignored by clearing the pad assignment.
        """
        output_sample_rate = self._get_output_sample_rate()
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
            if not self._is_cached_wav_usable(abs_path, output_sample_rate=output_sample_rate):
                self._clear_restored_pad(sample_id)
                changed = True
                continue

            if not self._schedule_restored_load(sample_id, rel):
                self._clear_restored_pad(sample_id)
                changed = True

        if changed:
            self._mark_project_changed()

    def _clear_restored_pad(self, sample_id: int) -> None:
        self._project.sample_paths[sample_id] = None
        self._project.sample_analysis[sample_id] = None
        self._on_pad_bpm_changed(sample_id)

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
        self._mark_project_changed()

        self._session.sample_load_errors.pop(sample_id, None)
        self._session.sample_load_progress.pop(sample_id, None)
        self._session.sample_load_stage.pop(sample_id, None)

        self._clear_analysis_task_state(sample_id)

        self._session.pending_sample_paths[sample_id] = path
        self._session.loading_sample_ids.add(sample_id)

        self._audio.load_sample_async(sample_id, path)

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

        old_path = self._project.sample_paths[sample_id]

        self._audio.unload_sample(sample_id)
        self._project.sample_paths[sample_id] = None
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

    def _handle_loader_started(self, sample_id: int, _event: dict[str, object]) -> None:
        if self._project.sample_paths[sample_id] is None:
            self._project.sample_analysis[sample_id] = None
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

        if target_path is not None and self._project.sample_paths[sample_id] != target_path:
            self._project.sample_paths[sample_id] = target_path
            self._mark_project_changed()

        self._store_sample_analysis(sample_id, event.get("analysis"))
        self._clear_analysis_task_state(sample_id)

    def _handle_loader_error(self, sample_id: int, event: dict[str, object]) -> None:
        self._session.loading_sample_ids.discard(sample_id)
        self._session.sample_load_progress.pop(sample_id, None)
        self._session.sample_load_stage.pop(sample_id, None)
        self._session.pending_sample_paths.pop(sample_id, None)
        self._clear_analysis_task_state(sample_id)

        if self._project.sample_paths[sample_id] is not None:
            self._project.sample_paths[sample_id] = None
            self._project.sample_analysis[sample_id] = None
            self._on_pad_bpm_changed(sample_id)
            self._mark_project_changed()

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
