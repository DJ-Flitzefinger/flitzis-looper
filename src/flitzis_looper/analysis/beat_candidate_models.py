"""Immutable selected-backend candidate lineage for private B2 diagnostics."""

from dataclasses import asdict, dataclass
from typing import Annotated, Literal

from pydantic import BaseModel, ConfigDict, Field, TypeAdapter

from flitzis_looper.analysis.contracts import (
    BeatComponentResult,
    BeatModelIdentity,
    BeatPredictions,
    BeatWorkerRequest,
)
from flitzis_looper.analysis.reference_inputs_models import (
    Digest,
    Seconds,
    StrictInput,
    Text,
    TrackId,
)


class CandidateSelection(StrictInput):
    """Select an explicitly supported lineage; no caller supplied trust checksum."""

    track_id: TrackId
    profile_id: Text


@dataclass(frozen=True, slots=True)
class ArtifactBinding:
    """Actual complete bytes of a retained private artifact."""

    path: str
    role: str
    sha256: str
    bytes: int


@dataclass(frozen=True, slots=True)
class ApprovedArtifact:
    """A separately checked immutable historical artifact, never user self-hashed."""

    name: str
    sha256: str


@dataclass(frozen=True, slots=True)
class HistoricalProfile:
    """Only a verified fixed historical lineage can replace retired request files."""

    profile_id: str
    track_id: TrackId
    directory: str
    summary_sha256: str
    artifacts: tuple[ApprovedArtifact, ...]
    retained_request: bool
    prior_artifacts: tuple[ApprovedArtifact, ...] = ()


@dataclass(frozen=True, slots=True)
class FreshNativeProfile:
    """The verifier owns these anchors; candidate JSON never supplies them."""

    lineage: HistoricalProfile
    provenance_sha256: str


@dataclass(frozen=True, slots=True)
class NativeCandidate:
    """Complete raw predictions bound to verified historical native provenance.

    The request retains its original retired export path. Its PCM identity is
    verified against the actual complete materialized reference PCM, and that
    replacement path is reported as a separate artifact binding.
    """

    profile_id: str
    track_id: TrackId
    lineage_kind: Literal["retained_request", "verified_historical_summary", "fresh_native_v2"]
    historical_source_path: str
    source_sha256: str
    source_bytes: int
    pcm_sha256: str
    native_channels: int
    request: BeatWorkerRequest
    predictions: BeatPredictions
    artifact_bindings: tuple[ArtifactBinding, ...]
    excluded_historical_attempts: tuple[ArtifactBinding, ...]
    native_extension_sha256: str = ""
    actual_source_path: str | None = None

    def report(self) -> dict[str, object]:
        """Preserve every raw array and separate original from materialized paths."""
        return {
            "profile_id": self.profile_id,
            "track_id": self.track_id,
            "lineage_kind": self.lineage_kind,
            "historical_source_path": self.historical_source_path,
            "source_sha256": self.source_sha256,
            "source_bytes": self.source_bytes,
            "pcm_sha256": self.pcm_sha256,
            "native_channels": self.native_channels,
            "native_extension_sha256": self.native_extension_sha256,
            "native_extension_used_by_candidate": self.lineage_kind != "fresh_native_v2",
            "executing_native_artifact_role": "actual_native_test_executable"
            if self.lineage_kind == "fresh_native_v2"
            else "historical_native_extension",
            "actual_source_path": self.actual_source_path,
            "request": TypeAdapter(BeatWorkerRequest).dump_python(self.request, mode="json"),
            "predictions": TypeAdapter(BeatPredictions).dump_python(self.predictions, mode="json"),
            "artifact_bindings": [asdict(binding) for binding in self.artifact_bindings],
            "excluded_historical_attempts": [
                asdict(binding) for binding in self.excluded_historical_attempts
            ],
        }


class NativeMetadata(StrictInput):
    """Complete native reservation identity and loaded source shape."""

    pad_id: Annotated[int, Field(strict=True, ge=0)]
    request_id: Annotated[int, Field(strict=True, gt=0)]
    source_id: Text
    source_generation: Annotated[int, Field(strict=True, gt=0)]
    sample_rate_hz: Annotated[int, Field(strict=True, gt=0, le=768000)]
    frame_count: Annotated[int, Field(strict=True, gt=0)]
    channels: Annotated[int, Field(strict=True, gt=0, le=64)]
    origin_seconds: Annotated[float, Field(strict=True, ge=0, le=0)]
    mono_rule: Literal["arithmetic-channel-mean-f64-v1"]


class NativeExport(StrictInput):
    """Original complete native mono export, including its retired path."""

    path: Text
    sha256: Digest
    bytes: Annotated[int, Field(strict=True, gt=0)]
    sample_rate_hz: Annotated[int, Field(strict=True, gt=0, le=768000)]
    frame_count: Annotated[int, Field(strict=True, gt=0)]
    origin_seconds: Annotated[float, Field(strict=True, ge=0, le=0)]
    dtype: Literal["float32-le"]
    channels: Annotated[int, Field(strict=True, ge=1, le=1)]
    first_frame: Annotated[float, Field(strict=True, allow_inf_nan=False)]
    last_frame: Annotated[float, Field(strict=True, allow_inf_nan=False)]
    scope: Text | None = None


class HistoricalSummary(BaseModel):
    """Protected semantic fields within an independently byte-pinned original summary.

    Unrelated original telemetry remains preserved by the whole-byte binding.
    Ignoring those fields is allowed only after the approved checksum matches.
    """

    model_config = ConfigDict(extra="ignore", strict=True, allow_inf_nan=False)
    mode: Literal["ready"]
    source: Text
    source_sha256: Digest
    source_bytes: Annotated[int, Field(strict=True, gt=0)] | None = None
    native_extension_sha256: Digest
    loaded_shape_rate_channels_frames: tuple[int, int, int]
    loaded_duration_seconds: Seconds
    model: BeatModelIdentity
    export: NativeExport
    beat: BeatComponentResult
    key: dict[str, object]
    completion_event_count: Annotated[int, Field(strict=True, ge=1, le=1)]
    worker_retired: Literal[True]
    worker_exit_codes: tuple[Annotated[int, Field(strict=True, ge=0, le=0)], ...]
    remaining_pcm_directories: tuple[Text, ...]
    remaining_request_directories: tuple[Text, ...]
    native_reservation_after_done: NativeMetadata
    expected_full_frontend_frames: Annotated[int, Field(strict=True, gt=0)]
    prediction_counts: dict[str, int] | None = None
    prediction_arrays: dict[str, dict[str, int | str]] | None = None
    final_schema_version: Annotated[int, Field(strict=True, ge=1, le=2)] | None = None
