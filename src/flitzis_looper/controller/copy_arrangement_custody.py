"""Independent durable A/B/C readers, separate from assignment and retirement owners."""

import hashlib
from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from flitzis_looper.controller.pair_arrangement_journal import (
    strict_json_object,
    validated_project_image,
)
from flitzis_looper.project_materials import original_asset, resolve_asset

if TYPE_CHECKING:
    from collections.abc import Iterable, Iterator
    from pathlib import Path

    from flitzis_looper.controller.asset_lifecycle import (
        ProjectAssetLifecycle,
    )
    from flitzis_looper.copy_snapshot import CopySnapshot
    from flitzis_looper_audio import ProjectAssetLease


def journal_project_images(encoded: Iterable[str]) -> Iterator[str]:
    """Yield original raw full images from already validated retained journal histories."""
    for text in encoded:
        record = strict_json_object(text)
        for name in ("captured_project_image_json", "project_image_json"):
            image = record.get(name)
            if image is not None:
                if not isinstance(image, str):
                    message = "Journal full project image must retain its raw JSON text"
                    raise TypeError(message)
                yield image


@dataclass(slots=True)
class CopyJournalCustody:
    """Retain one independent original/stem reader for each immutable musical seal."""

    readers: list[ProjectAssetLease] = field(default_factory=list)
    acquired: bool = False
    _owned: dict[tuple[int, str, Path], ProjectAssetLease] = field(default_factory=dict)
    _project_images: set[bytes] = field(default_factory=set)

    def acquire(
        self,
        assets: ProjectAssetLifecycle,
        root: Path,
        snapshots: tuple[CopySnapshot, ...],
        *,
        project_images: Iterable[str] = (),
    ) -> None:
        """Acquire all required paths or leave acquired readers owned for safe retry."""
        self.acquired = False
        errors: list[str] = []
        for index, seal in enumerate(snapshots):
            try:
                self._acquire_snapshot(assets, root, seal, index)
            except (OSError, RuntimeError, TypeError, ValueError) as error:
                errors.append(str(error))
        try:
            for raw_image in project_images:
                try:
                    self._acquire_project_image(assets, root, raw_image)
                except (OSError, RuntimeError, TypeError, ValueError) as error:
                    errors.append(str(error))
        except (OSError, RuntimeError, TypeError, ValueError) as error:
            errors.append(str(error))
        if errors:
            # Retain every available reader before ordinary cleanup can run, even
            # when another original/stem path is missing.
            message = "; ".join(errors)
            raise RuntimeError(message)
        self.acquired = True

    def _acquire_project_image(
        self, assets: ProjectAssetLifecycle, root: Path, raw_image: str
    ) -> None:
        fingerprint = hashlib.sha256(raw_image.encode("utf-8")).digest()
        if fingerprint in self._project_images:
            return
        validated_project_image(raw_image)
        self._project_images.add(fingerprint)

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
        if kind == "original":
            path = original_asset(reference, project_root=root).path
            exists = path.is_file()
        else:
            asset = resolve_asset(reference, project_root=root)
            path = asset.path
            exists = asset.kind == "stem_directory" and path.is_dir()
        if (index, kind, path) in self._owned:
            return
        if not exists:
            message = f"Copy journal {kind} custody is missing or unrecognized: {path}"
            raise FileNotFoundError(message)
        lease = assets.acquire(path)
        self.readers.append(lease)
        self._owned[index, kind, path] = lease

    def release(self) -> None:
        """Release readers only after the terminal journal and live owners are settled."""
        while self.readers:
            self.readers[-1].release()
            self.readers.pop()
        self.acquired = False
        self._owned.clear()
        self._project_images.clear()
