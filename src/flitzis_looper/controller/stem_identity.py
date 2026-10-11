"""Common normalized stem request identity; complete file proofs remain Native-owned."""

import hashlib
import json
import os
import re
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from flitzis_looper.controller.stem_generation import StemGenerationRequest

TRANSFORM_POLICY = "source-bound-aligned-pcm16-streaming-v2"


def request_signature(request: StemGenerationRequest) -> tuple[object, ...]:
    """Keep job sharing and durable compatibility on the same captured processing inputs."""
    source = request.source_version.rsplit("|sha256-v1:", 1)[-1]
    shape = request.target_shape
    return (
        TRANSFORM_POLICY,
        source,
        shape.sample_rate_hz,
        shape.channels,
        shape.frame_count,
        os.path.normcase(str(request.model_cache_dir.absolute())),
        request.separator,
        request.device_policy,
        request.demucs_shifts,
        request.demucs_overlap,
    )


def request_fingerprint(request: StemGenerationRequest) -> str | None:
    """Unknown old source metadata cannot be promoted into complete-content provenance."""
    if re.search(r"\|sha256-v1:[0-9a-f]{64}$", request.source_version) is None:
        return None
    encoded = json.dumps(request_signature(request), ensure_ascii=False, separators=(",", ":"))
    return hashlib.sha256(encoded.encode("utf-8")).hexdigest()
