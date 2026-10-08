"""Fail-closed historical candidate lineage without private audio or inference."""

import hashlib
import json
import struct
from dataclasses import asdict, dataclass, replace
from typing import TYPE_CHECKING

import pytest
from pydantic import TypeAdapter, ValidationError

from flitzis_looper.analysis import beat_candidates as candidates
from flitzis_looper.analysis.beat_candidate_models import (
    ApprovedArtifact,
    CandidateSelection,
    HistoricalProfile,
)
from flitzis_looper.analysis.contracts import (
    AnalysisIdentity,
    BeatComponentResult,
    BeatPredictions,
    BeatWorkerRequest,
    BeatWorkerResponse,
    MonoPcmInput,
)
from flitzis_looper.analysis.publication import encode_result
from flitzis_looper.analysis.reference_inputs_models import LoadedIdentity, PcmIdentity
from flitzis_looper.analysis.reference_inputs_validation import FrozenCorpus, FrozenTrack
from tests.flitzis_looper.analysis.test_reference_inputs import _reference_track

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.analysis.beat_candidate_models import NativeCandidate
    from flitzis_looper.analysis.reference_inputs_models import ReferenceTrack

_KEY: dict[str, object] = {"status": "ready", "key": "Gm", "provenance": "synthetic-key"}


def digest(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


@dataclass
class Fixture:
    workspace: Path
    directory: Path
    track: ReferenceTrack
    request: BeatWorkerRequest
    predictions: BeatPredictions
    retained: bool
    profile: HistoricalProfile
    monkeypatch: pytest.MonkeyPatch

    def pin(self) -> None:
        """Inject a trusted synthetic profile; production never accepts these updates."""
        self.profile = replace(
            self.profile,
            summary_sha256=digest((self.directory / "summary.json").read_bytes()),
            artifacts=tuple(
                ApprovedArtifact(item.name, digest((self.directory / item.name).read_bytes()))
                for item in self.profile.artifacts
            ),
        )
        self.monkeypatch.setattr(candidates, "_APPROVED_PROFILES", (self.profile,))

    def mutate(self, name: str, data: object) -> None:
        (self.directory / name).write_text(json.dumps(data, allow_nan=False), encoding="utf-8")
        self.pin()

    def load(self) -> NativeCandidate:
        return candidates.load_candidate(self.workspace, self.track, self.profile.profile_id)


def _artifacts(
    request: BeatWorkerRequest,
    predictions: BeatPredictions,
    export: dict[str, object],
    *,
    retained: bool,
) -> dict[str, object]:
    component = BeatComponentResult(request.identity, request.model, "ready", "ready", predictions)
    response = asdict(BeatWorkerResponse(request.identity, request.model, predictions, 1))
    wire = (
        encode_result(component, _KEY)
        if retained
        else json.dumps({
            "schema_version": 1,
            "identity": asdict(request.identity),
            "beat": asdict(component),
            "key": _KEY,
        })
    )
    event = {
        "type": "offline_analysis_completed",
        "id": request.identity.pad_id,
        "request_id": request.identity.request_id,
        "result_json": wire,
    }
    if not retained:
        return {
            "raw-worker-response.json": response,
            "raw-component.json": asdict(component),
            "result-envelope.json": json.loads(wire),
            "loader-events.json": [event],
        }
    native = {
        **asdict(request.identity),
        "sample_rate_hz": request.pcm.sample_rate_hz,
        "frame_count": request.pcm.frame_count,
        "channels": 2,
        "origin_seconds": 0.0,
        "mono_rule": "arithmetic-channel-mean-f64-v1",
    }
    return {
        "worker-request.raw.json": json.loads(TypeAdapter(BeatWorkerRequest).dump_json(request)),
        "worker-response.raw.json": response,
        "beat-component.raw.json": asdict(component),
        "complete-native-export-identity.json": export,
        "native-observations.json": [
            {"stage": stage, "metadata": native}
            for stage in (
                "before_prepare_export",
                "after_prepare_export",
                "before_finish",
                "after_finish",
                "job_done",
            )
        ],
        "result-envelope.raw.json": json.loads(wire),
        "native-finish-input.raw.json": json.loads(wire),
        "completion-event-0.raw.json": json.loads(wire),
        "loader-events.json": [event],
    }


@pytest.fixture(params=[False, True], ids=["historical-summary", "retained-request"])
def prepared(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, request: pytest.FixtureRequest
) -> Fixture:
    retained = bool(request.param)
    directory = tmp_path / "scratch" / "native"
    directory.mkdir(parents=True)
    pcm = struct.pack("<128f", *([0.25] * 128))
    pcm_path = tmp_path / "complete.f32le"
    pcm_path.write_bytes(pcm)
    source = tmp_path / "T01.source"
    source.write_bytes(b"synthetic source")
    identity = LoadedIdentity(
        track_id="T01",
        source_sha256=digest(source.read_bytes()),
        provenance="synthetic evidence",
        pcm=PcmIdentity(
            path=pcm_path.name,
            sha256=digest(pcm),
            sample_rate_hz=8,
            frame_count=128,
            origin_seconds=0.0,
            dtype="float32-le",
            channels=1,
        ),
    )
    frozen = FrozenTrack(
        id="T01",
        source_relative=source.name,
        source_sha256=identity.source_sha256,
        source_bytes=source.stat().st_size,
        split="development",
    )
    monkeypatch.setattr(
        candidates, "frozen_corpus", lambda _workspace: FrozenCorpus(tracks=(frozen,))
    )
    worker_request = BeatWorkerRequest(
        AnalysisIdentity(0, 2, "synthetic-loaded-0-1", 1),
        MonoPcmInput(directory / "retired" / "mono.f32le", 8, 128),
        candidates._FROZEN_MODEL,
    )
    count = candidates.expected_complete_logit_count(128, 8)
    predictions = BeatPredictions(
        (0.0, 0.5, 1.0), (0.0,), (-0.0,) + (1.5,) * (count - 1), (0.5,) * count
    )
    export = {
        "path": str(worker_request.pcm.path),
        "sha256": digest(pcm),
        "bytes": len(pcm),
        "sample_rate_hz": 8,
        "frame_count": 128,
        "origin_seconds": 0.0,
        "dtype": "float32-le",
        "channels": 1,
        "first_frame": 0.25,
        "last_frame": 0.25,
    }
    summary = {
        "mode": "ready",
        "source": str(source),
        "source_sha256": identity.source_sha256,
        "source_bytes": source.stat().st_size,
        "native_extension_sha256": "a" * 64,
        "loaded_shape_rate_channels_frames": [8, 2, 128],
        "loaded_duration_seconds": 16.0,
        "model": asdict(worker_request.model),
        "export": export,
        "beat": asdict(
            BeatComponentResult(worker_request.identity, worker_request.model, "ready", "ready")
        ),
        "key": _KEY,
        "completion_event_count": 1,
        "worker_retired": True,
        "worker_exit_codes": [0],
        "remaining_pcm_directories": [],
        "remaining_request_directories": [],
        "native_reservation_after_done": {
            **asdict(worker_request.identity),
            "request_id": 3,
            "sample_rate_hz": 8,
            "frame_count": 128,
            "channels": 2,
            "origin_seconds": 0.0,
            "mono_rule": "arithmetic-channel-mean-f64-v1",
        },
        "expected_full_frontend_frames": count,
    }
    artifacts = _artifacts(worker_request, predictions, export, retained=retained)
    for name, data in {"summary.json": summary, **artifacts}.items():
        (directory / name).write_text(json.dumps(data, allow_nan=False), encoding="utf-8")
    profile = HistoricalProfile(
        "trusted-fixture",
        "T01",
        "scratch/native",
        digest(b"pending"),
        tuple(ApprovedArtifact(name, digest(b"pending")) for name in artifacts),
        retained_request=retained,
    )
    fixture = Fixture(
        tmp_path,
        directory,
        _reference_track(identity),
        worker_request,
        predictions,
        retained,
        profile,
        monkeypatch,
    )
    fixture.pin()
    return fixture


def test_complete_lineage_preserves_original_full_arrays_and_retired_path(
    prepared: Fixture,
) -> None:
    candidate = prepared.load()
    assert candidates._predictions_identity(
        candidate.predictions
    ) == candidates._predictions_identity(prepared.predictions)
    assert candidate.request.pcm.path == prepared.request.pcm.path
    assert not candidate.request.pcm.path.exists()
    report = candidate.report()
    assert json.loads(json.dumps(report))["predictions"] == asdict(prepared.predictions) | {
        name: list(values) for name, values in asdict(prepared.predictions).items()
    }
    assert len(candidate.artifact_bindings) == len(prepared.profile.artifacts) + 2
    assert candidate.lineage_kind == (
        "retained_request" if prepared.retained else "verified_historical_summary"
    )
    assert struct.pack("<d", candidate.predictions.beat_logits[0]) == struct.pack("<d", -0.0)


def test_caller_cannot_submit_unapproved_self_hashed_lineage(prepared: Fixture) -> None:
    with pytest.raises(ValueError, match="unsupported_candidate_lineage"):
        candidates.load_candidate(prepared.workspace, prepared.track, "self-hashed-native-receipt")


def test_profile_cannot_be_rebound_to_another_reference_track(prepared: Fixture) -> None:
    identity = prepared.track.identity.model_copy(update={"track_id": "T02"})
    with pytest.raises(ValueError, match="candidate_profile_track_mismatch"):
        candidates.load_candidate(
            prepared.workspace,
            prepared.track.model_copy(update={"identity": identity}),
            prepared.profile.profile_id,
        )


@pytest.mark.parametrize(
    "field", ["summary.json", "raw", "component", "envelope", "loader-events.json"]
)
def test_any_original_retained_byte_change_is_rejected(prepared: Fixture, field: str) -> None:
    names = {
        "raw": "worker-response.raw.json" if prepared.retained else "raw-worker-response.json",
        "component": "beat-component.raw.json" if prepared.retained else "raw-component.json",
        "envelope": "result-envelope.raw.json" if prepared.retained else "result-envelope.json",
    }
    path = prepared.directory / names.get(field, field)
    path.write_bytes(path.read_bytes() + b" ")
    with pytest.raises(ValueError, match="approved_candidate_artifact_changed"):
        prepared.load()


@pytest.mark.parametrize(
    ("keys", "value"),
    [
        (("source",), "other.source"),
        (("source_sha256",), "b" * 64),
        (("export", "sha256"), "b" * 64),
        (("export", "frame_count"), 127),
        (("model", "environment_id"), "wrong"),
        (("native_reservation_after_done", "source_generation"), 2),
        (("native_reservation_after_done", "request_id"), 4),
        (("loaded_duration_seconds",), 15.0),
        (("expected_full_frontend_frames",), 800),
        (("remaining_pcm_directories",), ["still-owned"]),
        (("worker_exit_codes",), [259]),
        (("prediction_counts",), {"beat_seconds": 1}),
    ],
)
def test_semantic_summary_bindings_are_required_even_in_trusted_fixture(
    prepared: Fixture,
    keys: tuple[str, ...],
    value: object,
) -> None:
    summary = json.loads((prepared.directory / "summary.json").read_bytes())
    target = summary
    for key in keys[:-1]:
        target = target[key]
    target[keys[-1]] = value
    prepared.mutate("summary.json", summary)
    with pytest.raises((ValueError, ValidationError)):
        prepared.load()


def test_current_complete_materialized_pcm_is_rehashed(prepared: Fixture) -> None:
    pcm = prepared.workspace / prepared.track.identity.pcm.path
    pcm.write_bytes(struct.pack("<128f", *([0.5] * 128)))
    with pytest.raises(ValueError, match="candidate_actual_reference_pcm_changed"):
        prepared.load()


def test_partial_equal_length_logits_cannot_satisfy_full_native_extent(prepared: Fixture) -> None:
    name = "worker-response.raw.json" if prepared.retained else "raw-worker-response.json"
    raw = json.loads((prepared.directory / name).read_bytes())
    raw["predictions"]["beat_logits"].pop()
    raw["predictions"]["downbeat_logits"].pop()
    prepared.mutate(name, raw)
    with pytest.raises(ValueError, match="candidate_partial_full_track_logits"):
        prepared.load()


def test_signed_zero_difference_is_a_raw_lineage_mismatch(prepared: Fixture) -> None:
    name = "beat-component.raw.json" if prepared.retained else "raw-component.json"
    component = json.loads((prepared.directory / name).read_bytes())
    component["predictions"]["beat_logits"][0] = 0.0
    prepared.mutate(name, component)
    with pytest.raises(ValueError, match="candidate_full_raw_prediction_parity_mismatch"):
        prepared.load()


def test_duplicate_worker_key_is_rejected_before_strict_reader(prepared: Fixture) -> None:
    name = "worker-response.raw.json" if prepared.retained else "raw-worker-response.json"
    path = prepared.directory / name
    raw = path.read_bytes().replace(
        b'"schema_version": 1', b'"schema_version": 1, "schema_version": 1'
    )
    path.write_bytes(raw)
    prepared.pin()
    with pytest.raises(ValueError, match="duplicate_json_key"):
        prepared.load()


@pytest.mark.parametrize("duplicate", [False, True])
def test_nested_completion_wire_rejects_duplicate_keys_or_lossy_arrays(
    prepared: Fixture, *, duplicate: bool
) -> None:
    events = json.loads((prepared.directory / "loader-events.json").read_bytes())
    wire = json.loads(events[0]["result_json"])
    if duplicate:
        text = json.dumps(wire).replace(
            '"schema_version": ', '"schema_version": 1, "schema_version": ', 1
        )
    else:
        wire["schema_version"] = 1
        wire["beat"]["predictions"] = asdict(
            replace(prepared.predictions, beat_logits=(0.0, *prepared.predictions.beat_logits[1:]))
        )
        text = json.dumps(wire)
    events[0]["result_json"] = text
    prepared.mutate("loader-events.json", events)
    with pytest.raises(
        ValueError, match=r"duplicate_json_key|candidate_full_raw_prediction_parity_mismatch"
    ):
        prepared.load()


def test_duplicate_complete_publications_are_rejected(prepared: Fixture) -> None:
    events = json.loads((prepared.directory / "loader-events.json").read_bytes())
    prepared.mutate("loader-events.json", events * 2)
    with pytest.raises(ValueError, match="candidate_single_complete_publication_required"):
        prepared.load()


@pytest.mark.parametrize(
    "name",
    [
        "worker-request.raw.json",
        "complete-native-export-identity.json",
        "native-finish-input.raw.json",
        "native-observations.json",
    ],
)
@pytest.mark.parametrize("prepared", [True], indirect=True)
def test_retained_native_artifacts_cannot_diverge(prepared: Fixture, name: str) -> None:
    assert prepared.retained
    raw = json.loads((prepared.directory / name).read_bytes())
    match name:
        case "worker-request.raw.json":
            raw["identity"]["source_generation"] = 2
        case "complete-native-export-identity.json":
            raw["path"] = str(prepared.workspace / "other.f32le")
        case "native-finish-input.raw.json":
            raw["key"]["key"] = "Cm"
        case "native-observations.json":
            raw.pop()
    prepared.mutate(name, raw)
    with pytest.raises(ValueError, match="candidate_"):
        prepared.load()


@pytest.mark.parametrize(
    ("frames", "rate", "expected"),
    [
        (23136549, 96000, 12051),
        (16380564, 96000, 8532),
        (20390557, 96000, 10621),
        (34346214, 96000, 17889),
        (49778939, 96000, 25927),
        (2645, 44100, 4),
        (22050, 22050, 51),
    ],
)
def test_full_frontend_extent_uses_soxr_half_up_centered_stft(
    frames: int, rate: int, expected: int
) -> None:
    assert candidates.expected_complete_logit_count(frames, rate) == expected


@pytest.mark.parametrize(
    ("frames", "rate"), [(True, 22050), (100, 22050), (1000, 0), (1000, 768001), (2**40, 1)]
)
def test_full_frontend_rejects_invalid_short_or_overlimit_extent(frames: int, rate: int) -> None:
    with pytest.raises(ValueError, match=r"candidate|invalid_candidate"):
        candidates.expected_complete_logit_count(frames, rate)


def test_candidate_selection_is_strict_and_contains_no_caller_trust_fields() -> None:
    adapter = TypeAdapter(CandidateSelection)
    assert (
        adapter.validate_json(
            b'{"track_id":"T01","profile_id":"T01-native-retry"}', strict=True
        ).track_id
        == "T01"
    )
    with pytest.raises(ValidationError):
        adapter.validate_json(
            b'{"track_id":"T01","profile_id":"new","sha256":"self-hashed"}', strict=True
        )
