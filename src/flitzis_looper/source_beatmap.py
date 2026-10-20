"""Supported historical map envelope; Rust owns geometry and actual source checks."""

import json
from typing import Literal

from pydantic import (
    BaseModel,
    ConfigDict,
    JsonValue,
    TypeAdapter,
    field_serializer,
    field_validator,
)

MAX_SOURCE_BEATMAP_BYTES = 64 * 1024 * 1024
_RECORD_ADAPTER = TypeAdapter(dict[str, JsonValue])


class PersistedSourceBeatmap(BaseModel):
    """Recognize a supported envelope without granting map or audio authority.

    ``record`` stores immutable JSON text internally. Serialization reconstructs
    the durable record object; ``record_data`` returns an independent copy. This
    avoids treating Pydantic's shallow frozen dict/list fields as immutable.
    Neither recognition nor these copies validate native geometry, revisions,
    decoder custody, musical acceptance or current audible adoption.

    Use ``from_json`` for bounded external JSON admission. Native validation owns
    the complete canonical byte/count/numeric contract. The byte ceiling does
    not establish process-RAM, whole-project or simultaneous-preparation bounds.
    """

    model_config = ConfigDict(frozen=True, extra="forbid", strict=True)

    schema_version: Literal[1]
    encoding: Literal["source-beatmap-pwa-v1"]
    record: str

    @classmethod
    def from_json(cls, encoded: str | bytes) -> PersistedSourceBeatmap:
        """Check the external byte ceiling before Pydantic decodes the envelope."""
        byte_count = len(encoded.encode("utf-8")) if isinstance(encoded, str) else len(encoded)
        if byte_count > MAX_SOURCE_BEATMAP_BYTES:
            msg = "source beatmap envelope exceeds the 64 MiB byte limit"
            raise ValueError(msg)
        return cls.model_validate_json(encoded)

    @field_validator("schema_version", mode="before")
    @classmethod
    def _require_integer_version(cls, value: object) -> object:
        if type(value) is not int:
            msg = "source beatmap schema_version must be an integer"
            raise ValueError(msg)
        return value

    @field_validator("record", mode="before")
    @classmethod
    def _retain_immutable_record(cls, value: object) -> str:
        record = _RECORD_ADAPTER.validate_python(value, strict=True)
        if not record:
            msg = "source beatmap record must not be empty"
            raise ValueError(msg)
        # This is durable JSON storage, not a second canonical hash authority.
        encoded = json.dumps(record, ensure_ascii=False, allow_nan=False, separators=(",", ":"))
        if len(encoded.encode("utf-8")) > MAX_SOURCE_BEATMAP_BYTES:
            msg = "source beatmap record exceeds the 64 MiB byte limit"
            raise ValueError(msg)
        return encoded

    @field_serializer("record")
    def _serialize_record(self, value: str) -> dict[str, JsonValue]:
        return _RECORD_ADAPTER.validate_json(value)

    def record_data(self) -> dict[str, JsonValue]:
        """Return a defensive record copy; it is metadata, never native authority."""
        return _RECORD_ADAPTER.validate_json(self.record)
