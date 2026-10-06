"""Strict private B2 reference and measured correction input contracts."""

from datetime import datetime  # noqa: TC003 - Pydantic resolves timestamp annotations at runtime.
from typing import Annotated, Literal

from pydantic import BaseModel, ConfigDict, Field

type Text = Annotated[str, Field(strict=True, min_length=1, max_length=4096, pattern=r"\S")]
type Digest = Annotated[str, Field(strict=True, pattern=r"^[0-9a-f]{64}$")]
type Integer = Annotated[int, Field(strict=True, ge=-(2**53), le=2**53)]
type Count = Annotated[int, Field(strict=True, ge=0, le=250000)]
type Seconds = Annotated[float, Field(strict=True, allow_inf_nan=False, ge=0)]
type Number = Annotated[float, Field(strict=True, allow_inf_nan=False)]
type Truth = Annotated[bool, Field(strict=True)]
type One = Annotated[int, Field(strict=True, ge=1, le=1)]
type TrackId = Literal["T01", "T02", "T03", "T04", "T05"]
type Backend = Literal["beat_this_1.1.0_final0_minimal", "legacy_qm_units_fixed"]
type MusicalClass = Literal[
    "constant_tempo",
    "drift",
    "ramp",
    "abrupt_tempo_change",
    "swing",
    "sparse",
    "meter_change",
    "non_4_4",
]

MUSICAL_CLASSES: tuple[MusicalClass, ...] = (
    "constant_tempo",
    "drift",
    "ramp",
    "abrupt_tempo_change",
    "swing",
    "sparse",
    "meter_change",
    "non_4_4",
)
BACKENDS: tuple[Backend, Backend] = (
    "beat_this_1.1.0_final0_minimal",
    "legacy_qm_units_fixed",
)
TRACK_IDS: tuple[TrackId, ...] = ("T01", "T02", "T03", "T04", "T05")
HELD_OUT: tuple[TrackId, ...] = ("T03", "T04", "T05")


class StrictInput(BaseModel):
    """Reject extra fields, coercion and nonfinite numbers recursively."""

    model_config = ConfigDict(extra="forbid", strict=True, allow_inf_nan=False)


class PcmIdentity(StrictInput):
    """Identify the actual complete native loaded mono listening material."""

    path: Text
    sha256: Digest
    sample_rate_hz: Annotated[int, Field(strict=True, gt=0, le=768000)]
    frame_count: Annotated[int, Field(strict=True, gt=0, le=134217728)]
    origin_seconds: Annotated[float, Field(strict=True, ge=0, le=0)]
    dtype: Literal["float32-le"]
    channels: Annotated[int, Field(strict=True, ge=1, le=1)]


class LoadedIdentity(StrictInput):
    """Supply identity evidence without predictions or annotation claims."""

    track_id: TrackId
    source_sha256: Digest
    pcm: PcmIdentity
    provenance: Text


class LoadedIdentities(StrictInput):
    """The five native listening-material identities used to prepare drafts."""

    schema_version: One
    tracks: Annotated[tuple[LoadedIdentity, ...], Field(min_length=5, max_length=5)]


class ClassCertification(StrictInput):
    """A human assessment, including an honest absence, for each class."""

    musical_class: MusicalClass
    status: Literal["present", "absent"]
    provenance: Text


class Region(StrictInput):
    """A contiguous independently listened temporal classification."""

    start_seconds: Seconds
    end_seconds: Seconds
    kind: Literal["metrical", "nonrhythmic", "ambiguous"]
    beat_unit_quarters: Annotated[float, Field(strict=True, gt=0, le=4)] | None
    provenance: Text


class BeatLabel(StrictInput):
    """An independently timed quarter pulse and continuous count identity."""

    seconds: Seconds
    uncertainty_ms: Annotated[float, Field(strict=True, ge=0, le=1000)]
    count: Integer
    quarter: Number
    bar_id: Integer
    quarter_in_bar: Seconds


class BarLabel(StrictInput):
    """An independently timed downbeat with a meter and global quarter origin."""

    bar_id: Integer
    seconds: Seconds
    uncertainty_ms: Annotated[float, Field(strict=True, ge=0, le=1000)]
    start_quarter: Number
    meter_numerator: Annotated[int, Field(strict=True, gt=0, le=32)]
    meter_denominator: Annotated[int, Field(strict=True, gt=0, le=32)]


class GapTransition(StrictInput):
    """Explicit certified count/bar advance across a predeclared temporal gap."""

    before_count: Integer
    after_count: Integer
    before_bar_id: Integer
    after_bar_id: Integer
    quarter_advance: Annotated[float, Field(strict=True, gt=0)]
    provenance: Text


class CriticalFeature(StrictInput):
    """Select critical downbeats before predictions, or certify feature absence."""

    feature: Literal["first_unambiguous", "post_break", "late_track"]
    bar_ids: Annotated[tuple[Integer, ...], Field(max_length=10000)]
    absent_reason: Text | None
    provenance: Text


class ReferenceTrack(StrictInput):
    """Complete independently certified loaded-domain musical reference."""

    identity: LoadedIdentity
    split: Literal["development", "held_out"]
    annotator: Text
    revision: Text
    predictions_seen: Truth
    independent_listening: Truth
    listening_provenance: Text
    time_domain: Literal["native_loaded_frame_zero_seconds"]
    extent_start_seconds: Annotated[float, Field(strict=True, ge=0, le=0)]
    extent_end_seconds: Seconds
    assessor: Text
    certification_provenance: Text
    recording_groups: Annotated[tuple[Text, ...], Field(min_length=1, max_length=256)]
    classes: Annotated[tuple[ClassCertification, ...], Field(min_length=8, max_length=8)]
    regions: Annotated[tuple[Region, ...], Field(min_length=1, max_length=10000)]
    beats: Annotated[tuple[BeatLabel, ...], Field(min_length=1, max_length=250000)]
    bars: Annotated[tuple[BarLabel, ...], Field(min_length=1, max_length=250000)]
    gap_transitions: Annotated[tuple[GapTransition, ...], Field(max_length=10000)]
    critical_features: Annotated[tuple[CriticalFeature, ...], Field(min_length=3, max_length=3)]


class ReferenceBundle(StrictInput):
    """A ready-for-seal bundle; draft files can never satisfy this contract."""

    schema_version: One
    status: Literal["ready_for_seal"]
    manifest_sha256: Digest
    scoring_sha256: Digest
    loaded_identities_path: Text
    loaded_identities_sha256: Digest
    tracks: Annotated[tuple[ReferenceTrack, ...], Field(min_length=5, max_length=5)]


class TrackCoverage(StrictInput):
    """Keep temporal and eligible-event denominators independently visible."""

    track_id: TrackId
    confident_temporal_fraction: Number
    trusted_metrical_fraction: Number
    reference_beats: Count
    reference_downbeats: Count
    eligible_beats_40_and_70: Count
    eligible_downbeats_40_and_70: Count


class ReferenceSeal(StrictInput):
    """A content receipt, not musical acceptance or proof of human honesty."""

    schema_version: One
    status: Literal["sealed_reference_inputs"]
    sealed_at_utc: datetime
    bundle_path: Text
    bundle_sha256: Digest
    manifest_sha256: Digest
    scoring_sha256: Digest
    coverage: tuple[TrackCoverage, ...]
    absent_classes: tuple[MusicalClass, ...]
    corrections: Literal["pending"]
    musical_acceptance: Literal["pending"]
    default_adoption: Literal["blocked"]


class SessionSlot(StrictInput):
    """One predetermined held-out/backend session in the frozen order."""

    session_id: Text
    track_id: TrackId
    backend: Backend


class CorrectionOrder(StrictInput):
    """Freeze a shared editing workflow and balanced order before human work."""

    schema_version: One
    status: Literal["ready_for_seal"]
    reference_seal_sha256: Digest
    annotator: Text
    tool: Text
    workflow: Text
    endpoint: Text
    provenance: Text
    slots: Annotated[tuple[SessionSlot, ...], Field(min_length=6, max_length=6)]


class OrderSeal(StrictInput):
    """An exclusive pre-session schedule content receipt."""

    schema_version: One
    status: Literal["sealed_correction_order"]
    sealed_at_utc: datetime
    order_path: Text
    order_sha256: Digest
    reference_seal_sha256: Digest
    musical_acceptance: Literal["pending"]


class Operations(StrictInput):
    """Actual human operation counts; no generated or estimated timings."""

    inserts: Count
    deletes: Count
    moves: Count
    count: Count
    meter: Count
    phase: Count
    gap: Count


class ActiveInterval(StrictInput):
    """A directly measured active human interval within its editing phase."""

    start_utc: datetime
    end_utc: datetime
    provenance: Text


class CorrectionPhase(StrictInput):
    """Identify edit artifacts, operations and actual active intervals."""

    phase_id: Text
    input_sha256: Digest
    input_path: Text
    output_sha256: Digest
    output_path: Text
    start_utc: datetime
    end_utc: datetime
    operations: Operations
    active_intervals: Annotated[tuple[ActiveInterval, ...], Field(max_length=10000)]


class CriticalOutcome(StrictInput):
    """Measured corrected timing and bar-identity outcome for a critical bar."""

    bar_id: Integer
    bar_identity_correct: Truth
    timing_error_ms: Number
    provenance: Text


class ProducerIdentity(StrictInput):
    """Actual implementation bytes and explicitly attested backend configuration."""

    implementation_path: Text
    implementation_sha256: Digest
    configuration: Literal["beat_this_1.1.0_final0_minimal_cpu_fp32", "corrected_legacy_qm"]
    model_sha256: Digest | None
    legacy_units_fixed: Truth
    provenance: Text


class CorrectionSession(StrictInput):
    """A real session anchored to the frozen source, reference and schedule."""

    session_id: Text
    track_id: TrackId
    backend: Backend
    producer: ProducerIdentity
    source_sha256: Digest
    reference_revision: Text
    reference_seal_sha256: Digest
    annotator: Text
    tool: Text
    workflow: Text
    endpoint: Text
    human_measured: Truth
    provenance: Text
    zero_active_time_reason: Text | None
    initial_prediction_sha256: Digest
    initial_prediction_path: Text
    corrected_result_sha256: Digest
    corrected_result_path: Text
    start_utc: datetime
    end_utc: datetime
    phases: Annotated[tuple[CorrectionPhase, ...], Field(min_length=1, max_length=256)]
    critical_outcomes: Annotated[tuple[CriticalOutcome, ...], Field(max_length=10000)]


class CorrectionBundle(StrictInput):
    """Paired held-out human measurements, ready only after both receipts exist."""

    schema_version: One
    status: Literal["ready_for_validation"]
    reference_seal_sha256: Digest
    order_seal_sha256: Digest
    sessions: Annotated[tuple[CorrectionSession, ...], Field(min_length=6, max_length=6)]
