"""Strict offline contracts for real loopback evidence and synthetic tool fixtures."""

from datetime import datetime  # noqa: TC003 - Pydantic resolves this annotation at runtime.
from typing import Annotated, Literal

from pydantic import Field

from flitzis_looper.analysis.reference_inputs_models import Digest, StrictInput, Text

type Frame = Annotated[int, Field(strict=True, ge=0, le=2**53)]
type Channel = Annotated[int, Field(strict=True, ge=0, le=15)]
type Finite = Annotated[float, Field(strict=True, allow_inf_nan=False)]
type Positive = Annotated[float, Field(strict=True, gt=0, allow_inf_nan=False)]
type Nonnegative = Annotated[float, Field(strict=True, ge=0, allow_inf_nan=False)]
type EvidenceKind = Literal["real_device_loopback", "synthetic_fixture"]


class Artifact(StrictInput):
    """Bind exact private bytes, never a repository or external personal path."""

    path: Text
    sha256: Digest


class DetectorChannel(StrictInput):
    """Detect absolute-amplitude rising edges without reading predicted periods."""

    channel: Channel
    high: Positive
    low: Nonnegative
    minimum_gap_capture_frames: Annotated[int, Field(strict=True, ge=1, le=768000)]
    rearm_low_capture_frames: Annotated[int, Field(strict=True, ge=1, le=768000)]


class DetectorPolicy(StrictInput):
    """A declared pre-capture policy; its timing/provenance is a caller assertion."""

    schema_version: Literal[1]
    method: Literal["absolute_threshold_hysteresis_v1"]
    frozen_at_utc: datetime
    provenance: Text
    channels: Annotated[tuple[DetectorChannel, ...], Field(min_length=1, max_length=16)]


class Feature(StrictInput):
    """An observed edge ID; this is deliberately not a musical cycle label."""

    event_id: Frame
    capture_frame: Frame


class ChannelFeatures(StrictInput):
    """Retain every observed edge and detector exclusions for one channel."""

    channel: Channel
    peak_absolute: Nonnegative
    samples_at_or_above_full_scale: Frame
    rejected_by_minimum_gap: Frame
    edges: Annotated[tuple[Feature, ...], Field(max_length=100000)]


class FeatureEvidence(StrictInput):
    """Complete-file measured features, explicitly separate from musical labels."""

    schema_version: Literal[1]
    evidence_kind: EvidenceKind
    capture: Artifact
    policy: Artifact
    wave_format: Literal["pcm16", "pcm24", "pcm32", "float32"]
    capture_sample_rate_hz: Annotated[int, Field(strict=True, ge=1, le=768000)]
    capture_channel_count: Annotated[int, Field(strict=True, ge=1, le=16)]
    capture_frame_count: Frame
    measured_channels: tuple[ChannelFeatures, ...]
    scope: Literal["offline_features_only_no_device_or_listening_acceptance"]


class RecordingClock(StrictInput):
    """Relate two clocks only with named calibration or shared-clock evidence."""

    mode: Literal["unknown", "shared_digital_clock", "calibrated"]
    output_clock_identity: Text
    recording_clock_identity: Text
    output_frames_per_capture_frame: Positive | None
    ratio_halfwidth: Nonnegative | None
    valid_duration_seconds: Positive | None
    independent_of_candidate_period: bool | None
    provenance: Text | None
    evidence: Artifact | None


class EventCycle(StrictInput):
    """Explicit independent cycle association, never filled from edge array indices."""

    event_id: Frame
    cycle: Frame


class ChannelComparison(StrictInput):
    """Retain separate feature, sampling and audible DSP uncertainties."""

    channel: Channel
    pad_id: Annotated[int, Field(strict=True, ge=0, le=215)]
    cycle_count_provenance: Text
    independent_cycle_counts: bool
    stationary_configuration_asserted: bool
    feature_localization_halfwidth_capture_frames: Annotated[
        float, Field(strict=True, ge=0.5, allow_inf_nan=False)
    ]
    seam_feature_offset_output_frames: Finite
    seam_feature_variation_halfwidth_output_frames: Nonnegative
    seam_feature_provenance: Text
    audible_dsp_alignment_output_frames: Finite
    audible_dsp_variation_halfwidth_output_frames: Nonnegative
    audible_dsp_provenance: Text
    observations: Annotated[tuple[EventCycle, ...], Field(min_length=2, max_length=100000)]


class ComparisonInput(StrictInput):
    """A hash-bound observation packet; human assertions remain explicitly assertions."""

    schema_version: Literal[1]
    evidence_kind: EvidenceKind
    productive_run: Artifact
    features: Artifact
    capture_started_at_utc: datetime
    recorder_identity: Text
    capture_route: Text
    clock: RecordingClock
    channels: Annotated[tuple[ChannelComparison, ...], Field(min_length=1, max_length=16)]


class ListeningObservation(StrictInput):
    """Actual human observations, not inferred audio or timing labels."""

    elapsed_seconds: Nonnegative
    audible_drift: Literal["clear", "failed", "unassessed"]
    seam_clicks: Literal["clear", "failed", "unassessed"]
    missed_or_doubled_beats: Literal["clear", "failed", "unassessed"]
    interpad_alignment: Literal["clear", "failed", "unassessed"]
    keylock_stem_dsp: Literal["clear", "failed", "unassessed"]
    note: Text


class ListeningInput(StrictInput):
    """A sustained listening declaration; validation cannot establish its truth."""

    schema_version: Literal[1]
    evidence_kind: EvidenceKind
    productive_run: Artifact
    productive_run_end: Artifact
    features: Artifact
    capture_start_frame: Frame
    capture_end_frame: Frame
    capture_interval_provenance: Text
    observer: Text
    listening_provenance: Text
    started_at_utc: datetime
    ended_at_utc: datetime
    continuous_uninterrupted: bool
    actual_productive_app_listened_to: bool
    outcome: Literal["pass", "fail", "inconclusive"]
    observations: Annotated[tuple[ListeningObservation, ...], Field(min_length=2, max_length=1000)]
