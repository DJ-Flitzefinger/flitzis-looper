"""Full-content grouping from held native receipts, without a Python naming authority."""

from typing import TYPE_CHECKING

from flitzis_looper.controller.material_migration_cleanup import ArtifactRequest
from flitzis_looper.material_migration_model import MigrationArtifactEvidence, MigrationAssignment
from flitzis_looper.project_materials import original_asset

if TYPE_CHECKING:
    from flitzis_looper.controller.material_migration_cleanup import ArtifactCapture
    from flitzis_looper.models import ProjectState, SessionState


def current_candidates(
    project: ProjectState, session: SessionState
) -> tuple[MigrationAssignment, ...]:
    """Capture per-pad intent; unowned external or invalid references remain untouched."""
    candidates = []
    for sample_id, (reference, content) in enumerate(
        zip(project.sample_paths, project.pad_content, strict=True)
    ):
        if reference is None or content is None or sample_id in session.loading_sample_ids:
            continue
        try:
            asset = original_asset(reference)
        except OSError, ValueError:
            continue
        if asset.layout == "readable":
            continue
        candidates.append(
            MigrationAssignment(
                sample_id=sample_id, instance_id=content.instance_id, old_reference=reference
            )
        )
    return tuple(candidates)


def original_requests(
    source: str, candidates: tuple[MigrationAssignment, ...]
) -> list[ArtifactRequest]:
    """Require the selected source and independently verify other contained originals."""
    requests = [ArtifactRequest(source, "rollback")]
    seen = {original_asset(source).path}
    for item in candidates:
        path = original_asset(item.old_reference).path
        if path not in seen:
            seen.add(path)
            requests.append(ArtifactRequest(item.old_reference, "rollback", required=False))
    return requests


def retain_content_group(
    source: str, capture: ArtifactCapture, candidates: tuple[MigrationAssignment, ...]
) -> tuple[tuple[MigrationAssignment, ...], tuple[str, int]]:
    """Retain complete native SHA/length custody only for this material's old images."""
    source_path = original_asset(source).path
    evidence_by_path = {
        original_asset(record.evidence.reference).path: record.evidence
        for record in capture.records
        if record.evidence.kind == "original"
    }
    selected = evidence_by_path.get(source_path)
    if selected is None:
        message = "material grouping has no complete selected original proof"
        raise ValueError(message)
    selected_file = selected.files[0]
    witness = selected_file.sha256, selected_file.bytes
    equal_paths = {
        path
        for path, evidence in evidence_by_path.items()
        if (evidence.files[0].sha256, evidence.files[0].bytes) == witness
    }
    retained = []
    for lease in capture.leases:
        evidence = MigrationArtifactEvidence.model_validate_json(lease.receipt_json())
        if evidence.kind == "original" and original_asset(evidence.reference).path in equal_paths:
            retained.append(lease)
        else:
            lease.release()
    capture.leases = retained
    capture.records = tuple(
        record
        for record in capture.records
        if record.evidence.kind == "original"
        and original_asset(record.evidence.reference).path in equal_paths
    )
    return (
        tuple(
            item for item in candidates if original_asset(item.old_reference).path in equal_paths
        ),
        witness,
    )
