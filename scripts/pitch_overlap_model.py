"""Frozen offline source-segmented short-window WSOLA falsification model.

This model has full-source lookahead, heuristic source-only cues and explicit ZOH
aliasing risk. It is not a production renderer or a realtime causality proof.
Each segment retains every generated positive-time grain tail; segment streams
are summed without final gain normalization or a nominal-duration crop.
"""

import math
from dataclasses import asdict, dataclass
from itertools import pairwise
from typing import TYPE_CHECKING

import numpy as np

if TYPE_CHECKING:
    from numpy.typing import NDArray

type Pcm = NDArray[np.float64]
type Indices = NDArray[np.int64]

FS = 48_000
WINDOW_FRAMES = 192
HOP_FRAMES = 48
SOURCE_LAG_FRAMES = 192
BASELINE_FRAMES = 512
CUE_MULTIPLIER = 8.0
CUE_FLOOR = 1.0e-12
REFRACTORY_FRAMES = 240
_OFFSETS = np.arange(WINDOW_FRAMES, dtype=np.int64)
_WINDOW = np.sin(np.pi * _OFFSETS / WINDOW_FRAMES) ** 2


@dataclass(slots=True)
class _Statistics:
    grain_count: int = 0
    search_count: int = 0
    identity_bypass_grains: int = 0
    ncc_candidate_count: int = 0
    zero_reference_searches: int = 0
    zero_energy_candidates: int = 0
    minimum_selected_source_lag: float | None = None
    maximum_selected_source_lag: float | None = None

    def observe_origin(self, origin: int, start: int, rate: float, shift: float) -> None:
        """Count each admitted grain and its original-source origin displacement."""
        lag = shift * origin - rate * start
        self.grain_count += 1
        if self.minimum_selected_source_lag is None:
            self.minimum_selected_source_lag = lag
            self.maximum_selected_source_lag = lag
        else:
            self.minimum_selected_source_lag = min(self.minimum_selected_source_lag, lag)
            assert self.maximum_selected_source_lag is not None
            self.maximum_selected_source_lag = max(self.maximum_selected_source_lag, lag)


def _cue_indices(source: Pcm) -> list[int]:
    """Detect cues with past-only zero-padded statistics, then force both ends."""
    if not len(source):
        return [0]
    differences = np.empty_like(source)
    differences[0] = source[0]
    differences[1:] = source[1:] - source[:-1]
    magnitudes = np.linalg.norm(differences, axis=1)
    prefix = np.concatenate((np.zeros(1), np.cumsum(magnitudes)))
    positions = np.arange(len(source), dtype=np.int64)
    past_start = np.maximum(positions - BASELINE_FRAMES, 0)
    baseline = (prefix[:-1] - prefix[past_start]) / BASELINE_FRAMES
    candidates = np.flatnonzero((magnitudes > CUE_MULTIPLIER * baseline) & (magnitudes > CUE_FLOOR))
    accepted = []
    previous = -REFRACTORY_FRAMES
    for candidate in candidates.tolist():
        if candidate - previous >= REFRACTORY_FRAMES:
            accepted.append(candidate)
            previous = candidate
    return sorted({0, len(source), *accepted})


def _read_zero_extended(pcm: Pcm, indices: Indices) -> Pcm:
    """Read arbitrary integer indices with exact zero outside the segment."""
    values = np.zeros((*indices.shape, 2), dtype=np.float64)
    inside = (indices >= 0) & (indices < len(pcm))
    values[inside] = pcm[indices[inside]]
    return values


def _resample_segment(source: Pcm, begin: int, end: int, shift: float) -> Pcm:
    """Transpose all segment PCM using its declared local floor/ZOH sampler."""
    count = math.ceil((end - begin) / shift)
    positions = np.floor(shift * np.arange(count, dtype=np.float64)).astype(np.int64)
    positions = positions[(positions >= 0) & (positions < end - begin)]
    return source[begin + positions]


def _candidate_origins(z: Pcm, start: int, rate: float, shift: float) -> Indices:
    """Apply the source-lag bound and exclude the original first source tap."""
    minimum = max(1, math.ceil((rate * start - SOURCE_LAG_FRAMES) / shift))
    maximum = min(len(z) - 1, math.floor((rate * start + SOURCE_LAG_FRAMES) / shift))
    origins = np.arange(minimum, maximum + 1, dtype=np.int64)
    source_lags = shift * origins - rate * start
    allowed = (np.abs(source_lags) <= SOURCE_LAG_FRAMES) & (np.floor(shift * origins) >= 1)
    origins = origins[allowed]
    if not len(origins):
        message = f"No admitted positive grain origin at local output frame {start}"
        raise ValueError(message)
    return origins


def _select_origin(
    z: Pcm,
    start: int,
    previous: int,
    rate: float,
    shift: float,
    statistics: _Statistics,
) -> int:
    """Maximize pooled-stereo NCC; exact ties use source lag, then origin."""
    origins = _candidate_origins(z, start, rate, shift)
    overlap = np.arange(WINDOW_FRAMES - HOP_FRAMES, dtype=np.int64)
    reference = _read_zero_extended(z, previous + HOP_FRAMES + overlap)
    candidates = _read_zero_extended(z, origins[:, None] + overlap[None, :])
    reference_energy = float(np.sum(reference * reference))
    candidate_energy = np.sum(candidates * candidates, axis=(1, 2))
    scores = np.zeros(len(origins), dtype=np.float64)
    usable = (reference_energy > 0.0) & (candidate_energy > 0.0)
    scores[usable] = np.sum(candidates[usable] * reference, axis=(1, 2)) / np.sqrt(
        reference_energy * candidate_energy[usable]
    )
    assert np.all(np.isfinite(scores)), "Nonfinite normalized correlation"
    statistics.search_count += 1
    statistics.ncc_candidate_count += len(origins)
    statistics.zero_reference_searches += int(reference_energy == 0.0)
    statistics.zero_energy_candidates += int(np.count_nonzero(candidate_energy == 0.0))
    tied = np.flatnonzero(scores == np.max(scores)).tolist()
    return int(
        origins[
            min(
                tied,
                key=lambda index: (
                    abs(shift * int(origins[index]) - rate * start),
                    int(origins[index]),
                ),
            )
        ]
    )


def _add_window(numerator: Pcm, weights: Pcm, z: Pcm, origin: int, start: int) -> None:
    """Add a complete window, asserting that negative-time samples are zero."""
    positions = start + _OFFSETS
    values = _read_zero_extended(z, origin + _OFFSETS) * _WINDOW[:, None]
    negative = positions < 0
    assert not np.any(values[negative] != 0.0), "Nonzero negative-time segment response"
    inside = ~negative
    numerator[positions[inside]] += values[inside]
    weights[positions[inside]] += _WINDOW[inside, None]


def _render_segment(
    z: Pcm, duration: int, rate: float, shift: float, statistics: _Statistics
) -> Pcm:
    """Normalize all admitted windows and retain their complete positive tails."""
    positive_starts = range(HOP_FRAMES, duration, HOP_FRAMES)
    last_start = positive_starts[-1] if positive_starts else 0
    numerator = np.zeros((last_start + WINDOW_FRAMES, 2), dtype=np.float64)
    weights = np.zeros_like(numerator)
    for start in range(-3 * HOP_FRAMES, 1, HOP_FRAMES):
        _add_window(numerator, weights, z, start, start)
        statistics.observe_origin(start, start, rate, shift)
    previous = 0
    for start in positive_starts:
        if rate == 1.0 and shift == 1.0:
            origin = start
            statistics.identity_bypass_grains += 1
        else:
            origin = _select_origin(z, start, previous, rate, shift, statistics)
        _add_window(numerator, weights, z, origin, start)
        statistics.observe_origin(origin, start, rate, shift)
        previous = origin
    assert np.all(weights > 0.0), "Uncovered positive-time segment frame"
    return numerator / weights


def _validate_source(source: Pcm, rate: float, shift: float) -> Pcm:
    """Reject unsupported input geometry or nonfinite arithmetic parameters."""
    pcm = np.asarray(source, dtype=np.float64)
    if pcm.ndim != 2 or pcm.shape[1] != 2 or not np.all(np.isfinite(pcm)):
        message = "Source must be a finite float64 stereo array with shape (frames, 2)"
        raise ValueError(message)
    if not math.isfinite(rate) or rate <= 0.0 or not math.isfinite(shift) or shift <= 0.0:
        message = "Rate and pitch ratio must be finite and strictly positive"
        raise ValueError(message)
    return pcm


def _prepare_segment(
    pcm: Pcm, begin: int, end: int, rate: float, shift: float, statistics: _Statistics
) -> tuple[Pcm, dict]:
    """Keep the segment geometry and its complete generated stream together."""
    origin = math.ceil(begin / rate)
    duration = math.ceil(end / rate) - origin
    z = _resample_segment(pcm, begin, end, shift)
    stream = _render_segment(z, duration, rate, shift, statistics)
    return stream, {
        "source_begin": begin,
        "source_end": end,
        "output_origin": origin,
        "nominal_output_frames": duration,
        "resampled_frames": len(z),
        "source_phase_quantization": begin - rate * origin,
        "last_active_grain_end_exclusive": origin + len(stream),
    }


def _sum_segments(
    pcm: Pcm, rate: float, shift: float, cues: list[int], statistics: _Statistics
) -> tuple[Pcm, list[dict]]:
    """Sum complete segment streams in one bounded allocation without gain fitting."""
    output = np.zeros((math.ceil(len(pcm) / rate) + WINDOW_FRAMES, 2), dtype=np.float64)
    segments = []
    final_end = 0
    for begin, end in pairwise(cues):
        stream, segment = _prepare_segment(pcm, begin, end, rate, shift, statistics)
        retired = segment["last_active_grain_end_exclusive"]
        assert retired <= len(output), "Generated tail exceeds the fixed allocation"
        output[segment["output_origin"] : retired] += stream
        final_end = max(final_end, retired)
        segments.append(segment)
    assert np.all(np.isfinite(output)), "Nonfinite full generated output"
    return output[:final_end], segments


def render(source: Pcm, rate: float, semitones: int) -> tuple[Pcm, dict]:
    """Render the frozen source-only model without fixture or target information.

    Args:
        source: Original stereo PCM at the fixed 48000-Hz diagnostic sample rate.
        rate: Canonical source frames per output frame, independent of pitch.
        semitones: Deliberate pitch intent, with frequency ratio 2**(semitones/12).

    Returns:
        Complete finite generated PCM and JSON-compatible mechanism details.
        Nominal material duration is reported separately from grain retirement.
    """
    shift = 2.0 ** (semitones / 12.0)
    pcm = _validate_source(source, rate, shift)
    cues = _cue_indices(pcm)
    nominal_frames = math.ceil(len(pcm) / rate)
    statistics = _Statistics()
    output, segments = _sum_segments(pcm, rate, shift, cues, statistics)
    details = {
        "model": "source-segmented-short-window-wsola-v1",
        "sample_rate_hz": FS,
        "source_frames": len(pcm),
        "rate": rate,
        "semitones": semitones,
        "h": shift,
        "segment_stretch_factor": shift / rate,
        "window_output_frames": WINDOW_FRAMES,
        "hop_output_frames": HOP_FRAMES,
        "window": "periodic Hann: sin(pi*j/192)**2; j=0..191",
        "source_lag_limit_frames": SOURCE_LAG_FRAMES,
        "positive_origin_rule": "floor(h*a)>=1; never reread the first original source tap",
        "sampler": "segment-local x[e+floor(h*k)] with exact source-segment zero extension",
        "cue_detector": {
            "difference": "stereo Euclidean norm; x[-1]=0",
            "past_mean_frames": BASELINE_FRAMES,
            "past_mean_padding": "zero padding; divide by512; exclude current difference",
            "strict_multiplier": CUE_MULTIPLIER,
            "strict_numerical_floor": CUE_FLOOR,
            "greedy_refractory_source_frames": REFRACTORY_FRAMES,
            "forced_boundaries": "0 and source length, after greedy detection",
        },
        "cue_indices": cues,
        "segment_count": len(segments),
        "segments": segments,
        "nominal_output_frames": nominal_frames,
        "full_generated_output_frames": len(output),
        "last_active_grain_end_exclusive": len(output),
        "capture_allocation_limit_frames": nominal_frames + WINDOW_FRAMES,
        "negative_output_policy": "discard only mathematically zero samples; asserted per grain",
        "tail_policy": "all admitted grain tails retained; no segment-end or nominal-end crop",
        "normalization": ("static admitted window weights per segment; no final mix normalization"),
        "stereo_relation": "both channels share cues, origins, integer taps and scalar windows",
        "identity_control": "r=h=1 still uses segmentation and normalized OLA with a=b",
        "offline_full_source_lookahead": True,
        "native_used": False,
        "production_activation": False,
        "realtime_causality_claim": False,
        "overall_acoustic_acceptance": False,
        "risks": [
            "heuristic cues are not universal onset truth",
            "segment origins quantize source phase",
            "short windows may fail polyphonic pitch",
            "retained segment tails may double or comb background",
            "ZOH has aliasing and imaging; no antialias filter",
            "future cue boundaries can change earlier background",
        ],
        **asdict(statistics),
    }
    return output, details
