import hashlib
import json
import os
import re
from pathlib import Path

from flitzis_looper.models import STEM_KINDS, StemCacheEntry, StemFileSet, validate_sample_id
from flitzis_looper.project_materials import original_asset, resolve_asset

STEM_CACHE_ROOT = Path("samples") / "stems"
STEM_SET_MARKER_NAME = ".complete.json"
STEM_SET_MARKER_SCHEMA = "stem-set-sha256-v1"


def source_version_for_sample_path(
    sample_path: str, *, project_root: Path | None = None
) -> str | None:
    """Hash the complete source file, rejecting changes during the streaming read."""
    root = Path.cwd() if project_root is None else project_root
    path = Path(sample_path)
    abs_path = path if path.is_absolute() else root / path

    try:
        with abs_path.open("rb") as source:
            before = os.fstat(source.fileno())
            digest = hashlib.file_digest(source, "sha256").hexdigest()
            source.seek(0)
            verification_digest = hashlib.file_digest(source, "sha256").hexdigest()
            after = os.fstat(source.fileno())
        current = abs_path.stat()
    except OSError:
        return None

    def identity(stat: os.stat_result) -> tuple[int, int, int, int]:
        return (stat.st_dev, stat.st_ino, stat.st_size, stat.st_mtime_ns)

    if (
        digest != verification_digest
        or identity(before) != identity(after)
        # On Windows fstat and path stat can expose different ctime semantics.
        or before.st_ctime_ns != after.st_ctime_ns
        or identity(after) != identity(current)
    ):
        return None

    try:
        normalized_path = abs_path.resolve().relative_to(root.resolve()).as_posix()
    except OSError:
        normalized_path = sample_path
    except ValueError:
        normalized_path = abs_path.resolve().as_posix()

    return f"{normalized_path}|sha256-v1:{digest}"


def cache_dir_for_sample_id(sample_id: int, sample_path: str | None = None) -> str:
    """Return the project-relative stem cache directory for a pad."""
    validate_sample_id(sample_id)
    if sample_path is not None:
        asset = original_asset(sample_path)
        if asset.material_id is not None:
            return (asset.path.parent.parent / "stems").relative_to(Path.cwd()).as_posix()
    return (STEM_CACHE_ROOT / f"#{sample_id + 1}").as_posix()


def _safe_stem_cache_dir_path(cache_dir: str) -> Path | None:
    try:
        asset = resolve_asset(cache_dir)
    except (OSError, ValueError):
        return None
    return asset.path if asset.kind in {"stem_directory", "stem_artifact"} else None


def cache_dir_matches_sample_id(
    sample_id: int, cache_dir: str, sample_path: str | None = None
) -> bool:
    """Accept a legacy pad set or its immutable published generation only."""
    try:
        root = Path(cache_dir_for_sample_id(sample_id, sample_path))
        resolved = resolve_asset(cache_dir)
        if resolved.kind != "stem_directory":
            return False
        candidate = resolved.path.relative_to(Path.cwd())
    except (OSError, ValueError):
        return False
    return candidate == root or (
        candidate.parent == root
        and re.fullmatch(r"\.ready-[0-9a-f]{32}", candidate.name) is not None
    )


def expected_stem_files(cache_dir: str) -> StemFileSet:
    """Return the expected project-relative file names for a complete stem set."""
    files = StemFileSet()
    for kind in STEM_KINDS:
        files = files.with_kind(kind, f"{cache_dir}/{kind}.wav")
    return files


def _validated_cache_path(entry: StemCacheEntry) -> Path:
    target = _safe_stem_cache_dir_path(entry.cache_dir)
    if target is None or entry.stems != expected_stem_files(entry.cache_dir):
        msg = "Stem artifacts must use the expected project-local cache paths"
        raise ValueError(msg)
    return target


def _file_digest(path: Path) -> str:
    relative = path.absolute().relative_to(Path.cwd().absolute())
    target = _safe_stem_cache_dir_path(relative.as_posix())
    if target is None:
        message = "Stem artifact path contains an unsafe reference"
        raise ValueError(message)
    with target.open("rb") as artifact:
        return hashlib.file_digest(artifact, "sha256").hexdigest()


def promote_generation_artifacts(entry: StemCacheEntry, generation_dir: Path) -> StemCacheEntry:
    """Atomically publish an immutable complete generation with its marker last."""
    target = _validated_cache_path(entry)
    private = _validated_generation_path(generation_dir)
    if private.parent not in {target, target.parent}:
        msg = "Generation artifacts do not belong to the pad's cache directory"
        raise ValueError(msg)
    if any(not (private / f"{kind}.wav").is_file() for kind in STEM_KINDS):
        msg = "Stem generation completed but cache files are incomplete"
        raise OSError(msg)
    digests = {kind: _file_digest(private / f"{kind}.wav") for kind in STEM_KINDS}
    published = private.with_name(private.name.replace(".generation-", ".ready-", 1))
    if published.exists():
        message = "Stem generation already has a published owner"
        raise FileExistsError(message)
    marker_temp = private / STEM_SET_MARKER_NAME
    with marker_temp.open("x", encoding="utf-8") as marker:
        json.dump(
            {
                "schema": STEM_SET_MARKER_SCHEMA,
                "source_version": entry.source_version,
                "stems": digests,
            },
            marker,
        )
        marker.flush()
        os.fsync(marker.fileno())
    private.rename(published)
    cache_dir = published.relative_to(Path.cwd()).as_posix()
    return entry.model_copy(
        update={"cache_dir": cache_dir, "stems": expected_stem_files(cache_dir)}
    )


def verified_stem_cache_available(entry: StemCacheEntry) -> bool:
    """Validate the complete-set marker and every canonical stem content digest."""
    try:
        target = _validated_cache_path(entry)
        marker_path = target / STEM_SET_MARKER_NAME
        if _safe_stem_cache_dir_path(marker_path.relative_to(Path.cwd()).as_posix()) is None:
            return False
        marker: object = json.loads(marker_path.read_text(encoding="utf-8"))
        expected = {
            "schema": STEM_SET_MARKER_SCHEMA,
            "source_version": entry.source_version,
            "stems": {kind: _file_digest(target / f"{kind}.wav") for kind in STEM_KINDS},
        }
    except OSError, ValueError, UnicodeDecodeError:
        return False
    return marker == expected


def _validated_generation_path(cache_dir: Path) -> Path:
    relative = cache_dir.absolute().relative_to(Path.cwd().absolute())
    target = _safe_stem_cache_dir_path(relative.as_posix())
    if target is None or re.fullmatch(r"\.generation-[0-9a-f]{32}", target.name) is None:
        msg = "Generation artifact path is outside its private cache directory"
        raise ValueError(msg)
    return target
