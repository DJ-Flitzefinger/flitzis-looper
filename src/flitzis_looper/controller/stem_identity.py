"""Common normalized stem request identity; complete file proofs remain Native-owned."""

import hashlib
import json
import re
from typing import TYPE_CHECKING

from flitzis_looper.stem_pair_selection import STEM_FINGERPRINT_SCHEMA

if TYPE_CHECKING:
    from flitzis_looper.controller.stem_generation import StemGenerationRequest

TRANSFORM_POLICY = "source-bound-aligned-pcm16-streaming-v2"


def source_digest(source_version: str) -> str | None:
    """Return only a complete source SHA, never a locator or historical stat token."""
    matched = re.search(r"\|sha256-v1:([0-9a-f]{64})\Z", source_version)
    return matched.group(1) if matched is not None else None


def request_signature(request: StemGenerationRequest) -> tuple[object, ...]:
    """Capture effective source/configuration intent without treating locators as bytes."""
    shape = request.target_shape
    quality = (
        (request.demucs_shifts, request.demucs_overlap)
        if request.separator == "demucs:htdemucs"
        else ()
    )
    return (
        TRANSFORM_POLICY,
        source_digest(request.source_version),
        shape.sample_rate_hz,
        shape.channels,
        shape.frame_count,
        request.separator,
        request.device_policy,
        quality,
    )


def request_fingerprint(
    request: StemGenerationRequest, producer_identity: str | None = None
) -> str | None:
    """Bind full source/configuration intent to actual producer bytes and source extent."""
    if (
        source_digest(request.source_version) is None
        or producer_identity is None
        or re.fullmatch(r"[0-9a-f]{64}", producer_identity) is None
        or type(request.original_bytes) is not int
        or request.original_bytes <= 0
    ):
        return None
    encoded = json.dumps(
        (
            STEM_FINGERPRINT_SCHEMA,
            request_signature(request),
            request.original_bytes,
            producer_identity,
        ),
        ensure_ascii=False,
        separators=(",", ":"),
    )
    return hashlib.sha256(encoded.encode("utf-8")).hexdigest()
