"""Durable selection of one canonical stem pair, without runtime authority.

This schema checks structure only. Actual source, WAV/PCM integrity and current
publication permissions must come from the complete native descriptor and leases.
"""

from typing import Literal, Self

from pydantic import BaseModel, ConfigDict, Field, field_validator, model_validator

_MATERIAL = r"M[0-9a-f]{32}"
_GENERATION = r"[0-9a-f]{32}"


class StemPairSelection(BaseModel):
    """Immutable project references to a complete native-verifiable stem pair."""

    model_config = ConfigDict(frozen=True, extra="forbid", revalidate_instances="always")
    schema_version: Literal[1] = 1
    descriptor_reference: str = Field(
        strict=True,
        pattern=rf"\Asamples/materials/{_MATERIAL}/\.pcm-cache/stems/v1/\.pairs/"
        rf"{_GENERATION}\.json\z",
    )
    stem_set_identity: str = Field(strict=True, pattern=r"\A[0-9a-f]{64}\z")
    wav_generation: str = Field(
        strict=True,
        pattern=rf"\Asamples/materials/{_MATERIAL}/stems/\.ready-{_GENERATION}\z",
    )
    pcm_generation: str = Field(
        strict=True,
        pattern=rf"\Asamples/materials/{_MATERIAL}/\.pcm-cache/stems/v1/"
        rf"\.ready-{_GENERATION}\z",
    )

    @field_validator("schema_version", mode="before")
    @classmethod
    def validate_strict_revision(cls, value: object) -> object:
        """Prevent Literal's numeric equality from accepting bools or floats."""
        if type(value) is not int:
            message = "stem pair selection schema_version must be an integer"
            raise ValueError(message)
        return value

    @model_validator(mode="after")
    def validate_material(self) -> Self:
        """Bind all three already-canonical references to the same material."""
        materials = {
            self.descriptor_reference.split("/")[2],
            self.wav_generation.split("/")[2],
            self.pcm_generation.split("/")[2],
        }
        if len(materials) != 1:
            message = "stem pair selection references must belong to the same material"
            raise ValueError(message)
        return self
