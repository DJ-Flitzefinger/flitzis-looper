"""Capture native source authority for control operations using one timing snapshot."""

import struct
from dataclasses import dataclass
from typing import TYPE_CHECKING

from flitzis_looper.controller.current_timing import CurrentPadTiming, current_accepted_timing

if TYPE_CHECKING:
    from flitzis_looper_audio import AudioEngine, InputRuntimePadBinding


@dataclass(frozen=True)
class CurrentPadBindingSnapshot:
    """A native opaque permit and its control-only source/timing comparison values."""

    binding: InputRuntimePadBinding
    source_signature: tuple[object, ...]
    accepted_timing: CurrentPadTiming | None


def capture_current_pad_binding(
    audio: AudioEngine, sample_id: int, *, timing: CurrentPadTiming | None
) -> CurrentPadBindingSnapshot | None:
    """Capture authority only when the separately resolved current timing agrees.

    Automatic without acknowledgement cannot fall back to saved Legacy values.
    Native admission and execution still recheck the opaque permit; these copies
    support coherent loop calculation and never confer ownership themselves.
    """
    binding = audio.current_input_runtime_pad_binding(sample_id)
    if binding is None:
        return None
    metadata = binding.metadata()
    if metadata["pad_id"] != sample_id:
        return None
    accepted_metadata = metadata["accepted_timing"]
    accepted = (
        current_accepted_timing(accepted_metadata, sample_id=sample_id)
        if isinstance(accepted_metadata, dict)
        else None
    )
    if not _binding_matches_timing(metadata, timing, accepted):
        return None
    return CurrentPadBindingSnapshot(
        binding=binding,
        source_signature=tuple(
            metadata[key]
            for key in (
                "pad_id",
                "source_id",
                "source_generation",
                "source_sha256",
                "sample_rate_hz",
                "frame_count",
                "channels",
                "intent",
                "authority_revision",
            )
        )
        + tuple(
            metadata.get(key)
            for key in (
                "window_revision",
                "resident_start_frame",
                "resident_end_frame",
                "resident_pcm_identity",
                "resident_context",
            )
        ),
        accepted_timing=accepted,
    )


def _binding_matches_timing(
    metadata: dict[str, object], timing: CurrentPadTiming | None, accepted: CurrentPadTiming | None
) -> bool:
    if metadata["intent"] != "automatic":
        return accepted is None and (timing is None or timing.accepted_revision is None)
    if accepted is None or timing is None or timing.accepted_revision is None:
        return False
    if not _same_accepted_timing(timing, accepted):
        return False
    identity = accepted.accepted_identity
    if identity is None:
        return False
    return (
        metadata["source_id"],
        metadata["source_generation"],
        metadata["source_sha256"],
        metadata["sample_rate_hz"],
        metadata["frame_count"],
    ) == (
        identity.source_id,
        identity.source_generation,
        identity.source_sha256,
        accepted.sample_rate_hz,
        identity.frame_count,
    )


def _same_accepted_timing(left: CurrentPadTiming, right: CurrentPadTiming) -> bool:
    if left != right or left.accepted_identity is None or right.accepted_identity is None:
        return False
    return all(
        struct.pack("!d", first) == struct.pack("!d", second)
        for first, second in (
            (left.period_seconds, right.period_seconds),
            (left.origin_seconds, right.origin_seconds),
            (
                left.accepted_identity.source_zero_seconds,
                right.accepted_identity.source_zero_seconds,
            ),
        )
    )
