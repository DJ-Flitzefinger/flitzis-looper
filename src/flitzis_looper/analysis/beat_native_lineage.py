"""Fixed, independently reviewed hardware-free native v2 import contracts.

The producer runs separately. This reader runs no native code or inference and
accepts no caller-selected receipt checksum. Its approved anchors bind actual
producer/runtime bytes in addition to complete source/request/publication data.
"""

import json
from dataclasses import asdict
from typing import TYPE_CHECKING, Annotated, Literal

from pydantic import Field, TypeAdapter

from flitzis_looper.analysis import beat_candidates as historical
from flitzis_looper.analysis.beat_candidate_models import (
    ArtifactBinding,
    FreshNativeProfile,
    HistoricalProfile,
    HistoricalSummary,
    NativeCandidate,
    NativeMetadata,
)
from flitzis_looper.analysis.beat_native_profiles import APPROVED_FRESH_PROFILES as _FRESH_PROFILES
from flitzis_looper.analysis.contracts import BeatWorkerRequest
from flitzis_looper.analysis.publication import decode_result
from flitzis_looper.analysis.reference_inputs_models import Digest, StrictInput, Text
from flitzis_looper.analysis.reference_inputs_validation import (
    fail,
    frozen_corpus,
    private_path,
    read_json_bytes,
    sha256_file,
)

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.analysis.reference_inputs_models import ReferenceTrack


class FreshNativeMetadata(NativeMetadata):
    """Actual CompleteSourceReader identity, without discarding native extras."""

    complete_source_identity: Digest
    original_sha256: Digest
    complete_playback_sha256: Digest
    complete_mono_sha256: Digest
    source_zero_frame: Annotated[int, Field(strict=True, ge=0, le=0)]


class FreshSummary(HistoricalSummary):
    """Byte-pinned fresh summary with an actual subsequent complete-source request."""

    native_reservation_after_done: FreshNativeMetadata
    final_schema_version: Literal[2]


class FileBinding(StrictInput):
    """One complete retained file, rehashed against independently pinned provenance."""

    path: Text
    sha256: Digest
    bytes: Annotated[int, Field(strict=True, gt=0)]


def available_fresh_profiles() -> list[dict[str, str]]:
    """List supported fresh attempts without reading private data or asserting truth."""
    return [
        {"track_id": item.lineage.track_id, "profile_id": item.lineage.profile_id}
        for item in _FRESH_PROFILES
    ]


def _object(value: object) -> dict[str, object]:
    if not isinstance(value, dict) or any(not isinstance(key, str) for key in value):
        fail("fresh_native_provenance_object_required")
    return value


def _integer(value: object) -> int:
    if type(value) is not int:
        fail("fresh_native_integer_required")
    return value


def _file(workspace: Path, value: object, role: str, *, pcm: bool = False) -> ArtifactBinding:
    binding = TypeAdapter(FileBinding).validate_json(json.dumps(value), strict=True)
    path = private_path(workspace, binding.path)
    if path.stat().st_size != binding.bytes or sha256_file(path, pcm=pcm) != binding.sha256:
        fail("fresh_native_bound_file_changed")
    return ArtifactBinding(
        path.relative_to(workspace.resolve()).as_posix(), role, binding.sha256, binding.bytes
    )


def _source(
    workspace: Path, track: ReferenceTrack, summary: FreshSummary, provenance: dict[str, object]
) -> tuple[int, str, list[ArtifactBinding]]:
    frozen = next(
        item for item in frozen_corpus(workspace).tracks if item.id == track.identity.track_id
    )
    if (
        provenance.get("track_id"),
        provenance.get("original_source_relative"),
        provenance.get("original_source_sha256"),
        provenance.get("original_source_bytes"),
        summary.source_sha256,
        summary.source_bytes,
        track.identity.source_sha256,
    ) != (
        frozen.id,
        frozen.source_relative,
        frozen.source_sha256,
        frozen.source_bytes,
        frozen.source_sha256,
        frozen.source_bytes,
        frozen.source_sha256,
    ):
        fail("fresh_native_original_source_mismatch")
    actual = provenance.get("actual_source_path")
    if not isinstance(actual, str):
        fail("fresh_native_actual_source_path_required")
    source = private_path(workspace, actual)
    if private_path(workspace, summary.source) != source:
        fail("fresh_native_actual_source_summary_mismatch")
    if source.stat().st_size != frozen.source_bytes or sha256_file(source) != frozen.source_sha256:
        fail("fresh_native_original_source_changed")
    # The fixed profile pins both actual and original paths. Aliases retain the
    # original manifest and are separately revalidated by the reference-first CLI.
    if frozen.id not in {"T01", "T02"} and source != private_path(
        workspace, frozen.source_relative
    ):
        fail("fresh_native_unapproved_source_relocation")
    return (
        frozen.source_bytes,
        actual,
        [
            ArtifactBinding(
                actual, "actual_original_source", frozen.source_sha256, frozen.source_bytes
            )
        ],
    )


def _native_observations(
    raw: bytes, request: BeatWorkerRequest, summary: FreshSummary
) -> FreshNativeMetadata:
    observations = json.loads(raw)
    if not isinstance(observations, list):
        fail("fresh_native_observation_sequence_required")
    metadata: FreshNativeMetadata | None = None
    stages: list[str] = []
    for row in observations:
        observation = _object(row)
        current = TypeAdapter(FreshNativeMetadata).validate_json(
            json.dumps(observation.get("metadata")), strict=True
        )
        historical._validate_native(current, request, summary.loaded_shape_rate_channels_frames[1])
        if metadata is not None and current != metadata:
            fail("fresh_native_complete_source_changed_between_stages")
        metadata = current
        stage = observation.get("stage")
        if not isinstance(stage, str):
            fail("fresh_native_observation_stage_required")
        if stage == "after_finish" and observation.get("finish_accepted") is not True:
            fail("fresh_native_finish_not_accepted")
        stages.append(stage)
    required = (
        "admitted",
        "before_prepare_export",
        "after_prepare_export",
        "before_retire_pcm",
        "after_retire_pcm",
        "before_finish",
        "after_finish",
        "job_done",
    )
    filtered = [stage for stage in stages if stage in required]
    if filtered != list(required) or metadata is None:
        fail("fresh_native_observation_sequence_incomplete")
    subsequent = summary.native_reservation_after_done
    if replace_metadata_request(subsequent, request.identity.request_id) != metadata:
        fail("fresh_native_subsequent_source_changed")
    if (
        metadata.original_sha256 != summary.source_sha256
        or metadata.complete_mono_sha256 != summary.export.sha256
    ):
        fail("fresh_native_complete_source_export_mismatch")
    return metadata


def replace_metadata_request(metadata: FreshNativeMetadata, request_id: int) -> FreshNativeMetadata:
    """Compare natural readmission without conflating new request with source generation."""
    return metadata.model_copy(update={"request_id": request_id})


def _publication(raw: dict[str, bytes], request: BeatWorkerRequest) -> None:
    for name in (
        "result-envelope.raw.json",
        "native-finish-input.raw.json",
        "completion-event-0.raw.json",
    ):
        if decode_result(raw[name].decode(), request).schema_version != 2:
            fail("fresh_native_ready_v2_required")
    events = json.loads(raw["loader-events.json"])
    if not isinstance(events, list):
        fail("fresh_native_loader_events_required")
    completed = [
        _object(event)
        for event in events
        if _object(event).get("type") == "offline_analysis_completed"
    ]
    if len(completed) != 1 or (completed[0].get("id"), completed[0].get("request_id")) != (
        request.identity.pad_id,
        request.identity.request_id,
    ):
        fail("fresh_native_completion_outer_identity_mismatch")


def _cold_source(
    workspace: Path, native: dict[str, object], metadata: FreshNativeMetadata, summary: FreshSummary
) -> list[ArtifactBinding]:
    declared = TypeAdapter(FreshNativeMetadata).validate_json(
        json.dumps(native.get("native_metadata")), strict=True
    )
    if (
        declared != metadata
        or native.get("complete_mono_sha256") != metadata.complete_mono_sha256
        or native.get("acknowledged_source_generation") != metadata.source_generation
    ):
        fail("fresh_native_cold_source_metadata_mismatch")
    loaded = _object(native.get("complete_loaded_pcm"))
    shape = summary.loaded_shape_rate_channels_frames
    if (
        loaded.get("rate_hz"),
        loaded.get("channels"),
        loaded.get("full_frames"),
        loaded.get("sha256"),
        loaded.get("bytes"),
        loaded.get("finite_values"),
    ) != (*shape, metadata.complete_playback_sha256, shape[1] * shape[2] * 4, True):
        fail("fresh_native_complete_loaded_pcm_mismatch")
    bindings = [
        _file(
            workspace,
            {key: loaded[key] for key in ("path", "sha256", "bytes")},
            "actual_complete_loaded_pcm",
            pcm=True,
        ),
        _file(workspace, native.get("cold_manifest"), "actual_cold_source_manifest"),
        _file(workspace, native.get("cached_original"), "actual_sealed_cached_original"),
    ]
    if (bindings[2].sha256, bindings[2].bytes) != (summary.source_sha256, summary.source_bytes):
        fail("fresh_native_sealed_original_mismatch")
    manifest = _object(json.loads(read_json_bytes(private_path(workspace, bindings[1].path))))
    descriptor = _object(manifest.get("descriptor"))
    original = _object(_object(descriptor.get("decoder")).get("original"))
    pcm = _object(_object(descriptor.get("playback")).get("pcm"))
    if (
        manifest.get("identity"),
        original.get("sha256"),
        original.get("bytes"),
        pcm.get("rate_hz"),
        pcm.get("channels"),
        pcm.get("full_frames"),
        pcm.get("full_bytes"),
        pcm.get("interleaved_sha256"),
        pcm.get("mono_sha256"),
        pcm.get("source_zero_bits"),
        pcm.get("mono_revision"),
    ) != (
        metadata.complete_source_identity,
        summary.source_sha256,
        summary.source_bytes,
        *shape,
        shape[1] * shape[2] * 4,
        metadata.complete_playback_sha256,
        metadata.complete_mono_sha256,
        "0000000000000000",
        metadata.mono_rule,
    ):
        fail("fresh_native_cold_manifest_identity_mismatch")
    window = _object(native.get("resident_window"))
    start, end, revision = (
        _integer(window.get(name)) for name in ("start_frame", "end_frame", "window_revision")
    )
    if not 0 <= start < end <= shape[2] or revision < 1:
        fail("fresh_native_resident_window_invalid")
    return bindings


def _runtime(
    workspace: Path, runtime: dict[str, object], summary: FreshSummary
) -> list[ArtifactBinding]:
    if runtime.get("build_profile") not in {"debug", "release"}:
        fail("fresh_native_runtime_profile_required")
    bindings = [
        _file(workspace, runtime.get("native_test_executable"), "actual_native_test_executable")
    ]
    installed = _object(runtime.get("installed_native_extension"))
    if installed.get("used_by_probe") is not False:
        fail("fresh_native_embedded_runtime_misattributed")
    bindings.append(
        _file(workspace, installed.get("binding"), "separate_installed_pyd_not_used_by_probe")
    )
    if bindings[-1].sha256 != summary.native_extension_sha256:
        fail("fresh_native_separate_installed_pyd_mismatch")
    key_model = _object(runtime.get("native_key_model"))
    if "binding" in key_model:
        bindings.append(_file(workspace, key_model["binding"], "actual_native_key_model"))
    elif key_model.get("status") != "unavailable" or summary.key.get("status") == "ready":
        fail("fresh_native_key_model_identity_required")
    return bindings


def _worker(
    workspace: Path, worker: dict[str, object], summary: FreshSummary
) -> list[ArtifactBinding]:
    if (
        TypeAdapter(type(summary.model)).validate_json(json.dumps(worker.get("model")), strict=True)
        != summary.model
    ):
        fail("fresh_native_worker_configuration_model_mismatch")
    if worker.get("worker_limits") != asdict(historical._LIMITS):
        fail("fresh_native_worker_limits_changed")
    bindings = [
        _file(workspace, worker.get(name), f"actual_worker_{name}")
        for name in (
            "interpreter",
            "script",
            "checkpoint",
            "manifest",
            "pointer",
            "lock",
            "environment",
            "pyproject",
        )
    ]
    checkpoint = next(binding for binding in bindings if binding.role == "actual_worker_checkpoint")
    lock = next(binding for binding in bindings if binding.role == "actual_worker_lock")
    if (
        checkpoint.sha256 != summary.model.sha256
        or f"uv-lock-sha256:{lock.sha256}" != summary.model.environment_id
    ):
        fail("fresh_native_worker_model_lock_mismatch")
    return bindings


def _provenance(
    workspace: Path,
    provenance: dict[str, object],
    metadata: FreshNativeMetadata,
    summary: FreshSummary,
) -> list[ArtifactBinding]:
    if (
        provenance.get("schema_version") != 1
        or provenance.get("producer") != "hardware-free-native-b2-v1"
        or provenance.get("finish_accepted") is not True
        or provenance.get("diagnostic_only") is not True
    ):
        fail("fresh_native_supported_producer_required")
    bindings = [
        _file(workspace, provenance.get("frozen_manifest"), "frozen_manifest"),
        _file(workspace, provenance.get("probe_config"), "actual_native_probe_config"),
    ]
    bindings.extend(
        _cold_source(workspace, _object(provenance.get("native_source")), metadata, summary)
    )
    bindings.extend(_runtime(workspace, _object(provenance.get("runtime")), summary))
    bindings.extend(_worker(workspace, _object(provenance.get("worker_configuration")), summary))
    implementations = provenance.get("producer_implementation")
    if not isinstance(implementations, list) or len(implementations) != 2:
        fail("fresh_native_producer_implementation_required")
    bindings.extend(
        _file(workspace, value, "actual_producer_implementation") for value in implementations
    )
    # Retained dependency copies and original-path mappings are part of the fixed
    # approved producer packet too, not unverified extra telemetry.
    auxiliary = provenance.get("file_bindings")
    if not isinstance(auxiliary, list) or not 1 <= len(auxiliary) <= 128:
        fail("fresh_native_producer_dependencies_required")
    for value in auxiliary:
        item = _object(value)
        role = item.get("role")
        if not isinstance(role, str) or not role:
            fail("fresh_native_producer_dependency_role_required")
        bindings.append(
            _file(workspace, {key: item.get(key) for key in ("path", "sha256", "bytes")}, role)
        )
    return bindings


def _read_profile(
    workspace: Path, track: ReferenceTrack, approved: FreshNativeProfile
) -> tuple[FreshSummary, dict[str, object], dict[str, bytes], list[ArtifactBinding]]:
    profile = approved.lineage
    if profile.track_id != track.identity.track_id:
        fail("candidate_profile_track_mismatch")
    summary_raw, summary_binding = historical._bound_json(
        workspace,
        f"{profile.directory}/summary.json",
        profile.summary_sha256,
        "approved_fresh_native_summary",
    )
    summary = TypeAdapter(FreshSummary).validate_json(summary_raw, strict=True)
    provenance_raw, provenance_binding = historical._bound_json(
        workspace,
        f"{profile.directory}/producer-provenance.json",
        approved.provenance_sha256,
        "approved_actual_native_producer",
    )
    provenance = _object(json.loads(provenance_raw))
    source_bytes, _, source_bindings = _source(workspace, track, summary, provenance)
    historical._validate_summary(workspace, track, summary, verified_source_bytes=source_bytes)
    raw: dict[str, bytes] = {}
    bindings = [summary_binding, provenance_binding, *source_bindings]
    for artifact in profile.artifacts:
        value, binding = historical._bound_json(
            workspace, f"{profile.directory}/{artifact.name}", artifact.sha256, artifact.name
        )
        raw[artifact.name] = value
        bindings.append(binding)
    return summary, provenance, raw, bindings


def _pcm_bindings(
    workspace: Path, track: ReferenceTrack, profile: HistoricalProfile, summary: FreshSummary
) -> list[ArtifactBinding]:
    return [
        _file(
            workspace,
            {"path": path, "sha256": summary.export.sha256, "bytes": summary.export.bytes},
            role,
            pcm=True,
        )
        for path, role in (
            (
                f"{profile.directory}/complete-native-export.f32le",
                "retained_complete_native_export",
            ),
            (track.identity.pcm.path, "actual_complete_reference_pcm"),
        )
    ]


def load_fresh_candidate(
    workspace: Path, track: ReferenceTrack, profile_id: str
) -> NativeCandidate | None:
    """Validate an approved actual native lineage after complete reference validation."""
    approved = next(
        (item for item in _FRESH_PROFILES if item.lineage.profile_id == profile_id), None
    )
    if approved is None:
        return None
    profile = approved.lineage
    summary, provenance, raw, bindings = _read_profile(workspace, track, approved)
    request = TypeAdapter(BeatWorkerRequest).validate_json(
        raw["worker-request.raw.json"], strict=True
    )
    if request != historical._summary_request(workspace, summary):
        fail("candidate_retained_request_summary_mismatch")
    predictions = historical._candidate_predictions(raw, request, summary, retained=True)
    historical._validate_publications(
        raw, request, predictions, summary, retained=True, metadata_type=FreshNativeMetadata
    )
    _publication(raw, request)
    native = _native_observations(raw["native-observations.json"], request, summary)
    bindings.extend(_provenance(workspace, provenance, native, summary))
    bindings.extend(_pcm_bindings(workspace, track, profile, summary))
    prior = tuple(
        historical._bound_json(
            workspace, item.name, item.sha256, "excluded_prior_attempt_never_scored"
        )[1]
        for item in profile.prior_artifacts
    )
    return NativeCandidate(
        profile.profile_id,
        profile.track_id,
        "fresh_native_v2",
        str(provenance["original_source_relative"]),
        summary.source_sha256,
        _integer(provenance["original_source_bytes"]),
        summary.export.sha256,
        summary.loaded_shape_rate_channels_frames[1],
        request,
        predictions,
        tuple(bindings),
        prior,
        summary.native_extension_sha256,
        str(provenance["actual_source_path"]),
    )
