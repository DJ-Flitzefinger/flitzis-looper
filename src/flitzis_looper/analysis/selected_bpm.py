"""Off-thread selected-backend BPM metadata using the shared native G2 fitter."""

import json
import math
import struct
from dataclasses import asdict
from typing import TYPE_CHECKING

from pydantic import TypeAdapter

from flitzis_looper.analysis.contracts import (
    BeatComponentResult,
    BeatWorkerRequest,
    WorkerLimits,
    validate_component_result,
)
from flitzis_looper.analysis.publication import decode_result
from flitzis_looper.analysis.selected_bpm_models import SelectedBpmAssessment, SelectedBpmReport
from flitzis_looper_audio import summarize_selected_bpm_json

if TYPE_CHECKING:
    from flitzis_looper.analysis.publication import PublishedAnalysisResult

_ASSESSMENT_ADAPTER = TypeAdapter(SelectedBpmAssessment)
_REQUEST_ADAPTER = TypeAdapter(BeatWorkerRequest)
_LIMITS = WorkerLimits()


def _validate_input(
    beats: tuple[float, ...],
    rate: int,
    frames: int,
    counts: tuple[int | None, ...] | None,
    denominator: int,
    provenance: str,
) -> None:
    dimensions = ((rate, 8000, 768000), (frames, 1, 2**53), (denominator, 1, 64))
    if any(
        type(value) is not int or not lower <= value <= upper for value, lower, upper in dimensions
    ):
        msg = "invalid selected BPM dimensions"
        raise ValueError(msg)
    if len(beats) > _LIMITS.max_prediction_count or any(
        type(value) not in {int, float} for value in beats
    ):
        msg = "invalid selected BPM input types or extent"
        raise ValueError(msg)
    try:
        finite = all(math.isfinite(value) for value in beats)
    except OverflowError as error:
        msg = "selected BPM position exceeds binary64"
        raise ValueError(msg) from error
    if not finite:
        msg = "invalid nonfinite complete beat positions"
        raise ValueError(msg)
    if not isinstance(provenance, str) or not provenance.strip() or len(provenance) > 4096:
        msg = "invalid selected BPM count provenance"
        raise ValueError(msg)
    if counts is not None and (
        len(counts) != len(beats)
        or any(
            value is not None and (type(value) is not int or abs(value) > 2**52) for value in counts
        )
    ):
        msg = "invalid selected BPM complete count extent or values"
        raise ValueError(msg)


def assess_beat_sequence(
    beat_seconds: tuple[float, ...],
    sample_rate_hz: int,
    frame_count: int,
    *,
    quarter_counts: tuple[int | None, ...] | None = None,
    quarter_note_denominator: int = 1,
    count_provenance: str = "selected-backend ordinal assumption",
) -> SelectedBpmAssessment:
    """Assess complete immutable timestamps under conditional, unverified quarter units.

    Explicit counts retain jumps and exclusions; no timing ratio infers missing beats
    or comparable snare units. Neither caller assertions nor a good fit certify them.
    All fitting and JSON allocation occur outside audio/control realtime paths.
    """
    _validate_input(
        beat_seconds,
        sample_rate_hz,
        frame_count,
        quarter_counts,
        quarter_note_denominator,
        count_provenance,
    )
    encoded = summarize_selected_bpm_json(
        beat_seconds,
        sample_rate_hz,
        frame_count,
        quarter_counts=quarter_counts,
        quarter_note_denominator=quarter_note_denominator,
        count_provenance=count_provenance,
    )
    assessment = _ASSESSMENT_ADAPTER.validate_json(encoded, strict=True)
    # Bind the derived metadata to the exact binary64 sequence rather than float equality.
    before = struct.pack(f"<{len(beat_seconds)}d", *beat_seconds)
    after = struct.pack(f"<{len(assessment.raw_beat_seconds)}d", *assessment.raw_beat_seconds)
    if (
        before != after
        or assessment.sample_rate_hz != sample_rate_hz
        or assessment.frame_count != frame_count
        or assessment.raw_position_count != len(beat_seconds)
    ):
        msg = "selected BPM metadata does not retain the complete input identity"
        raise ValueError(msg)
    return assessment


def summarize_beats(
    component: BeatComponentResult, request: BeatWorkerRequest
) -> SelectedBpmReport | None:
    """Derive metadata for a validated ready/retired component without accessing PCM/models."""
    # Preserve malformed booleans for strict validation: an int serializer can
    # otherwise coerce True to 1 before the reader sees the invalid input.
    metadata = asdict(request)
    metadata["pcm"]["path"] = str(request.pcm.path)
    encoded_request = json.dumps(metadata, allow_nan=False).encode("utf-8")
    if len(encoded_request) > _LIMITS.max_request_bytes:
        msg = "selected BPM request metadata limit exceeded"
        raise ValueError(msg)
    _REQUEST_ADAPTER.validate_json(encoded_request, strict=True)
    validate_component_result(component, request, _LIMITS)
    if component.status != "ready":
        return None
    if not component.resources_released:
        msg = "selected BPM resources are still retiring"
        raise ValueError(msg)
    predictions = component.predictions
    if predictions is None:
        msg = "selected BPM requires full ready predictions"
        raise ValueError(msg)
    assessment = assess_beat_sequence(
        predictions.beat_seconds, request.pcm.sample_rate_hz, request.pcm.frame_count
    )
    return SelectedBpmReport(
        request.identity,
        request.model,
        request.pcm.sample_rate_hz,
        request.pcm.frame_count,
        request.pcm.origin_seconds,
        predictions,
        assessment,
    )


def summarize_published(
    encoded: str, request: BeatWorkerRequest
) -> tuple[PublishedAnalysisResult, SelectedBpmReport | None]:
    """Opt into numerical metadata after validating an unchanged v1/v2 full envelope.

    The native-free candidate/seal readers retain their existing decode_result path.
    This explicit path reads no model or PCM and never restores/adopts project timing.
    """
    result = decode_result(encoded, request)
    return result, summarize_beats(result.beat, request)
