"""Persistable App estimates; these DTOs never authorize Native timing or maps."""

import math
from itertools import pairwise
from typing import Annotated, Literal

from pydantic import BaseModel, ConfigDict, Field, model_validator

_Digest = Annotated[str, Field(pattern=r"^[0-9a-f]{64}$")]
_Time = Annotated[float, Field(allow_inf_nan=False, ge=0)]


class TempoRegion(BaseModel):
    model_config = ConfigDict(extra="forbid")

    window_seconds: Annotated[list[_Time], Field(min_length=2, max_length=2)]
    bpm: Annotated[float, Field(allow_inf_nan=False, ge=30, le=300)]
    retained: Annotated[int, Field(ge=8, le=250_000)]
    excluded_raw_indices: Annotated[list[int], Field(max_length=250_000)]
    max_residual_seconds: Annotated[float, Field(allow_inf_nan=False, ge=0, le=0.025)]
    rms_residual_seconds: Annotated[float, Field(allow_inf_nan=False, ge=0, le=0.025)]


class AppTempoMethod(BaseModel):
    """Source/protocol provenance and raw positions, without job/staging handles."""

    model_config = ConfigDict(extra="forbid")

    schema_version: Literal[1]
    backend: Literal["beat_this"]
    policy: Literal["neural-stable-regions-v1"]
    status: Literal[
        "estimated", "single_region_estimate", "insufficient", "invalid", "uncorroborated"
    ]
    bpm: Annotated[float | None, Field(allow_inf_nan=False, ge=30, le=300)]
    selected: TempoRegion | None
    regions: Annotated[list[TempoRegion], Field(max_length=5)]
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
        for values in (self.raw_beats, self.raw_downbeats):
            if any(not math.isfinite(t) or t >= duration for t in values) or any(
                a >= b for a, b in pairwise(values)
            ):
                message = "Tempo observations must be ordered inside the complete source"
                raise ValueError(message)
        if self.status in {"estimated", "single_region_estimate"}:
            if self.bpm is None or self.selected is None or self.bpm != self.selected.bpm:
                message = "Estimated tempo requires its matching selected region"
                raise ValueError(message)
            if self.status == "estimated" and self.selected.retained < 24:
                message = "Long-region estimates require at least 24 retained observations"
                raise ValueError(message)
            if self.selected not in self.regions:
                message = "Selected tempo must be one of the retained regions"
                raise ValueError(message)
        elif self.bpm is not None or self.selected is not None:
            message = "Unavailable tempo must not carry a selected BPM"
            raise ValueError(message)
        for region in self.regions:
            start, end = region.window_seconds
            if not start < end <= duration or any(
                not 0 <= i < len(self.raw_beats) for i in region.excluded_raw_indices
            ):
                message = "Tempo region differs from complete observation extent"
                raise ValueError(message)
        return self
