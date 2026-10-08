"""Retained native producer sources tied to actual preflight and compiled copies."""

from pathlib import Path, PurePosixPath
from typing import Annotated

from pydantic import Field, TypeAdapter

from flitzis_looper.analysis.beat_candidate_models import ArtifactBinding
from flitzis_looper.analysis.beat_native_lineage import _file, _object
from flitzis_looper.analysis.reference_inputs_models import Digest, StrictInput, Text
from flitzis_looper.analysis.reference_inputs_validation import (
    fail,
    private_path,
    sha256_file,
    unique_json,
)


class SourceFileBinding(StrictInput):
    """Complete source bytes may legitimately be empty, including package initializers."""

    path: Text
    sha256: Digest
    bytes: Annotated[int, Field(strict=True, ge=0)]


def source_file(workspace: Path, value: object, role: str) -> ArtifactBinding:
    """Rehash a retained source without weakening PCM or executable artifact dimensions."""
    declared = TypeAdapter(SourceFileBinding).validate_python(value, strict=True)
    path = private_path(workspace, declared.path)
    if path.stat().st_size != declared.bytes or sha256_file(path) != declared.sha256:
        fail("corrected_legacy_retained_source_bytes_changed")
    return ArtifactBinding(
        path.relative_to(workspace.resolve()).as_posix(), role, declared.sha256, declared.bytes
    )


PRODUCER_FILES = (
    "rust/crates/looper/src/lib.rs",
    "rust/crates/looper/src/messages.rs",
    "rust/crates/looper/src/audio_engine/mod.rs",
    "rust/crates/looper/src/audio_engine/b2_native_candidate_probe.rs",
    "rust/crates/looper/src/audio_engine/b2_corrected_legacy_probe.py",
    "rust/crates/looper/src/audio_engine/cold_residency_tests.rs",
    "rust/crates/looper/src/audio_engine/cold_load.rs",
    "rust/crates/looper/src/audio_engine/cold_store.rs",
    "rust/crates/looper/src/audio_engine/cold_store/staging.rs",
    "rust/crates/looper/src/audio_engine/cold_store/residency.rs",
    "rust/crates/looper/src/audio_engine/sample_loader.rs",
    "rust/crates/looper/src/audio_engine/sample_loader/cold.rs",
    "rust/crates/looper/src/audio_engine/complete_context.rs",
    "rust/crates/looper/src/audio_engine/source_reader.rs",
    "rust/crates/looper/src/audio_engine/analysis_jobs.rs",
    "rust/crates/looper/src/audio_engine/corrected_legacy.rs",
    "rust/crates/looper/src/audio_engine/analysis_pcm.rs",
    "rust/crates/looper/src/audio_engine/analysis_pcm/fft.rs",
    "rust/crates/looper/src/audio_engine/analysis_pcm/streamed.rs",
    "rust/crates/looper/src/audio_engine/analysis_pcm/tempo_gate.rs",
    "rust/crates/analysis/src/lib.rs",
    "rust/crates/analysis/src/bpm_pipeline.rs",
    "rust/crates/analysis/src/detection_function.rs",
    "rust/crates/analysis/src/phase_vocoder.rs",
    "rust/crates/analysis/src/tempotrack.rs",
    "rust/crates/analysis/src/downbeat.rs",
    "rust/crates/analysis/src/math_utils.rs",
    "rust/crates/analysis/src/window.rs",
    "src/flitzis_looper/analysis/reference_inputs_validation.py",
    "src/flitzis_looper/analysis/reference_inputs_models.py",
    "src/flitzis_looper/analysis/reference_source_aliases.py",
)
COMPILED_SOURCES = {
    "compiled-producer.rs": "rust/crates/looper/src/audio_engine/b2_native_candidate_probe.rs",
    "compiled-producer.py": "rust/crates/looper/src/audio_engine/b2_corrected_legacy_probe.py",
}


def _manifest(workspace: Path, binding: ArtifactBinding) -> dict[str, SourceFileBinding]:
    value = _object(unique_json(private_path(workspace, binding.path).read_bytes()))
    rows = value.get("files")
    if not isinstance(rows, list) or not rows:
        fail("corrected_legacy_source_manifest_required")
    result: dict[str, SourceFileBinding] = {}
    for row in rows:
        declared = TypeAdapter(SourceFileBinding).validate_python(row, strict=True)
        source = (workspace / declared.path).resolve()
        if not source.is_relative_to(workspace.resolve() / "repo"):
            fail("corrected_legacy_manifest_source_outside_repository")
        path = source.relative_to(workspace.resolve()).as_posix()
        if path in result:
            fail("corrected_legacy_duplicate_manifest_source")
        result[path] = declared
    return result


def _single(extras: list[ArtifactBinding], role: str) -> ArtifactBinding:
    matches = [binding for binding in extras if binding.role == role]
    if len(matches) != 1:
        fail("corrected_legacy_unique_source_manifest_required")
    return matches[0]


def _source_manifests(
    workspace: Path, extras: list[ArtifactBinding]
) -> tuple[dict[str, SourceFileBinding], ArtifactBinding]:
    manifests = []
    for role in (
        "complete_native_source_preflight_manifest",
        "same_native_source_postflight_manifest",
    ):
        matches = [b for b in extras if b.role == role]
        if len(matches) != 1:
            fail("corrected_legacy_source_preflight_postflight_required")
        manifests.append(matches[0])
    before, after = (_manifest(workspace, b) for b in manifests)
    after_value = _object(unique_json(private_path(workspace, manifests[1].path).read_bytes()))
    if before != after or after_value.get("identical_to_preflight") is not True:
        fail("corrected_legacy_producer_changed_during_native_runs")
    prior = _file(workspace, after_value.get("before"), "actual_source_preflight_binding")
    if (prior.path, prior.sha256, prior.bytes) != (
        manifests[0].path,
        manifests[0].sha256,
        manifests[0].bytes,
    ):
        fail("corrected_legacy_postflight_preflight_binding_mismatch")
    return before, prior


def _snapshot_row(
    workspace: Path, row: dict[str, object], before: dict[str, SourceFileBinding]
) -> tuple[str, ArtifactBinding]:
    original = row.get("original_path")
    if (
        not isinstance(original, str)
        or not Path(original).is_absolute()
        or not Path(original).resolve().is_relative_to(workspace.resolve() / "repo")
        or row.get("role") != "complete_preflight_local_source_and_build_snapshot"
    ):
        fail("corrected_legacy_complete_source_snapshot_mapping_invalid")
    relative = Path(original).resolve().relative_to(workspace.resolve()).as_posix()
    expected = before.get(relative)
    retained = source_file(
        workspace,
        {key: row.get(key) for key in ("path", "sha256", "bytes")},
        "complete_preflight_local_source_and_build_snapshot",
    )
    if expected is None or (expected.sha256, expected.bytes) != (retained.sha256, retained.bytes):
        fail("corrected_legacy_complete_source_snapshot_preflight_mismatch")
    return relative, retained


def _complete_snapshot(
    workspace: Path, before: dict[str, SourceFileBinding], extras: list[ArtifactBinding]
) -> None:
    manifest = _single(extras, "complete_source_snapshot_manifest")
    value = _object(unique_json(private_path(workspace, manifest.path).read_bytes()))
    for field, role in (
        ("before", "complete_native_source_preflight_manifest"),
        ("after", "same_native_source_postflight_manifest"),
    ):
        bound = _file(workspace, value.get(field), "actual_complete_snapshot_manifest_link")
        expected = _single(extras, role)
        if (bound.path, bound.sha256, bound.bytes) != (
            expected.path,
            expected.sha256,
            expected.bytes,
        ):
            fail("corrected_legacy_complete_snapshot_manifest_link_mismatch")
    rows = value.get("files")
    if not isinstance(rows, list) or len(rows) != len(before):
        fail("corrected_legacy_complete_source_snapshot_required")
    copies = [b for b in extras if b.role == "complete_preflight_local_source_and_build_snapshot"]
    remaining = {(b.path, b.sha256, b.bytes) for b in copies}
    if len(remaining) != len(copies) or len(copies) != len(rows):
        fail("corrected_legacy_complete_snapshot_file_bijection_required")
    originals: set[str] = set()
    for row in rows:
        original, bound = _snapshot_row(workspace, _object(row), before)
        key = (bound.path, bound.sha256, bound.bytes)
        if original in originals or key not in remaining:
            fail("corrected_legacy_complete_snapshot_file_bijection_required")
        originals.add(original)
        remaining.remove(key)
    if originals != set(before):
        fail("corrected_legacy_complete_source_snapshot_required")


def _retained_sources(
    workspace: Path, provenance: dict[str, object]
) -> tuple[list[ArtifactBinding], list[object]]:
    implementations, mappings = (
        provenance.get("producer_implementation"),
        provenance.get("producer_source_files"),
    )
    if (
        not isinstance(implementations, list)
        or not isinstance(mappings, list)
        or len(implementations) != len(PRODUCER_FILES) + len(COMPILED_SOURCES)
        or len(mappings) != len(implementations)
    ):
        fail("corrected_legacy_complete_producer_sources_required")
    sources = [
        _file(workspace, item, "checked_qm_producer_implementation") for item in implementations
    ]
    if len({(b.path, b.sha256, b.bytes) for b in sources}) != len(sources):
        fail("corrected_legacy_duplicate_retained_producer_source")
    return sources, mappings


def _mapped_location(workspace: Path, row: dict[str, object], root: Path) -> str:
    relative, original = row.get("repository_relative"), row.get("original_path")
    if (
        not isinstance(relative, str)
        or relative not in PRODUCER_FILES
        or PurePosixPath(relative).as_posix() != relative
    ):
        fail("corrected_legacy_retained_source_mapping_invalid")
    if (
        not isinstance(original, str)
        or not Path(original).is_absolute()
        or Path(original).resolve() != root / relative
        or not (root / relative).is_relative_to(workspace.resolve() / "repo")
    ):
        fail("corrected_legacy_retained_source_mapping_invalid")
    return relative


def _mapped_sources(
    workspace: Path,
    root: Path,
    before: dict[str, SourceFileBinding],
    sources: list[ArtifactBinding],
    mappings: list[object],
) -> tuple[dict[str, ArtifactBinding], dict[str, ArtifactBinding]]:
    remaining = {(b.path, b.sha256, b.bytes) for b in sources}
    support: dict[str, ArtifactBinding] = {}
    compiled: dict[str, ArtifactBinding] = {}
    for value in mappings:
        row = _object(value)
        bound = _file(workspace, row.get("binding"), "mapped_qm_producer_source")
        key = (bound.path, bound.sha256, bound.bytes)
        if key not in remaining:
            fail("corrected_legacy_source_mapping_binding_mismatch")
        remaining.remove(key)
        relative = _mapped_location(workspace, row, root)
        original_relative = (root / relative).relative_to(workspace.resolve()).as_posix()
        expected = before.get(original_relative)
        if expected is None or (expected.sha256, expected.bytes) != (bound.sha256, bound.bytes):
            fail("corrected_legacy_retained_source_preflight_mismatch")
        source_role = row.get("role")
        if source_role == "checked_shared_qm_source_or_native_producer_support":
            target = support
        elif (
            source_role == "actual_exe_include_str_compiled_producer"
            and relative in COMPILED_SOURCES.values()
        ):
            target = compiled
        else:
            fail("corrected_legacy_retained_source_role_invalid")
        if relative in target:
            fail("corrected_legacy_duplicate_producer_source_mapping")
        target[relative] = bound
    return support, compiled


def source_bindings(
    workspace: Path,
    provenance: dict[str, object],
    native_runtime: dict[str, object],
    extras: list[ArtifactBinding],
) -> tuple[list[ArtifactBinding], dict[str, ArtifactBinding]]:
    """Verify retained sources; original checkout paths are metadata, never input artifacts."""
    repository = native_runtime.get("repository")
    if (
        not isinstance(repository, str)
        or not Path(repository).is_absolute()
        or Path(repository).resolve() != workspace.resolve() / "repo"
    ):
        fail("corrected_legacy_native_repository_required")
    before, prior = _source_manifests(workspace, extras)
    _complete_snapshot(workspace, before, extras)
    sources, mappings = _retained_sources(workspace, provenance)
    support, compiled = _mapped_sources(
        workspace, Path(repository).resolve(), before, sources, mappings
    )
    if set(support) != set(PRODUCER_FILES) or set(compiled) != set(COMPILED_SOURCES.values()):
        fail("corrected_legacy_shared_producer_support_incomplete")
    for name, relative in COMPILED_SOURCES.items():
        left, right = compiled[relative], support[relative]
        if (
            Path(left.path).name != name
            or private_path(workspace, left.path).read_bytes()
            != private_path(workspace, right.path).read_bytes()
        ):
            fail("corrected_legacy_compiled_producer_source_mismatch")
    return [*sources, prior], support
