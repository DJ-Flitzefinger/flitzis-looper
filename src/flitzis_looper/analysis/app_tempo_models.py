"""Persistable App estimates; these DTOs never authorize Native timing or maps."""

import math
from itertools import pairwise
from typing import TYPE_CHECKING, Annotated, Literal

from pydantic import BaseModel, ConfigDict, Field, field_validator, model_validator

from flitzis_looper.analysis.app_tempo_corroboration import QmCorroboration, validate_corroboration
from flitzis_looper.analysis.app_tempo_inferred import OrdinalFit, validate_ordinal_fit
from flitzis_looper.analysis.app_tempo_periods import validate_period_fit
from flitzis_looper.analysis.app_tempo_prior import NominalSelection, validate_nominal_selection
from flitzis_looper.analysis.app_tempo_probe import (
    AdaptiveTempoProbe,
    IntegerTempoProbe,
    TempoRegion,
    historical_probe,
    validate_adaptive_probe,
)
from flitzis_looper.analysis.app_tempo_range import BpmRangeResult, validate_bpm_range
from flitzis_looper.analysis.execution import (
    ACTUAL_EXECUTION_ADAPTER,
    ActualExecutionReceipt,
    validate_execution,
)

if TYPE_CHECKING:
    from flitzis_looper.analysis.app_tempo_probe import TempoMathView

_Digest = Annotated[str, Field(pattern=r"^[0-9a-f]{64}$")]
_Time = Annotated[float, Field(allow_inf_nan=False, ge=0)]


class AppTempoMethod(BaseModel):
    """Source/protocol provenance and raw positions, without job/staging handles."""

    model_config = ConfigDict(extra="forbid")

    schema_version: Literal[1, 2, 3, 4, 5, 6, 7, 8, 9]
    backend: Literal[
        "beat_this", "beat_this_qm_dsp", "beat_this_bpm_range", "beat_this_qm_tempocnn"
    ]
    policy: Literal[
        "neural-stable-regions-v1",
        "neural-stable-regions-v2",
        "neural-stable-regions-v3",
        "neural-stable-regions-v4",
        "neural-inferred-ordinals-v5",
        "neural-qm-same-period-v6",
        "neural-user-range-v7",
        "neural-qm-nominal-prior-v8",
        "neural-inferred-periods-v9",
    ]
    status: Literal[
        "estimated", "single_region_estimate", "insufficient", "invalid", "uncorroborated"
    ]
    bpm: Annotated[float | None, Field(allow_inf_nan=False, ge=30, le=300)]
    selected: TempoRegion | None
    regions: Annotated[list[TempoRegion], Field(max_length=5)]
    integer_probe: IntegerTempoProbe | AdaptiveTempoProbe | None = Field(
        default=None, exclude_if=lambda value: value is None
    )
    ordinal_fit: OrdinalFit | None = Field(default=None, exclude_if=lambda value: value is None)
    model_sha256: _Digest
    frontend_id: Literal["beat-this-1.1.0-soxr-hq-logmel-v1"]
    source_sha256: _Digest
    mono_sha256: _Digest
    sample_rate_hz: Annotated[int, Field(ge=8000, le=384_000)]
    frame_count: Annotated[int, Field(gt=0, le=2**63 - 1)]
    raw_beats: Annotated[list[_Time], Field(max_length=250_000)]
    raw_downbeats: Annotated[list[_Time], Field(max_length=250_000)]
    execution: ActualExecutionReceipt | None = Field(
        default=None, exclude_if=lambda value: value is None
    )
    qm_corroboration: QmCorroboration | None = Field(
        default=None, exclude_if=lambda value: value is None
    )
    bpm_range: BpmRangeResult | None = Field(default=None, exclude_if=lambda value: value is None)
    nominal_selection: NominalSelection | None = Field(
        default=None, exclude_if=lambda value: value is None
    )

    @model_validator(mode="before")
    @classmethod
    def _reject_legacy_corroboration(cls, value: object) -> object:
        if (
            isinstance(value, dict)
            and value.get("schema_version") not in {6, 7}
            and "qm_corroboration" in value
        ):
            message = "Acoustic corroboration fields require version six"
            raise ValueError(message)
        if (
            isinstance(value, dict)
            and value.get("schema_version") in {6, 7, 8, 9}
            and any(
                type(value.get(key)) is not int
                for key in ("schema_version", "sample_rate_hz", "frame_count")
            )
        ):
            message = "Joint source geometry requires strict integer fields"
            raise ValueError(message)
        return value

    @model_validator(mode="before")
    @classmethod
    def _range_field_presence(cls, value: object) -> object:
        if not isinstance(value, dict):
            return value
        if value.get("schema_version") != 7 and "bpm_range" in value:
            message = "Automatic range fields require version seven"
            raise ValueError(message)
        if value.get("schema_version") == 7 and "qm_corroboration" in value:
            evidence = value.get("bpm_range")
            primary_version: object
            if isinstance(evidence, BpmRangeResult):
                primary_version = evidence.primary.schema_version
            else:
                primary = evidence.get("primary") if isinstance(evidence, dict) else None
                primary_version = (
                    primary.get("schema_version") if isinstance(primary, dict) else None
                )
            if primary_version != 6:
                message = "Range corroboration fields require an original version six primary"
                raise ValueError(message)
        return value

    @model_validator(mode="before")
    @classmethod
    def _prior_field_presence(cls, value: object) -> object:
        if (
            isinstance(value, dict)
            and value.get("schema_version") != 8
            and "nominal_selection" in value
        ):
            message = "Nominal selection fields require version eight"
            raise ValueError(message)
        return value

    @field_validator("execution", mode="before")
    @classmethod
    def _load_execution(cls, value: object) -> ActualExecutionReceipt | None:
        # Copy snapshots validate Python dumps strictly. Reconstruct this embedded
        # dataclass without overriding its explicit strict fields or extra guard.
        return None if value is None else ACTUAL_EXECUTION_ADAPTER.validate_python(value)

    @model_validator(mode="after")
    def _coherent_estimate(self) -> AppTempoMethod:
        self._validate_estimate()
        return self

    def _validate_estimate(self) -> None:
        if self.execution is not None:
            if self.schema_version not in {4, 5, 6, 7, 8, 9}:
                message = "Execution receipts require tempo metadata version four through nine"
                raise ValueError(message)
            validate_execution(self.execution)
        self._validate_policy_version()
        if self.schema_version == 8:
            validate_nominal_selection(self, validate_math_estimate, validate_original_method)
        elif self.schema_version == 7:
            validate_bpm_range(self, validate_math_estimate, validate_original_method)
        elif self.schema_version == 6:
            validate_corroboration(
                self,
                validate_math_estimate,
                lambda primary: validate_math_estimate(primary, phase_only=True),
            )
        else:
            validate_math_estimate(self)

    def _validate_policy_version(self) -> None:
        if self.schema_version == 8:
            self._validate_prior_policy()
            return
        if self.schema_version == 7:
            self._validate_range_policy()
            return
        if self.schema_version == 6:
            self._validate_joint_policy()
            return
        expected = {5: "neural-inferred-ordinals-v5", 9: "neural-inferred-periods-v9"}.get(
            self.schema_version, f"neural-stable-regions-v{self.schema_version}"
        )
        if (
            self.backend != "beat_this"
            or self.policy != expected
            or (self.schema_version == 1 and self.integer_probe is not None)
        ):
            message = "Tempo policy differs from its metadata version"
            raise ValueError(message)
        if (self.schema_version in {5, 9}) != (self.ordinal_fit is not None):
            message = "Inferred evidence requires its explicit version five or nine policy"
            raise ValueError(message)

    def _validate_range_policy(self) -> None:
        if (
            self.backend != "beat_this_bpm_range"
            or self.policy != "neural-user-range-v7"
            or self.bpm_range is None
        ):
            message = "Version seven requires explicit automatic range evidence"
            raise ValueError(message)
        if (self.bpm_range.primary.schema_version == 6) != (self.qm_corroboration is not None):
            message = "Range metadata must retain exactly its original joint evidence"
            raise ValueError(message)

    def _validate_prior_policy(self) -> None:
        if (
            self.backend != "beat_this_qm_tempocnn"
            or self.policy != "neural-qm-nominal-prior-v8"
            or self.nominal_selection is None
        ):
            message = "Version eight requires explicit nominal prior evidence"
            raise ValueError(message)
        if self.bpm_range is not None or self.qm_corroboration is not None:
            message = "Nominal prior evidence cannot nest other scalar wrappers"
            raise ValueError(message)

    def _validate_joint_policy(self) -> None:
        if (
            self.backend != "beat_this_qm_dsp"
            or self.policy != "neural-qm-same-period-v6"
            or self.qm_corroboration is None
        ):
            message = "Version six requires explicit joint acoustic evidence"
            raise ValueError(message)


def validate_original_method(method: AppTempoMethod) -> None:
    """Validate a shallow original header without duplicating its raw observations."""
    method._validate_estimate()


def validate_math_estimate(method: TempoMathView, *, phase_only: bool = False) -> None:
    """Replay existing scalar mathematics without model or execution identity."""
    duration = method.frame_count / method.sample_rate_hz
    _validate_observations(method, duration)
    if method.schema_version == 9:
        validate_period_fit(method, phase_only=phase_only)
    elif method.schema_version == 5:
        validate_ordinal_fit(method, phase_only=phase_only)
    else:
        if phase_only:
            message = "Phase-only corroboration requires original version five evidence"
            raise ValueError(message)
        _validate_selected_estimate(method, duration)
    _validate_regions(method, duration)


def _validate_selected_estimate(method: TempoMathView, duration: float) -> None:
    if method.status in {"estimated", "single_region_estimate"}:
        if method.bpm is None or method.selected is None:
            message = "Estimated tempo requires its matching selected region"
            raise ValueError(message)
        if method.status == "estimated" and method.selected.retained < 24:
            message = "Long-region estimates require at least 24 retained observations"
            raise ValueError(message)
        if method.selected not in method.regions:
            message = "Selected tempo must be one of the retained regions"
            raise ValueError(message)
        expected = method.selected.bpm
        if method.schema_version >= 2:
            _validate_integer_probe(method, duration)
            probe = method.integer_probe
            if probe is not None and probe.status == "supported_integer_estimate":
                expected = probe.candidate_bpm
        if method.bpm != expected:
            message = "Final tempo contradicts the selected fit and integer counterprobe"
            raise ValueError(message)
    elif method.bpm is not None or method.selected is not None:
        message = "Unavailable tempo must not carry a selected BPM"
        raise ValueError(message)
    elif method.integer_probe is not None:
        message = "Unavailable tempo cannot carry an integer preference"
        raise ValueError(message)


def _validate_observations(method: TempoMathView, duration: float) -> None:
    for values in (method.raw_beats, method.raw_downbeats):
        if any(not math.isfinite(t) or t >= duration for t in values) or any(
            a >= b for a, b in pairwise(values)
        ):
            message = "Tempo observations must be ordered inside the complete source"
            raise ValueError(message)


def _validate_regions(method: TempoMathView, duration: float) -> None:
    extra = method.integer_probe.windows if method.integer_probe is not None else []
    for region in [*method.regions, *extra]:
        start, end = region.window_seconds
        if not start < end <= duration or any(
            not 0 <= i < len(method.raw_beats) for i in region.excluded_raw_indices
        ):
            message = "Tempo region differs from complete observation extent"
            raise ValueError(message)
        if method.schema_version >= 2:
            excluded = region.excluded_raw_indices
            available = sum(start <= t < end for t in method.raw_beats)
            if (
                excluded != sorted(set(excluded))
                or any(not start <= method.raw_beats[i] < end for i in excluded)
                or region.retained != available - len(excluded)
                or region.rms_residual_seconds > region.max_residual_seconds
            ):
                message = "Tempo region retention differs from its original observations"
                raise ValueError(message)


def _validate_integer_probe(method: TempoMathView, duration: float) -> None:
    probe = method.integer_probe
    if method.schema_version == 4:
        validate_adaptive_probe(method, probe)
        return
    probe = historical_probe(probe)
    if probe is None or method.selected is None or probe.base_bpm != method.selected.bpm:
        message = "Version two requires the counterprobe of its base regional estimate"
        raise ValueError(message)
    if probe.candidate_bpm != min(300, max(30, math.floor(probe.base_bpm + 0.5))):
        message = "Counterprobe must evaluate the nearest integer BPM"
        raise ValueError(message)
    if probe.common_region is not None and (
        probe.common_region not in method.regions
        or probe.common_region.window_seconds != [0.2 * duration, 0.8 * duration]
    ):
        message = "Counterprobe common coverage differs from the stable middle fit"
        raise ValueError(message)
    expected_reasons = {
        "supported_integer_estimate": {"two_windows_and_common_phase"},
        "fractional_estimate": {"long_span_mismatch", "fractional_preferred"},
        "inconclusive": {"insufficient_inner_coverage"},
    }
    if probe.reason not in expected_reasons[probe.status]:
        message = "Counterprobe status contradicts its reason"
        raise ValueError(message)
    if probe.status == "supported_integer_estimate":
        _validate_integer_support(method, probe, duration)


def _validate_integer_support(
    method: TempoMathView, probe: IntegerTempoProbe, duration: float
) -> None:
    lower, upper = 0.2 * duration, 0.8 * duration
    if (
        abs(probe.base_bpm - probe.candidate_bpm) > 0.005
        or probe.common_region is None
        or probe.common_region.retained < 24
        or [w.window_seconds for w in probe.windows]
        != [[lower, lower + 16.0], [upper - 16.0, upper]]
        or upper - lower < 42.0
    ):
        message = "Integer estimate lacks stable inner coverage and base proximity"
        raise ValueError(message)
    maximum = probe.common_max_residual_seconds
    rms = probe.common_rms_residual_seconds
    penalty = probe.rms_penalty_seconds
    difference = probe.window_phase_difference_seconds
    if maximum is None or rms is None or penalty is None or difference is None:
        message = "Integer estimate lacks common phase diagnostics"
        raise ValueError(message)
    if (
        maximum > min(0.025, 0.05 * (60 / probe.candidate_bpm))
        or rms > maximum
        or penalty > rms
        or penalty > 0.00025
        or difference > 0.005
    ):
        message = "Integer estimate does not satisfy its declared finite policy"
        raise ValueError(message)
    _validate_integer_window_spans(method, probe)


def _validate_integer_window_spans(method: TempoMathView, probe: IntegerTempoProbe) -> None:
    if probe.common_region is None:
        message = "Integer estimate lacks its common region"
        raise ValueError(message)
    common_excluded = set(probe.common_region.excluded_raw_indices)
    for window in probe.windows:
        start, end = window.window_seconds
        # Local exclusions qualify stability only, not the shared phase.
        for excluded in (set(window.excluded_raw_indices), common_excluded):
            retained = [
                t for i, t in enumerate(method.raw_beats) if start <= t < end and i not in excluded
            ]
            if len(retained) < 8 or retained[-1] - retained[0] < 10:
                message = "Integer estimate lacks actual ten-second window coverage"
                raise ValueError(message)
