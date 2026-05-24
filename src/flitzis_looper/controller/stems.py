import hashlib
import threading
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from queue import Empty, SimpleQueue
from typing import TYPE_CHECKING, Literal

from flitzis_looper.controller.base import BaseController
from flitzis_looper.controller.stem_generation import (
    AudioShape,
    DemucsStemGenerationBackend,
    StemGenerationBackend,
    StemGenerationRequest,
    StemGenerationResult,
    default_demucs_model_cache_dir,
)
from flitzis_looper.models import (
    STEM_COMPONENT_MASK,
    STEM_KINDS,
    STEM_MASK_DISPLAY_MODES,
    STEM_MIX_MODES,
    StemCacheEntry,
    StemFileSet,
    StemGridIndicatorState,
    StemMaskDisplayMode,
    StemMixMode,
    validate_sample_id,
)

if TYPE_CHECKING:
    from flitzis_looper.models import ProjectState, SessionState
    from flitzis_looper_audio import AudioEngine

STEM_CACHE_ROOT = Path("samples") / "stems"
type StemTaskRunner = Callable[[Callable[[], None]], None]
type _StemBackendEventType = Literal["progress", "success", "error"]


@dataclass(frozen=True, slots=True)
class _StemBackendEvent:
    sample_id: int
    source_version: str
    event_type: _StemBackendEventType
    percent: float | None = None
    stage: str | None = None
    error: str | None = None
    result: StemGenerationResult | None = None


def _start_stem_generation_thread(target: Callable[[], None]) -> None:
    thread = threading.Thread(target=target, daemon=True)
    thread.start()


def source_version_for_sample_path(
    sample_path: str, *, project_root: Path | None = None
) -> str | None:
    """Return a deterministic source-version token for a project sample path."""
    root = Path.cwd() if project_root is None else project_root
    path = Path(sample_path)
    abs_path = path if path.is_absolute() else root / path

    try:
        stat = abs_path.stat()
    except OSError:
        return None

    try:
        normalized_path = abs_path.resolve().relative_to(root.resolve()).as_posix()
    except OSError:
        normalized_path = sample_path
    except ValueError:
        normalized_path = abs_path.resolve().as_posix()

    return f"{normalized_path}|{stat.st_size}|{stat.st_mtime_ns}"


def cache_dir_for_source_version(source_version: str) -> str:
    """Return the project-relative stem cache directory for a source-version token."""
    digest = hashlib.sha256(source_version.encode("utf-8")).hexdigest()[:16]
    return (STEM_CACHE_ROOT / digest).as_posix()


def expected_stem_files(cache_dir: str) -> StemFileSet:
    """Return the expected project-relative file names for a complete stem set."""
    files = StemFileSet()
    for kind in STEM_KINDS:
        files = files.with_kind(kind, f"{cache_dir}/{kind}.wav")
    return files


class StemController(BaseController):
    """Manage offline stem cache metadata and generation task gating."""

    def __init__(
        self,
        project: ProjectState,
        session: SessionState,
        audio: AudioEngine,
        on_project_changed: Callable[[], None] | None = None,
        stem_backend: StemGenerationBackend | None = None,
        stem_task_runner: StemTaskRunner | None = None,
    ) -> None:
        super().__init__(project, session, audio, on_project_changed)
        self._stem_backend = (
            stem_backend if stem_backend is not None else DemucsStemGenerationBackend()
        )
        self._stem_task_runner = (
            stem_task_runner if stem_task_runner is not None else _start_stem_generation_thread
        )
        self._stem_generation_events: SimpleQueue[_StemBackendEvent] = SimpleQueue()
        self._on_frame_render_callbacks.append(self._poll_generation_events)

    def generate_stems_async(self, sample_id: int) -> bool:
        """Schedule offline stem generation for a stopped loaded pad when allowed."""
        validate_sample_id(sample_id)
        self._clear_stem_generation_messages(sample_id)

        blocker = self.stem_generation_block_reason(sample_id)
        if blocker is not None:
            self._session.stem_generation_errors[sample_id] = blocker
            return False

        source_version = self.source_version_for_pad(sample_id)
        if source_version is None:
            self._session.stem_generation_errors[sample_id] = (
                "Cannot generate stems because the source file is missing"
            )
            return False

        self._session.stem_generating_sample_ids.add(sample_id)
        self._session.stem_generation_source_versions[sample_id] = source_version
        cache_dir = cache_dir_for_source_version(source_version)
        target_shape = self._target_shape_for_pad(sample_id)
        if target_shape is None:
            self._clear_stem_generation_state(sample_id)
            self._session.stem_generation_errors[sample_id] = (
                "Cannot generate stems because the loaded sample shape is unavailable"
            )
            return False

        sample_path = self._project.sample_paths[sample_id]
        if sample_path is None:
            self._clear_stem_generation_state(sample_id)
            self._session.stem_generation_errors[sample_id] = (
                "Cannot generate stems without a loaded sample"
            )
            return False

        source_path = Path(sample_path)
        if not source_path.is_absolute():
            source_path = Path.cwd() / source_path

        request = StemGenerationRequest(
            sample_id=sample_id,
            source_path=source_path,
            source_version=source_version,
            cache_dir=Path.cwd() / cache_dir,
            target_shape=target_shape,
            model_cache_dir=default_demucs_model_cache_dir(project_root=Path.cwd()),
            device_policy="auto",
        )

        self._project.stem_cache[sample_id] = StemCacheEntry(
            source_version=source_version,
            cache_dir=cache_dir,
            stems=expected_stem_files(cache_dir),
            available=False,
        )
        self._mark_project_changed()
        self._stem_task_runner(lambda: self._run_stem_backend(request))
        return True

    def restore_stem_cache_from_project_state(self) -> None:
        """Validate restored stem cache metadata against current project-local files."""
        changed = False
        for sample_id, entry in enumerate(self._project.stem_cache):
            if entry is None:
                continue

            source_version = self.source_version_for_pad(sample_id)
            if source_version is None or source_version != entry.source_version:
                self._project.stem_cache[sample_id] = None
                changed = True
                continue

            available = self._entry_files_available(entry)
            if entry.available != available:
                self._project.stem_cache[sample_id] = entry.model_copy(
                    update={"available": available}
                )
                changed = True

        if changed:
            self._mark_project_changed()

    def invalidate_stem_cache(self, sample_id: int) -> None:
        """Mark cached stem metadata unavailable for a pad."""
        validate_sample_id(sample_id)
        self._clear_stem_generation_state(sample_id)
        self._clear_stem_generation_messages(sample_id)

        if self._project.stem_cache[sample_id] is not None:
            self._project.stem_cache[sample_id] = None
            self._mark_project_changed()
        self._session.pad_stem_enabled_mask[sample_id] = STEM_COMPONENT_MASK
        self._session.pad_stem_last_custom_mask[sample_id] = STEM_COMPONENT_MASK
        self._session.pad_stem_mask_display_mode[sample_id] = "all"

    def stem_mix_mode(self, sample_id: int) -> StemMixMode:
        """Return the durable stem mix preference for a pad."""
        validate_sample_id(sample_id)
        return self._project.pad_stem_mix_mode[sample_id]

    def stems_available(self, sample_id: int) -> bool:
        """Return whether current stem cache metadata is available for UI controls."""
        validate_sample_id(sample_id)
        entry = self._project.stem_cache[sample_id]
        return entry is not None and entry.available

    def stem_enabled_mask(self, sample_id: int) -> int:
        """Return the session-only enabled component-stem mask for a pad."""
        validate_sample_id(sample_id)
        return int(self._session.pad_stem_enabled_mask[sample_id])

    def stem_mask_display_mode(self, sample_id: int) -> StemMaskDisplayMode:
        """Return the session-only bottom-bar stem mask display mode."""
        validate_sample_id(sample_id)
        return self._session.pad_stem_mask_display_mode[sample_id]

    def stem_mask_controls_enabled(self, sample_id: int) -> bool:
        """Return whether selected-pad per-stem mask controls should be interactive."""
        validate_sample_id(sample_id)
        if self._project.pad_stem_mix_mode[sample_id] != "all_stems":
            return False
        return self.stems_available(sample_id)

    def stem_grid_indicator_state(self, sample_id: int) -> StemGridIndicatorState | None:
        """Return the compact stem status shown on a performance pad."""
        validate_sample_id(sample_id)
        if self._session.stem_generation_errors.get(sample_id):
            return "error"
        if sample_id in self._session.stem_generating_sample_ids:
            return "generating"
        if self.stems_available(sample_id):
            return "available"
        if self._project.sample_paths[sample_id] is not None and self._stem_generation_blocker(
            sample_id
        ):
            return "blocked"
        return None

    def set_stem_mix_mode(self, sample_id: int, mode: StemMixMode) -> bool:
        """Set the durable full-mix/all-stems mode for a pad."""
        validate_sample_id(sample_id)
        if mode not in STEM_MIX_MODES:
            msg = "stem mix mode must be full_mix or all_stems"
            raise ValueError(msg)

        if mode == "full_mix":
            if mode == self._project.pad_stem_mix_mode[sample_id]:
                return True

            try:
                self._audio.set_stem_mix_mode(sample_id, mode)
            except (RuntimeError, ValueError) as err:
                self._session.stem_generation_errors[sample_id] = f"Stem mix update failed: {err}"
                return False

            self._project.pad_stem_mix_mode[sample_id] = mode
            self._mark_project_changed()
            return True

        if mode != self._project.pad_stem_mix_mode[sample_id]:
            self._project.pad_stem_mix_mode[sample_id] = mode
            self._mark_project_changed()

        if not self.publish_stem_mix_mode_if_available(sample_id):
            return False

        return self.publish_stem_enabled_mask_if_available(sample_id)

    def set_stem_enabled_mask(
        self,
        sample_id: int,
        enabled_stem_mask: int,
        display_mode: StemMaskDisplayMode = "custom",
    ) -> bool:
        """Set the session-only enabled component-stem mask for a pad."""
        validate_sample_id(sample_id)
        if enabled_stem_mask < 0 or enabled_stem_mask & ~STEM_COMPONENT_MASK:
            msg = "stem enabled mask must contain only component stems"
            raise ValueError(msg)
        if display_mode not in STEM_MASK_DISPLAY_MODES:
            msg = "stem mask display mode must be custom, instrumental, or all"
            raise ValueError(msg)

        current_display_mode = self._session.pad_stem_mask_display_mode[sample_id]
        if display_mode == "custom":
            self._session.pad_stem_last_custom_mask[sample_id] = enabled_stem_mask
        elif current_display_mode == "custom":
            self._session.pad_stem_last_custom_mask[sample_id] = (
                self._session.pad_stem_enabled_mask[sample_id]
            )

        if (
            enabled_stem_mask == self._session.pad_stem_enabled_mask[sample_id]
            and display_mode == current_display_mode
        ):
            return True

        self._session.pad_stem_enabled_mask[sample_id] = enabled_stem_mask
        self._session.pad_stem_mask_display_mode[sample_id] = display_mode
        return self.publish_stem_enabled_mask_if_available(sample_id)

    def publish_stem_mix_mode_if_available(self, sample_id: int) -> bool:
        """Publish all-stems mode to Rust when current prepared stems are available."""
        validate_sample_id(sample_id)
        if self._project.pad_stem_mix_mode[sample_id] != "all_stems":
            return True

        entry = self._project.stem_cache[sample_id]
        if entry is None or not entry.available:
            return True

        source_version = self.source_version_for_pad(sample_id)
        if source_version is None or source_version != entry.source_version:
            return True

        try:
            self._audio.set_stem_mix_mode(sample_id, "all_stems", source_version)
        except (RuntimeError, ValueError) as err:
            self._session.stem_generation_errors[sample_id] = f"Stem mix update failed: {err}"
            return False

        return True

    def publish_stem_enabled_mask_if_available(self, sample_id: int) -> bool:
        """Publish the session stem mask to Rust when current prepared stems are available."""
        validate_sample_id(sample_id)
        if self._project.pad_stem_mix_mode[sample_id] != "all_stems":
            return True

        entry = self._project.stem_cache[sample_id]
        if entry is None or not entry.available:
            return True

        source_version = self.source_version_for_pad(sample_id)
        if source_version is None or source_version != entry.source_version:
            return True

        try:
            self._audio.set_stem_enabled_mask(
                sample_id,
                self._session.pad_stem_enabled_mask[sample_id],
                source_version,
            )
        except (RuntimeError, ValueError) as err:
            self._session.stem_generation_errors[sample_id] = f"Stem mask update failed: {err}"
            return False

        return True

    def source_version_for_pad(self, sample_id: int) -> str | None:
        """Return the current loaded source-version token for a pad."""
        validate_sample_id(sample_id)
        sample_path = self._project.sample_paths[sample_id]
        if sample_path is None:
            return None
        return source_version_for_sample_path(sample_path)

    def is_stem_generation_running(self, sample_id: int) -> bool:
        """Return whether a pad has an in-flight stem generation task."""
        validate_sample_id(sample_id)
        return sample_id in self._session.stem_generating_sample_ids

    def stem_generation_error(self, sample_id: int) -> str | None:
        """Return the last stem generation error for a pad."""
        validate_sample_id(sample_id)
        return self._session.stem_generation_errors.get(sample_id)

    def stem_generation_progress(self, sample_id: int) -> float | None:
        """Return best-effort stem generation progress for a pad."""
        validate_sample_id(sample_id)
        value = self._session.stem_generation_progress.get(sample_id)
        return float(value) if value is not None else None

    def stem_generation_stage(self, sample_id: int) -> str | None:
        """Return the last reported stem generation stage for a pad."""
        validate_sample_id(sample_id)
        return self._session.stem_generation_stage.get(sample_id)

    def stem_generation_block_reason(self, sample_id: int) -> str | None:
        """Return the current non-I/O blocker for performer stem generation."""
        validate_sample_id(sample_id)
        return self._stem_generation_blocker(sample_id)

    def _poll_generation_events(self) -> None:
        """Drain Python backend generation events from background worker threads."""
        while True:
            try:
                event = self._stem_generation_events.get_nowait()
            except Empty:
                return

            if not self._is_current_generation(event.sample_id, event.source_version):
                continue

            if event.event_type == "progress":
                self._handle_stem_generation_progress(
                    event.sample_id,
                    event.percent,
                    event.stage,
                )
            elif event.event_type == "success":
                self._record_generation_result(event.sample_id, event.result)
                self._handle_stem_generation_success(event.sample_id)
            elif event.error is not None:
                self._handle_stem_generation_error(event.sample_id, event.error)

    def _handle_stem_generation_started(self, sample_id: int) -> None:
        """Apply a stem-generation start event from a backend event source."""
        validate_sample_id(sample_id)
        self._session.stem_generating_sample_ids.add(sample_id)
        self._clear_stem_generation_messages(sample_id)

    def _handle_stem_generation_progress(
        self,
        sample_id: int,
        percent: float | None,
        stage: str | None,
    ) -> None:
        """Apply a stem-generation progress event from a backend event source."""
        validate_sample_id(sample_id)
        if sample_id not in self._session.stem_generating_sample_ids:
            return

        if stage is not None:
            self._session.stem_generation_stage[sample_id] = stage
        if percent is not None:
            self._session.stem_generation_progress[sample_id] = float(percent)

    def _handle_stem_generation_success(self, sample_id: int) -> None:
        """Publish completed stems when the pad and source version are still eligible."""
        validate_sample_id(sample_id)
        if sample_id not in self._session.stem_generating_sample_ids:
            return

        source_version = self._session.stem_generation_source_versions.get(sample_id)
        self._clear_stem_generation_state(sample_id)
        self._session.stem_generation_progress.pop(sample_id, None)
        self._session.stem_generation_stage.pop(sample_id, None)
        if source_version is None:
            return

        entry = self._project.stem_cache[sample_id]
        if entry is None or entry.source_version != source_version:
            return

        current_source_version = self.source_version_for_pad(sample_id)
        if current_source_version != source_version:
            self._project.stem_cache[sample_id] = None
            self._mark_project_changed()
            return

        if sample_id in self._session.active_sample_ids:
            return

        if not self._entry_files_available(entry):
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

    def _handle_stem_generation_error(self, sample_id: int, message: str) -> None:
        """Apply a stem-generation failure event from a backend event source."""
        validate_sample_id(sample_id)
        if sample_id not in self._session.stem_generating_sample_ids:
            return

        self._session.stem_generating_sample_ids.discard(sample_id)
        self._session.stem_generation_source_versions.pop(sample_id, None)
        self._session.stem_generation_progress.pop(sample_id, None)
        self._session.stem_generation_stage.pop(sample_id, None)
        self._session.stem_generation_errors[sample_id] = message

    def _stem_generation_blocker(self, sample_id: int) -> str | None:
        if self._project.sample_paths[sample_id] is None:
            return "Cannot generate stems without a loaded sample"
        if sample_id in self._session.active_sample_ids:
            return "Cannot generate stems while the pad is playing"
        if sample_id in self._session.loading_sample_ids:
            return "Cannot generate stems while the pad is loading"
        if sample_id in self._session.analyzing_sample_ids:
            return "Cannot generate stems while another pad task is running"
        if sample_id in self._session.stem_generating_sample_ids:
            return "Stem generation is already running for this pad"
        return None

    def _entry_files_available(self, entry: StemCacheEntry) -> bool:
        for kind in STEM_KINDS:
            path = entry.stems.path_for(kind)
            if path is None:
                return False
            if not (Path.cwd() / Path(path)).is_file():
                return False
        return True

    def _clear_stem_generation_state(self, sample_id: int) -> None:
        self._session.stem_generating_sample_ids.discard(sample_id)
        self._session.stem_generation_source_versions.pop(sample_id, None)

    def _clear_stem_generation_messages(self, sample_id: int) -> None:
        self._session.stem_generation_errors.pop(sample_id, None)
        self._session.stem_generation_diagnostics.pop(sample_id, None)
        self._session.stem_generation_progress.pop(sample_id, None)
        self._session.stem_generation_stage.pop(sample_id, None)

    def _target_shape_for_pad(self, sample_id: int) -> AudioShape | None:
        fn = getattr(self._audio, "loaded_sample_shape", None)
        if fn is None:
            return self._duration_based_target_shape(sample_id)

        try:
            raw_shape = fn(sample_id)
        except (RuntimeError, TypeError, ValueError):
            return self._duration_based_target_shape(sample_id)

        if not isinstance(raw_shape, tuple) or len(raw_shape) != 3:
            return self._duration_based_target_shape(sample_id)
        sample_rate_hz, channels, frame_count = raw_shape
        if (
            not isinstance(sample_rate_hz, int)
            or not isinstance(channels, int)
            or not isinstance(frame_count, int)
        ):
            return self._duration_based_target_shape(sample_id)
        if sample_rate_hz <= 0 or channels <= 0 or frame_count <= 0:
            return self._duration_based_target_shape(sample_id)
        return AudioShape(
            sample_rate_hz=sample_rate_hz,
            channels=channels,
            frame_count=frame_count,
        )

    def _duration_based_target_shape(self, sample_id: int) -> AudioShape | None:
        sample_rate_hz = self._output_sample_rate_hz()
        duration_s = self._project.sample_durations[sample_id]
        if sample_rate_hz is None or sample_rate_hz <= 0 or duration_s is None:
            return None
        frame_count = round(duration_s * sample_rate_hz)
        if frame_count <= 0:
            return None
        return AudioShape(sample_rate_hz=sample_rate_hz, channels=1, frame_count=frame_count)

    def _run_stem_backend(self, request: StemGenerationRequest) -> None:
        def report_progress(percent: float, stage: str) -> None:
            self._stem_generation_events.put(
                _StemBackendEvent(
                    sample_id=request.sample_id,
                    source_version=request.source_version,
                    event_type="progress",
                    percent=percent,
                    stage=stage,
                )
            )

        try:
            result = self._stem_backend.generate(request, report_progress)
        except (OSError, RuntimeError) as err:
            self._stem_generation_events.put(
                _StemBackendEvent(
                    sample_id=request.sample_id,
                    source_version=request.source_version,
                    event_type="error",
                    error=str(err),
                )
            )
            return

        self._stem_generation_events.put(
            _StemBackendEvent(
                sample_id=request.sample_id,
                source_version=request.source_version,
                event_type="success",
                result=result,
            )
        )

    def _is_current_generation(self, sample_id: int, source_version: str) -> bool:
        return (
            sample_id in self._session.stem_generating_sample_ids
            and self._session.stem_generation_source_versions.get(sample_id) == source_version
        )

    def _record_generation_result(
        self,
        sample_id: int,
        result: StemGenerationResult | None,
    ) -> None:
        if result is None:
            return
        if result.diagnostic:
            self._session.stem_generation_diagnostics[sample_id] = result.diagnostic
        elif result.cpu_fallback:
            self._session.stem_generation_diagnostics[sample_id] = (
                "Stem generation completed on CPU after CUDA fallback"
            )

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
