"""Complete corrected QM diagnostics; these records never authorize musical timing."""

import base64
import math
import struct
from dataclasses import asdict, dataclass
from itertools import pairwise
from typing import TYPE_CHECKING, Annotated, Literal

from pydantic import Field, TypeAdapter

from flitzis_looper.analysis.contracts import AnalysisIdentity, BeatPredictions
from flitzis_looper.analysis.publication import MAX_ENVELOPE_BYTES, _unpack
from flitzis_looper.analysis.reference_inputs_models import Digest, One, StrictInput, TrackId
from flitzis_looper.analysis.reference_inputs_validation import unique_json

if TYPE_CHECKING:
    from flitzis_looper.analysis.beat_candidate_models import ApprovedArtifact, ArtifactBinding


type PositiveInteger = Annotated[int, Field(strict=True, gt=0, le=2**53)]
type Packed = Annotated[str, Field(strict=True, max_length=2666668)]


class LegacyLoaded(StrictInput):
    """Actual complete loaded mono source, distinct from resampled analyzer input."""

    sample_rate_hz: Annotated[int, Field(strict=True, ge=8000, le=768000)]
    frame_count: PositiveInteger
    origin_seconds: Annotated[float, Field(strict=True, ge=0.0, le=0.0)]
    mono_sha256: Digest
    mono_rule: Literal["arithmetic-channel-mean-f64-v1"]


class LegacyAnalyzer(StrictInput):
    """Executed converter's full binary64 analyzer input and integer ODF timebase."""

    sample_rate_hz: Literal[44100]
    frame_count: PositiveInteger
    sha256: Digest
    odf_hop_samples: PositiveInteger
    transform_revision: Literal["rubato-fft-1.0-44100-delay-trim-tail-flush-v1"]


class LegacyConfiguration(StrictInput):
    """Retain all requested QM defaults, including historically inactive settings."""

    step_secs: Annotated[float, Field(strict=True, ge=0.01161, le=0.01161)]
    max_bin_hz: Annotated[float, Field(strict=True, ge=50.0, le=50.0)]
    input_tempo: Annotated[float, Field(strict=True, ge=120.0, le=120.0)]
    alpha: Annotated[float, Field(strict=True, ge=0.9, le=0.9)]
    tightness: Annotated[float, Field(strict=True, ge=4.0, le=4.0)]
    viterbi_sigma: Annotated[float, Field(strict=True, ge=8.0, le=8.0)]
    window_length: Literal[512]
    hop_size: Literal[128]


class LegacyRaw(StrictInput):
    """Canonical lossless complete detector frames, associations and source seconds."""

    encoding: Literal["float64-le/base64+uint64-le/base64"]
    beat_frames: Packed
    downbeat_raw_indices: Packed
    beat_seconds: Packed
    downbeat_seconds: Packed


class LegacyCompatibility(StrictInput):
    """Existing binary32 compatibility projection from that same single analysis."""

    encoding: Literal["float32-le/base64"]
    bpm: Packed
    beats: Packed
    downbeats: Packed
    bars: Packed


class LegacyEnvelope(StrictInput):
    """Dedicated native QM result; no invented model identity, logits or worker."""

    schema_version: One
    backend: Literal["corrected-qm-native-v1"]
    diagnostic_only: Literal[True]
    identity: AnalysisIdentity
    loaded: LegacyLoaded
    analyzer: LegacyAnalyzer
    configuration: LegacyConfiguration
    raw: LegacyRaw
    compatibility: LegacyCompatibility


@dataclass(frozen=True, slots=True)
class CorrectedLegacyResult:
    """Decoded full arrays with their unchanged native publication contract."""

    envelope: LegacyEnvelope
    beat_frames: tuple[float, ...]
    downbeat_raw_indices: tuple[int, ...]
    beat_seconds: tuple[float, ...]
    downbeat_seconds: tuple[float, ...]
    compatibility_bpm: float
    compatibility_beats: tuple[float, ...]
    compatibility_downbeats: tuple[float, ...]
    compatibility_bars: tuple[float, ...]

    def timing_predictions(self) -> BeatPredictions:
        """Adapt event times to the existing pure temporal core; QM has no logits."""
        return BeatPredictions(self.beat_seconds, self.downbeat_seconds, (), ())


@dataclass(frozen=True, slots=True)
class CorrectedLegacyProfile:
    """Verifier-owned immutable anchors, frozen after independent producer review."""

    profile_id: str
    track_id: TrackId
    directory: str
    provenance_sha256: str
    artifacts: tuple[ApprovedArtifact, ...]


@dataclass(frozen=True, slots=True)
class CorrectedLegacyCandidate:
    """Supported complete native comparator, separate from selected Beat This candidates."""

    profile_id: str
    track_id: TrackId
    source_sha256: str
    source_bytes: int
    result: CorrectedLegacyResult
    artifact_bindings: tuple[ArtifactBinding, ...]

    def report(self) -> dict[str, object]:
        """Retain every complete raw/projection array and separately verified artifact."""
        return {
            "profile_id": self.profile_id,
            "track_id": self.track_id,
            "source_sha256": self.source_sha256,
            "source_bytes": self.source_bytes,
            "envelope": self.result.envelope.model_dump(mode="json"),
            "arrays": {
                name: getattr(self.result, name)
                for name in (
                    "beat_frames",
                    "downbeat_raw_indices",
                    "beat_seconds",
                    "downbeat_seconds",
                    "compatibility_bpm",
                    "compatibility_beats",
                    "compatibility_downbeats",
                    "compatibility_bars",
                )
            },
            "artifact_bindings": [asdict(binding) for binding in self.artifact_bindings],
            "musical_acceptance": "pending",
            "default_adoption": "blocked",
        }


def _typed_array(encoded: str, code: Literal["Q", "f"]) -> tuple[int, ...] | tuple[float, ...]:
    width = struct.calcsize(code)
    if len(encoded) > 4 * ((250000 * width + 2) // 3):
        msg = "legacy packed array exceeds complete extent limit"
        raise ValueError(msg)
    raw = base64.b64decode(encoded, validate=True)
    if len(raw) % width or len(raw) // width > 250000 or base64.b64encode(raw).decode() != encoded:
        msg = "legacy packed array is incomplete or noncanonical"
        raise ValueError(msg)
    return tuple(value for (value,) in struct.iter_unpack(f"<{code}", raw))


def _same_bits(left: tuple[float, ...], right: tuple[float, ...], code: str) -> bool:
    return struct.pack(f"<{len(left)}{code}", *left) == struct.pack(f"<{len(right)}{code}", *right)


def _envelope(raw: bytes) -> LegacyEnvelope:
    if len(raw) > MAX_ENVELOPE_BYTES:
        msg = "legacy native result exceeds publication limit"
        raise ValueError(msg)
    declared = unique_json(raw)
    if not isinstance(declared, dict) or declared.get("diagnostic_only") is not True:
        msg = "legacy diagnostic boolean must be true"
        raise ValueError(msg)
    for section, field, expected_type in (
        ("loaded", "origin_seconds", float),
        ("analyzer", "sample_rate_hz", int),
        ("configuration", "window_length", int),
        ("configuration", "hop_size", int),
        ("configuration", "step_secs", float),
        ("configuration", "max_bin_hz", float),
        ("configuration", "input_tempo", float),
        ("configuration", "alpha", float),
        ("configuration", "tightness", float),
        ("configuration", "viterbi_sigma", float),
    ):
        obj = declared.get(section)
        if not isinstance(obj, dict) or type(obj.get(field)) is not expected_type:
            msg = "legacy literal field must retain its declared numeric type"
            raise ValueError(msg)
    envelope = TypeAdapter(LegacyEnvelope).validate_json(raw, strict=True)
    analyzer, loaded = envelope.analyzer, envelope.loaded
    expected_frames = (
        loaded.frame_count * 44100 + loaded.sample_rate_hz - 1
    ) // loaded.sample_rate_hz
    if analyzer.frame_count != expected_frames or analyzer.odf_hop_samples != int(0.01161 * 44100):
        msg = "legacy analyzer dimensions or corrected ODF timebase mismatch"
        raise ValueError(msg)
    return envelope


def _decoded(envelope: LegacyEnvelope) -> CorrectedLegacyResult:
    compatibility = envelope.compatibility
    bpm = _typed_array(compatibility.bpm, "f")
    if len(bpm) != 1:
        msg = "legacy compatibility requires exactly one BPM value"
        raise ValueError(msg)
    return CorrectedLegacyResult(
        envelope,
        _unpack(envelope.raw.beat_frames),
        tuple(int(v) for v in _typed_array(envelope.raw.downbeat_raw_indices, "Q")),
        _unpack(envelope.raw.beat_seconds),
        _unpack(envelope.raw.downbeat_seconds),
        float(bpm[0]),
        tuple(float(v) for v in _typed_array(compatibility.beats, "f")),
        tuple(float(v) for v in _typed_array(compatibility.downbeats, "f")),
        tuple(float(v) for v in _typed_array(compatibility.bars, "f")),
    )


def _validate_positions(result: CorrectedLegacyResult) -> None:
    beats, indices = result.beat_seconds, result.downbeat_raw_indices
    arrays = (
        result.beat_frames,
        beats,
        result.downbeat_seconds,
        result.compatibility_beats,
        result.compatibility_downbeats,
        result.compatibility_bars,
    )
    if tuple(map(len, arrays)) != (
        len(beats),
        len(beats),
        len(indices),
        len(beats),
        len(indices),
        len(indices),
    ):
        msg = "legacy complete raw/projection array lengths mismatch"
        raise ValueError(msg)
    if (
        any(not math.isfinite(v) for array in arrays for v in array)
        or not math.isfinite(result.compatibility_bpm)
        or result.compatibility_bpm < 0
    ):
        msg = "legacy nonfinite or negative compatibility BPM"
        raise ValueError(msg)
    loaded = result.envelope.loaded
    duration = loaded.frame_count / loaded.sample_rate_hz
    if any(v < 0 or v >= duration for v in beats) or any(a >= b for a, b in pairwise(beats)):
        msg = "legacy beat positions outside complete source or nonmonotone"
        raise ValueError(msg)
    if any(i >= len(beats) for i in indices) or any(a >= b for a, b in pairwise(indices)):
        msg = "legacy downbeat raw associations invalid"
        raise ValueError(msg)
    analyzer = result.envelope.analyzer
    expected = tuple(
        frame * (analyzer.odf_hop_samples / analyzer.sample_rate_hz) for frame in result.beat_frames
    )
    if not _same_bits(beats, expected, "d") or not _same_bits(
        result.downbeat_seconds, tuple(beats[i] for i in indices), "d"
    ):
        msg = "legacy raw seconds do not preserve corrected integer-hop conversion"
        raise ValueError(msg)


def _validate_projection(result: CorrectedLegacyResult) -> None:
    if (
        not _same_bits(result.compatibility_beats, result.beat_seconds, "f")
        or not _same_bits(result.compatibility_downbeats, result.downbeat_seconds, "f")
        or not _same_bits(result.compatibility_bars, result.compatibility_downbeats, "f")
    ):
        msg = "legacy compatibility projection differs from complete raw evidence"
        raise ValueError(msg)
    total = 0.0
    for first, last in pairwise(result.beat_frames):
        total += last - first
    expected_bpm = (
        0.0
        if len(result.beat_frames) < 2
        else 60.0
        / (
            total
            / (len(result.beat_frames) - 1)
            * (result.envelope.analyzer.odf_hop_samples / 44100)
        )
    )
    if (
        not math.isfinite(expected_bpm)
        or abs(expected_bpm) > 3.4028234663852886e38
        or struct.pack("<f", result.compatibility_bpm) != struct.pack("<f", expected_bpm)
    ):
        msg = "legacy compatibility BPM differs from the same raw tracking result"
        raise ValueError(msg)


def decode_corrected_legacy(raw: bytes) -> CorrectedLegacyResult:
    """Validate the dedicated full native result without importing native code or PCM."""
    result = _decoded(_envelope(raw))
    _validate_positions(result)
    _validate_projection(result)
    return result
