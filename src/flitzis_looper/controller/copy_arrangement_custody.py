"""Independent durable A/B/C readers, separate from assignment and retirement owners."""

import hashlib
from dataclasses import dataclass, field
from typing import TYPE_CHECKING

from flitzis_looper.controller.pair_arrangement_journal import (
    strict_json_object,
    validated_project_image,
)
from flitzis_looper.project_materials import original_asset, resolve_asset
from flitzis_looper.source_beatmap_intent import SourceBeatmapReference

if TYPE_CHECKING:
    from collections.abc import Iterable, Iterator
    from pathlib import Path

    from flitzis_looper.controller.asset_lifecycle import (
        AssignmentImage,
        AssignmentPublicationPlan,
        ProjectAssetLifecycle,
    )
    from flitzis_looper.copy_snapshot import CopySnapshot
    from flitzis_looper.models import ProjectState
    from flitzis_looper_audio import ProjectAssetLease


type MapRecoveryPublication = tuple[AssignmentImage, AssignmentPublicationPlan]


def prepare_recovery_map_publication(
    assets: ProjectAssetLifecycle, project: ProjectState, previous: MapRecoveryPublication | None
) -> MapRecoveryPublication | None:
    """Retain a complete prepared image in the existing recovery owner before Config commit."""
    if previous is not None:
        image, publication = previous
        if publication.owns_unpublished_image:
            assets.release_assignment_image(image)
        elif not publication.settled:
            publication.apply()
    if previous is None and not any(
        intent is not None and intent.selected is not None
        for intent in project.pad_source_beatmap_intent
    ):
        return None
    image = assets.prepare_assignment_image(project)
    try:
        return image, assets.prepare_assignment_publication(image)
    except OSError, RuntimeError, TypeError, ValueError:
        assets.release_assignment_image(image)
        raise


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
    _map_references: dict[Path, SourceBeatmapReference] = field(default_factory=dict)
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
        project = validated_project_image(raw_image)
        errors: list[str] = []
        for intent in project.pad_source_beatmap_intent:
            if intent is not None and intent.selected is not None:
                try:
                    self._acquire_map(assets, root, intent.selected)
                except (OSError, RuntimeError, TypeError, ValueError) as error:
                    errors.append(str(error))
        if errors:
            raise RuntimeError("; ".join(errors))
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
        intent = data.source_beatmap_intent
        if intent is not None and intent.selected is not None:
            try:
                self._acquire_map(assets, root, intent.selected)
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

    def _acquire_map(
        self, assets: ProjectAssetLifecycle, root: Path, reference: SourceBeatmapReference
    ) -> None:
        reference = SourceBeatmapReference.model_validate(reference)
        path = resolve_asset(reference.reference, project_root=root).path
        previous = self._map_references.get(path)
        if previous is not None and previous != reference:
            message = "Copy journal has contradictory immutable map receipts for one path"
            raise ValueError(message)
        self._map_references[path] = reference
        if (-1, "source_beatmap", path) in self._owned:
            return
        preservation = (-2, "source_beatmap", path.parent)
        if preservation not in self._owned:
            # Acquire the actual typed generation FIRST. If leaf binding fails,
            # this transient ancestor still blocks an already queued cleanup.
            # It conveys no verified leaf bytes/hash or source qualification.
            lease = assets.acquire(path.parent)
            self.readers.append(lease)
            self._owned[preservation] = lease
        try:
            lease = assets._acquire_map(reference)
        except OSError, RuntimeError, ValueError:
            # History requires actual retention, not current map qualification.
            # The already acquired generation pin remains until real settlement.
            return
        self.readers.append(lease)
        self._owned[-1, "source_beatmap", path] = lease
        # The exact bound leaf now provides the same path protection. Retain a
        # failed preservation release in the original reader owner for retry.
        reader = self._owned[preservation]
        reader.release()
        self.readers.remove(reader)
        self._owned.pop(preservation)

    def release(self) -> None:
        """Release readers only after the terminal journal and live owners are settled."""
        while self.readers:
            self.readers[-1].release()
            self.readers.pop()
        self.acquired = False
        self._owned.clear()
        self._map_references.clear()
        self._project_images.clear()
