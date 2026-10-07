"""Historical native timing evidence; only native verification can adopt it."""

from typing import Literal

from pydantic import BaseModel, ConfigDict, Field, JsonValue, field_validator


class PersistedAcceptedTiming(BaseModel):
    """Preserve a supported complete native record without treating it as current.

    The native codec owns evidence validation, source checks, accepted identity
    reconstruction and fresh runtime adoption. Python recognizes the versioned
    envelope only. Every binary64 evidence value inside ``record`` is represented
    by its exact hexadecimal bits, independently of the legacy display BPM.
    """

    model_config = ConfigDict(frozen=True, extra="forbid", strict=True)

    schema_version: Literal[1]
    encoding: Literal["accepted-constant-timing-qm-raw-v1"]
    record: dict[str, JsonValue] = Field(min_length=1)

    @field_validator("schema_version", mode="before")
    @classmethod
    def _require_integer_schema_version(cls, value: object) -> object:
        if type(value) is not int:
            msg = "accepted timing schema_version must be an integer"
            raise ValueError(msg)
        return value
