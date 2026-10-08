"""Fail-closed fixed native QM provenance, with no caller-selected trust receipts."""

import json
import math
import struct
from typing import TYPE_CHECKING

from pydantic import TypeAdapter

from flitzis_looper.analysis.beat_candidates import _bound_json
from flitzis_looper.analysis.beat_native_lineage import (
    FreshNativeMetadata,
    _cold_source,
    _file,
    _object,
    replace_metadata_request,
)
from flitzis_looper.analysis.contracts import AnalysisIdentity
from flitzis_looper.analysis.corrected_legacy_models import (
    CorrectedLegacyCandidate,
    decode_corrected_legacy,
)
from flitzis_looper.analysis.corrected_legacy_profiles import APPROVED_LEGACY_PROFILES
from flitzis_looper.analysis.corrected_legacy_provenance import runtime_bindings
from flitzis_looper.analysis.reference_inputs_validation import (
    fail,
    frozen_corpus,
    parse_source_aliases,
    private_path,
    unique_json,
)

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.analysis.beat_candidate_models import ArtifactBinding
    from flitzis_looper.analysis.corrected_legacy_models import CorrectedLegacyResult
    from flitzis_looper.analysis.reference_inputs_models import ReferenceTrack
    from flitzis_looper.analysis.reference_inputs_validation import FrozenCorpus, FrozenTrack

_PROFILES = APPROVED_LEGACY_PROFILES
_STAGES = (
    "admitted",
    "before_prepare_export",
    "after_prepare_export",
    "before_analyze_corrected_legacy",
    "after_analyze_corrected_legacy",
    "before_retire_pcm",
    "after_retire_pcm",
    "before_finish",
    "after_finish",
    "job_done",
)


def available_corrected_legacy_profiles() -> list[dict[str, str]]:
    """List separately approved comparators without adding any product backend selection."""
    return [{"track_id": p.track_id, "profile_id": p.profile_id} for p in _PROFILES]


def _observations(raw: dict[str, bytes]) -> FreshNativeMetadata:
    metadata: FreshNativeMetadata | None = None
    stages: list[str] = []
    observations = unique_json(raw["native-observations.json"])
    if not isinstance(observations, list):
        fail("corrected_legacy_native_observations_required")
    for value in observations:
        row = _object(value)
        current = TypeAdapter(FreshNativeMetadata).validate_json(
            json.dumps(row.get("metadata")), strict=True
        )
        if metadata is not None and metadata != current:
            fail("corrected_legacy_native_source_changed")
        metadata = current
        if row.get("stage") == "after_finish" and row.get("finish_accepted") is not True:
            fail("corrected_legacy_finish_rejected")
        if row.get("stage") not in _STAGES:
            fail("corrected_legacy_native_chain_incomplete")
        stages.append(str(row["stage"]))
    if stages != list(_STAGES) or metadata is None:
        fail("corrected_legacy_native_chain_incomplete")
    return metadata


def _native_chain(
    workspace: Path,
    raw: dict[str, bytes],
    result: CorrectedLegacyResult,
    provenance: dict[str, object],
) -> FreshNativeMetadata:
    summary = _object(unique_json(raw["summary.json"]))
    if (
        (
            summary.get("finish_accepted"),
            summary.get("completion_event_count"),
            summary.get("retired"),
        )
        != (True, 1, True)
        or summary.get("finish_accepted") is not True
        or summary.get("retired") is not True
        or type(summary.get("completion_event_count")) is not int
    ):
        fail("corrected_legacy_actual_finish_retirement_required")
    metadata = _observations(raw)
    identity, loaded = result.envelope.identity, result.envelope.loaded
    if (metadata.pad_id, metadata.request_id, metadata.source_id, metadata.source_generation) != (
        identity.pad_id,
        identity.request_id,
        identity.source_id,
        identity.source_generation,
    ) or (metadata.sample_rate_hz, metadata.frame_count, metadata.complete_mono_sha256) != (
        loaded.sample_rate_hz,
        loaded.frame_count,
        loaded.mono_sha256,
    ):
        fail("corrected_legacy_result_native_identity_mismatch")
    subsequent = TypeAdapter(FreshNativeMetadata).validate_json(
        json.dumps(summary.get("subsequent_metadata")), strict=True
    )
    if (
        subsequent.request_id != identity.request_id + 1
        or replace_metadata_request(subsequent, identity.request_id) != metadata
    ):
        fail("corrected_legacy_natural_readmission_source_mismatch")
    _request(raw, result, metadata)
    _request_paths(workspace, raw, provenance)
    _publications(raw, result)
    return metadata


def _request(
    raw: dict[str, bytes], result: CorrectedLegacyResult, metadata: FreshNativeMetadata
) -> None:
    request = _object(unique_json(raw["native-request.raw.json"]))
    if set(request) != {
        "schema_version",
        "backend",
        "identity",
        "native_metadata",
        "pcm_export_path",
        "analyzer_export_path",
    }:
        fail("corrected_legacy_request_fields_mismatch")
    if (
        request.get("schema_version") != 1
        or type(request.get("schema_version")) is not int
        or request.get("backend") != "corrected-qm-native-v1"
    ):
        fail("corrected_legacy_request_contract_mismatch")
    declared_identity = TypeAdapter(AnalysisIdentity).validate_json(
        json.dumps(request.get("identity")), strict=True
    )
    if declared_identity != result.envelope.identity:
        fail("corrected_legacy_request_identity_mismatch")
    declared = TypeAdapter(FreshNativeMetadata).validate_json(
        json.dumps(request.get("native_metadata")), strict=True
    )
    if declared != metadata:
        fail("corrected_legacy_request_source_mismatch")


def _request_paths(workspace: Path, raw: dict[str, bytes], provenance: dict[str, object]) -> None:
    request = _object(unique_json(raw["native-request.raw.json"]))
    analyzer_path = _object(provenance.get("analyzer_input")).get("path")
    declared_path = request.get("analyzer_export_path")
    if (
        not isinstance(declared_path, str)
        or not isinstance(analyzer_path, str)
        or private_path(workspace, declared_path) != private_path(workspace, analyzer_path)
    ):
        fail("corrected_legacy_request_analyzer_export_mismatch")
    export = request.get("pcm_export_path")
    if not isinstance(export, str):
        fail("corrected_legacy_request_export_path_required")
    private_path(workspace, export)


def _publications(raw: dict[str, bytes], result: CorrectedLegacyResult) -> None:
    identity = result.envelope.identity
    for name in (
        "native-finish-input.raw.json",
        "result-envelope.raw.json",
        "completion-event-0.raw.json",
    ):
        if raw[name] != raw["native-result.raw.json"]:
            fail("corrected_legacy_native_publication_bytes_mismatch")
    events = unique_json(raw["loader-events.json"])
    if not isinstance(events, list):
        fail("corrected_legacy_complete_loader_events_required")
    completions = [
        _object(e) for e in events if _object(e).get("type") == "offline_analysis_completed"
    ]
    if (
        len(completions) != 1
        or type(completions[0].get("id")) is not int
        or type(completions[0].get("request_id")) is not int
        or (completions[0].get("id"), completions[0].get("request_id"))
        != (identity.pad_id, identity.request_id)
        or completions[0].get("result_json") != raw["native-result.raw.json"].decode()
    ):
        fail("corrected_legacy_complete_event_identity_or_bytes_mismatch")


def _analyzer(
    workspace: Path, provenance: dict[str, object], result: CorrectedLegacyResult
) -> ArtifactBinding:
    binding = _file(
        workspace, provenance.get("analyzer_input"), "actual_complete_qm_analyzer_input"
    )
    analyzer = result.envelope.analyzer
    if (binding.bytes, binding.sha256) != (analyzer.frame_count * 8, analyzer.sha256):
        fail("corrected_legacy_complete_analyzer_input_mismatch")
    with private_path(workspace, binding.path).open("rb") as handle:
        while chunk := handle.read(65536):
            if len(chunk) % 8 or any(
                not math.isfinite(v)
                or abs(v) > 3.4028234663852886e38
                or float(struct.unpack("<f", struct.pack("<f", v))[0]) != v
                for (v,) in struct.iter_unpack("<d", chunk)
            ):
                fail("corrected_legacy_analyzer_not_finite_exact_f32_promotion")
    return binding


def _source_path(
    workspace: Path, provenance: dict[str, object], corpus: FrozenCorpus, frozen: FrozenTrack
) -> list[ArtifactBinding]:
    actual = provenance.get("actual_source_path")
    if not isinstance(actual, str):
        fail("corrected_legacy_unapproved_source_relocation")
    if private_path(workspace, actual) == private_path(workspace, frozen.source_relative):
        return []
    if frozen.id not in {"T01", "T02"}:
        fail("corrected_legacy_unapproved_source_relocation")
    values = provenance.get("file_bindings")
    if not isinstance(values, list):
        fail("corrected_legacy_explicit_source_alias_required")
    declared = [
        _object(v)
        for v in values
        if _object(v).get("role") == "unchanged_explicit_original_source_aliases"
    ]
    if len(declared) != 1:
        fail("corrected_legacy_explicit_source_alias_required")
    binding = _file(
        workspace,
        {k: declared[0][k] for k in ("path", "sha256", "bytes")},
        "verified_corrected_legacy_source_aliases",
    )
    aliases = parse_source_aliases(
        workspace, private_path(workspace, binding.path).read_bytes(), corpus
    )
    allowed = next((a for a in aliases if a.track_id == frozen.id), None)
    if allowed is None or private_path(workspace, allowed.actual_source_path) != private_path(
        workspace, actual
    ):
        fail("corrected_legacy_unapproved_source_relocation")
    return [binding]


def load_corrected_legacy(
    workspace: Path, track: ReferenceTrack, profile_id: str
) -> CorrectedLegacyCandidate:
    """Verify one fixed reviewed full chain; caller hashes cannot register new producers."""
    profile = next((p for p in _PROFILES if p.profile_id == profile_id), None)
    if profile is None:
        fail("unsupported_corrected_legacy_lineage")
    if profile.track_id != track.identity.track_id:
        fail("corrected_legacy_profile_track_mismatch")
    provenance_raw, binding = _bound_json(
        workspace,
        f"{profile.directory}/producer-provenance.json",
        profile.provenance_sha256,
        "approved_corrected_qm_native_producer",
    )
    provenance = _object(unique_json(provenance_raw))
    if (
        provenance.get("schema_version") != 1
        or type(provenance.get("schema_version")) is not int
        or provenance.get("producer") != "hardware-free-native-corrected-qm-v1"
        or provenance.get("diagnostic_only") is not True
        or provenance.get("finish_accepted") is not True
    ):
        fail("corrected_legacy_supported_producer_required")
    corpus = frozen_corpus(workspace)
    frozen = next(t for t in corpus.tracks if t.id == profile.track_id)
    if (
        provenance.get("track_id"),
        provenance.get("original_source_relative"),
        provenance.get("original_source_sha256"),
        provenance.get("original_source_bytes"),
        track.identity.source_sha256,
    ) != (
        frozen.id,
        frozen.source_relative,
        frozen.source_sha256,
        frozen.source_bytes,
        frozen.source_sha256,
    ):
        fail("corrected_legacy_original_source_mismatch")
    bindings = [
        binding,
        *_source_path(workspace, provenance, corpus, frozen),
        _file(
            workspace,
            {
                "path": provenance.get("actual_source_path"),
                "sha256": frozen.source_sha256,
                "bytes": frozen.source_bytes,
            },
            "actual_unchanged_original_source",
        ),
    ]
    raw = {}
    for artifact in profile.artifacts:
        data, checked = _bound_json(
            workspace, f"{profile.directory}/{artifact.name}", artifact.sha256, artifact.name
        )
        raw[artifact.name] = data
        bindings.append(checked)
    result = decode_corrected_legacy(raw["native-result.raw.json"])
    loaded, pcm = result.envelope.loaded, track.identity.pcm
    if (loaded.sample_rate_hz, loaded.frame_count, loaded.origin_seconds, loaded.mono_sha256) != (
        pcm.sample_rate_hz,
        pcm.frame_count,
        pcm.origin_seconds,
        pcm.sha256,
    ):
        fail("corrected_legacy_complete_reference_pcm_mismatch")
    bindings.append(
        _file(
            workspace,
            {"path": pcm.path, "sha256": pcm.sha256, "bytes": pcm.frame_count * 4},
            "complete_reference_listening_pcm",
            pcm=True,
        )
    )
    metadata = _native_chain(workspace, raw, result, provenance)
    if metadata.original_sha256 != frozen.source_sha256:
        fail("corrected_legacy_native_original_identity_mismatch")
    bindings.extend(
        _cold_source(
            workspace,
            _object(provenance.get("native_source")),
            metadata,
            shape=(metadata.sample_rate_hz, metadata.channels, metadata.frame_count),
            source_sha256=frozen.source_sha256,
            source_bytes=frozen.source_bytes,
        )
    )
    bindings.append(_analyzer(workspace, provenance, result))
    export = _file(
        workspace, provenance.get("native_export"), "actual_complete_native_mono_export", pcm=True
    )
    if (export.bytes, export.sha256) != (loaded.frame_count * 4, loaded.mono_sha256):
        fail("corrected_legacy_native_export_mismatch")
    bindings.append(export)
    bindings.extend(runtime_bindings(workspace, provenance, raw))
    return CorrectedLegacyCandidate(
        profile_id,
        profile.track_id,
        frozen.source_sha256,
        frozen.source_bytes,
        result,
        tuple(bindings),
    )
