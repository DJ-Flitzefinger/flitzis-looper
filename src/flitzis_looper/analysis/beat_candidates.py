"""Read complete approved native candidates after independent reference sealing.

This module opens no model, imports no native engine and runs no inference.
Historical checksums are trust anchors from separately checked retained evidence,
not checksums submitted by the candidate. Arbitrary new native receipts remain
unsupported until their producer provenance has an explicit verified contract.
"""

import hashlib
import json
import struct
from dataclasses import asdict
from typing import TYPE_CHECKING

from pydantic import TypeAdapter

from flitzis_looper.analysis.beat_candidate_models import (
    ApprovedArtifact,
    ArtifactBinding,
    HistoricalProfile,
    HistoricalSummary,
    NativeCandidate,
    NativeExport,
    NativeMetadata,
)
from flitzis_looper.analysis.contracts import (
    BeatComponentResult,
    BeatModelIdentity,
    BeatPredictions,
    BeatWorkerRequest,
    WorkerLimits,
    decode_response,
    validate_component_result,
)
from flitzis_looper.analysis.publication import decode_result
from flitzis_looper.analysis.reference_inputs_validation import (
    MODEL_SHA256,
    fail,
    frozen_corpus,
    private_path,
    read_json_bytes,
    sha256_file,
    unique_json,
)

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.analysis.publication import PublishedAnalysisResult
    from flitzis_looper.analysis.reference_inputs_models import ReferenceTrack

_ARRAY_NAMES = ("beat_seconds", "downbeat_seconds", "beat_logits", "downbeat_logits")
_LIMITS = WorkerLimits()
_FROZEN_MODEL = BeatModelIdentity(
    sha256=MODEL_SHA256,
    frontend_id="beat-this-1.1.0-soxr-hq-logmel-v1",
    environment_id="uv-lock-sha256:e7d180bd53a02736693e0054dfb62a868da77478922b8ca3027c07fe12f98a75",
)

# Each summary SHA is independently bound by the approved loaded identity
# inventory. Every retained candidate artifact below was additionally inspected
# through the strict readers and checked for complete binary64 prediction parity.
_APPROVED_PROFILES = (
    HistoricalProfile(
        "T01-native-retry",
        "T01",
        "scratch/b2a/native-T01-retry",
        "5af21e82dafdf94d4c165f0de51e7026e577c1749112ba064d0427d26852d609",
        (
            ApprovedArtifact(
                "raw-component.json",
                "24b7a4c8a7e90833d511533bdefe8632d99df2c38bed9e8e905d3913067ea7a0",
            ),
            ApprovedArtifact(
                "raw-worker-response.json",
                "76f072619e01920c9748204010155223ee170c26491005391e18303c9848a6dc",
            ),
            ApprovedArtifact(
                "result-envelope.json",
                "6561afe725cd29388b03b92461aed0fb8b6868ef0e37ba51b3549398dbbdf93b",
            ),
            ApprovedArtifact(
                "loader-events.json",
                "bf84b7261ce0579d1bac649fcfa4d256929d05d0ee79b5634821f239450b2ca4",
            ),
        ),
        retained_request=False,
        prior_artifacts=(
            ApprovedArtifact(
                "scratch/b2a/native-T01/raw-component.json",
                "24b7a4c8a7e90833d511533bdefe8632d99df2c38bed9e8e905d3913067ea7a0",
            ),
            ApprovedArtifact(
                "scratch/b2a/native-T01/raw-worker-response.json",
                "76f072619e01920c9748204010155223ee170c26491005391e18303c9848a6dc",
            ),
            ApprovedArtifact(
                "scratch/b2a/native-T01/result-envelope.json",
                "6561afe725cd29388b03b92461aed0fb8b6868ef0e37ba51b3549398dbbdf93b",
            ),
        ),
    ),
    HistoricalProfile(
        "T02-native",
        "T02",
        "scratch/b2a/native-T02",
        "bf69b12e70407c2884d15e42ad8fdc1fb7363342de7d5d4d21765cb44b909062",
        (
            ApprovedArtifact(
                "raw-component.json",
                "a3a320697e1f7b6215d0af4c977d0b6448692700f935693dccd06ec9498f8689",
            ),
            ApprovedArtifact(
                "raw-worker-response.json",
                "9de39cf5d1a29b269f15f06e9af670b1b76e200c6305678a72816def92882c18",
            ),
            ApprovedArtifact(
                "result-envelope.json",
                "65aabfd75ae54af6bf59c4cd68c0136b885c6d52652dcb582fb0b1f8e56d8639",
            ),
            ApprovedArtifact(
                "loader-events.json",
                "4217fc52cdeb6c489d11c7972a71b0547bc9c772d89af16eb78160a3aa4d2790",
            ),
        ),
        retained_request=False,
    ),
    HistoricalProfile(
        "T03-native-post-fix",
        "T03",
        "scratch/b2a/native-T03-post-fix",
        "b3f91385b5fe1252724595502dce4b76e6d219423230ebf96d01fed09ed28e3a",
        (
            ApprovedArtifact(
                "raw-component.json",
                "a57827fa03e62459ffa9298e5c05757f29f3141aef92085c81936e82392b534d",
            ),
            ApprovedArtifact(
                "raw-worker-response.json",
                "5611dd61bded4c14c95c28a8a3bbb33fabd51d95f284ded7200300cfeb017d91",
            ),
            ApprovedArtifact(
                "result-envelope.json",
                "1cf9f3f9bf80b9632bb39107b52dcd2aa31e605f7436060101c3866341e92995",
            ),
            ApprovedArtifact(
                "loader-events.json",
                "75949dedafdda1808e54dcae42b1341bc67b3f7d1e087977b0a14e299b4f27f0",
            ),
        ),
        retained_request=False,
        prior_artifacts=(
            ApprovedArtifact(
                "scratch/b2a/native-T03/raw-worker-response.json",
                "5611dd61bded4c14c95c28a8a3bbb33fabd51d95f284ded7200300cfeb017d91",
            ),
            ApprovedArtifact(
                "scratch/b2a/native-T03-refusal/native-refusal.json",
                "d8df69069e24149551eec6b58c36e49b625c62c7b46f58e710369c5f112a8ca5",
            ),
            ApprovedArtifact(
                "scratch/b2a/native-T03-refusal/pre-native-finish-envelope.json",
                "dac8adf5eae17ef6f4e6c83dbdfe7bb3a8a1004685a1a4ea6e228eb23c9bd9dd",
            ),
            ApprovedArtifact(
                "scratch/b2a/native-T03-refusal/raw-worker-response.json",
                "5611dd61bded4c14c95c28a8a3bbb33fabd51d95f284ded7200300cfeb017d91",
            ),
        ),
    ),
    HistoricalProfile(
        "T04-native-final",
        "T04",
        "scratch/b2b2/native-T04-final",
        "0b80da025bfcf41bcc483dbd3075e0a9c728fbc4a33862862843ed7f9a5d87b6",
        (
            ApprovedArtifact(
                "beat-component.raw.json",
                "d7beefc5d7da8e7b1023da48f5943176c788fea2b6e576c89ab24865c2e866d5",
            ),
            ApprovedArtifact(
                "complete-native-export-identity.json",
                "3104913b85e0c4bfc288c3cf0c6c195e118880db1b173b41335b889f55d404ff",
            ),
            ApprovedArtifact(
                "completion-event-0.raw.json",
                "bc3b073040b38228673297f5d9b14fb8cda003853385b76802b9520c2e1b61f3",
            ),
            ApprovedArtifact(
                "loader-events.json",
                "da51d8c64b936052b49b2fe966600dc31db4a77e02b3261cc0431785a5ddadcf",
            ),
            ApprovedArtifact(
                "native-finish-input.raw.json",
                "02dbef1b1e64069acb966ab50970a08a4673566e39a18350613a448110c61d89",
            ),
            ApprovedArtifact(
                "native-observations.json",
                "3d93e2f703c9ae94eff43052dbeb052c778bff8fab3df465111d9d3f80c5f1ba",
            ),
            ApprovedArtifact(
                "result-envelope.raw.json",
                "02dbef1b1e64069acb966ab50970a08a4673566e39a18350613a448110c61d89",
            ),
            ApprovedArtifact(
                "worker-request.raw.json",
                "b612c5dfe2e21083f973132bcd1f4f8379d7882c2412898a1273737f8bee9cc6",
            ),
            ApprovedArtifact(
                "worker-response.raw.json",
                "c1be9ac13812765bb790bd50a77a1b833b11f55228da33afc05871f9c3defa8b",
            ),
        ),
        retained_request=True,
        prior_artifacts=(
            ApprovedArtifact(
                "scratch/b2a/native-T04/admission-rejection.json",
                "8ff666a0ea4f1c3e3ddae2e1cca9300cac2043ea7dc0b967e4da73400a4c3116",
            ),
        ),
    ),
    HistoricalProfile(
        "T05-native-final",
        "T05",
        "scratch/b2b2/native-T05-final",
        "f6f1266cddb3510af1cfd1e297269c704eb82b268ca0bea789430b647b9d78b9",
        (
            ApprovedArtifact(
                "beat-component.raw.json",
                "4ea95a4a4a8d0aef6c73c1d7a3dfe42d6b08716c61940af57914fc8475daab6b",
            ),
            ApprovedArtifact(
                "complete-native-export-identity.json",
                "3317e81b1efbfbf26cfec4f69aeb06839ce2901c7d6656739360afa8b21d6832",
            ),
            ApprovedArtifact(
                "completion-event-0.raw.json",
                "16fa25fa6e7de4a611daaaadda497c7d09f724669e8966ea959c6c6eaeed9717",
            ),
            ApprovedArtifact(
                "loader-events.json",
                "d4824301dc79efc522e53a45995d7dacdfc7e57f17fc40df89fb1dab3eb34f7f",
            ),
            ApprovedArtifact(
                "native-finish-input.raw.json",
                "531b42b169fda512c52eba09dd95bb20782fb4290ea92270f2c8d9dce855270d",
            ),
            ApprovedArtifact(
                "native-observations.json",
                "3fba8f6f633caecd6f50d9ff658dba41c94f6eca280f954c8345c1b0f4520dd1",
            ),
            ApprovedArtifact(
                "result-envelope.raw.json",
                "531b42b169fda512c52eba09dd95bb20782fb4290ea92270f2c8d9dce855270d",
            ),
            ApprovedArtifact(
                "worker-request.raw.json",
                "510358da3b58302a39cddb1e199946d73c45197c843ea3485119a757a40304e5",
            ),
            ApprovedArtifact(
                "worker-response.raw.json",
                "7cd1219efa59e3c76124a372759073b6538f68510ea539dc26da4d13cfd5cb94",
            ),
        ),
        retained_request=True,
        prior_artifacts=(
            ApprovedArtifact(
                "scratch/b2a/native-T05/admission-rejection.json",
                "d9c9b31f8a6b60d1a1676789a365f0d2539bffc37609c228365e1845fba4b2a3",
            ),
        ),
    ),
)


def available_historical_profiles() -> list[dict[str, str]]:
    """List supported historical import attempts; validity still requires all checks.

    The original T01-T03 snapshots currently fail exact completion-event parity.
    Their identifiers never assert complete or acceptable publication lineage.
    """
    return [
        {"track_id": profile.track_id, "profile_id": profile.profile_id}
        for profile in _APPROVED_PROFILES
    ]


def expected_complete_logit_count(frame_count: int, sample_rate_hz: int) -> int:
    """Apply pinned soxr half-up length and centered 441-frame-hop STFT extent."""
    if (
        type(frame_count) is not int
        or type(sample_rate_hz) is not int
        or frame_count <= 0
        or not 0 < sample_rate_hz <= 768000
    ):
        fail("invalid_candidate_pcm_extent")
    resampled = (2 * frame_count * 22050 + sample_rate_hz) // (2 * sample_rate_hz)
    if resampled <= 512:
        fail("candidate_too_short_for_reference_reflect_padding")
    count = resampled // 441 + 1
    if count > _LIMITS.max_prediction_count:
        fail("candidate_complete_logits_limit")
    return count


def _bound_json(
    workspace: Path, path: str, digest: str, role: str
) -> tuple[bytes, ArtifactBinding]:
    actual = private_path(workspace, path)
    raw = read_json_bytes(actual)
    actual_digest = hashlib.sha256(raw).hexdigest()
    if actual_digest != digest:
        fail("approved_candidate_artifact_changed")
    binding = ArtifactBinding(
        actual.relative_to(workspace.resolve()).as_posix(), role, digest, len(raw)
    )
    return raw, binding


def _predictions_identity(predictions: BeatPredictions) -> dict[str, dict[str, int | str]]:
    return {
        name: {
            "count": len(values),
            "float64_le_sha256": hashlib.sha256(
                struct.pack(f"<{len(values)}d", *values)
            ).hexdigest(),
        }
        for name in _ARRAY_NAMES
        for values in (getattr(predictions, name),)
    }


def _same_predictions(before: BeatPredictions | None, after: BeatPredictions) -> None:
    if before is None or _predictions_identity(before) != _predictions_identity(after):
        fail("candidate_full_raw_prediction_parity_mismatch")


def _summary_request(workspace: Path, summary: HistoricalSummary) -> BeatWorkerRequest:
    export = summary.export
    # This reconstructs only the documented historical request metadata, without
    # claiming that an original request file survived retirement.
    path = private_path(workspace, export.path)
    return TypeAdapter(BeatWorkerRequest).validate_json(
        json.dumps({
            "schema_version": 1,
            "identity": asdict(summary.beat.identity),
            "pcm": {
                "path": str(path),
                "sample_rate_hz": export.sample_rate_hz,
                "frame_count": export.frame_count,
                "origin_seconds": export.origin_seconds,
                "dtype": export.dtype,
                "channels": export.channels,
            },
            "model": asdict(summary.model),
        }),
        strict=True,
    )


def _validate_source(
    workspace: Path,
    track: ReferenceTrack,
    summary: HistoricalSummary,
) -> int:
    corpus = frozen_corpus(workspace)
    frozen = next(item for item in corpus.tracks if item.id == track.identity.track_id)
    historical_source = private_path(workspace, summary.source)
    if historical_source != private_path(workspace, frozen.source_relative):
        fail("candidate_historical_source_path_mismatch")
    if (
        summary.source_sha256 != frozen.source_sha256
        or summary.source_sha256 != track.identity.source_sha256
    ):
        fail("candidate_original_source_identity_mismatch")
    if summary.source_bytes is not None and summary.source_bytes != frozen.source_bytes:
        fail("candidate_original_source_size_mismatch")
    return frozen.source_bytes


def _validate_summary(
    workspace: Path,
    track: ReferenceTrack,
    summary: HistoricalSummary,
) -> int:
    source_bytes = _validate_source(workspace, track, summary)
    export, pcm = summary.export, track.identity.pcm
    if (
        export.sha256,
        export.sample_rate_hz,
        export.frame_count,
        export.origin_seconds,
        export.dtype,
        export.channels,
        export.bytes,
    ) != (
        pcm.sha256,
        pcm.sample_rate_hz,
        pcm.frame_count,
        pcm.origin_seconds,
        pcm.dtype,
        pcm.channels,
        pcm.frame_count * 4,
    ):
        fail("candidate_native_pcm_reference_mismatch")
    rate, channels, frames = summary.loaded_shape_rate_channels_frames
    native = summary.native_reservation_after_done
    if (rate, channels, frames) != (export.sample_rate_hz, native.channels, export.frame_count):
        fail("candidate_complete_native_shape_mismatch")
    if summary.loaded_duration_seconds != frames / rate:
        fail("candidate_complete_native_duration_mismatch")
    expected = _summary_request(workspace, summary)
    _validate_native(native, expected, channels, subsequent=True)
    if summary.model != _FROZEN_MODEL or summary.beat.model != _FROZEN_MODEL:
        fail("candidate_frozen_model_identity_mismatch")
    if summary.beat.status != "ready" or not summary.beat.resources_released:
        fail("candidate_not_ready_or_retired")
    if (
        summary.remaining_pcm_directories
        or summary.remaining_request_directories
        or not summary.worker_exit_codes
    ):
        fail("candidate_historical_resources_not_retired")
    complete = expected_complete_logit_count(frames, rate)
    if summary.expected_full_frontend_frames != complete:
        fail("candidate_full_frontend_extent_mismatch")
    return source_bytes


def _validate_native(
    native: NativeMetadata,
    request: BeatWorkerRequest,
    channels: int,
    *,
    subsequent: bool = False,
) -> None:
    identity, pcm = request.identity, request.pcm
    if (
        native.pad_id,
        native.request_id,
        native.source_id,
        native.source_generation,
        native.sample_rate_hz,
        native.frame_count,
        native.origin_seconds,
        native.channels,
    ) != (
        identity.pad_id,
        identity.request_id + int(subsequent),
        identity.source_id,
        identity.source_generation,
        pcm.sample_rate_hz,
        pcm.frame_count,
        pcm.origin_seconds,
        channels,
    ):
        fail("candidate_native_request_source_mismatch")


def _validate_publications(
    raw: dict[str, bytes],
    request: BeatWorkerRequest,
    predictions: BeatPredictions,
    summary: HistoricalSummary,
    *,
    retained: bool,
) -> None:
    envelope_name = "result-envelope.raw.json" if retained else "result-envelope.json"
    result = decode_result(raw[envelope_name].decode("utf-8"), request)
    if result.beat.status != "ready":
        fail("candidate_final_publication_not_ready")
    _same_predictions(result.beat.predictions, predictions)
    if result.key != summary.key:
        fail("candidate_independent_key_lineage_mismatch")
    events = json.loads(raw["loader-events.json"])
    if not isinstance(events, list):
        fail("candidate_loader_events_must_be_complete_array")
    completions = [
        event
        for event in events
        if isinstance(event, dict) and event.get("type") == "offline_analysis_completed"
    ]
    if len(completions) != 1:
        fail("candidate_single_complete_publication_required")
    wire = completions[0].get("result_json")
    if not isinstance(wire, str):
        fail("candidate_completion_has_no_final_envelope")
    # Nested JSON strings are checked for duplicate keys too; the outer bounded
    # read does not inspect objects embedded in a string.
    unique_json(wire)
    event_result = decode_result(wire, request)
    _same_predictions(event_result.beat.predictions, predictions)
    if event_result.key != result.key:
        fail("candidate_completion_key_mismatch")
    if retained:
        _validate_retained_publication(raw, request, summary, result, wire)


def _validate_retained_publication(
    raw: dict[str, bytes],
    request: BeatWorkerRequest,
    summary: HistoricalSummary,
    result: PublishedAnalysisResult,
    wire: str,
) -> None:
    finish = decode_result(raw["native-finish-input.raw.json"].decode("utf-8"), request)
    completion = decode_result(raw["completion-event-0.raw.json"].decode("utf-8"), request)
    if result.beat.predictions is None:
        fail("candidate_retained_publication_missing_raw_predictions")
    _same_predictions(finish.beat.predictions, result.beat.predictions)
    _same_predictions(completion.beat.predictions, result.beat.predictions)
    if (
        finish != result
        or completion != result
        or json.loads(wire) != json.loads(raw["completion-event-0.raw.json"])
    ):
        fail("candidate_native_final_envelope_lineage_mismatch")
    if raw["native-finish-input.raw.json"] != raw["result-envelope.raw.json"]:
        fail("candidate_native_finish_bytes_mismatch")
    export = TypeAdapter(NativeExport).validate_json(
        raw["complete-native-export-identity.json"],
        strict=True,
    )
    if export != summary.export:
        fail("candidate_retained_native_export_mismatch")
    _validate_observations(raw["native-observations.json"], request, summary)


def _validate_observations(
    raw: bytes, request: BeatWorkerRequest, summary: HistoricalSummary
) -> None:
    observations = json.loads(raw)
    if not isinstance(observations, list) or not observations:
        fail("candidate_native_observations_missing")
    stages: set[str] = set()
    for observation in observations:
        if not isinstance(observation, dict):
            fail("candidate_native_observation_invalid")
        native = TypeAdapter(NativeMetadata).validate_json(
            json.dumps(observation.get("metadata")), strict=True
        )
        _validate_native(native, request, summary.loaded_shape_rate_channels_frames[1])
        stage = observation.get("stage")
        if not isinstance(stage, str):
            fail("candidate_native_observation_stage_invalid")
        stages.add(stage)
    if not {
        "before_prepare_export",
        "after_prepare_export",
        "before_finish",
        "after_finish",
        "job_done",
    }.issubset(stages):
        fail("candidate_native_observation_lineage_incomplete")


def _read_bound_profile(
    workspace: Path,
    track: ReferenceTrack,
    profile: HistoricalProfile,
) -> tuple[HistoricalSummary, int, dict[str, bytes], tuple[ArtifactBinding, ...]]:
    summary_raw, summary_binding = _bound_json(
        workspace,
        f"{profile.directory}/summary.json",
        profile.summary_sha256,
        "approved_historical_summary",
    )
    summary = TypeAdapter(HistoricalSummary).validate_json(summary_raw, strict=True)
    source_bytes = _validate_summary(workspace, track, summary)
    pcm_path = private_path(workspace, track.identity.pcm.path)
    if (
        pcm_path.stat().st_size != summary.export.bytes
        or sha256_file(pcm_path, pcm=True) != summary.export.sha256
    ):
        fail("candidate_actual_reference_pcm_changed")
    pcm_binding = ArtifactBinding(
        pcm_path.relative_to(workspace.resolve()).as_posix(),
        "actual_complete_materialized_native_pcm",
        summary.export.sha256,
        summary.export.bytes,
    )
    raw: dict[str, bytes] = {}
    bindings = [summary_binding, pcm_binding]
    for artifact in profile.artifacts:
        value, binding = _bound_json(
            workspace, f"{profile.directory}/{artifact.name}", artifact.sha256, artifact.name
        )
        raw[artifact.name] = value
        bindings.append(binding)
    return summary, source_bytes, raw, tuple(bindings)


def _candidate_predictions(
    raw: dict[str, bytes],
    request: BeatWorkerRequest,
    summary: HistoricalSummary,
    *,
    retained: bool,
) -> BeatPredictions:
    response_name = "worker-response.raw.json" if retained else "raw-worker-response.json"
    component_name = "beat-component.raw.json" if retained else "raw-component.json"
    predictions = decode_response(raw[response_name], request, _LIMITS)
    complete = expected_complete_logit_count(request.pcm.frame_count, request.pcm.sample_rate_hz)
    if len(predictions.beat_logits) != complete or len(predictions.downbeat_logits) != complete:
        fail("candidate_partial_full_track_logits")
    component = TypeAdapter(BeatComponentResult).validate_json(raw[component_name], strict=True)
    validate_component_result(component, request, _LIMITS)
    if not component.resources_released:
        fail("candidate_raw_component_not_retired")
    _same_predictions(component.predictions, predictions)
    identity = _predictions_identity(predictions)
    if summary.prediction_counts is not None and summary.prediction_counts != {
        name: len(getattr(predictions, name)) for name in _ARRAY_NAMES
    }:
        fail("candidate_historical_prediction_counts_mismatch")
    if summary.prediction_arrays is not None and summary.prediction_arrays != identity:
        fail("candidate_historical_prediction_arrays_mismatch")
    return predictions


def load_candidate(workspace: Path, track: ReferenceTrack, profile_id: str) -> NativeCandidate:
    """Bind one complete historical native candidate after reference/source validation.

    Callers must first revalidate the actual complete ReferenceSeal, including
    original source aliases and all source/PCM/coverage bindings. Only approved
    historical lineages are supported; self-hashed arrays or generic native
    receipts cannot replace these independently verified trust anchors.
    """
    profile = next((item for item in _APPROVED_PROFILES if item.profile_id == profile_id), None)
    if profile is None:
        fail("unsupported_candidate_lineage")
    if profile.track_id != track.identity.track_id:
        fail("candidate_profile_track_mismatch")
    summary, source_bytes, raw, bindings = _read_bound_profile(workspace, track, profile)
    request = _summary_request(workspace, summary)
    if profile.retained_request:
        retained = TypeAdapter(BeatWorkerRequest).validate_json(
            raw["worker-request.raw.json"],
            strict=True,
        )
        if retained != request:
            fail("candidate_retained_request_summary_mismatch")
        request = retained
    predictions = _candidate_predictions(raw, request, summary, retained=profile.retained_request)
    _validate_publications(raw, request, predictions, summary, retained=profile.retained_request)
    prior = tuple(
        _bound_json(workspace, item.name, item.sha256, "excluded_prior_attempt_never_scored")[1]
        for item in profile.prior_artifacts
    )
    return NativeCandidate(
        profile.profile_id,
        profile.track_id,
        "retained_request" if profile.retained_request else "verified_historical_summary",
        summary.source,
        summary.source_sha256,
        source_bytes,
        summary.export.sha256,
        summary.loaded_shape_rate_channels_frames[1],
        request,
        predictions,
        bindings,
        prior,
        summary.native_extension_sha256,
    )
