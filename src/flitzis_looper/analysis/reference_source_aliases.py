"""Explicit private relocations that preserve the unchanged frozen source identity."""

from typing import Annotated, Literal

from pydantic import Field

from flitzis_looper.analysis.reference_inputs_models import Digest, One, StrictInput, Text


class SourcePathAlias(StrictInput):
    """Identify one renamed source without rewriting its historical manifest path."""

    track_id: Literal["T01", "T02"]
    original_source_relative: Text
    actual_source_path: Text


class SourcePathAliases(StrictInput):
    """A caller-selected private alias file bound to the original manifest bytes."""

    schema_version: One
    manifest_sha256: Digest
    aliases: Annotated[tuple[SourcePathAlias, ...], Field(min_length=1, max_length=2)]
