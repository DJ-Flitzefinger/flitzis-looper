"""Bounded nominal-unit evidence; replay is saved-data validation, never inference."""

import base64
import binascii
import hashlib
import math
import struct
from typing import TYPE_CHECKING, Annotated, Literal, NoReturn

from pydantic import BaseModel, ConfigDict, Field, model_validator

from flitzis_looper.analysis.app_tempo_corroboration import (
    CorroborationSource,
    QmEvidence,
    validate_qm_evidence,
)
from flitzis_looper.analysis.app_tempo_probe import _close
from flitzis_looper.analysis.metrical_prior_setup import (
    E4_SHA256,
    FRONTEND_SHA256,
    INFERENCE_SHA256,
    NODE_SHA256,
    NUMERIC_PROFILE_SHA256,
    PYTHON_DLL_SHA256,
    PYTHON_SHA256,
    REFERENCE_NODE_SHA256,
)

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.analysis.app_tempo_models import AppTempoMethod
    from flitzis_looper.analysis.app_tempo_probe import TempoMathView

_Digest = Annotated[str, Field(strict=True, pattern=r"^[0-9a-f]{64}$")]
_Hex64 = Annotated[str, Field(strict=True, pattern=r"^[0-9a-f]{16}$")]
_Count = Annotated[int, Field(strict=True, ge=1, le=2**63 - 1)]
_Bpm = Annotated[float, Field(strict=True, allow_inf_nan=False, ge=30, le=300)]
_Zero = Annotated[float, Field(strict=True, allow_inf_nan=False, ge=0, le=0)]
_Bytes = Annotated[int, Field(strict=True, ge=0, le=536870912)]
_FeatureDimension = Annotated[int, Field(strict=True, ge=1, le=1024)]
CDF_SHA256 = "6bede0d19e36cec9466aa1ab20d07f8634b54828d5c056acd33c1f43b5a3bc52"
INPUT_SHA256 = "7230218c541822b40aa55db5148d98de1e5e4d2fd046bba09ae29845ec279523"
FRONTEND_SEAL_SHA256 = "487eebdad8e2c7c1bb8fda975577ee74072e006829a38396c0a4fde0181e921e"


def _invalid(message: str) -> NoReturn:
    raise ValueError(message)


def unpack_packet(data: str, expected_bytes: int, expected_sha256: str) -> bytes:
    """Check size before decode; require one canonical ASCII base64 spelling."""
    if len(data) != 4 * ((expected_bytes + 2) // 3):
        _invalid("Packed prior evidence exceeds its exact byte shape")
    try:
        raw = base64.b64decode(data, validate=True)
    except (ValueError, binascii.Error) as error:
        message = "Invalid canonical prior base64"
        raise ValueError(message) from error
    if (
        len(raw) != expected_bytes
        or base64.b64encode(raw).decode("ascii") != data
        or hashlib.sha256(raw).hexdigest() != expected_sha256
    ):
        _invalid("Packed prior bytes/hash differ")
    return raw


def f64_from_hex(value: str) -> float:
    """Read an exact binary64 statistic already constrained by its saved field."""
    return float(struct.unpack("<d", bytes.fromhex(value))[0])


class _Strict(BaseModel):
    model_config = ConfigDict(extra="forbid")


class PriorProfile(_Strict):
    profile_id: Literal["tempocnn-0.0.8-deeptemp-k16-3-tfjs4.22-node25.5-cpu-f32-v1"]
    profile_sha256: _Digest
    numeric_profile_sha256: _Digest
    python_version: Literal["3.12.13"]
    python_executable_sha256: _Digest
    python_dll_sha256: _Digest
    node_version: Literal["v25.5.0"]
    node_executable_sha256: _Digest
    tfjs_version: Literal["4.22.0"]
    graph_sha256: _Digest
    shard_sha256: _Digest
    frontend_reference_sha256: _Digest
    frontend_runner_sha256: _Digest
    inference_reference_sha256: _Digest
    inference_runner_sha256: _Digest

    @model_validator(mode="after")
    def _pinned(self) -> PriorProfile:
        expected = {
            "numeric_profile_sha256": NUMERIC_PROFILE_SHA256,
            "python_executable_sha256": PYTHON_SHA256,
            "python_dll_sha256": PYTHON_DLL_SHA256,
            "node_executable_sha256": NODE_SHA256,
            "graph_sha256": "f93693c5a913b69aa378438665937600c09286f57714359155f2763496aba184",
            "shard_sha256": "f7688e8868706813cd74610442f9923fca51162c1ebde6cf7fecfd62ebd24a15",
            "frontend_reference_sha256": E4_SHA256,
            "frontend_runner_sha256": FRONTEND_SHA256,
            "inference_reference_sha256": REFERENCE_NODE_SHA256,
            "inference_runner_sha256": INFERENCE_SHA256,
        }
        if any(getattr(self, key) != value for key, value in expected.items()):
            _invalid("Prior numeric/runtime/profile pins differ")
        return self


class PriorInput(_Strict):
    source_sha256: _Digest
    mono_sha256: _Digest
    sample_rate_hz: Annotated[int, Field(strict=True, ge=8000, le=384000)]
    frame_count: _Count
    origin_seconds: _Zero
    downmix_revision: Literal["arithmetic-channel-mean-f64-v1"]


class PriorNormalization(_Strict):
    scope: Literal["entire-overlapping-window-tensor"]
    precision: Literal["float64"]
    roundtrip: Literal["float16-to-float32"]
    mean_f64_le_hex: _Hex64
    std_f64_le_hex: _Hex64

    @model_validator(mode="after")
    def _finite_stats(self) -> PriorNormalization:
        mean, deviation = f64_from_hex(self.mean_f64_le_hex), f64_from_hex(self.std_f64_le_hex)
        if not math.isfinite(mean) or not math.isfinite(deviation) or deviation <= 0:
            _invalid("Prior normalization requires finite positive variance")
        return self


class PriorFrontend(_Strict):
    frontend_id: Literal["tempocnn-author-482c64ad-librosa011-global-std-f16-v1"]
    sample_rate_hz: Annotated[int, Field(strict=True, ge=11025, le=11025)]
    frame_count: _Count
    origin_seconds: _Zero
    mono_sha256: _Digest
    feature_shape: Annotated[list[_FeatureDimension], Field(min_length=4, max_length=4)]
    feature_sha256: _Digest
    normalization: PriorNormalization


class PriorProbabilities(_Strict):
    encoding: Literal["float32-le/base64"]
    shape: Annotated[list[_FeatureDimension], Field(min_length=2, max_length=2)]
    sha256: _Digest
    data: Annotated[str, Field(strict=True, max_length=1398104)]


class PriorGlobal(_Strict):
    encoding: Literal["float64-le/base64"]
    shape: Annotated[
        list[Annotated[int, Field(strict=True, ge=256, le=256)]],
        Field(min_length=1, max_length=1),
    ]
    sha256: _Digest
    data: Annotated[str, Field(strict=True, min_length=2732, max_length=2732)]
    first_argmax_class: Annotated[int, Field(strict=True, ge=30, le=285)]


class CachedPriorObservation(_Strict):
    runner_sha256: _Digest
    cdf_sha256: _Digest
    input_manifest_sha256: _Digest
    frontend_seal_sha256: _Digest
    numeric_profile_sha256: _Digest

    @model_validator(mode="after")
    def _sealed(self) -> CachedPriorObservation:
        if (
            self.runner_sha256,
            self.cdf_sha256,
            self.input_manifest_sha256,
            self.frontend_seal_sha256,
            self.numeric_profile_sha256,
        ) != (
            REFERENCE_NODE_SHA256,
            CDF_SHA256,
            INPUT_SHA256,
            FRONTEND_SEAL_SHA256,
            NUMERIC_PROFILE_SHA256,
        ):
            _invalid("Cached prior observation lacks its original sealed profile/input proof")
        return self


class PriorExecution(_Strict):
    origin: Literal["computed", "cached_observation"]
    device: Literal["cpu"]
    cached_observation: CachedPriorObservation | None = Field(
        default=None, exclude_if=lambda value: value is None
    )

    @model_validator(mode="before")
    @classmethod
    def _cached_presence(cls, value: object) -> object:
        if (
            isinstance(value, dict)
            and value.get("origin") == "computed"
            and "cached_observation" in value
        ):
            _invalid("Computed execution must omit historical cache proof")
        return value

    @model_validator(mode="after")
    def _origin(self) -> PriorExecution:
        if (self.origin == "cached_observation") != (self.cached_observation is not None):
            _invalid("Prior execution origin differs from its observation proof")
        return self


class PriorResources(_Strict):
    native_live_bytes: _Bytes
    aggregate_limit_bytes: Annotated[int, Field(strict=True, ge=536870912, le=536870912)]
    frontend_admitted_child_peak_bytes: _Bytes
    frontend_observed_child_peak_bytes: _Bytes
    classifier_observed_child_peak_bytes: _Bytes
    measurement: Literal["windows-private-and-working-set-peaks", "cached-rss-only"]
    aggregate_claim: Annotated[bool, Field(strict=True)]

    @model_validator(mode="after")
    def _bounded(self) -> PriorResources:
        if self.aggregate_claim != (self.measurement == "windows-private-and-working-set-peaks"):
            _invalid("Prior resource claim differs from its measurement")
        child_peak = max(
            self.frontend_admitted_child_peak_bytes,
            self.frontend_observed_child_peak_bytes,
            self.classifier_observed_child_peak_bytes,
        )
        if self.native_live_bytes + child_peak > self.aggregate_limit_bytes:
            _invalid("Prior held parent plus child exceeds512MiB")
        return self


class PriorEvidence(_Strict):
    profile: PriorProfile
    input: PriorInput
    frontend: PriorFrontend
    probabilities: PriorProbabilities
    global_: PriorGlobal = Field(alias="global", serialization_alias="global")
    execution: PriorExecution
    resources: PriorResources

    model_config = ConfigDict(extra="forbid", serialize_by_alias=True, validate_by_alias=True)

    @model_validator(mode="after")
    def _complete_geometry_and_cdf(self) -> PriorEvidence:
        target = (
            self.input.frame_count * 11025 + self.input.sample_rate_hz - 1
        ) // self.input.sample_rate_hz
        patches = 1 + (max(256, 1 + target // 512) - 256) // 128
        # Complete geometry and all dimensions are bounded before packed decoding.
        if (
            not 1 <= patches <= 1024
            or self.frontend.frame_count != target
            or self.frontend.feature_shape != [patches, 40, 256, 1]
            or self.probabilities.shape != [patches, 256]
        ):
            _invalid("Prior full-source/frontend/patch geometry differs")
        if self.execution.origin == "computed" and not self.resources.aggregate_claim:
            _invalid("Computed prior needs actual aggregate memory evidence")
        if self.execution.origin == "cached_observation" and self.resources.aggregate_claim:
            _invalid("Historical CDF must not invent current aggregate execution")
        raw = unpack_packet(self.probabilities.data, patches * 256 * 4, self.probabilities.sha256)
        sums = [0.0] * 256
        values = struct.iter_unpack("<f", raw)
        for _row in range(patches):
            total = 0.0
            for column in range(256):
                probability = next(values)[0]
                if not math.isfinite(probability) or not 0 <= probability <= 1:
                    _invalid("Prior probabilities are not finite unit values")
                total += probability
                sums[column] += probability
            if abs(total - 1) > 1e-5:
                _invalid("Prior row does not satisfy its unchanged softmax check")
        expected = b"".join(struct.pack("<d", total / patches) for total in sums)
        actual = unpack_packet(self.global_.data, 2048, self.global_.sha256)
        if actual != expected:
            _invalid("Prior global mean differs from sequential binary64 replay")
        means = struct.unpack("<256d", actual)
        if self.global_.first_argmax_class != max(range(256), key=means.__getitem__) + 30:
            _invalid("Prior first maximum differs from its exact mean")
        return self

    def means(self) -> tuple[float, ...]:
        """Return only the256 validated global means, never a full float matrix."""
        return struct.unpack("<256d", unpack_packet(self.global_.data, 2048, self.global_.sha256))


class PriorNeuralResult(_Strict):
    schema_version: Annotated[int, Field(strict=True, ge=4, le=5)]
    backend: Literal["beat_this"]
    policy: Literal["neural-stable-regions-v4", "neural-inferred-ordinals-v5"]
    status: Literal["estimated", "single_region_estimate"]
    bpm: _Bpm


class PriorRangeConstraint(_Strict):
    minimum_bpm: _Bpm
    maximum_bpm: _Bpm

    @model_validator(mode="after")
    def _bounds(self) -> PriorRangeConstraint:
        if self.minimum_bpm >= self.maximum_bpm:
            _invalid("Prior request range is not ordered")
        return self


class NominalSelection(_Strict):
    neural_result: PriorNeuralResult
    source: CorroborationSource
    qm: QmEvidence
    period_drift_seconds: Annotated[float, Field(strict=True, allow_inf_nan=False, ge=0, le=0.005)]
    range_constraint: PriorRangeConstraint | None = Field(
        default=None, exclude_if=lambda value: value is None
    )
    prior: PriorEvidence


def _candidate_class(bpm: float) -> int:
    candidate = math.floor(bpm + 0.5)
    if not 30 <= bpm <= 285 or bpm - math.floor(bpm) == 0.5 or not 30 <= candidate <= 285:
        _invalid("Nominal prior candidate has no unique supported class")
    return candidate


def validate_nominal_selection(
    method: AppTempoMethod,
    validate_math: Callable[[TempoMathView], None],
    validate_primary: Callable[[AppTempoMethod], None],
) -> None:
    """Replay independent timing and saved nominal evidence without runtime access."""
    evidence = method.nominal_selection
    if evidence is None:
        _invalid("Version eight requires its explicit nominal selection")
    header = evidence.neural_result
    primary = method.model_copy(
        update={
            "schema_version": header.schema_version,
            "backend": header.backend,
            "policy": header.policy,
            "status": header.status,
            "bpm": header.bpm,
            "nominal_selection": None,
        }
    )
    validate_primary(primary)
    chosen = evidence.qm.estimate
    if chosen.kernel_version not in {4, 5}:
        _invalid("Saved nominal selection retains its original QM kernel versions")
    validate_qm_evidence(method, evidence.source, evidence.qm, validate_math)
    source = evidence.prior.input
    if (
        source.source_sha256 != method.source_sha256
        or source.mono_sha256 != method.mono_sha256
        or source.sample_rate_hz != method.sample_rate_hz
        or source.frame_count != method.frame_count
    ):
        _invalid("Nominal prior differs from its complete neural mono source")
    nn_period, qm_period = 60.0 / header.bpm, 60.0 / chosen.bpm
    slow, fast = max(nn_period, qm_period), min(nn_period, qm_period)
    drift = abs(slow - 2 * fast) * (
        min(
            method.frame_count / method.sample_rate_hz,
            evidence.qm.analyzer.frame_count / evidence.qm.analyzer.sample_rate_hz,
        )
        / slow
    )
    if (
        not math.isfinite(drift)
        or drift > 0.005
        or not _close(drift, evidence.period_drift_seconds)
    ):
        _invalid("Nominal candidates lack full-extent half/double agreement")
    constraint = evidence.range_constraint
    if constraint is not None and not all(
        constraint.minimum_bpm <= bpm <= constraint.maximum_bpm for bpm in (header.bpm, chosen.bpm)
    ):
        _invalid("Nominal candidate escaped its captured request range")
    nn_class, qm_class = _candidate_class(header.bpm), _candidate_class(chosen.bpm)
    means = evidence.prior.means()
    maximum = max(means)
    if (
        nn_class == qm_class
        or means[qm_class - 30] != maximum
        or means.count(maximum) != 1
        or means[qm_class - 30] <= means[nn_class - 30]
    ):
        _invalid("Nominal evidence does not uniquely prefer the QM candidate")
    if method.bpm != chosen.bpm or method.status != chosen.status:
        _invalid("Nominal selected scalar/status differs from independent QM")
