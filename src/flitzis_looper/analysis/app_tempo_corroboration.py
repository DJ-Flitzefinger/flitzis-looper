"""Bounded acoustic support for an unchanged phase-refused neural estimate."""

import hashlib
import json
import math
from dataclasses import dataclass
from itertools import pairwise
from typing import TYPE_CHECKING, Annotated, Literal, NoReturn

from pydantic import BaseModel, ConfigDict, Field, model_validator

from flitzis_looper.analysis.app_tempo_inferred import EvaluatedOrdinalFit, OrdinalFit
from flitzis_looper.analysis.app_tempo_probe import (
    AdaptiveTempoProbe,
    IntegerTempoProbe,
    TempoRegion,
    _close,
)

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.analysis.app_tempo_models import AppTempoMethod
    from flitzis_looper.analysis.app_tempo_probe import TempoMathView

_Digest = Annotated[str, Field(strict=True, pattern=r"^[0-9a-f]{64}$")]
_Number = Annotated[float, Field(strict=True, allow_inf_nan=False)]
_Zero = Annotated[float, Field(strict=True, allow_inf_nan=False, ge=0, le=0)]
_Count = Annotated[int, Field(strict=True, ge=1, le=2**63 - 1)]
_Index = Annotated[int, Field(strict=True, ge=0, le=249_999)]
_Bpm = Annotated[float, Field(allow_inf_nan=False, ge=30, le=300)]


def _invalid(message: str) -> NoReturn:
    raise ValueError(message)


class NeuralResult(BaseModel):
    model_config = ConfigDict(extra="forbid")

    schema_version: Annotated[int, Field(strict=True, ge=5, le=5)]
    policy: Literal["neural-inferred-ordinals-v5"]
    status: Literal["insufficient", "uncorroborated"]
    bpm: None


class CorroborationSource(BaseModel):
    model_config = ConfigDict(extra="forbid")

    source_sha256: _Digest
    decoder_identity: _Digest
    decoder_descriptor_json: Annotated[str, Field(strict=True, min_length=1, max_length=65536)]
    loaded_sample_rate_hz: Annotated[int, Field(strict=True, ge=8000, le=384_000)]
    loaded_channels: Annotated[int, Field(strict=True, ge=1, le=2)]
    loaded_frame_count: _Count
    loaded_origin_seconds: _Zero
    loaded_interleaved_sha256: _Digest
    verified_nn_mono_sha256: _Digest

    @model_validator(mode="after")
    def _decoder_binding(self) -> CorroborationSource:
        encoded = self.decoder_descriptor_json.encode("utf-8")
        if len(encoded) > 65536 or hashlib.sha256(encoded).hexdigest() != self.decoder_identity:
            _invalid("Decoder identity differs from its exact bounded descriptor bytes")
        _check_json_depth(self.decoder_descriptor_json)
        descriptor: object = json.loads(
            self.decoder_descriptor_json, object_pairs_hook=_unique_object, parse_constant=_invalid
        )
        if _descriptor_size(descriptor) > 65536:
            _invalid("Decoder descriptor exceeds its existing allocation bound")
        _check_decoder(descriptor, self.source_sha256)
        return self


class QmConfiguration(BaseModel):
    model_config = ConfigDict(extra="forbid")

    alpha: _Number
    hop_size: Annotated[int, Field(strict=True, ge=128, le=128)]
    input_tempo: _Number
    max_bin_hz: _Number
    step_secs: _Number
    tightness: _Number
    viterbi_sigma: _Number
    window_length: Annotated[int, Field(strict=True, ge=512, le=512)]

    @model_validator(mode="after")
    def _unchanged_defaults(self) -> QmConfiguration:
        actual = (
            self.alpha,
            self.input_tempo,
            self.max_bin_hz,
            self.step_secs,
            self.tightness,
            self.viterbi_sigma,
        )
        if actual != (0.9, 120.0, 50.0, 0.01161, 4.0, 8.0):
            _invalid("Acoustic support requires the unchanged QM configuration")
        return self


class QmAnalyzer(BaseModel):
    model_config = ConfigDict(extra="forbid")

    sample_rate_hz: Annotated[int, Field(strict=True, ge=44100, le=44100)]
    frame_count: _Count
    origin_seconds: _Zero
    f64_le_sha256: _Digest
    resampler: Literal["identity-44100-v1", "rubato-fft-1.0-44100-delay-trim-tail-flush-v1"]


class QmEstimate(BaseModel):
    """The existing scalar kernel's numeric evidence, without a neural identity."""

    model_config = ConfigDict(extra="forbid")

    kernel_version: Annotated[int, Field(strict=True, ge=4, le=5)]
    status: Literal["estimated", "single_region_estimate"]
    bpm: _Bpm
    selected: TempoRegion | None
    regions: Annotated[list[TempoRegion], Field(max_length=5)]
    integer_probe: IntegerTempoProbe | AdaptiveTempoProbe | None = Field(
        default=None, exclude_if=lambda value: value is None
    )
    ordinal_fit: OrdinalFit | None = Field(default=None, exclude_if=lambda value: value is None)

    @model_validator(mode="after")
    def _kernel_shape(self) -> QmEstimate:
        if (self.kernel_version == 5) != (self.ordinal_fit is not None):
            _invalid("QM scalar evidence differs from its numeric kernel version")
        return self


class QmEvidence(BaseModel):
    model_config = ConfigDict(extra="forbid")

    backend: Literal["qm_dsp"]
    device: Literal["cpu"]
    configuration: QmConfiguration
    normal_mono_f32_sha256: _Digest
    downmix_revision: Literal[
        "normal-analysis-mono-identity-f32-v1",
        "normal-analysis-stereo-f32-add-multiply-half-v1",
    ]
    analyzer: QmAnalyzer
    odf_hop_samples: Annotated[int, Field(strict=True, ge=1, le=2**20)]
    raw_beat_frames: Annotated[
        list[Annotated[float, Field(strict=True, allow_inf_nan=False, ge=0)]],
        Field(max_length=250_000),
    ]
    raw_downbeat_indices: Annotated[list[_Index], Field(max_length=250_000)]
    estimate: QmEstimate

    @model_validator(mode="after")
    def _raw_coordinates(self) -> QmEvidence:
        if self.odf_hop_samples != int(self.analyzer.sample_rate_hz * self.configuration.step_secs):
            _invalid("QM evidence differs from its actual configured ODF hop")
        if any(a >= b for a, b in pairwise(self.raw_beat_frames)) or any(
            a >= b for a, b in pairwise(self.raw_downbeat_indices)
        ):
            _invalid("QM coordinates must retain their ordered original associations")
        if any(index >= len(self.raw_beat_frames) for index in self.raw_downbeat_indices):
            _invalid("QM downbeat index lies outside its original beat coordinates")
        return self


class QmCorroboration(BaseModel):
    model_config = ConfigDict(extra="forbid")

    neural_result: NeuralResult
    source: CorroborationSource
    qm: QmEvidence
    period_drift_seconds: Annotated[float, Field(allow_inf_nan=False, ge=0, le=0.005)]


@dataclass(frozen=True, slots=True)
class QmMathView:
    """Borrowed numeric arrays only; this view has no model or device receipt."""

    schema_version: int
    status: str
    bpm: float | None
    selected: TempoRegion | None
    regions: list[TempoRegion]
    integer_probe: IntegerTempoProbe | AdaptiveTempoProbe | None
    ordinal_fit: OrdinalFit | None
    sample_rate_hz: int
    frame_count: int
    raw_beats: list[float]
    raw_downbeats: list[float]


def validate_corroboration(
    method: AppTempoMethod,
    validate_math: Callable[[TempoMathView], None],
    validate_neural: Callable[[TempoMathView], None],
) -> None:
    evidence = method.qm_corroboration
    if evidence is None:
        _invalid("Joint scalar selection requires its explicit acoustic evidence")
    primary = method.model_copy(
        update={
            "schema_version": 5,
            "backend": "beat_this",
            "policy": evidence.neural_result.policy,
            "status": evidence.neural_result.status,
            "bpm": None,
            "qm_corroboration": None,
        }
    )
    primary._validate_policy_version()
    validate_neural(primary)
    validate_qm_evidence(method, evidence.source, evidence.qm, validate_math)
    _check_selection(method, evidence)


def validate_qm_evidence(
    method: AppTempoMethod,
    source: CorroborationSource,
    qm: QmEvidence,
    validate_math: Callable[[TempoMathView], None],
) -> None:
    """Reuse the same complete source preparation and independent scalar replay."""
    _check_source_extent(method, source, qm)
    step = qm.odf_hop_samples / qm.analyzer.sample_rate_hz
    beats = [frame * step for frame in qm.raw_beat_frames]
    estimate = qm.estimate
    view = QmMathView(
        estimate.kernel_version,
        estimate.status,
        estimate.bpm,
        estimate.selected,
        estimate.regions,
        estimate.integer_probe,
        estimate.ordinal_fit,
        qm.analyzer.sample_rate_hz,
        qm.analyzer.frame_count,
        beats,
        [beats[index] for index in qm.raw_downbeat_indices],
    )
    validate_math(view)


def _check_source_extent(
    method: AppTempoMethod, source: CorroborationSource, qm: QmEvidence
) -> None:
    if (
        source.source_sha256 != method.source_sha256
        or source.verified_nn_mono_sha256 != method.mono_sha256
        or source.loaded_sample_rate_hz != method.sample_rate_hz
        or source.loaded_frame_count != method.frame_count
    ):
        _invalid("Acoustic support differs from its complete neural source domain")
    expected_frames = (
        method.frame_count * 44100 + method.sample_rate_hz - 1
    ) // method.sample_rate_hz
    resampler = (
        "identity-44100-v1"
        if method.sample_rate_hz == 44100
        else "rubato-fft-1.0-44100-delay-trim-tail-flush-v1"
    )
    downmix = (
        "normal-analysis-mono-identity-f32-v1"
        if source.loaded_channels == 1
        else "normal-analysis-stereo-f32-add-multiply-half-v1"
    )
    if (
        qm.analyzer.frame_count != expected_frames
        or qm.analyzer.resampler != resampler
        or qm.downmix_revision != downmix
    ):
        _invalid("QM analyzer must retain its complete actual source preparation")


def _check_selection(method: AppTempoMethod, evidence: QmCorroboration) -> None:
    fit = method.ordinal_fit
    if not isinstance(fit, EvaluatedOrdinalFit) or fit.winner is None:
        _invalid("Joint scalar selection requires its unchanged neural free model")
    nn_period = fit.winner.period_seconds
    qm_period = 60.0 / evidence.qm.estimate.bpm
    extent = min(
        method.frame_count / method.sample_rate_hz,
        evidence.qm.analyzer.frame_count / evidence.qm.analyzer.sample_rate_hz,
    )
    drift = abs(qm_period - nn_period) * (extent / nn_period)
    if (
        not math.isfinite(drift)
        or drift > 0.005
        or not _close(evidence.period_drift_seconds, drift)
    ):
        _invalid("Acoustic scalar differs from its full-extent same-period agreement")
    if method.bpm != evidence.qm.estimate.bpm or method.status != evidence.qm.estimate.status:
        _invalid("Joint scalar selection differs from the independently admitted QM value")


def _unique_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            _invalid("Decoder descriptor contains duplicate fields")
        result[key] = value
    return result


def _check_json_depth(text: str) -> None:
    depth, quoted, escaped = 0, False, False
    for character in text:
        if quoted:
            if escaped:
                escaped = False
            elif character == "\\":
                escaped = True
            elif character == '"':
                quoted = False
        elif character == '"':
            quoted = True
        elif character in "[{":
            depth += 1
            if depth > 33:
                _invalid("Decoder descriptor exceeds its existing nesting bound")
        elif character in "]}":
            depth -= 1


def _descriptor_size(value: object, depth: int = 0) -> int:
    if depth > 32:
        _invalid("Decoder descriptor exceeds its existing nesting bound")
    size = 16
    if isinstance(value, dict):
        size += sum(
            len(key.encode("utf-8")) + 32 + _descriptor_size(v, depth + 1)
            for key, v in value.items()
        )
    elif isinstance(value, list):
        size += sum(_descriptor_size(v, depth + 1) for v in value)
    elif isinstance(value, str):
        size += len(value.encode("utf-8"))
    elif isinstance(value, float) and not math.isfinite(value):
        _invalid("Decoder descriptor contains nonfinite values")
    return size


def _uint(value: object, *, minimum: int = 1, maximum: int = 2**64 - 1) -> bool:
    return type(value) is int and minimum <= value <= maximum


def _digest(value: object) -> bool:
    return (
        isinstance(value, str) and len(value) == 64 and all(c in "0123456789abcdef" for c in value)
    )


def _check_decoder(value: object, source_sha256: str) -> None:
    if not isinstance(value, dict):
        _invalid("Decoder descriptor must be the existing source object")
    original, pcm = value.get("original"), value.get("pcm")
    if not isinstance(original, dict) or not isinstance(pcm, dict):
        _invalid("Decoder descriptor lacks original or PCM geometry")
    if (
        type(value.get("schema_version")) is not int
        or value["schema_version"] != 1
        or original.get("sha256") != source_sha256
        or not _uint(original.get("bytes"))
    ):
        _invalid("Decoder descriptor differs from its original source identity")
    _check_decoder_pcm(pcm)


def _check_decoder_pcm(pcm: dict[str, object]) -> None:
    if (
        not _uint(pcm.get("full_frames"))
        or not _uint(pcm.get("channels"), maximum=32)
        or not _uint(pcm.get("rate_hz"), maximum=2**32 - 1)
    ):
        _invalid("Decoder descriptor has invalid existing PCM dimensions")
    frames, channels = pcm["full_frames"], pcm["channels"]
    if not isinstance(frames, int) or not isinstance(channels, int):
        _invalid("Decoder descriptor requires integer PCM dimensions")
    full_bytes = frames * channels * 4
    required = {
        "format": "f32-le-interleaved-v1",
        "channel_layout": "interleaved-channel-index-order-v1",
        "source_zero_bits": "0000000000000000",
        "full_bytes": full_bytes,
    }
    if full_bytes > 2**64 - 1 or any(pcm.get(k) != v for k, v in required.items()):
        _invalid("Decoder descriptor differs from its complete PCM encoding")
    if (
        not isinstance(pcm.get("provenance"), dict)
        or not isinstance(pcm.get("mono_revision"), str)
        or not pcm["mono_revision"]
        or not all(_digest(pcm.get(key)) for key in ("interleaved_sha256", "mono_sha256"))
        or type(pcm.get("full_bytes")) is not int
    ):
        _invalid("Decoder descriptor lacks its existing PCM provenance")
