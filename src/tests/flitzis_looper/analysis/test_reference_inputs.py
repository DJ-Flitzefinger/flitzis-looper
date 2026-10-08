"""Behavioral validation of blind references and actual paired correction inputs."""

import hashlib
import json
import struct
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import TYPE_CHECKING

import pytest
from pydantic import TypeAdapter, ValidationError

from flitzis_looper.analysis import reference_inputs as cli
from flitzis_looper.analysis import reference_inputs_validation as validation
from flitzis_looper.analysis.beat_scoring import score_track_timing
from flitzis_looper.analysis.contracts import BeatPredictions
from flitzis_looper.analysis.reference_inputs_models import (
    BACKENDS,
    HELD_OUT,
    MUSICAL_CLASSES,
    TRACK_IDS,
    ActiveInterval,
    BarLabel,
    BeatLabel,
    ClassCertification,
    CorrectionBundle,
    CorrectionOrder,
    CorrectionPhase,
    CorrectionSession,
    CriticalFeature,
    CriticalOutcome,
    GapTransition,
    LoadedIdentities,
    LoadedIdentity,
    Operations,
    OrderSeal,
    PcmIdentity,
    ProducerIdentity,
    ReferenceBundle,
    ReferenceSeal,
    ReferenceTrack,
    Region,
    SessionSlot,
)

if TYPE_CHECKING:
    from collections.abc import Callable
    from typing import IO


def _digest(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


@dataclass
class Prepared:
    workspace: Path
    bundle: ReferenceBundle
    corpus: validation.FrozenCorpus


@pytest.fixture
def prepared(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Prepared:
    private = tmp_path / "scratch" / "b2a"
    private.mkdir(parents=True)
    sources = []
    identities = []
    pcm = struct.pack("<128f", *([0.25] * 128))
    for index, track in enumerate((*TRACK_IDS, "R01")):
        source = tmp_path / f"{track}.source"
        raw = f"synthetic source {track}".encode()
        source.write_bytes(raw)
        sources.append({
            "id": track,
            "source_relative": source.name,
            "source_sha256": _digest(raw),
            "source_bytes": len(raw),
            "split": "resource_only"
            if track == "R01"
            else "development"
            if track in {"T01", "T02"}
            else "held_out",
        })
        if track != "R01":
            pcm_path = tmp_path / f"{track}.f32le"
            pcm_path.write_bytes(pcm)
            identities.append(
                LoadedIdentity(
                    track_id=TRACK_IDS[index],
                    source_sha256=_digest(raw),
                    provenance="synthetic native evidence",
                    pcm=PcmIdentity(
                        path=pcm_path.name,
                        sha256=_digest(pcm),
                        sample_rate_hz=8,
                        frame_count=128,
                        origin_seconds=0.0,
                        dtype="float32-le",
                        channels=1,
                    ),
                )
            )
    manifest = json.dumps({"tracks": sources}).encode()
    (private / "frozen-manifest.json").write_bytes(manifest)
    scoring = b'{"fixture": "immutable scoring"}'
    (private / "scoring-addendum.json").write_bytes(scoring)
    inventory = (
        LoadedIdentities(schema_version=1, tracks=tuple(identities)).model_dump_json().encode()
    )
    (tmp_path / "identities.json").write_bytes(inventory)
    for module in (cli, validation):
        monkeypatch.setattr(module, "MANIFEST_SHA256", _digest(manifest))
        monkeypatch.setattr(module, "SCORING_SHA256", _digest(scoring))
        monkeypatch.setattr(module, "LOADED_IDENTITIES_SHA256", _digest(inventory))
    tracks = tuple(_reference_track(identity) for identity in identities)
    bundle = ReferenceBundle(
        schema_version=1,
        status="ready_for_seal",
        manifest_sha256=_digest(manifest),
        scoring_sha256=_digest(scoring),
        loaded_identities_path="identities.json",
        loaded_identities_sha256=_digest(inventory),
        tracks=tracks,
    )
    return Prepared(tmp_path, bundle, validation.frozen_corpus(tmp_path))


def _reference_track(identity: LoadedIdentity) -> ReferenceTrack:
    return ReferenceTrack(
        identity=identity,
        split="development" if identity.track_id in {"T01", "T02"} else "held_out",
        annotator="independent annotator",
        revision="human-v1",
        predictions_seen=False,
        independent_listening=True,
        listening_provenance="listened exact loaded PCM independently",
        time_domain="native_loaded_frame_zero_seconds",
        extent_start_seconds=0.0,
        extent_end_seconds=16.0,
        assessor="independent assessor",
        certification_provenance="full recording checked by human",
        recording_groups=(f"group-{identity.track_id}",),
        classes=tuple(
            ClassCertification(
                musical_class=name,
                status="present" if name == "constant_tempo" else "absent",
                provenance="human classification",
            )
            for name in MUSICAL_CLASSES
        ),
        regions=(
            Region(
                start_seconds=0.0,
                end_seconds=16.0,
                kind="metrical",
                beat_unit_quarters=1.0,
                provenance="complete listening",
            ),
        ),
        beats=tuple(
            BeatLabel(
                seconds=index * 0.5,
                uncertainty_ms=5.0,
                count=index,
                quarter=float(index),
                bar_id=index // 4,
                quarter_in_bar=float(index % 4),
            )
            for index in range(32)
        ),
        bars=tuple(
            BarLabel(
                bar_id=index,
                seconds=index * 2.0,
                uncertainty_ms=5.0,
                start_quarter=index * 4.0,
                meter_numerator=4,
                meter_denominator=4,
            )
            for index in range(8)
        ),
        gap_transitions=(),
        critical_features=(
            CriticalFeature(
                feature="first_unambiguous",
                bar_ids=(0,),
                absent_reason=None,
                provenance="first independently certain bar",
            ),
            CriticalFeature(
                feature="post_break",
                bar_ids=(),
                absent_reason="no break heard",
                provenance="full-span listening",
            ),
            CriticalFeature(
                feature="late_track",
                bar_ids=(7,),
                absent_reason=None,
                provenance="independently selected late bar",
            ),
        ),
    )


def _first(bundle: ReferenceBundle, **updates: object) -> ReferenceBundle:
    track = bundle.tracks[0].model_copy(update=updates)
    return bundle.model_copy(update={"tracks": (track, *bundle.tracks[1:])})


def _validate(prepared: Prepared, bundle: ReferenceBundle) -> None:
    parsed = TypeAdapter(ReferenceBundle).validate_json(bundle.model_dump_json(), strict=True)
    validation.validate_reference(prepared.workspace, parsed, prepared.corpus)


def test_blind_seal_retains_pending_gates_and_never_reads_predictions(
    prepared: Prepared, monkeypatch: pytest.MonkeyPatch
) -> None:
    candidate = prepared.workspace / "candidate-predictions.json"
    candidate.write_text("never ground truth")
    actual_open = Path.open

    def guarded_open(
        path: Path,
        mode: str = "r",
        buffering: int = -1,
        encoding: str | None = None,
        errors: str | None = None,
        newline: str | None = None,
    ) -> IO[str] | IO[bytes]:
        if path == candidate:
            pytest.fail("blind sealing opened a candidate prediction")
        return actual_open(path, mode, buffering, encoding, errors, newline)

    monkeypatch.setattr(Path, "open", guarded_open)
    path = prepared.workspace / "reference.json"
    path.write_text(prepared.bundle.model_dump_json())
    seal_path = cli.seal_reference(prepared.workspace, path.name, "reference-seal.json")
    seal = TypeAdapter(ReferenceSeal).validate_json(seal_path.read_bytes(), strict=True)
    assert seal.musical_acceptance == seal.corrections == "pending"
    assert seal.default_adoption == "blocked"
    assert seal.coverage[0].reference_beats == 32
    assert "ramp" in seal.absent_classes
    with pytest.raises(FileExistsError):
        cli.seal_reference(prepared.workspace, path.name, seal_path.name)


def test_draft_never_certifies_labels_or_prediction_blindness(prepared: Prepared) -> None:
    result = cli.draft_reference(prepared.workspace, "identities.json", "draft.json")
    data = json.loads(result.read_bytes())
    assert data["status"] == "draft"
    assert data["tracks"][0]["predictions_seen"] is None
    assert data["tracks"][0]["beats"] == []
    with pytest.raises(ValidationError):
        TypeAdapter(ReferenceBundle).validate_json(result.read_bytes(), strict=True)


@pytest.mark.parametrize(
    ("updates", "reason"),
    [
        ({"predictions_seen": True}, "blind_independent"),
        ({"independent_listening": False}, "blind_independent"),
        ({"extent_end_seconds": 15.0}, "complete_loaded_extent"),
        ({"recording_groups": ("group-T03",)}, "contaminates"),
    ],
)
def test_independence_extent_and_split_cannot_be_assumed(
    prepared: Prepared, updates: dict[str, object], reason: str
) -> None:
    with pytest.raises(ValueError, match=reason):
        _validate(prepared, _first(prepared.bundle, **updates))


def test_source_and_actual_pcm_integrity(prepared: Prepared) -> None:
    pcm = prepared.workspace / "T01.f32le"
    pcm.write_bytes(pcm.read_bytes()[:-4])
    with pytest.raises(ValueError, match="complete_loaded_pcm_size"):
        _validate(prepared, prepared.bundle)
    pcm.write_bytes(struct.pack("<128f", *([0.5] * 128)))
    with pytest.raises(ValueError, match="loaded_pcm_hash"):
        _validate(prepared, prepared.bundle)
    pcm.write_bytes(struct.pack("<128f", *([math_nan()] * 128)))
    with pytest.raises(ValueError, match="finite_complete"):
        _validate(prepared, prepared.bundle)


def math_nan() -> float:
    return float("nan")


def test_arbitrary_self_hashed_pcm_is_not_native_evidence(prepared: Prepared) -> None:
    pcm = prepared.workspace / "T01.f32le"
    raw = bytes(128 * 4)
    pcm.write_bytes(raw)
    identity = prepared.bundle.tracks[0].identity
    changed = identity.model_copy(
        update={"pcm": identity.pcm.model_copy(update={"sha256": _digest(raw)})}
    )
    with pytest.raises(ValueError, match="not_bound_to_native_loaded"):
        _validate(prepared, _first(prepared.bundle, identity=changed))


@pytest.mark.parametrize(
    ("field", "value"), [("sample_rate_hz", 16), ("frame_count", 256), ("sha256", "f" * 64)]
)
def test_loaded_rate_extent_and_hash_must_match_approved_inventory(
    prepared: Prepared, field: str, value: object
) -> None:
    identity = prepared.bundle.tracks[0].identity
    changed = identity.model_copy(update={"pcm": identity.pcm.model_copy(update={field: value})})
    with pytest.raises(ValueError, match="not_bound_to_native_loaded"):
        _validate(prepared, _first(prepared.bundle, identity=changed))


def test_original_source_bytes_remain_frozen(prepared: Prepared) -> None:
    (prepared.workspace / "T01.source").write_bytes(b"replacement original")
    with pytest.raises(ValueError, match="original_source_hash_or_size"):
        _validate(prepared, prepared.bundle)


def test_frozen_protocol_and_private_inventory_cannot_be_revised(prepared: Prepared) -> None:
    (prepared.workspace / "identities.json").write_bytes(b"{}")
    with pytest.raises(ValueError, match="inventory_hash"):
        _validate(prepared, prepared.bundle)
    (prepared.workspace / "scratch/b2a/scoring-addendum.json").write_bytes(b"{}")
    with pytest.raises(ValueError, match="frozen_protocol_hash"):
        validation.frozen_corpus(prepared.workspace)


@pytest.mark.parametrize(("field", "value"), [("schema_version", True), ("schema_version", "1")])
def test_strict_integer_fields_reject_bool_and_string(
    prepared: Prepared, field: str, value: object
) -> None:
    data = prepared.bundle.model_dump(mode="json")
    data[field] = value
    with pytest.raises(ValidationError):
        TypeAdapter(ReferenceBundle).validate_json(json.dumps(data), strict=True)


@pytest.mark.parametrize(
    ("mutation", "reason"),
    [
        (
            lambda t: {
                "bars": (t.bars[0].model_copy(update={"meter_denominator": 3}), *t.bars[1:])
            },
            "invalid_meter",
        ),
        (
            lambda t: {
                "beats": (t.beats[0], t.beats[1].model_copy(update={"count": 9}), *t.beats[2:])
            },
            "declared_gap",
        ),
        (
            lambda t: {
                "beats": (
                    *t.beats[:3],
                    t.beats[3].model_copy(update={"seconds": 2.1}),
                    *t.beats[4:],
                )
            },
            "outside_its_bar",
        ),
        (
            lambda t: {
                "bars": tuple(bar.model_copy(update={"uncertainty_ms": 11.0}) for bar in t.bars)
            },
            "eligible",
        ),
        (
            lambda t: {
                "beats": tuple(beat.model_copy(update={"uncertainty_ms": 11.0}) for beat in t.beats)
            },
            "eligible",
        ),
        (
            lambda t: {"regions": (t.regions[0].model_copy(update={"start_seconds": 0.1}),)},
            "contiguous",
        ),
        (
            lambda t: {
                "regions": (
                    Region(
                        start_seconds=0.0,
                        end_seconds=2.0,
                        kind="ambiguous",
                        beat_unit_quarters=None,
                        provenance="uncertain",
                    ),
                    Region(
                        start_seconds=2.0,
                        end_seconds=16.0,
                        kind="metrical",
                        beat_unit_quarters=1.0,
                        provenance="confident",
                    ),
                )
            },
            "confident",
        ),
    ],
)
def test_count_meter_coverage_and_uncertainty_checks(
    prepared: Prepared, mutation: Callable[[ReferenceTrack], dict[str, object]], reason: str
) -> None:
    with pytest.raises(ValueError, match=reason):
        _validate(prepared, _first(prepared.bundle, **mutation(prepared.bundle.tracks[0])))


def test_compound_meter_and_negative_quarter_origin_are_valid(prepared: Prepared) -> None:
    track = prepared.bundle.tracks[0]
    bars = tuple(
        BarLabel(
            bar_id=i,
            seconds=i * 2.0,
            uncertainty_ms=5.0,
            start_quarter=i * 3.0 - 3.0,
            meter_numerator=6,
            meter_denominator=8,
        )
        for i in range(8)
    )
    beats = tuple(
        BeatLabel(
            seconds=i * 1.0,
            uncertainty_ms=5.0,
            count=i - 2,
            quarter=i * 1.5 - 3.0,
            bar_id=i // 2,
            quarter_in_bar=(i % 2) * 1.5,
        )
        for i in range(16)
    )
    regions = (track.regions[0].model_copy(update={"beat_unit_quarters": 1.5}),)
    _validate(prepared, _first(prepared.bundle, bars=bars, beats=beats, regions=regions))


def test_odd_meter_eighth_pulses_keep_continuous_quarter_interpretation(prepared: Prepared) -> None:
    track = prepared.bundle.tracks[0]
    bars = tuple(
        BarLabel(
            bar_id=i,
            seconds=i * 1.5,
            uncertainty_ms=5.0,
            start_quarter=i * 1.5,
            meter_numerator=3,
            meter_denominator=8,
        )
        for i in range(11)
    )
    beats = tuple(
        BeatLabel(
            seconds=i * 0.5,
            uncertainty_ms=5.0,
            count=i,
            quarter=i * 0.5,
            bar_id=i // 3,
            quarter_in_bar=(i % 3) * 0.5,
        )
        for i in range(32)
    )
    regions = (track.regions[0].model_copy(update={"beat_unit_quarters": 0.5}),)
    critical = tuple(
        feature.model_copy(update={"bar_ids": (10,)})
        if feature.feature == "late_track"
        else feature
        for feature in track.critical_features
    )
    _validate(
        prepared,
        _first(
            prepared.bundle, bars=bars, beats=beats, regions=regions, critical_features=critical
        ),
    )


def test_declared_break_preserves_count_bar_and_reentry_identity(prepared: Prepared) -> None:
    track = prepared.bundle.tracks[0]
    regions = (
        Region(
            start_seconds=0.0,
            end_seconds=3.0,
            kind="metrical",
            beat_unit_quarters=1.0,
            provenance="listened",
        ),
        Region(
            start_seconds=3.0,
            end_seconds=4.0,
            kind="ambiguous",
            beat_unit_quarters=None,
            provenance="predeclared break",
        ),
        Region(
            start_seconds=4.0,
            end_seconds=16.0,
            kind="metrical",
            beat_unit_quarters=1.0,
            provenance="reentry listened",
        ),
    )
    beats = tuple(beat for beat in track.beats if not 3 <= beat.seconds < 4)
    gap = GapTransition(
        before_count=5,
        after_count=8,
        before_bar_id=1,
        after_bar_id=2,
        quarter_advance=3.0,
        provenance="human certified continuing count",
    )
    critical = tuple(
        feature.model_copy(update={"bar_ids": (2,), "absent_reason": None})
        if feature.feature == "post_break"
        else feature
        for feature in track.critical_features
    )
    changed = _first(
        prepared.bundle,
        regions=regions,
        beats=beats,
        gap_transitions=(gap,),
        critical_features=critical,
    )
    _validate(prepared, changed)
    with pytest.raises(ValueError, match="declared_gap"):
        _validate(prepared, _first(changed, gap_transitions=()))


def test_independently_selected_metrical_reentry_can_be_critical(prepared: Prepared) -> None:
    track = prepared.bundle.tracks[0]
    critical = tuple(
        feature.model_copy(update={"bar_ids": (3,), "absent_reason": None})
        if feature.feature == "post_break"
        else feature
        for feature in track.critical_features
    )
    _validate(prepared, _first(prepared.bundle, critical_features=critical))


def test_json_duplicates_extra_fields_and_size_are_rejected(
    prepared: Prepared, monkeypatch: pytest.MonkeyPatch
) -> None:
    path = prepared.workspace / "bad.json"
    path.write_bytes(b'{"status":"draft", "status":"ready_for_seal"}')
    with pytest.raises(ValueError, match="duplicate_json_key"):
        validation.read_json_bytes(path)
    data = prepared.bundle.model_dump(mode="json")
    data["predictions"] = []
    with pytest.raises(ValidationError, match="extra_forbidden"):
        TypeAdapter(ReferenceBundle).validate_json(json.dumps(data), strict=True)
    monkeypatch.setattr(validation, "MAX_JSON_BYTES", 10)
    path.write_bytes(bytes(11))
    with pytest.raises(ValueError, match="json_size_limit"):
        validation.read_json_bytes(path)


def test_private_outputs_remain_inside_workspace_outside_repo(prepared: Prepared) -> None:
    for path in ("../outside.json", "repo/private.json"):
        with pytest.raises(ValueError, match="private_path"):
            validation.write_output(prepared.workspace, path, b"{}")


def _paired(prepared: Prepared) -> tuple[CorrectionOrder, CorrectionBundle, datetime]:
    timestamp = datetime.now(UTC) - timedelta(days=1)
    slots = tuple(
        SessionSlot(session_id=f"{track}-{position}", track_id=track, backend=backend)
        for index, track in enumerate(HELD_OUT)
        for position, backend in enumerate(
            BACKENDS if index % 2 == 0 else tuple(reversed(BACKENDS))
        )
    )
    order = CorrectionOrder(
        schema_version=1,
        status="ready_for_seal",
        reference_seal_sha256="a" * 64,
        annotator="same human",
        tool="same editor",
        workflow="same procedure",
        endpoint="complete corrected reference endpoint",
        provenance="human pre-session confirmation",
        slots=slots,
    )
    tracks = {track.identity.track_id: track for track in prepared.bundle.tracks}
    implementation = prepared.workspace / "repo" / "synthetic-backend.pyd"
    implementation.parent.mkdir(exist_ok=True)
    implementation.write_bytes(b"synthetic producer implementation")
    sessions = []
    for index, slot in enumerate(slots):
        initial = prepared.workspace / f"{slot.session_id}-candidate.json"
        final = prepared.workspace / f"{slot.session_id}-corrected.json"
        initial.write_bytes(f"actual candidate {slot.session_id}".encode())
        final.write_bytes(f"actual corrected {slot.session_id}".encode())
        start = timestamp + timedelta(minutes=index + 1)
        end = start + timedelta(seconds=30)
        track = tracks[slot.track_id]
        phase = CorrectionPhase(
            phase_id="edit-and-verify",
            input_sha256=validation.sha256_file(initial),
            input_path=initial.name,
            output_sha256=validation.sha256_file(final),
            output_path=final.name,
            start_utc=start,
            end_utc=end,
            operations=Operations(inserts=1, deletes=0, moves=0, count=0, meter=0, phase=0, gap=0),
            active_intervals=(
                ActiveInterval(
                    start_utc=start,
                    end_utc=start + timedelta(seconds=10),
                    provenance="human stopwatch",
                ),
            ),
        )
        sessions.append(
            CorrectionSession(
                **slot.model_dump(),
                producer=ProducerIdentity(
                    implementation_path="repo/synthetic-backend.pyd",
                    implementation_sha256=validation.sha256_file(implementation),
                    configuration="beat_this_1.1.0_final0_minimal_cpu_fp32"
                    if slot.backend == BACKENDS[0]
                    else "corrected_legacy_qm",
                    model_sha256=validation.MODEL_SHA256 if slot.backend == BACKENDS[0] else None,
                    legacy_units_fixed=slot.backend == BACKENDS[1],
                    provenance="actual verified implementation bytes; human producer attestation",
                ),
                source_sha256=track.identity.source_sha256,
                reference_revision=track.revision,
                reference_seal_sha256="a" * 64,
                annotator=order.annotator,
                tool=order.tool,
                workflow=order.workflow,
                endpoint=order.endpoint,
                human_measured=True,
                provenance="actual paired human session",
                zero_active_time_reason=None,
                initial_prediction_sha256=phase.input_sha256,
                initial_prediction_path=initial.name,
                corrected_result_sha256=phase.output_sha256,
                corrected_result_path=final.name,
                start_utc=start,
                end_utc=end,
                phases=(phase,),
                critical_outcomes=tuple(
                    CriticalOutcome(
                        bar_id=i,
                        bar_identity_correct=True,
                        timing_error_ms=0.0,
                        provenance="human independently checked",
                    )
                    for i in (0, 7)
                ),
            )
        )
    bundle = CorrectionBundle(
        schema_version=1,
        status="ready_for_validation",
        reference_seal_sha256="a" * 64,
        order_seal_sha256="b" * 64,
        sessions=tuple(sessions),
    )
    return order, bundle, timestamp


def _validate_pairs(
    prepared: Prepared, order: CorrectionOrder, bundle: CorrectionBundle, timestamp: datetime
) -> tuple[tuple[str, int, float], ...]:
    parsed = TypeAdapter(CorrectionBundle).validate_json(bundle.model_dump_json(), strict=True)
    return validation.validate_corrections(
        prepared.workspace, parsed, prepared.bundle, order, ("a" * 64, "b" * 64), timestamp
    )


def test_actual_pairs_and_true_zero_baseline_are_representable(prepared: Prepared) -> None:
    order, bundle, timestamp = _paired(prepared)
    validation.validate_order(order, "a" * 64)
    assert _validate_pairs(prepared, order, bundle, timestamp)[0][1:] == (1, 10.0)
    session = bundle.sessions[1]
    phase = session.phases[0].model_copy(
        update={
            "active_intervals": (),
            "operations": Operations(
                inserts=0, deletes=0, moves=0, count=0, meter=0, phase=0, gap=0
            ),
        }
    )
    zero = session.model_copy(
        update={"phases": (phase,), "zero_active_time_reason": "human measured no active work"}
    )
    changed = bundle.model_copy(
        update={"sessions": (bundle.sessions[0], zero, *bundle.sessions[2:])}
    )
    assert _validate_pairs(prepared, order, changed, timestamp)[1][1:] == (0, 0.0)


@pytest.mark.parametrize(
    ("updates", "reason"),
    [
        ({"annotator": "different human"}, "paired_workflow"),
        ({"tool": "different editor"}, "paired_workflow"),
        ({"reference_revision": "wrong revision"}, "session_source_revision"),
        ({"source_sha256": "c" * 64}, "session_source_revision"),
        ({"human_measured": False}, "actual_human_measurement"),
        ({"initial_prediction_sha256": "c" * 64}, "artifact_size_or_hash"),
        ({"start_utc": datetime(2020, 1, 1, tzinfo=UTC).replace(tzinfo=None)}, "aware_UTC"),
        ({"start_utc": datetime(2099, 1, 1, tzinfo=UTC)}, "future_human"),
        ({"critical_outcomes": ()}, "complete_critical"),
    ],
)
def test_fabricated_or_mismatched_session_inputs_rejected(
    prepared: Prepared, updates: dict[str, object], reason: str
) -> None:
    order, bundle, timestamp = _paired(prepared)
    session = bundle.sessions[0].model_copy(update=updates)
    changed = bundle.model_copy(update={"sessions": (session, *bundle.sessions[1:])})
    with pytest.raises(ValueError, match=reason):
        _validate_pairs(prepared, order, changed, timestamp)


def test_frozen_order_actual_order_and_phase_time_must_agree(prepared: Prepared) -> None:
    order, bundle, timestamp = _paired(prepared)
    unbalanced = order.model_copy(
        update={
            "slots": tuple(
                SessionSlot(session_id=f"{track}-{i}", track_id=track, backend=backend)
                for track in HELD_OUT
                for i, backend in enumerate(BACKENDS)
            )
        }
    )
    with pytest.raises(ValueError, match="balanced"):
        validation.validate_order(unbalanced, "a" * 64)
    swapped = bundle.model_copy(
        update={"sessions": (bundle.sessions[1], bundle.sessions[0], *bundle.sessions[2:])}
    )
    with pytest.raises(ValueError, match="actual_session_order"):
        _validate_pairs(prepared, order, swapped, timestamp)
    session = bundle.sessions[0]
    interval = session.phases[0].active_intervals[0]
    phase = session.phases[0].model_copy(update={"active_intervals": (interval, interval)})
    changed = bundle.model_copy(
        update={"sessions": (session.model_copy(update={"phases": (phase,)}), *bundle.sessions[1:])}
    )
    with pytest.raises(ValueError, match="intervals_overlap"):
        _validate_pairs(prepared, order, changed, timestamp)
    with pytest.raises(ValueError, match="outside_phase_session_or_seal"):
        _validate_pairs(prepared, order, bundle, timestamp + timedelta(minutes=2))


def test_zero_time_cannot_hide_real_edits_and_artifact_phase_chain_is_checked(
    prepared: Prepared,
) -> None:
    order, bundle, timestamp = _paired(prepared)
    session = bundle.sessions[0]
    phase = session.phases[0].model_copy(update={"active_intervals": ()})
    changed = session.model_copy(
        update={"phases": (phase,), "zero_active_time_reason": "declared zero time"}
    )
    with pytest.raises(ValueError, match="phase_operations_require_positive_active_time"):
        _validate_pairs(
            prepared,
            order,
            bundle.model_copy(update={"sessions": (changed, *bundle.sessions[1:])}),
            timestamp,
        )
    phase = session.phases[0].model_copy(update={"input_sha256": "f" * 64})
    changed = session.model_copy(update={"phases": (phase,)})
    with pytest.raises(ValueError, match="input_output_chain"):
        _validate_pairs(
            prepared,
            order,
            bundle.model_copy(update={"sessions": (changed, *bundle.sessions[1:])}),
            timestamp,
        )


def test_other_phase_time_cannot_cover_unmeasured_edits(prepared: Prepared) -> None:
    order, bundle, timestamp = _paired(prepared)
    session = bundle.sessions[0]
    edit = session.phases[0].model_copy(
        update={
            "end_utc": session.start_utc + timedelta(seconds=15),
            "active_intervals": (),
        }
    )
    no_operations = Operations(inserts=0, deletes=0, moves=0, count=0, meter=0, phase=0, gap=0)
    verification = edit.model_copy(
        update={
            "phase_id": "verify-endpoint",
            "input_path": session.corrected_result_path,
            "input_sha256": session.corrected_result_sha256,
            "start_utc": edit.end_utc,
            "end_utc": session.end_utc,
            "operations": no_operations,
            "active_intervals": (
                ActiveInterval(
                    start_utc=edit.end_utc,
                    end_utc=edit.end_utc + timedelta(seconds=10),
                    provenance="human measured endpoint verification",
                ),
            ),
        }
    )
    changed = session.model_copy(update={"phases": (edit, verification)})
    with pytest.raises(ValueError, match="phase_operations_require_positive_active_time"):
        _validate_pairs(
            prepared,
            order,
            bundle.model_copy(update={"sessions": (changed, *bundle.sessions[1:])}),
            timestamp,
        )
    idle = edit.model_copy(
        update={
            "operations": no_operations,
            "output_path": session.initial_prediction_path,
            "output_sha256": session.initial_prediction_sha256,
        }
    )
    measured_edit = verification.model_copy(
        update={
            "input_path": session.initial_prediction_path,
            "input_sha256": session.initial_prediction_sha256,
            "operations": edit.operations,
        }
    )
    changed = session.model_copy(update={"phases": (idle, measured_edit)})
    assert _validate_pairs(
        prepared,
        order,
        bundle.model_copy(update={"sessions": (changed, *bundle.sessions[1:])}),
        timestamp,
    )[0][1:] == (1, 10.0)


def test_producer_requires_actual_implementation_and_repaired_legacy_attestation(
    prepared: Prepared,
) -> None:
    order, bundle, timestamp = _paired(prepared)
    session = bundle.sessions[0]
    producer = session.producer.model_copy(update={"implementation_sha256": "f" * 64})
    changed = session.model_copy(update={"producer": producer})
    with pytest.raises(ValueError, match="producer_implementation_size_or_hash"):
        _validate_pairs(
            prepared,
            order,
            bundle.model_copy(update={"sessions": (changed, *bundle.sessions[1:])}),
            timestamp,
        )
    session = bundle.sessions[1]
    producer = session.producer.model_copy(update={"legacy_units_fixed": False})
    changed = session.model_copy(update={"producer": producer})
    with pytest.raises(ValueError, match="repaired_legacy_producer_attestation"):
        _validate_pairs(
            prepared,
            order,
            bundle.model_copy(
                update={"sessions": (bundle.sessions[0], changed, *bundle.sessions[2:])}
            ),
            timestamp,
        )


def test_failed_critical_outcome_is_valid_measurement_not_acceptance(prepared: Prepared) -> None:
    order, bundle, timestamp = _paired(prepared)
    session = bundle.sessions[0]
    outcomes = tuple(
        outcome.model_copy(update={"bar_identity_correct": False, "timing_error_ms": 80.0})
        for outcome in session.critical_outcomes
    )
    changed = session.model_copy(update={"critical_outcomes": outcomes})
    assert _validate_pairs(
        prepared,
        order,
        bundle.model_copy(update={"sessions": (changed, *bundle.sessions[1:])}),
        timestamp,
    )


def test_changed_reference_and_forged_pre_reference_order_are_rejected(prepared: Prepared) -> None:
    bundle_path = prepared.workspace / "reference.json"
    bundle_path.write_text(prepared.bundle.model_dump_json())
    seal_path = cli.seal_reference(prepared.workspace, bundle_path.name, "reference-seal.json")
    seal, _, reference_hash = validation.reference_seal(prepared.workspace, seal_path.name)
    order, _, _ = _paired(prepared)
    order = order.model_copy(update={"reference_seal_sha256": reference_hash})
    order_path = prepared.workspace / "order.json"
    order_path.write_text(order.model_dump_json())
    forged = OrderSeal(
        schema_version=1,
        status="sealed_correction_order",
        sealed_at_utc=seal.sealed_at_utc - timedelta(seconds=1),
        order_path=order_path.name,
        order_sha256=validation.sha256_file(order_path),
        reference_seal_sha256=reference_hash,
        musical_acceptance="pending",
    )
    (prepared.workspace / "order-seal.json").write_text(forged.model_dump_json())
    with pytest.raises(ValueError, match="order_must_follow_reference_seal"):
        cli.draft_corrections(
            prepared.workspace, seal_path.name, "order-seal.json", "bad-draft.json"
        )
    with pytest.raises(ValueError, match="order_must_follow_reference_seal"):
        cli.validate_measured_corrections(
            prepared.workspace,
            "missing-corrections.json",
            seal_path.name,
            "order-seal.json",
            "bad-receipt.json",
        )
    bundle_path.write_text(bundle_path.read_text() + " ")
    with pytest.raises(ValueError, match="sealed_reference_bundle_changed"):
        validation.reference_seal(prepared.workspace, seal_path.name)


def test_pure_temporal_projection_preserves_critical_timing_without_certifying_bar_identity(
    prepared: Prepared,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    track = prepared.bundle.tracks[0]
    predictions = BeatPredictions(
        beat_seconds=tuple(beat.seconds for beat in track.beats),
        downbeat_seconds=tuple(bar.seconds for bar in track.bars),
        beat_logits=(0.0, -1.0),
        downbeat_logits=(1.0, 0.0),
    )
    monkeypatch.setattr(
        Path, "open", lambda *_args, **_kwargs: pytest.fail("pure core opens no files")
    )
    report = score_track_timing(track, predictions)
    assert report.beats.tolerances[2].f1 == report.downbeats.tolerances[2].f1 == 1.0
    assert tuple(result.bar_id for result in report.critical_downbeat_timing) == (0, 7)
    assert all(
        result.candidate_bar_identity == "unchecked" for result in report.critical_downbeat_timing
    )
    assert report.input_certification == "unchecked_by_metric_core"
    assert report.quarter_count_and_bar_identity == report.paired_correction_burden == "pending"
    assert report.musical_acceptance == "pending"
    assert report.default_adoption == "blocked"
    assert report.regional_scope == "supplied_regions_only_no_inferred_startup_break_tail"


@pytest.mark.parametrize(
    ("beat_logits", "downbeat_logits"), [((0.0,), ()), ((float("nan"),), (0.0,))]
)
def test_track_temporal_projection_rejects_incomplete_or_nonfinite_logits(
    prepared: Prepared,
    beat_logits: tuple[float, ...],
    downbeat_logits: tuple[float, ...],
) -> None:
    with pytest.raises(ValueError, match=r"logits.*equal|logits.*finite"):
        score_track_timing(
            prepared.bundle.tracks[0], BeatPredictions((), (), beat_logits, downbeat_logits)
        )


def test_track_temporal_projection_rejects_clipped_extent_or_unknown_critical_bar(
    prepared: Prepared,
) -> None:
    track = prepared.bundle.tracks[0]
    predictions = BeatPredictions((), (), (), ())
    with pytest.raises(ValueError, match="complete native"):
        score_track_timing(track.model_copy(update={"extent_end_seconds": 15.0}), predictions)
    feature = track.critical_features[0].model_copy(update={"bar_ids": (99,)})
    with pytest.raises(ValueError, match="supplied reference bar"):
        score_track_timing(track.model_copy(update={"critical_features": (feature,)}), predictions)
