import hashlib
import json
import os
import shutil
from pathlib import Path

from flitzis_looper.models import STEM_KINDS, StemCacheEntry, StemFileSet, validate_sample_id

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


def cache_dir_for_sample_id(sample_id: int) -> str:
    """Return the project-relative stem cache directory for a pad."""
    validate_sample_id(sample_id)
    return (STEM_CACHE_ROOT / f"#{sample_id + 1}").as_posix()


def _safe_stem_cache_dir_path(cache_dir: str) -> Path | None:
    rel = Path(cache_dir)
    if rel.is_absolute():
        return None

    root = (Path.cwd() / STEM_CACHE_ROOT).resolve(strict=False)
    target = (Path.cwd() / rel).resolve(strict=False)

    try:
        target.relative_to(root)
    except ValueError:
        return None

    if target == root:
        return None

    return target


def delete_stem_cache_dirs(sample_id: int, *cache_dirs: str | None) -> bool:
    """Delete known project-local stem cache directories for a pad."""
    validate_sample_id(sample_id)
    deleted = False
    seen: set[Path] = set()
    for cache_dir in (*cache_dirs, cache_dir_for_sample_id(sample_id)):
        if cache_dir is None:
            continue
        target = _safe_stem_cache_dir_path(cache_dir)
        if target is None or target in seen:
            continue
        seen.add(target)
        if target.exists():
            shutil.rmtree(target)
            deleted = True
    return deleted


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
    path.resolve().relative_to((Path.cwd() / STEM_CACHE_ROOT).resolve())
    with path.open("rb") as artifact:
        return hashlib.file_digest(artifact, "sha256").hexdigest()


def promote_generation_artifacts(entry: StemCacheEntry, generation_dir: Path) -> None:
    """Promote one complete private generation, writing its integrity marker last."""
    target = _validated_cache_path(entry)
    private = _validated_generation_path(generation_dir)
    if private.parent != target:
        msg = "Generation artifacts do not belong to the pad's cache directory"
        raise ValueError(msg)
    if any(not (private / f"{kind}.wav").is_file() for kind in STEM_KINDS):
        msg = "Stem generation completed but cache files are incomplete"
        raise OSError(msg)
    digests = {kind: _file_digest(private / f"{kind}.wav") for kind in STEM_KINDS}
    marker = target / STEM_SET_MARKER_NAME
    marker.unlink(missing_ok=True)
    for kind in STEM_KINDS:
        (private / f"{kind}.wav").replace(target / f"{kind}.wav")
    marker_temp = private / STEM_SET_MARKER_NAME
    marker_temp.write_text(
        json.dumps({
            "schema": STEM_SET_MARKER_SCHEMA,
            "source_version": entry.source_version,
            "stems": digests,
        }),
        encoding="utf-8",
    )
    marker_temp.replace(marker)


def verified_stem_cache_available(entry: StemCacheEntry) -> bool:
    """Validate the complete-set marker and every canonical stem content digest."""
    try:
        target = _validated_cache_path(entry)
        marker_path = target / STEM_SET_MARKER_NAME
        marker_path.resolve().relative_to(target)
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
    relative = cache_dir.resolve().relative_to(Path.cwd().resolve())
    target = _safe_stem_cache_dir_path(relative.as_posix())
    if target is None or not target.name.startswith(".generation-"):
        msg = "Generation artifact path is outside its private cache directory"
        raise ValueError(msg)
    return target


def discard_generation_artifacts(cache_dir: Path) -> None:
    """Delete only a completed job's checked project-local private artifact directory."""
    target = _validated_generation_path(cache_dir)
    if target.exists():
        shutil.rmtree(target)
