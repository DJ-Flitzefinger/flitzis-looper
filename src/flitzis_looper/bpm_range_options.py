"""Immutable preferences captured by future automatic analysis requests."""

from typing import Annotated

from pydantic import BaseModel, ConfigDict, Field, model_validator

_Bound = Annotated[float, Field(strict=True, allow_inf_nan=False, ge=30, le=300)]


class BpmRangeOptions(BaseModel):
    model_config = ConfigDict(
        frozen=True, extra="forbid", revalidate_instances="always", strict=True
    )

    enabled: bool = False
    minimum_bpm: _Bound = 50.0
    maximum_bpm: _Bound = 170.0

    @model_validator(mode="after")
    def _ordered_bounds(self) -> BpmRangeOptions:
        if self.minimum_bpm >= self.maximum_bpm:
            message = "Minimum automatic BPM must be below maximum automatic BPM"
            raise ValueError(message)
        return self
