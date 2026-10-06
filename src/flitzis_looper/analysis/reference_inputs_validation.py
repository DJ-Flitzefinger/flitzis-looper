"""Bounded file and semantic validation for private, blind B2 input receipts."""

import hashlib
import json
import math
import struct
from bisect import bisect_left, bisect_right
from datetime import UTC, datetime
from itertools import pairwise
from typing import TYPE_CHECKING

from pydantic import BaseModel, ConfigDict, TypeAdapter

from flitzis_looper.analysis.reference_inputs_models import (
    BACKENDS,
    HELD_OUT,
    MUSICAL_CLASSES,
    TRACK_IDS,
    CorrectionBundle,
    CorrectionOrder,
    Digest,
    LoadedIdentities,
    LoadedIdentity,
    OrderSeal,
    ProducerIdentity,
    ReferenceBundle,
    ReferenceSeal,
    TrackCoverage,
)

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.analysis.reference_inputs_models import (
        CorrectionPhase,
        CorrectionSession,
        CriticalFeature,
        MusicalClass,
        ReferenceTrack,
    )

MANIFEST_SHA256 = "df0d3e62731c58383c83fbb7f6474f6534648eb873d1045f6238b81d28fe8ac5"
SCORING_SHA256 = "056d6db87684252844b3b7243bee647063e31396826083d43ccd731051ec925d"
MAX_JSON_BYTES = 16 * 1024 * 1024
_HASH_CHUNK = 64 * 1024
LOADED_IDENTITIES_SHA256 = "7952861334862194f2de40b5da7b74d0e2a7e121bd717bffb310a9da4983cabb"
MODEL_SHA256 = "8c328b45f59d8dd3dff219253ff6a8d6482be57d0133a29140e2febbf8eb8331"


class FrozenTrack(BaseModel):
    """Read only the identity fields of the byte-verified immutable manifest."""

    model_config = ConfigDict(extra="ignore", strict=True)
    id: str
    source_relative: str
    source_sha256: Digest
    source_bytes: int
    split: str


class FrozenCorpus(BaseModel):
    """The immutable manifest's private source inventory."""

    model_config = ConfigDict(extra="ignore", strict=True)
    tracks: tuple[FrozenTrack, ...]


def fail(reason: str) -> None:
    """Raise a stable, actionable validation reason."""
    raise ValueError(reason)


def private_path(workspace: Path, value: str | Path) -> Path:
    """Resolve only workspace-contained private artifacts, excluding Git content."""
    root = workspace.resolve(strict=True)
    path = (root / value).resolve()
    if not path.is_relative_to(root) or path.is_relative_to(root / "repo"):
        fail("private_path_outside_workspace_or_inside_repo")
    return path


def _implementation_path(workspace: Path, value: str) -> Path:
    root = workspace.resolve(strict=True)
    path = (root / value).resolve(strict=True)
    if not path.is_relative_to(root):
        fail("implementation_file_outside_workspace")
    return path


def sha256_file(path: Path, *, pcm: bool = False) -> str:
    """Hash bounded chunks, optionally rejecting nonfinite little-endian float32 PCM."""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(_HASH_CHUNK):
            digest.update(chunk)
            if pcm and (
                len(chunk) % 4
                or any(not math.isfinite(v[0]) for v in struct.iter_unpack("<f", chunk))
            ):
                fail("pcm_must_be_finite_complete_float32_le")
    return digest.hexdigest()


def _unique_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            fail("duplicate_json_key")
        result[key] = value
    return result


def read_json_bytes(path: Path) -> bytes:
    """Read bounded JSON and reject ambiguous duplicate keys before strict parsing."""
    with path.open("rb") as handle:
        raw = handle.read(MAX_JSON_BYTES + 1)
    if len(raw) > MAX_JSON_BYTES:
        fail("json_size_limit")
    try:
        json.loads(raw, object_pairs_hook=_unique_object)
    except RecursionError as error:
        msg = "json_nesting_limit"
        raise ValueError(msg) from error
    return raw


def read_input[T](workspace: Path, path: str | Path, adapter: TypeAdapter[T]) -> T:
    """Read a strictly typed private JSON artifact with bounded input size."""
    return adapter.validate_json(read_json_bytes(private_path(workspace, path)), strict=True)


def write_output(workspace: Path, path: str | Path, data: bytes) -> Path:
    """Create a private receipt/draft exclusively; never overwrite an existing file."""
    target = private_path(workspace, path)
    if len(data) > MAX_JSON_BYTES:
        fail("json_size_limit")
    target.parent.mkdir(parents=True, exist_ok=True)
    with target.open("xb") as handle:
        handle.write(data)
    return target


def frozen_corpus(workspace: Path) -> FrozenCorpus:
    """Require both original immutable protocol byte identities before any input work."""
    manifest = private_path(workspace, "scratch/b2a/frozen-manifest.json")
    scoring = private_path(workspace, "scratch/b2a/scoring-addendum.json")
    if sha256_file(manifest) != MANIFEST_SHA256 or sha256_file(scoring) != SCORING_SHA256:
        fail("frozen_protocol_hash_mismatch")
    corpus = TypeAdapter(FrozenCorpus).validate_json(read_json_bytes(manifest), strict=True)
    expected: dict[str, str] = {
        track: "development" if track in TRACK_IDS[:2] else "held_out" for track in TRACK_IDS
    }
    expected["R01"] = "resource_only"
    if len(corpus.tracks) != 6 or {track.id: track.split for track in corpus.tracks} != expected:
        fail("frozen_track_split_mismatch")
    return corpus


def validate_identities(identities: LoadedIdentities, corpus: FrozenCorpus) -> None:
    """Bind all and only T01-T05 to their unchanged original hashes and frozen split."""
    if {item.track_id for item in identities.tracks} != set(TRACK_IDS):
        fail("complete_T01_T05_identities_required_R01_resource_only")
    frozen = {track.id: track for track in corpus.tracks}
    for identity in identities.tracks:
        if identity.source_sha256 != frozen[identity.track_id].source_sha256:
            fail("source_identity_mismatch")


def loaded_identities(workspace: Path, path: str | Path) -> LoadedIdentities:
    """Read the independently verified private native inventory, never model outputs."""
    identity_path = private_path(workspace, path)
    if sha256_file(identity_path) != LOADED_IDENTITIES_SHA256:
        fail("approved_loaded_identity_inventory_hash_mismatch")
    return read_input(workspace, identity_path, TypeAdapter(LoadedIdentities))


def _validate_material(workspace: Path, identity: LoadedIdentity, source: FrozenTrack) -> None:
    original = private_path(workspace, source.source_relative)
    if (
        original.stat().st_size != source.source_bytes
        or sha256_file(original) != source.source_sha256
    ):
        fail("original_source_hash_or_size_mismatch")
    pcm = private_path(workspace, identity.pcm.path)
    if pcm.stat().st_size != identity.pcm.frame_count * 4:
        fail("complete_loaded_pcm_size_mismatch")
    if sha256_file(pcm, pcm=True) != identity.pcm.sha256:
        fail("loaded_pcm_hash_mismatch")


def _validate_regions(track: ReferenceTrack, duration: float) -> tuple[float, float]:
    if track.extent_end_seconds != duration:
        fail("complete_loaded_extent_required")
    end = 0.0
    confident = metrical = 0.0
    for region in track.regions:
        if region.start_seconds != end or region.end_seconds <= end:
            fail("regions_must_be_contiguous_from_frame_zero")
        if (region.kind == "metrical") != (region.beat_unit_quarters is not None):
            fail("beat_unit_required_only_for_metrical_regions")
        length = region.end_seconds - end
        if region.kind != "ambiguous":
            confident += length
        if region.kind == "metrical":
            metrical += length
        end = region.end_seconds
    if end != duration:
        fail("regions_must_cover_complete_exclusive_end")
    if confident / duration < 0.9 or metrical == 0:
        fail("insufficient_confident_temporal_or_metrical_coverage")
    return confident / duration, metrical / duration


class _RegionIndex:
    """Bound semantic lookup work by binary-searching already validated regions."""

    def __init__(self, track: ReferenceTrack) -> None:
        self.regions = track.regions
        self.starts = tuple(region.start_seconds for region in track.regions)
        gaps = tuple(region for region in track.regions if region.kind != "metrical")
        self.gap_starts = tuple(region.start_seconds for region in gaps)
        self.gap_ends = tuple(region.end_seconds for region in gaps)

    def metrical(self, seconds: float) -> bool:
        index = bisect_right(self.starts, seconds) - 1
        return (
            index >= 0
            and self.regions[index].kind == "metrical"
            and seconds < self.regions[index].end_seconds
        )

    def beat_unit(self, seconds: float) -> float:
        index = bisect_right(self.starts, seconds) - 1
        unit = self.regions[index].beat_unit_quarters
        if unit is None:
            fail("beat_unit_required_in_metrical_region")
        assert unit is not None
        return unit

    def gap_between(self, before: float, after: float) -> bool:
        index = bisect_right(self.gap_starts, before)
        return index < len(self.gap_ends) and self.gap_ends[index] <= after


def _validate_bars(track: ReferenceTrack, duration: float, regions: _RegionIndex) -> None:
    declared_transitions = {(gap.before_bar_id, gap.after_bar_id) for gap in track.gap_transitions}
    for bar in track.bars:
        if bar.meter_denominator not in {1, 2, 4, 8, 16, 32}:
            fail("invalid_meter_denominator")
        if bar.seconds >= duration or not regions.metrical(bar.seconds):
            fail("downbeat_outside_metrical_source")
    if len({bar.bar_id for bar in track.bars}) != len(track.bars):
        fail("duplicate_bar_identity")
    for before, after in pairwise(track.bars):
        if before.seconds >= after.seconds or before.bar_id >= after.bar_id:
            fail("bars_must_increase_in_time_and_identity")
        next_quarter = before.start_quarter + before.meter_numerator * 4 / before.meter_denominator
        if after.bar_id == before.bar_id + 1 and after.start_quarter == next_quarter:
            continue
        declared = (before.bar_id, after.bar_id) in declared_transitions
        if not declared or not regions.gap_between(before.seconds, after.seconds):
            fail("bar_count_or_meter_discontinuity_without_declared_gap")
        if after.start_quarter <= before.start_quarter:
            fail("bar_quarters_must_increase")


def _validate_beat_positions(track: ReferenceTrack, duration: float, regions: _RegionIndex) -> None:
    bars = {bar.bar_id: bar for bar in track.bars}
    bar_ends = {before.bar_id: after.seconds for before, after in pairwise(track.bars)}
    for beat in track.beats:
        if (
            beat.seconds >= duration
            or not regions.metrical(beat.seconds)
            or beat.bar_id not in bars
        ):
            fail("beat_outside_metrical_source_or_unknown_bar")
        bar = bars[beat.bar_id]
        length = bar.meter_numerator * 4 / bar.meter_denominator
        if beat.quarter_in_bar >= length or beat.quarter != bar.start_quarter + beat.quarter_in_bar:
            fail("beat_quarter_bar_identity_mismatch")
        if beat.seconds < bar.seconds or (beat.quarter_in_bar == 0 and beat.seconds != bar.seconds):
            fail("beat_downbeat_time_identity_mismatch")
        if beat.seconds >= bar_ends.get(beat.bar_id, duration):
            fail("beat_time_outside_its_bar")


def _validate_beat_continuity(track: ReferenceTrack, regions: _RegionIndex) -> None:
    transitions = {(gap.before_count, gap.after_count): gap for gap in track.gap_transitions}
    if len(transitions) != len(track.gap_transitions):
        fail("duplicate_gap_transition")
    used: set[tuple[int, int]] = set()
    for before, after in pairwise(track.beats):
        advance = after.count - before.count
        quarter_advance = after.quarter - before.quarter
        if before.seconds >= after.seconds or advance <= 0 or quarter_advance <= 0:
            fail("beat_time_count_or_quarter_discontinuity")
        key = (before.count, after.count)
        if key in transitions:
            gap = transitions[key]
            if (gap.before_bar_id, gap.after_bar_id) != (before.bar_id, after.bar_id):
                fail("gap_bar_identity_mismatch")
            if not regions.gap_between(before.seconds, after.seconds):
                fail("gap_transition_requires_predeclared_temporal_gap")
            if quarter_advance != gap.quarter_advance:
                fail("gap_quarter_advance_mismatch")
            used.add(key)
        elif advance != 1 or quarter_advance != regions.beat_unit(before.seconds):
            fail("beat_count_or_quarter_advance_requires_declared_gap")
    if used != set(transitions):
        fail("unused_gap_transition")


def _critical_selection(feature: CriticalFeature, known_bars: set[int]) -> set[int]:
    selected = set(feature.bar_ids)
    if len(selected) != len(feature.bar_ids) or not selected <= known_bars:
        fail("critical_downbeat_selection_incomplete_or_wrong_identity")
    if bool(selected) == (feature.absent_reason is not None):
        fail("critical_feature_needs_selected_bars_or_absence_reason")
    return selected


def _validate_critical(track: ReferenceTrack, duration: float) -> None:
    features = {str(feature.feature): feature for feature in track.critical_features}
    if set(features) != {"first_unambiguous", "post_break", "late_track"}:
        fail("all_critical_features_required")
    expected = {"first_unambiguous": {track.bars[0].bar_id}, "post_break": set()}
    bar_seconds = tuple(bar.seconds for bar in track.bars)
    for region in track.regions:
        if region.kind == "metrical" or region.start_seconds == 0 or region.end_seconds == duration:
            continue
        index = bisect_left(bar_seconds, region.end_seconds)
        if index < len(track.bars):
            expected["post_break"].add(track.bars[index].bar_id)
    known_bars = {bar.bar_id for bar in track.bars}
    for name, ids in expected.items():
        selected = _critical_selection(features[name], known_bars)
        if (name == "first_unambiguous" and selected != ids) or not ids <= selected:
            fail("critical_downbeat_selection_incomplete_or_wrong_identity")
    _critical_selection(features["late_track"], known_bars)


def _validate_certification(track: ReferenceTrack) -> None:
    if track.predictions_seen or not track.independent_listening:
        fail("blind_independent_listening_attestation_required")
    if len(set(track.recording_groups)) != len(track.recording_groups):
        fail("duplicate_recording_group")
    if {item.musical_class for item in track.classes} != set(MUSICAL_CLASSES):
        fail("complete_independent_class_certification_required")


def validate_reference(
    workspace: Path, bundle: ReferenceBundle, corpus: FrozenCorpus
) -> tuple[tuple[TrackCoverage, ...], tuple[MusicalClass, ...]]:
    """Validate real listening files and independent full-span/count/coverage assertions."""
    if (bundle.manifest_sha256, bundle.scoring_sha256) != (MANIFEST_SHA256, SCORING_SHA256):
        fail("reference_protocol_hash_mismatch")
    if bundle.loaded_identities_sha256 != LOADED_IDENTITIES_SHA256:
        fail("reference_loaded_inventory_identity_mismatch")
    inventory = loaded_identities(workspace, bundle.loaded_identities_path)
    supplied = {track.identity.track_id: track.identity for track in bundle.tracks}
    if supplied != {identity.track_id: identity for identity in inventory.tracks}:
        fail("supplied_pcm_not_bound_to_native_loaded_export")
    validate_identities(
        LoadedIdentities(schema_version=1, tracks=tuple(t.identity for t in bundle.tracks)), corpus
    )
    sources = {track.id: track for track in corpus.tracks}
    groups: dict[str, str] = {}
    coverage: list[TrackCoverage] = []
    present_classes: set[str] = set()
    for track in bundle.tracks:
        source = sources[track.identity.track_id]
        if track.split != source.split:
            fail("reference_split_mismatch")
        _validate_material(workspace, track.identity, source)
        _validate_certification(track)
        for group in track.recording_groups:
            if group in groups and groups[group] != track.split:
                fail("recording_group_contaminates_development_held_out_split")
            groups[group] = track.split
        present_classes.update(
            item.musical_class for item in track.classes if item.status == "present"
        )
        duration = track.identity.pcm.frame_count / track.identity.pcm.sample_rate_hz
        confident, metrical = _validate_regions(track, duration)
        regions = _RegionIndex(track)
        _validate_bars(track, duration, regions)
        _validate_beat_positions(track, duration, regions)
        _validate_beat_continuity(track, regions)
        _validate_critical(track, duration)
        eligible_beats = sum(beat.uncertainty_ms <= 10 for beat in track.beats)
        eligible_bars = sum(bar.uncertainty_ms <= 10 for bar in track.bars)
        if eligible_beats / len(track.beats) < 0.9 or eligible_bars / len(track.bars) < 0.9:
            fail("insufficient_40_70ms_eligible_beats_or_downbeats")
        coverage.append(
            TrackCoverage(
                track_id=track.identity.track_id,
                confident_temporal_fraction=confident,
                trusted_metrical_fraction=metrical,
                reference_beats=len(track.beats),
                reference_downbeats=len(track.bars),
                eligible_beats_40_and_70=eligible_beats,
                eligible_downbeats_40_and_70=eligible_bars,
            )
        )
    absent = tuple(name for name in MUSICAL_CLASSES if name not in present_classes)
    return tuple(coverage), absent


def utc_now() -> datetime:
    """Timestamp exclusive receipts with the actual current UTC time."""
    return datetime.now(UTC)


def utc_time(value: datetime) -> None:
    """Reject naive, non-UTC and future session timestamps."""
    offset = value.utcoffset()
    if offset is None or offset.total_seconds() != 0:
        fail("timestamps_must_be_aware_UTC")
    if value > utc_now():
        fail("future_human_session_timestamp")


def reference_seal(workspace: Path, path: str | Path) -> tuple[ReferenceSeal, ReferenceBundle, str]:
    """Revalidate receipt-bound original bytes and listening material before session work."""
    receipt_path = private_path(workspace, path)
    raw = read_json_bytes(receipt_path)
    seal = TypeAdapter(ReferenceSeal).validate_json(raw, strict=True)
    utc_time(seal.sealed_at_utc)
    bundle_path = private_path(workspace, seal.bundle_path)
    if sha256_file(bundle_path) != seal.bundle_sha256:
        fail("sealed_reference_bundle_changed")
    bundle = TypeAdapter(ReferenceBundle).validate_json(read_json_bytes(bundle_path), strict=True)
    coverage, absent = validate_reference(workspace, bundle, frozen_corpus(workspace))
    if seal.coverage != coverage or tuple(seal.absent_classes) != absent:
        fail("reference_receipt_coverage_mismatch")
    if (seal.manifest_sha256, seal.scoring_sha256) != (MANIFEST_SHA256, SCORING_SHA256):
        fail("reference_receipt_protocol_mismatch")
    return seal, bundle, hashlib.sha256(raw).hexdigest()


def validate_order(order: CorrectionOrder, reference_hash: str) -> None:
    """Require the complete held-out pair schedule and balanced opposite backend orders."""
    if order.reference_seal_sha256 != reference_hash:
        fail("order_reference_seal_mismatch")
    if len({slot.session_id for slot in order.slots}) != 6:
        fail("unique_session_ids_required")
    first_orders = []
    for track in HELD_OUT:
        backends = [slot.backend for slot in order.slots if slot.track_id == track]
        if len(backends) != 2 or set(backends) != set(BACKENDS):
            fail("complete_held_out_backend_pairs_required")
        first_orders.append(backends[0])
    if abs(first_orders.count(BACKENDS[0]) - first_orders.count(BACKENDS[1])) > 1:
        fail("backend_order_must_be_balanced_before_sessions")


def order_seal(
    workspace: Path, path: str | Path, reference_hash: str
) -> tuple[OrderSeal, CorrectionOrder, str]:
    """Revalidate the byte-frozen order and its exact reference receipt identity."""
    raw = read_json_bytes(private_path(workspace, path))
    seal = TypeAdapter(OrderSeal).validate_json(raw, strict=True)
    utc_time(seal.sealed_at_utc)
    order_path = private_path(workspace, seal.order_path)
    if sha256_file(order_path) != seal.order_sha256 or seal.reference_seal_sha256 != reference_hash:
        fail("sealed_order_changed_or_reference_mismatch")
    order = TypeAdapter(CorrectionOrder).validate_json(read_json_bytes(order_path), strict=True)
    validate_order(order, reference_hash)
    return seal, order, hashlib.sha256(raw).hexdigest()


def _validate_range(start: datetime, end: datetime, lower: datetime, upper: datetime) -> None:
    utc_time(start)
    utc_time(end)
    if not lower <= start < end <= upper:
        fail("human_time_range_outside_phase_session_or_seal")


def _phase_measurement(phase: CorrectionPhase) -> tuple[int, float]:
    end = phase.start_utc
    seconds = 0.0
    for interval in phase.active_intervals:
        _validate_range(interval.start_utc, interval.end_utc, phase.start_utc, phase.end_utc)
        if interval.start_utc < end:
            fail("human_active_intervals_overlap_or_unsorted")
        end = interval.end_utc
        seconds += (interval.end_utc - interval.start_utc).total_seconds()
    operations = sum((
        phase.operations.inserts,
        phase.operations.deletes,
        phase.operations.moves,
        phase.operations.count,
        phase.operations.meter,
        phase.operations.phase,
        phase.operations.gap,
    ))
    return operations, seconds


def _validate_session_identity(
    session: CorrectionSession, order: CorrectionOrder, track: ReferenceTrack
) -> None:
    if not session.human_measured:
        fail("actual_human_measurement_attestation_required")
    if (session.annotator, session.tool, session.workflow, session.endpoint) != (
        order.annotator,
        order.tool,
        order.workflow,
        order.endpoint,
    ):
        fail("paired_workflow_annotator_tool_or_endpoint_mismatch")
    if (session.source_sha256, session.reference_revision, session.reference_seal_sha256) != (
        track.identity.source_sha256,
        track.revision,
        order.reference_seal_sha256,
    ):
        fail("session_source_revision_or_reference_seal_mismatch")
    critical = {bar for feature in track.critical_features for bar in feature.bar_ids}
    if (
        len(session.critical_outcomes) != len(critical)
        or {outcome.bar_id for outcome in session.critical_outcomes} != critical
    ):
        fail("complete_critical_outcomes_required")


def _artifact(workspace: Path, path: str, digest: str) -> None:
    file = private_path(workspace, path)
    if file.stat().st_size > MAX_JSON_BYTES or sha256_file(file) != digest:
        fail("actual_correction_artifact_size_or_hash_mismatch")


def _validate_producer(workspace: Path, session: CorrectionSession) -> None:
    producer = session.producer
    path = _implementation_path(workspace, producer.implementation_path)
    if (
        path.stat().st_size > 512 * 1024 * 1024
        or sha256_file(path) != producer.implementation_sha256
    ):
        fail("producer_implementation_size_or_hash_mismatch")
    expected: tuple[str, str | None, bool]
    if session.backend == BACKENDS[0]:
        expected = ("beat_this_1.1.0_final0_minimal_cpu_fp32", MODEL_SHA256, False)
    else:
        expected = ("corrected_legacy_qm", None, True)
    if (producer.configuration, producer.model_sha256, producer.legacy_units_fixed) != expected:
        fail("selected_model_or_repaired_legacy_producer_attestation_required")


def _session_measurement(workspace: Path, session: CorrectionSession) -> tuple[int, float]:
    if len({phase.phase_id for phase in session.phases}) != len(session.phases):
        fail("unique_phase_ids_required")
    previous_end = session.start_utc
    previous_hash = session.initial_prediction_sha256
    _artifact(workspace, session.initial_prediction_path, session.initial_prediction_sha256)
    _artifact(workspace, session.corrected_result_path, session.corrected_result_sha256)
    operations = 0
    seconds = 0.0
    for phase in session.phases:
        _validate_range(phase.start_utc, phase.end_utc, session.start_utc, session.end_utc)
        if phase.start_utc < previous_end or phase.input_sha256 != previous_hash:
            fail("phase_order_or_input_output_chain_mismatch")
        _artifact(workspace, phase.input_path, phase.input_sha256)
        _artifact(workspace, phase.output_path, phase.output_sha256)
        phase_operations, phase_seconds = _phase_measurement(phase)
        if phase_operations and phase_seconds == 0:
            fail("correction_phase_operations_require_positive_active_time")
        operations += phase_operations
        seconds += phase_seconds
        previous_end, previous_hash = phase.end_utc, phase.output_sha256
    if previous_hash != session.corrected_result_sha256:
        fail("corrected_endpoint_artifact_mismatch")
    if seconds == 0:
        if operations or session.zero_active_time_reason is None:
            fail("zero_active_time_requires_zero_operations_and_human_reason")
    elif session.zero_active_time_reason is not None:
        fail("zero_active_time_reason_conflicts_with_measured_time")
    return operations, seconds


def validate_corrections(
    workspace: Path,
    bundle: CorrectionBundle,
    reference: ReferenceBundle,
    order: CorrectionOrder,
    seals: tuple[str, str],
    order_timestamp: datetime,
) -> tuple[tuple[str, int, float], ...]:
    """Validate paired inputs and artifact hashes without parsing or deciding acceptance."""
    if (bundle.reference_seal_sha256, bundle.order_seal_sha256) != seals:
        fail("correction_bundle_receipt_mismatch")
    tracks = {track.identity.track_id: track for track in reference.tracks}
    measurements: list[tuple[str, int, float]] = []
    producers: dict[str, ProducerIdentity] = {}
    previous_end = order_timestamp
    for slot, session in zip(order.slots, bundle.sessions, strict=True):
        if (session.session_id, session.track_id, session.backend) != (
            slot.session_id,
            slot.track_id,
            slot.backend,
        ):
            fail("actual_session_order_differs_from_frozen_order")
        _validate_range(session.start_utc, session.end_utc, previous_end, utc_now())
        previous_end = session.end_utc
        _validate_session_identity(session, order, tracks[session.track_id])
        _validate_producer(workspace, session)
        if session.backend in producers and session.producer != producers[session.backend]:
            fail("backend_producer_changed_between_paired_sessions")
        producers[session.backend] = session.producer
        operations, active_seconds = _session_measurement(workspace, session)
        measurements.append((session.session_id, operations, active_seconds))
    return tuple(measurements)
