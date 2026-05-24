import hashlib
from pathlib import Path
from typing import TYPE_CHECKING

from flitzis_looper.controller.base import BaseController
from flitzis_looper.models import (
    STEM_COMPONENT_MASK,
    STEM_KINDS,
    STEM_MASK_DISPLAY_MODES,
    STEM_MIX_MODES,
    StemCacheEntry,
    StemFileSet,
    StemMaskDisplayMode,
    StemMixMode,
    validate_sample_id,
)

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.models import ProjectState, SessionState
    from flitzis_looper_audio import AudioEngine

STEM_CACHE_ROOT = Path("samples") / "stems"


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
    ) -> None:
        super().__init__(project, session, audio, on_project_changed)

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

        try:
            self._audio.generate_stems_async(sample_id, source_version, cache_dir)
        except (RuntimeError, ValueError) as err:
            self._clear_stem_generation_state(sample_id)
            self._session.stem_generation_errors[sample_id] = str(err)
            return False

        self._project.stem_cache[sample_id] = StemCacheEntry(
            source_version=source_version,
            cache_dir=cache_dir,
            stems=expected_stem_files(cache_dir),
            available=False,
        )
        self._mark_project_changed()
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

        if (
            enabled_stem_mask == self._session.pad_stem_enabled_mask[sample_id]
            and display_mode == self._session.pad_stem_mask_display_mode[sample_id]
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
        self._session.stem_generation_progress.pop(sample_id, None)
        self._session.stem_generation_stage.pop(sample_id, None)
