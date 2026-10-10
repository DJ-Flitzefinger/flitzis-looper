"""Control-plane access to the native, typed project asset namespace."""

from dataclasses import dataclass
from pathlib import Path

from flitzis_looper_audio import resolve_project_asset


@dataclass(frozen=True)
class ProjectAssetPath:
    kind: str
    path: Path
    material_id: str | None


def resolve_asset(value: str | Path, *, project_root: Path | None = None) -> ProjectAssetPath:
    """Validate lexical containment and artifact kind before any normalization."""
    root = Path.cwd() if project_root is None else project_root
    kind, path, material_id = resolve_project_asset(str(root / "samples"), str(value))
    return ProjectAssetPath(kind, Path(path), material_id)


def original_asset(value: str | Path, *, project_root: Path | None = None) -> ProjectAssetPath:
    asset = resolve_asset(value, project_root=project_root)
    if asset.kind != "original":
        message = "Source assignment must name a typed original"
        raise ValueError(message)
    return asset
