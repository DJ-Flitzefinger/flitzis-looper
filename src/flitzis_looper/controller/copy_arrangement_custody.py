"""Independent durable A/B/C readers, separate from assignment and retirement owners."""

from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from flitzis_looper.project_materials import original_asset, resolve_asset

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
    from flitzis_looper.copy_snapshot import CopySnapshot
    from flitzis_looper_audio import ProjectAssetLease


@dataclass(slots=True)
class CopyJournalCustody:
    """Retain one independent original/stem reader for each immutable musical seal."""

    readers: list[ProjectAssetLease] = field(default_factory=list)
    acquired: bool = False
    _owned: set[tuple[int, str]] = field(default_factory=set)

    def acquire(
        self, assets: ProjectAssetLifecycle, root: Path, snapshots: tuple[CopySnapshot, ...]
    ) -> None:
        """Acquire all required paths or leave acquired readers owned for safe retry."""
        if self.acquired:
            return
        errors: list[str] = []
        for index, seal in enumerate(snapshots):
            try:
                self._acquire_snapshot(assets, root, seal, index)
            except (OSError, RuntimeError, TypeError, ValueError) as error:
                errors.append(str(error))
        if errors:
            # Retain every available reader before ordinary cleanup can run, even
            # when another original/stem path is missing.
            message = "; ".join(errors)
            raise RuntimeError(message)
        self.acquired = True

    def _acquire_snapshot(
        self, assets: ProjectAssetLifecycle, root: Path, seal: CopySnapshot, index: int
    ) -> None:
        data = seal.projected()
        references = [("original", data.source_path)]
        if data.stem_cache is not None:
            references.append(("stems", data.stem_cache.cache_dir))
        errors: list[str] = []
        for kind, reference in references:
            try:
                self._acquire_reference(assets, root, index, kind, reference)
            except (OSError, RuntimeError, TypeError, ValueError) as error:
                errors.append(str(error))
        if errors:
            message = "; ".join(errors)
            raise RuntimeError(message)

    def _acquire_reference(
        self, assets: ProjectAssetLifecycle, root: Path, index: int, kind: str, reference: str
    ) -> None:
        if (index, kind) in self._owned:
            return
        if kind == "original":
            path = original_asset(reference, project_root=root).path
            exists = path.is_file()
        else:
            asset = resolve_asset(reference, project_root=root)
            path = asset.path
            exists = asset.kind == "stem_directory" and path.is_dir()
        if not exists:
            message = f"Copy journal {kind} custody is missing or unrecognized: {path}"
            raise FileNotFoundError(message)
        self.readers.append(assets.acquire(path))
        self._owned.add((index, kind))

    def release(self) -> None:
        """Release readers only after the terminal journal and live owners are settled."""
        while self.readers:
            self.readers[-1].release()
            self.readers.pop()
        self.acquired = False
        self._owned.clear()
