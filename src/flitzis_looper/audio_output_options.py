"""Validated requested output settings, independent of device observations."""

from typing import Annotated, Self

from pydantic import BaseModel, ConfigDict, Field, StrictInt, StrictStr, model_validator

type OutputRate = Annotated[StrictInt, Field(gt=0, le=0xFFFF_FFFF)]
type OutputChannel = Annotated[StrictInt, Field(ge=0, le=0x7FFF_FFFF)]
type OutputFrames = Annotated[StrictInt, Field(gt=0, le=0x7FFF_FFFF)]


class AudioOutputOptions(BaseModel):
    """Requested backend and physical routing; Native validates current support.

    Automatic rate and buffer values are represented by None. The backend's
    installed configuration and callback observations never overwrite this intent.
    Device identifiers are opaque values returned by the selected backend.
    """

    model_config = ConfigDict(
        extra="forbid", frozen=True, strict=True, revalidate_instances="always"
    )

    backend: Annotated[StrictStr, Field(pattern=r"^[a-z][a-z0-9_-]{0,31}$")] = "auto"
    device_id: Annotated[StrictStr, Field(min_length=1, max_length=4096)] | None = None
    sample_rate_hz: OutputRate | None = None
    buffer_frames: OutputFrames | None = None
    left_channel: OutputChannel | None = None
    right_channel: OutputChannel | None = None

    @model_validator(mode="after")
    def _distinct_outputs(self) -> Self:
        if (self.left_channel is None) != (self.right_channel is None):
            message = "Select both physical outputs or use automatic routing"
            raise ValueError(message)
        if self.left_channel is not None and self.left_channel == self.right_channel:
            message = "Left and right outputs must be distinct physical channels"
            raise ValueError(message)
        return self
