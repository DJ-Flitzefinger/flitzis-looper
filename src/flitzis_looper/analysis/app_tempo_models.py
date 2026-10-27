"""Persistable App estimates; these DTOs never authorize Native timing or maps."""

import math
from itertools import pairwise
from typing import Annotated, Literal

from pydantic import BaseModel, ConfigDict, Field, model_validator

from flitzis_looper.analysis.app_tempo_probe import (
    AdaptiveTempoProbe,
    IntegerTempoProbe,
    TempoRegion,
    historical_probe,
    validate_adaptive_probe,
)

_Digest = Annotated[str, Field(pattern=r"^[0-9a-f]{64}$")]
_Time = Annotated[float, Field(allow_inf_nan=False, ge=0)]


class AppTempoMethod(BaseModel):
    """Source/protocol provenance and raw positions, without job/staging handles."""

    model_config = ConfigDict(extra="forbid")

    schema_version: Literal[1, 2, 3, 4]
    backend: Literal["beat_this"]
    policy: Literal[
        "neural-stable-regions-v1",
        "neural-stable-regions-v2",
        "neural-stable-regions-v3",
        "neural-stable-regions-v4",
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
    model_sha256: _Digest
    frontend_id: Literal["beat-this-1.1.0-soxr-hq-logmel-v1"]
    source_sha256: _Digest
    mono_sha256: _Digest
    sample_rate_hz: Annotated[int, Field(ge=8000, le=384_000)]
    frame_count: Annotated[int, Field(gt=0, le=2**63 - 1)]
    raw_beats: Annotated[list[_Time], Field(max_length=250_000)]
    raw_downbeats: Annotated[list[_Time], Field(max_length=250_000)]

    @model_validator(mode="after")
    def _coherent_estimate(self) -> AppTempoMethod:
        duration = self.frame_count / self.sample_rate_hz
        self._validate_policy_version()
        self._validate_observations(duration)
        self._validate_selected_estimate(duration)
        self._validate_regions(duration)
        return self

    def _validate_policy_version(self) -> None:
        if self.policy != f"neural-stable-regions-v{self.schema_version}" or (
            self.schema_version == 1 and self.integer_probe is not None
        ):
            message = "Tempo policy differs from its metadata version"
            raise ValueError(message)

    def _validate_selected_estimate(self, duration: float) -> None:
        if self.status in {"estimated", "single_region_estimate"}:
            if self.bpm is None or self.selected is None:
                message = "Estimated tempo requires its matching selected region"
                raise ValueError(message)
            if self.status == "estimated" and self.selected.retained < 24:
                message = "Long-region estimates require at least 24 retained observations"
                raise ValueError(message)
            if self.selected not in self.regions:
                message = "Selected tempo must be one of the retained regions"
                raise ValueError(message)
            expected = self.selected.bpm
            if self.schema_version >= 2:
                self._validate_integer_probe(duration)
                probe = self.integer_probe
                if probe is not None and probe.status == "supported_integer_estimate":
                    expected = probe.candidate_bpm
            if self.bpm != expected:
                message = "Final tempo contradicts the selected fit and integer counterprobe"
                raise ValueError(message)
        elif self.bpm is not None or self.selected is not None:
            message = "Unavailable tempo must not carry a selected BPM"
            raise ValueError(message)
        elif self.integer_probe is not None:
            message = "Unavailable tempo cannot carry an integer preference"
            raise ValueError(message)

    def _validate_observations(self, duration: float) -> None:
        for values in (self.raw_beats, self.raw_downbeats):
            if any(not math.isfinite(t) or t >= duration for t in values) or any(
                a >= b for a, b in pairwise(values)
            ):
                message = "Tempo observations must be ordered inside the complete source"
                raise ValueError(message)

    def _validate_regions(self, duration: float) -> None:
        extra = self.integer_probe.windows if self.integer_probe is not None else []
        for region in [*self.regions, *extra]:
            start, end = region.window_seconds
            if not start < end <= duration or any(
                not 0 <= i < len(self.raw_beats) for i in region.excluded_raw_indices
            ):
                message = "Tempo region differs from complete observation extent"
                raise ValueError(message)
            if self.schema_version >= 2:
                excluded = region.excluded_raw_indices
                available = sum(start <= t < end for t in self.raw_beats)
                if (
                    excluded != sorted(set(excluded))
                    or any(not start <= self.raw_beats[i] < end for i in excluded)
                    or region.retained != available - len(excluded)
                    or region.rms_residual_seconds > region.max_residual_seconds
                ):
                    message = "Tempo region retention differs from its original observations"
                    raise ValueError(message)

    def _validate_integer_probe(self, duration: float) -> None:
        probe = self.integer_probe
        if self.schema_version == 4:
            validate_adaptive_probe(self, probe)
            return
        probe = historical_probe(probe)
        if probe is None or self.selected is None or probe.base_bpm != self.selected.bpm:
            message = "Version two requires the counterprobe of its base regional estimate"
            raise ValueError(message)
        if probe.candidate_bpm != min(300, max(30, math.floor(probe.base_bpm + 0.5))):
            message = "Counterprobe must evaluate the nearest integer BPM"
            raise ValueError(message)
        if probe.common_region is not None and (
            probe.common_region not in self.regions
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
            self._validate_integer_support(probe, duration)

    def _validate_integer_support(self, probe: IntegerTempoProbe, duration: float) -> None:
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
        self._validate_integer_window_spans(probe)

    def _validate_integer_window_spans(self, probe: IntegerTempoProbe) -> None:
        if probe.common_region is None:
            message = "Integer estimate lacks its common region"
            raise ValueError(message)
        common_excluded = set(probe.common_region.excluded_raw_indices)
        for window in probe.windows:
            start, end = window.window_seconds
            # Local exclusions qualify stability only, not the shared phase.
            for excluded in (set(window.excluded_raw_indices), common_excluded):
                retained = [
                    t
                    for i, t in enumerate(self.raw_beats)
                    if start <= t < end and i not in excluded
                ]
                if len(retained) < 8 or retained[-1] - retained[0] < 10:
                    message = "Integer estimate lacks actual ten-second window coverage"
                    raise ValueError(message)
