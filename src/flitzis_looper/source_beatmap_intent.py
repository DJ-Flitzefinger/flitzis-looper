"""Compact historical map selection, independent of current Native qualification."""

import re
import string
from typing import Literal, Self

from pydantic import BaseModel, ConfigDict, Field, field_validator, model_validator

from flitzis_looper.source_beatmap import MAX_SOURCE_BEATMAP_BYTES

MAX_SOURCE_BEATMAP_REVISION = (1 << 64) - 1
MAX_SOURCE_BEATMAP_RECEIPT_BYTES = 8 * 1024
_DIGEST = r"\A[0-9a-f]{64}\z"
_GENERATION = re.compile(r"\.ready-[0-9a-f]{32}\Z")
_MATERIAL = re.compile(r"M[0-9a-f]{32}\Z")
_WINDOWS_DEVICES = {"CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"}
_RESERVED_ROOTS = {"samples", "materials", "stems", "config.json"}
_ASCII_UPPER = str.maketrans(string.ascii_lowercase, string.ascii_uppercase)
_ASCII_LOWER = str.maketrans(string.ascii_uppercase, string.ascii_lowercase)


def _readable_root(name: str) -> bool:
    device = name.split(".", 1)[0].translate(_ASCII_UPPER)
    reserved = device in _WINDOWS_DEVICES or (
        device[:3] in {"COM", "LPT"} and device[3:] in "123456789¹²³" and len(device) == 4
    )
    return (
        bool(name)
        and not reserved
        and not name.startswith((".", "#"))
        and not name.endswith((".", " "))
        and name.translate(_ASCII_LOWER) not in _RESERVED_ROOTS
        and not any(
            ord(character) < 32 or 127 <= ord(character) <= 159 or character in '<>:"/\\|?*'
            for character in name
        )
    )


class SourceBeatmapReference(BaseModel):
    """The exact portable Native receipt; presence never verifies a saved file or source."""

    model_config = ConfigDict(
        frozen=True, extra="forbid", strict=True, revalidate_instances="always"
    )

    schema_version: int = Field(strict=True, ge=1, le=1)
    kind: Literal["source-beatmap-v1"]
    reference: str = Field(strict=True, min_length=1, max_length=4096)
    bytes: int = Field(strict=True, gt=0, le=MAX_SOURCE_BEATMAP_BYTES)
    sha256: str = Field(strict=True, pattern=_DIGEST)
    map_revision: str = Field(strict=True, pattern=_DIGEST)
    decoder_identity: str = Field(strict=True, pattern=_DIGEST)

    @field_validator("reference")
    @classmethod
    def _validate_reference(cls, value: str) -> str:
        parts = value.split("/")
        root = parts[:-5]
        valid_root = (
            root == ["samples"]
            or (len(root) == 2 and root[0] == "samples" and _readable_root(root[1]))
            or (
                len(root) == 3
                and root[:2] == ["samples", "materials"]
                and _MATERIAL.fullmatch(root[2]) is not None
            )
        )
        if (
            len(value.encode("utf-8")) > 4096
            or not valid_root
            or parts[-5:-2] != [".pcm-cache", "source-beatmaps", "v1"]
            or _GENERATION.fullmatch(parts[-2]) is None
            or parts[-1] != "map.json"
        ):
            message = "source beatmap receipt requires a normalized typed artifact reference"
            raise ValueError(message)
        return value

    @model_validator(mode="after")
    def _validate_receipt_bound(self) -> Self:
        if len(self.model_dump_json().encode("utf-8")) > MAX_SOURCE_BEATMAP_RECEIPT_BYTES:
            message = "source beatmap receipt exceeds the Native 8 KiB byte limit"
            raise ValueError(message)
        return self


class PadSourceBeatmapIntent(BaseModel):
    """Immutable per-content request revision and optional historical selection."""

    model_config = ConfigDict(
        frozen=True, extra="forbid", strict=True, revalidate_instances="always"
    )

    schema_version: int = Field(strict=True, ge=1, le=1)
    revision: int = Field(strict=True, ge=0, le=MAX_SOURCE_BEATMAP_REVISION)
    selected: SourceBeatmapReference | None
    automatic: bool = Field(default=False, strict=True, exclude_if=lambda value: value is False)

    @model_validator(mode="after")
    def _validate_automatic_selection(self) -> Self:
        if self.automatic and self.selected is None:
            message = "Automatic map provenance requires its selected historical receipt."
            raise ValueError(message)
        return self


def next_source_beatmap_intent(
    current: PadSourceBeatmapIntent | None,
    selected: SourceBeatmapReference | None,
    *,
    automatic: bool = False,
) -> PadSourceBeatmapIntent:
    """Advance every selection or clear request without wrapping or reusing revisions.

    None represents untouched legacy intent at revision zero. A clear retains its
    new revision, so a late completion cannot revive the preceding selection.

    Args:
        current: Existing content intent, independently revalidated before use.
        selected: Exact historical receipt to select, or None to request a clear.
        automatic: Identify a verified automatic selection, preserving manual priority.

    Returns:
        A detached validated request with its next checked revision.
    """
    validated = None if current is None else PadSourceBeatmapIntent.model_validate(current)
    revision = 0 if validated is None else validated.revision
    if revision == MAX_SOURCE_BEATMAP_REVISION:
        message = "source beatmap intent revision capacity exhausted"
        raise ValueError(message)
    return PadSourceBeatmapIntent(
        schema_version=1, revision=revision + 1, selected=selected, automatic=automatic
    )
