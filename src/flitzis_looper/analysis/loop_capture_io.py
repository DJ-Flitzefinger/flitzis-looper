"""Hash-bound private artifact I/O shared by offline loop evidence commands."""

import json
from typing import TYPE_CHECKING

from pydantic import TypeAdapter

from flitzis_looper.analysis.loop_capture_models import Artifact, FeatureEvidence
from flitzis_looper.analysis.loop_capture_wav import inspect_wave
from flitzis_looper.analysis.reference_inputs_validation import (
    fail,
    private_path,
    read_json_bytes,
    sha256_file,
)

if TYPE_CHECKING:
    from pathlib import Path


def _json(data: object) -> bytes:
    return (json.dumps(data, indent=2, ensure_ascii=False, allow_nan=False) + "\n").encode()


def artifact(workspace: Path, path: str) -> Artifact:
    """Bind existing private bytes in a receipt; never manufacture identity."""
    return Artifact(path=path, sha256=sha256_file(private_path(workspace, path)))


def verify_artifact(workspace: Path, item: Artifact) -> Path:
    """Reject changed input before any measurement/comparison/receipt operation."""
    path = private_path(workspace, item.path)
    if sha256_file(path) != item.sha256:
        fail("artifact_sha256_mismatch")
    return path


def verified_features(workspace: Path, identity: Artifact) -> FeatureEvidence:
    """Require unchanged capture/policy/feature bytes before downstream receipt work."""
    path = verify_artifact(workspace, identity)
    features = TypeAdapter(FeatureEvidence).validate_json(read_json_bytes(path), strict=True)
    capture_path = verify_artifact(workspace, features.capture)
    verify_artifact(workspace, features.policy)
    info = inspect_wave(capture_path)
    if (info.format, info.rate, info.channels, info.frames) != (
        features.wave_format,
        features.capture_sample_rate_hz,
        features.capture_channel_count,
        features.capture_frame_count,
    ):
        fail("feature_receipt_timebase_or_extent_differs_from_actual_wave")
    _feature_geometry(features)
    return features


def _feature_geometry(features: FeatureEvidence) -> None:
    channels = [channel.channel for channel in features.measured_channels]
    if len(channels) != len(set(channels)) or any(
        channel >= features.capture_channel_count for channel in channels
    ):
        fail("feature_receipt_duplicate_or_missing_wave_channel")
    for channel in features.measured_channels:
        event_ids = [edge.event_id for edge in channel.edges]
        frames = [edge.capture_frame for edge in channel.edges]
        if event_ids != list(range(len(event_ids))) or frames != sorted(set(frames)):
            fail("feature_receipt_event_ids_or_capture_frames_not_unique_ordered")
        if frames and frames[-1] >= features.capture_frame_count:
            fail("feature_receipt_edge_outside_actual_wave")
        if channel.samples_at_or_above_full_scale > features.capture_frame_count:
            fail("feature_receipt_full_scale_count_exceeds_actual_wave")
