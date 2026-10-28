"""Saved future-request range policy, replayed without inference or timing authority."""

import math
from typing import TYPE_CHECKING, Annotated, Literal

from pydantic import BaseModel, ConfigDict, Field, model_validator

from flitzis_looper.analysis.app_tempo_corroboration import (
    CorroborationSource,
    QmEvidence,
    validate_qm_evidence,
)
from flitzis_looper.analysis.app_tempo_probe import _close

if TYPE_CHECKING:
    from collections.abc import Callable

    from flitzis_looper.analysis.app_tempo_models import AppTempoMethod
    from flitzis_looper.analysis.app_tempo_probe import TempoMathView

_Bpm = Annotated[float, Field(strict=True, allow_inf_nan=False, ge=30, le=300)]


class RangePrimary(BaseModel):
    model_config = ConfigDict(extra="forbid")

    schema_version: Annotated[int, Field(strict=True, ge=4, le=6)]
    backend: Literal["beat_this", "beat_this_qm_dsp"]
    policy: Literal[
        "neural-stable-regions-v4", "neural-inferred-ordinals-v5", "neural-qm-same-period-v6"
    ]
    status: Literal["estimated", "single_region_estimate"]
    bpm: _Bpm


class RangeSelection(BaseModel):
    model_config = ConfigDict(extra="forbid")

    source: CorroborationSource
    qm: QmEvidence
    period_drift_seconds: Annotated[float, Field(strict=True, allow_inf_nan=False, ge=0, le=0.005)]


class BpmRangeResult(BaseModel):
    model_config = ConfigDict(extra="forbid")

    minimum_bpm: _Bpm
    maximum_bpm: _Bpm
    primary: RangePrimary
    outcome: Literal["qm_half_double", "no_supported_candidate"]
    selection: RangeSelection | None = Field(default=None, exclude_if=lambda value: value is None)

    @model_validator(mode="before")
    @classmethod
    def _selection_presence(cls, value: object) -> object:
        if (
            isinstance(value, dict)
            and value.get("outcome") == "no_supported_candidate"
            and "selection" in value
        ):
            message = "A rejected automatic range result must omit selection"
            raise ValueError(message)
        return value

    @model_validator(mode="after")
    def _coherent_range(self) -> BpmRangeResult:
        if self.minimum_bpm >= self.maximum_bpm:
            message = "Automatic range bounds must be ordered"
            raise ValueError(message)
        if self.minimum_bpm <= self.primary.bpm <= self.maximum_bpm:
            message = "An in-range primary must retain its original method"
            raise ValueError(message)
        if (self.outcome == "qm_half_double") != (self.selection is not None):
            message = "Automatic range outcome differs from its acoustic selection"
            raise ValueError(message)
        return self


def validate_bpm_range(
    method: AppTempoMethod,
    validate_math: Callable[[TempoMathView], None],
    validate_primary: Callable[[AppTempoMethod], None],
) -> None:
    evidence = method.bpm_range
    if evidence is None:
        message = "Version seven requires its explicit automatic range result"
        raise ValueError(message)
    header = evidence.primary
    primary = method.model_copy(
        update={
            "schema_version": header.schema_version,
            "backend": header.backend,
            "policy": header.policy,
            "status": header.status,
            "bpm": header.bpm,
            "bpm_range": None,
        }
    )
    validate_primary(primary)
    selection = evidence.selection
    if selection is None:
        if method.status != "insufficient" or method.bpm is not None:
            message = "An unsupported range must publish no new automatic BPM"
            raise ValueError(message)
        return
    if header.schema_version not in {4, 5}:
        message = "Range acoustic fallback requires an independently successful neural primary"
        raise ValueError(message)
    validate_qm_evidence(method, selection.source, selection.qm, validate_math)
    chosen = selection.qm.estimate
    if not evidence.minimum_bpm <= chosen.bpm <= evidence.maximum_bpm:
        message = "Selected acoustic BPM lies outside its recorded request range"
        raise ValueError(message)
    nn_period, qm_period = 60.0 / header.bpm, 60.0 / chosen.bpm
    slow, fast = max(nn_period, qm_period), min(nn_period, qm_period)
    extent = min(
        method.frame_count / method.sample_rate_hz,
        selection.qm.analyzer.frame_count / selection.qm.analyzer.sample_rate_hz,
    )
    drift = abs(slow - 2 * fast) * (extent / slow)
    if (
        not math.isfinite(drift)
        or drift > 0.005
        or not _close(selection.period_drift_seconds, drift)
    ):
        message = "Selected acoustic BPM lacks full-extent half/double agreement"
        raise ValueError(message)
    if method.bpm != chosen.bpm or method.status != chosen.status:
        message = "Range scalar/status differs from the independently admitted QM value"
        raise ValueError(message)
