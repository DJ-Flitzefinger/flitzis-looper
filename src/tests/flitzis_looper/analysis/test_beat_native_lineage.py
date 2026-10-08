"""Adversarial fresh provenance tests with independently pinned synthetic anchors."""

import hashlib
import json
import struct
from dataclasses import asdict, dataclass, replace
from typing import TYPE_CHECKING, cast

import pytest
from pydantic import ValidationError

from flitzis_looper.analysis import beat_candidates as historical
from flitzis_looper.analysis import beat_native_lineage as fresh
from flitzis_looper.analysis.beat_candidate_models import (
    ApprovedArtifact,
    FreshNativeProfile,
    HistoricalProfile,
)
from flitzis_looper.analysis.beat_candidate_reader import load_candidate
from flitzis_looper.analysis.contracts import (
    AnalysisIdentity,
    BeatModelIdentity,
    BeatPredictions,
    BeatWorkerRequest,
    MonoPcmInput,
    WorkerLimits,
)
from flitzis_looper.analysis.reference_inputs_models import LoadedIdentity, PcmIdentity
from flitzis_looper.analysis.reference_inputs_validation import FrozenCorpus, FrozenTrack
from tests.flitzis_looper.analysis.test_beat_candidates import _artifacts
from tests.flitzis_looper.analysis.test_reference_inputs import _reference_track

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.analysis.beat_candidate_models import NativeCandidate
    from flitzis_looper.analysis.reference_inputs_models import ReferenceTrack

_STAGES = (
    "admitted",
    "before_prepare_export",
    "after_prepare_export",
    "before_retire_pcm",
    "after_retire_pcm",
    "before_finish",
    "after_finish",
    "job_done",
)


def _digest(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _mapping(value: object) -> dict[str, object]:
    assert isinstance(value, dict)
    return cast("dict[str, object]", value)


def _rows(value: object) -> list[dict[str, object]]:
    assert isinstance(value, list)
    return [_mapping(row) for row in value]


def _read(path: Path) -> dict[str, object]:
    return _mapping(json.loads(path.read_bytes()))


def _write(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, allow_nan=False), encoding="utf-8")


def _binding(workspace: Path, path: Path) -> dict[str, object]:
    raw = path.read_bytes()
    return {
        "path": path.relative_to(workspace).as_posix(),
        "sha256": _digest(raw),
        "bytes": len(raw),
    }


@dataclass
class FreshFixture:
    workspace: Path
    directory: Path
    track: ReferenceTrack
    request: BeatWorkerRequest
    predictions: BeatPredictions
    profile: FreshNativeProfile
    monkeypatch: pytest.MonkeyPatch

    def pin(self) -> None:
        """Replace only synthetic trust anchors, without bypassing semantic validators."""
        lineage = replace(
            self.profile.lineage,
            summary_sha256=_digest((self.directory / "summary.json").read_bytes()),
            artifacts=tuple(
                ApprovedArtifact(item.name, _digest((self.directory / item.name).read_bytes()))
                for item in self.profile.lineage.artifacts
            ),
        )
        self.profile = FreshNativeProfile(
            lineage, _digest((self.directory / "producer-provenance.json").read_bytes())
        )
        self.monkeypatch.setattr(fresh, "_FRESH_PROFILES", (self.profile,))

    def mutate(self, name: str, value: object) -> None:
        _write(self.directory / name, value)
        self.pin()

    def load(self) -> NativeCandidate:
        return load_candidate(self.workspace, self.track, self.profile.lineage.profile_id)

    def provenance(self) -> dict[str, object]:
        return _read(self.directory / "producer-provenance.json")

    def summary(self) -> dict[str, object]:
        return _read(self.directory / "summary.json")

    def observations(self) -> list[dict[str, object]]:
        return _rows(json.loads((self.directory / "native-observations.json").read_bytes()))


@dataclass(frozen=True)
class SyntheticMaterial:
    source: Path
    loaded: Path
    listening: Path
    mono: bytes
    files: dict[str, dict[str, object]]
    frozen: FrozenTrack
    request: BeatWorkerRequest
    predictions: BeatPredictions
    export: dict[str, object]
    metadata: dict[str, object]


def _material(tmp_path: Path, directory: Path) -> SyntheticMaterial:
    source = tmp_path / "test-audio" / "1.mp3"
    source.parent.mkdir()
    source.write_bytes(b"synthetic unchanged original audio")
    mono = struct.pack("<128f", *([0.25] * 128))
    export_path = directory / "complete-native-export.f32le"
    listening_path = tmp_path / "listening.f32le"
    export_path.write_bytes(mono)
    listening_path.write_bytes(mono)
    loaded = directory / "complete-native-loaded.f32le"
    loaded.write_bytes(struct.pack("<256f", *([0.25] * 256)))
    worker_files: dict[str, dict[str, object]] = {}
    for name in (
        "interpreter",
        "script",
        "checkpoint",
        "manifest",
        "pointer",
        "lock",
        "environment",
        "pyproject",
        "native-test.exe",
        "installed.pyd",
        "key-model.onnx",
        "producer.rs",
        "producer.py",
        "cold-manifest.json",
        "probe-config.json",
        "frozen-manifest.json",
    ):
        path = directory / name
        path.write_bytes(f"synthetic independently pinned {name}".encode())
        worker_files[name] = _binding(tmp_path, path)
    (directory / "cached-original.mp3").write_bytes(source.read_bytes())
    worker_files["cached-original.mp3"] = _binding(tmp_path, directory / "cached-original.mp3")
    model = BeatModelIdentity(
        sha256=str(worker_files["checkpoint"]["sha256"]),
        frontend_id=historical._FROZEN_MODEL.frontend_id,
        environment_id=f"uv-lock-sha256:{worker_files['lock']['sha256']}",
    )
    frozen = FrozenTrack(
        id="T01",
        source_relative="test-audio/original.mp3",
        source_sha256=_digest(source.read_bytes()),
        source_bytes=source.stat().st_size,
        split="development",
    )
    request = BeatWorkerRequest(
        AnalysisIdentity(0, 2, "loaded-0-1", 1),
        MonoPcmInput(directory / "retired" / "mono.f32le", 8, 128),
        model,
    )
    count = historical.expected_complete_logit_count(128, 8)
    predictions = BeatPredictions(
        (0.0, 0.5, 1.0),
        (0.0,),
        (-0.0,) + (1.5,) * (count - 1),
        (0.5,) * count,
    )
    export = {
        "path": str(request.pcm.path),
        "sha256": _digest(mono),
        "bytes": len(mono),
        "sample_rate_hz": 8,
        "frame_count": 128,
        "origin_seconds": 0.0,
        "dtype": "float32-le",
        "channels": 1,
        "first_frame": 0.25,
        "last_frame": 0.25,
    }
    metadata = {
        **asdict(request.identity),
        "sample_rate_hz": 8,
        "frame_count": 128,
        "channels": 2,
        "origin_seconds": 0.0,
        "mono_rule": "arithmetic-channel-mean-f64-v1",
        "complete_source_identity": _digest(b"synthetic actual transform"),
        "original_sha256": frozen.source_sha256,
        "complete_playback_sha256": _digest(loaded.read_bytes()),
        "complete_mono_sha256": _digest(mono),
        "source_zero_frame": 0,
    }
    _write(
        directory / "cold-manifest.json",
        {
            "identity": metadata["complete_source_identity"],
            "descriptor": {
                "decoder": {
                    "original": {
                        "sha256": frozen.source_sha256,
                        "bytes": frozen.source_bytes,
                    }
                },
                "playback": {
                    "pcm": {
                        "rate_hz": 8,
                        "channels": 2,
                        "full_frames": 128,
                        "full_bytes": 1024,
                        "interleaved_sha256": metadata["complete_playback_sha256"],
                        "mono_sha256": metadata["complete_mono_sha256"],
                        "source_zero_bits": "0000000000000000",
                        "mono_revision": metadata["mono_rule"],
                    }
                },
            },
        },
    )
    worker_files["cold-manifest.json"] = _binding(tmp_path, directory / "cold-manifest.json")
    return SyntheticMaterial(
        source,
        loaded,
        listening_path,
        mono,
        worker_files,
        frozen,
        request,
        predictions,
        export,
        metadata,
    )


@pytest.fixture
def prepared(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> FreshFixture:
    directory = tmp_path / "scratch" / "fresh-native"
    directory.mkdir(parents=True)
    data = _material(tmp_path, directory)
    # Corpus/model are synthetic independent anchors; all actual validators remain active.
    monkeypatch.setattr(historical, "_FROZEN_MODEL", data.request.model)
    corpus = FrozenCorpus(tracks=(data.frozen,))
    monkeypatch.setattr(fresh, "frozen_corpus", lambda _workspace: corpus)
    monkeypatch.setattr(historical, "frozen_corpus", lambda _workspace: corpus)
    artifacts = _artifacts(data.request, data.predictions, data.export, retained=True)
    artifacts["native-observations.json"] = [
        {
            "stage": stage,
            "metadata": data.metadata,
            **({"finish_accepted": True} if stage == "after_finish" else {}),
        }
        for stage in _STAGES
    ]
    summary = {
        "mode": "ready",
        "source": str(data.source),
        "source_sha256": data.frozen.source_sha256,
        "source_bytes": data.frozen.source_bytes,
        "native_extension_sha256": data.files["installed.pyd"]["sha256"],
        "loaded_shape_rate_channels_frames": [8, 2, 128],
        "loaded_duration_seconds": 16.0,
        "model": asdict(data.request.model),
        "export": data.export,
        "beat": {
            "identity": asdict(data.request.identity),
            "model": asdict(data.request.model),
            "status": "ready",
            "reason": "ready",
            "predictions": None,
            "resources_released": True,
        },
        "key": {"status": "ready", "key": "Gm", "provenance": "synthetic-key"},
        "completion_event_count": 1,
        "worker_retired": True,
        "worker_exit_codes": [0],
        "remaining_pcm_directories": [],
        "remaining_request_directories": [],
        "native_reservation_after_done": data.metadata | {"request_id": 3},
        "expected_full_frontend_frames": len(data.predictions.beat_logits),
        "final_schema_version": 2,
    }
    provenance = {
        "schema_version": 1,
        "producer": "hardware-free-native-b2-v1",
        "track_id": "T01",
        "original_source_relative": data.frozen.source_relative,
        "original_source_sha256": data.frozen.source_sha256,
        "original_source_bytes": data.frozen.source_bytes,
        "actual_source_path": data.source.relative_to(tmp_path).as_posix(),
        "finish_accepted": True,
        "diagnostic_only": True,
        "frozen_manifest": data.files["frozen-manifest.json"],
        "probe_config": data.files["probe-config.json"],
        "native_source": {
            "native_metadata": data.metadata,
            "complete_mono_sha256": _digest(data.mono),
            "acknowledged_source_generation": 1,
            "complete_loaded_pcm": _binding(tmp_path, data.loaded)
            | {
                "rate_hz": 8,
                "channels": 2,
                "full_frames": 128,
                "finite_values": True,
            },
            "cold_manifest": data.files["cold-manifest.json"],
            "cached_original": data.files["cached-original.mp3"],
            "resident_window": {"start_frame": 16, "end_frame": 48, "window_revision": 1},
        },
        "runtime": {
            "build_profile": "release",
            "native_test_executable": data.files["native-test.exe"],
            "installed_native_extension": {
                "used_by_probe": False,
                "binding": data.files["installed.pyd"],
            },
            "native_key_model": {"binding": data.files["key-model.onnx"]},
        },
        "worker_configuration": {
            "model": asdict(data.request.model),
            "worker_limits": asdict(WorkerLimits()),
            **{
                name: data.files[name]
                for name in (
                    "interpreter",
                    "script",
                    "checkpoint",
                    "manifest",
                    "pointer",
                    "lock",
                    "environment",
                    "pyproject",
                )
            },
        },
        "producer_implementation": [data.files["producer.rs"], data.files["producer.py"]],
        "file_bindings": [data.files["script"] | {"role": "actual_auxiliary_dependency"}],
    }
    for name, value in {
        "summary.json": summary,
        "producer-provenance.json": provenance,
        **artifacts,
    }.items():
        _write(directory / name, value)
    identity = LoadedIdentity(
        track_id="T01",
        source_sha256=data.frozen.source_sha256,
        provenance="synthetic native evidence",
        pcm=PcmIdentity(
            path=data.listening.name,
            sha256=_digest(data.mono),
            sample_rate_hz=8,
            frame_count=128,
            origin_seconds=0.0,
            dtype="float32-le",
            channels=1,
        ),
    )
    profile = FreshNativeProfile(
        HistoricalProfile(
            "synthetic-native-v2",
            "T01",
            "scratch/fresh-native",
            "",
            tuple(ApprovedArtifact(name, "") for name in artifacts),
            retained_request=True,
        ),
        "",
    )
    fixture = FreshFixture(
        tmp_path,
        directory,
        _reference_track(identity),
        data.request,
        data.predictions,
        profile,
        monkeypatch,
    )
    fixture.pin()
    return fixture


def test_complete_native_v2_preserves_actual_alias_original_and_retired_request(
    prepared: FreshFixture,
) -> None:
    candidate = prepared.load()
    assert candidate.lineage_kind == "fresh_native_v2"
    assert candidate.historical_source_path == "test-audio/original.mp3"
    assert candidate.actual_source_path == "test-audio/1.mp3"
    assert candidate.request == prepared.request
    assert not candidate.request.pcm.path.exists()
    assert historical._predictions_identity(
        candidate.predictions
    ) == historical._predictions_identity(prepared.predictions)
    assert struct.pack("<d", candidate.predictions.beat_logits[0]) == struct.pack("<d", -0.0)
    roles = {binding.role for binding in candidate.artifact_bindings}
    assert {
        "actual_complete_loaded_pcm",
        "actual_native_test_executable",
        "separate_installed_pyd_not_used_by_probe",
        "actual_worker_lock",
        "actual_worker_checkpoint",
        "retained_complete_native_export",
        "actual_complete_reference_pcm",
        "actual_producer_implementation",
    } <= roles
    assert candidate.report()["actual_source_path"] == "test-audio/1.mp3"


def test_generic_self_hashed_profile_is_not_an_import_contract(prepared: FreshFixture) -> None:
    assert fresh.load_fresh_candidate(prepared.workspace, prepared.track, "generic-receipt") is None
    with pytest.raises(ValueError, match="unsupported_candidate_lineage"):
        load_candidate(prepared.workspace, prepared.track, "generic-receipt")


@pytest.mark.parametrize(
    "name",
    [
        "summary.json",
        "producer-provenance.json",
        "worker-request.raw.json",
        "worker-response.raw.json",
        "beat-component.raw.json",
        "native-finish-input.raw.json",
        "completion-event-0.raw.json",
        "native-observations.json",
    ],
)
def test_unreviewed_json_bytes_cannot_replace_the_fixed_profile(
    prepared: FreshFixture,
    name: str,
) -> None:
    path = prepared.directory / name
    path.write_bytes(path.read_bytes() + b" ")
    with pytest.raises(ValueError, match="approved_candidate_artifact_changed"):
        prepared.load()


@pytest.mark.parametrize(
    "name",
    [
        "interpreter",
        "script",
        "checkpoint",
        "manifest",
        "pointer",
        "lock",
        "environment",
        "pyproject",
        "native-test.exe",
        "installed.pyd",
        "key-model.onnx",
        "producer.rs",
        "producer.py",
        "cold-manifest.json",
        "probe-config.json",
        "frozen-manifest.json",
    ],
)
def test_actual_runtime_worker_and_producer_file_bytes_are_rehashed(
    prepared: FreshFixture,
    name: str,
) -> None:
    path = prepared.directory / name
    path.write_bytes(path.read_bytes() + b" changed")
    with pytest.raises(ValueError, match="fresh_native_bound_file_changed"):
        prepared.load()


@pytest.mark.parametrize(
    ("keys", "value"),
    [
        (("track_id",), "T02"),
        (("original_source_relative",), "other.mp3"),
        (("original_source_sha256",), "b" * 64),
        (("original_source_bytes",), 1),
        (("actual_source_path",), "test-audio/original.mp3"),
        (("producer",), "generic-self-hashed-native"),
        (("finish_accepted",), False),
        (("diagnostic_only",), False),
        (("native_source", "acknowledged_source_generation"), 2),
        (("native_source", "complete_mono_sha256"), "b" * 64),
        (("native_source", "complete_loaded_pcm", "rate_hz"), 16),
        (("native_source", "complete_loaded_pcm", "channels"), 1),
        (("native_source", "complete_loaded_pcm", "full_frames"), 127),
        (("native_source", "complete_loaded_pcm", "sha256"), "b" * 64),
        (("native_source", "complete_loaded_pcm", "bytes"), 512),
        (("native_source", "complete_loaded_pcm", "finite_values"), False),
        (("runtime", "build_profile"), "historical-unknown"),
        (("runtime", "installed_native_extension", "used_by_probe"), True),
        (("worker_configuration", "model", "frontend_id"), "other-frontend"),
        (("worker_configuration", "worker_limits", "max_pcm_bytes"), 1024**3),
        (("producer_implementation",), []),
    ],
)
def test_repinning_does_not_bypass_producer_semantic_relationships(
    prepared: FreshFixture,
    keys: tuple[str, ...],
    value: object,
) -> None:
    provenance = prepared.provenance()
    target = provenance
    for key in keys[:-1]:
        target = _mapping(target[key])
    target[keys[-1]] = value
    prepared.mutate("producer-provenance.json", provenance)
    with pytest.raises((ValueError, ValidationError)):
        prepared.load()


@pytest.mark.parametrize(
    "field",
    [
        "complete_source_identity",
        "original_sha256",
        "complete_playback_sha256",
        "complete_mono_sha256",
        "source_zero_frame",
    ],
)
def test_complete_source_metadata_cannot_be_discarded_to_fit_historical_schema(
    prepared: FreshFixture,
    field: str,
) -> None:
    observations = prepared.observations()
    del _mapping(observations[0]["metadata"])[field]
    prepared.mutate("native-observations.json", observations)
    with pytest.raises(ValidationError):
        prepared.load()


@pytest.mark.parametrize(
    "field",
    [
        "complete_source_identity",
        "original_sha256",
        "complete_playback_sha256",
        "complete_mono_sha256",
        "source_id",
        "source_generation",
        "request_id",
    ],
)
def test_complete_source_cannot_change_between_native_stages(
    prepared: FreshFixture,
    field: str,
) -> None:
    observations = prepared.observations()
    metadata = _mapping(observations[3]["metadata"])
    current = metadata[field]
    metadata[field] = current + 1 if isinstance(current, int) else "b" * 64
    prepared.mutate("native-observations.json", observations)
    with pytest.raises(ValueError, match=r"(fresh_native|candidate_native)"):
        prepared.load()


@pytest.mark.parametrize("change", ["missing", "reordered", "duplicate", "finish_rejected"])
def test_actual_finish_and_complete_ordered_native_sequence_are_required(
    prepared: FreshFixture,
    change: str,
) -> None:
    observations = prepared.observations()
    if change == "missing":
        del observations[3]
    elif change == "reordered":
        observations[3], observations[4] = observations[4], observations[3]
    elif change == "duplicate":
        observations.insert(3, observations[3])
    else:
        observations[6]["finish_accepted"] = False
    prepared.mutate("native-observations.json", observations)
    with pytest.raises(
        ValueError, match=r"fresh_native_(observation_sequence|finish_not_accepted)"
    ):
        prepared.load()


@pytest.mark.parametrize("field", ["complete_source_identity", "complete_playback_sha256"])
def test_readmission_cannot_bind_a_different_complete_source(
    prepared: FreshFixture,
    field: str,
) -> None:
    summary = prepared.summary()
    _mapping(summary["native_reservation_after_done"])[field] = "b" * 64
    prepared.mutate("summary.json", summary)
    with pytest.raises(ValueError, match="fresh_native_subsequent_source_changed"):
        prepared.load()


@pytest.mark.parametrize(("field", "value"), [("id", 1), ("request_id", 3)])
def test_completion_outer_identity_must_match_its_nested_native_request(
    prepared: FreshFixture,
    field: str,
    value: int,
) -> None:
    events = _rows(json.loads((prepared.directory / "loader-events.json").read_bytes()))
    events[0][field] = value
    prepared.mutate("loader-events.json", events)
    with pytest.raises(ValueError, match="fresh_native_completion_outer_identity_mismatch"):
        prepared.load()


def test_lossless_v1_is_still_not_the_fresh_v2_contract(prepared: FreshFixture) -> None:
    wire = json.dumps({
        "schema_version": 1,
        "identity": asdict(prepared.request.identity),
        "beat": _read(prepared.directory / "beat-component.raw.json"),
        "key": prepared.summary()["key"],
    })
    for name in (
        "result-envelope.raw.json",
        "native-finish-input.raw.json",
        "completion-event-0.raw.json",
    ):
        _write(prepared.directory / name, json.loads(wire))
    events = _rows(json.loads((prepared.directory / "loader-events.json").read_bytes()))
    events[0]["result_json"] = wire
    prepared.mutate("loader-events.json", events)
    with pytest.raises(ValueError, match="fresh_native_ready_v2_required"):
        prepared.load()


def test_component_signed_zero_loss_is_a_raw_bit_parity_failure(prepared: FreshFixture) -> None:
    component = _read(prepared.directory / "beat-component.raw.json")
    logits = _mapping(component["predictions"])["beat_logits"]
    assert isinstance(logits, list)
    logits[0] = 0.0
    prepared.mutate("beat-component.raw.json", component)
    with pytest.raises(ValueError, match="candidate_full_raw_prediction_parity_mismatch"):
        prepared.load()


@pytest.mark.parametrize("name", ["checkpoint", "lock"])
def test_self_rebound_checkpoint_and_lock_cannot_replace_selected_model(
    prepared: FreshFixture,
    name: str,
) -> None:
    path = prepared.directory / name
    path.write_bytes(b"another independently hashable artifact")
    provenance = prepared.provenance()
    _mapping(provenance["worker_configuration"])[name] = _binding(prepared.workspace, path)
    prepared.mutate("producer-provenance.json", provenance)
    with pytest.raises(ValueError, match="fresh_native_worker_model_lock_mismatch"):
        prepared.load()


@pytest.mark.parametrize(
    "name",
    [
        "complete-native-export.f32le",
        "complete-native-loaded.f32le",
        "listening.f32le",
    ],
)
@pytest.mark.parametrize("change", ["truncated", "same_size_mutation"])
def test_full_actual_pcm_bytes_cannot_be_replaced_by_shape_or_own_hash(
    prepared: FreshFixture,
    name: str,
    change: str,
) -> None:
    path = prepared.workspace / name if name == "listening.f32le" else prepared.directory / name
    raw = path.read_bytes()
    path.write_bytes(raw[:-4] if change == "truncated" else struct.pack("<f", 0.5) + raw[4:])
    with pytest.raises(ValueError, match="fresh_native_bound_file_changed"):
        prepared.load()


def test_rehashed_loaded_pcm_with_nonfinite_samples_still_rejects(prepared: FreshFixture) -> None:
    path = prepared.directory / "complete-native-loaded.f32le"
    raw = path.read_bytes()
    path.write_bytes(raw[:8] + struct.pack("<f", float("nan")) + raw[12:])
    changed_hash = _digest(path.read_bytes())
    observations = prepared.observations()
    for row in observations:
        _mapping(row["metadata"])["complete_playback_sha256"] = changed_hash
    prepared.mutate("native-observations.json", observations)
    summary = prepared.summary()
    _mapping(summary["native_reservation_after_done"])["complete_playback_sha256"] = changed_hash
    prepared.mutate("summary.json", summary)
    provenance = prepared.provenance()
    native = _mapping(provenance["native_source"])
    _mapping(native["native_metadata"])["complete_playback_sha256"] = changed_hash
    _mapping(native["complete_loaded_pcm"]).update(_binding(prepared.workspace, path))
    prepared.mutate("producer-provenance.json", provenance)
    with pytest.raises(ValueError, match="pcm_must_be_finite_complete_float32_le"):
        prepared.load()


@pytest.mark.parametrize(
    ("keys", "value"),
    [
        (("identity",), "b" * 64),
        (("descriptor", "decoder", "original", "sha256"), "b" * 64),
        (("descriptor", "decoder", "original", "bytes"), 1),
        (("descriptor", "playback", "pcm", "full_frames"), 127),
        (("descriptor", "playback", "pcm", "interleaved_sha256"), "b" * 64),
        (("descriptor", "playback", "pcm", "mono_sha256"), "b" * 64),
        (("descriptor", "playback", "pcm", "source_zero_bits"), "3ff0000000000000"),
        (("descriptor", "playback", "pcm", "mono_revision"), "other-mean-rule"),
    ],
)
def test_rehashed_cold_manifest_cannot_invent_native_source_relationships(
    prepared: FreshFixture,
    keys: tuple[str, ...],
    value: object,
) -> None:
    path = prepared.directory / "cold-manifest.json"
    manifest = _read(path)
    target = manifest
    for key in keys[:-1]:
        target = _mapping(target[key])
    target[keys[-1]] = value
    _write(path, manifest)
    provenance = prepared.provenance()
    _mapping(provenance["native_source"])["cold_manifest"] = _binding(prepared.workspace, path)
    prepared.mutate("producer-provenance.json", provenance)
    with pytest.raises(ValueError, match="fresh_native_cold_manifest_identity_mismatch"):
        prepared.load()


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("start_frame", -1),
        ("start_frame", 48),
        ("end_frame", 129),
        ("window_revision", 0),
    ],
)
def test_resident_window_is_distinct_from_full_source_and_must_be_valid(
    prepared: FreshFixture,
    field: str,
    value: int,
) -> None:
    provenance = prepared.provenance()
    _mapping(_mapping(provenance["native_source"])["resident_window"])[field] = value
    prepared.mutate("producer-provenance.json", provenance)
    with pytest.raises(ValueError, match="fresh_native_resident_window_invalid"):
        prepared.load()


def test_rehashed_cached_original_must_still_be_the_frozen_source(prepared: FreshFixture) -> None:
    path = prepared.directory / "cached-original.mp3"
    path.write_bytes(b"different original audio")
    provenance = prepared.provenance()
    _mapping(provenance["native_source"])["cached_original"] = _binding(prepared.workspace, path)
    prepared.mutate("producer-provenance.json", provenance)
    with pytest.raises(ValueError, match="fresh_native_sealed_original_mismatch"):
        prepared.load()


def test_rehashed_separate_installed_pyd_is_not_the_recorded_runtime(
    prepared: FreshFixture,
) -> None:
    path = prepared.directory / "installed.pyd"
    path.write_bytes(b"another installed runtime")
    provenance = prepared.provenance()
    installed = _mapping(_mapping(provenance["runtime"])["installed_native_extension"])
    installed["binding"] = _binding(prepared.workspace, path)
    prepared.mutate("producer-provenance.json", provenance)
    with pytest.raises(ValueError, match="fresh_native_separate_installed_pyd_mismatch"):
        prepared.load()


@pytest.mark.parametrize(
    "value",
    [
        [],
        [{"role": ""}],
        [{"role": "dependency", "path": "../outside", "sha256": "a" * 64, "bytes": 1}],
    ],
)
def test_dependency_receipts_are_complete_and_workspace_contained(
    prepared: FreshFixture,
    value: object,
) -> None:
    provenance = prepared.provenance()
    provenance["file_bindings"] = value
    prepared.mutate("producer-provenance.json", provenance)
    with pytest.raises(ValueError, match=r"fresh_native_|private_path_outside_workspace"):
        prepared.load()


def test_unknown_source_bytes_cannot_replace_the_actual_alias(prepared: FreshFixture) -> None:
    (prepared.workspace / "test-audio" / "1.mp3").write_bytes(b"other source audio")
    with pytest.raises(ValueError, match="fresh_native_original_source_changed"):
        prepared.load()


def test_complete_request_cannot_be_relabelled_as_a_short_window(prepared: FreshFixture) -> None:
    request = _read(prepared.directory / "worker-request.raw.json")
    _mapping(request["pcm"])["frame_count"] = 32
    prepared.mutate("worker-request.raw.json", request)
    with pytest.raises(ValueError, match="candidate_retained_request_summary_mismatch"):
        prepared.load()


def test_complete_raw_worker_logits_cannot_be_a_prefix(prepared: FreshFixture) -> None:
    response = _read(prepared.directory / "worker-response.raw.json")
    predictions = _mapping(response["predictions"])
    for name in ("beat_logits", "downbeat_logits"):
        values = predictions[name]
        assert isinstance(values, list)
        predictions[name] = values[:-1]
    prepared.mutate("worker-response.raw.json", response)
    with pytest.raises(ValueError, match="candidate_partial_full_track_logits"):
        prepared.load()


def test_repinning_duplicate_json_fields_cannot_create_an_ambiguous_producer(
    prepared: FreshFixture,
) -> None:
    path = prepared.directory / "producer-provenance.json"
    path.write_bytes(
        path.read_bytes().replace(
            b'{"schema_version": 1,', b'{"schema_version": 1,"schema_version": 1,', 1
        )
    )
    prepared.pin()
    with pytest.raises(ValueError, match="duplicate_json_key"):
        prepared.load()


@pytest.mark.parametrize(
    "value", [None, {}, {"status": "ready"}, {"status": "unavailable"}, {"binding": None}]
)
def test_ready_native_key_cannot_hide_the_actual_model_binding(
    prepared: FreshFixture,
    value: object,
) -> None:
    provenance = prepared.provenance()
    _mapping(provenance["runtime"])["native_key_model"] = value
    prepared.mutate("producer-provenance.json", provenance)
    with pytest.raises(ValueError, match=r"fresh_native_|FileBinding"):
        prepared.load()


def test_unavailable_key_model_preserves_complete_beats_when_key_failed(
    prepared: FreshFixture,
) -> None:
    key = {"status": "failed", "key": "unknown", "provenance": "synthetic missing KeyNet"}
    summary = prepared.summary()
    summary["key"] = key
    prepared.mutate("summary.json", summary)
    wire = ""
    for name in (
        "result-envelope.raw.json",
        "native-finish-input.raw.json",
        "completion-event-0.raw.json",
    ):
        envelope = _read(prepared.directory / name)
        envelope["key"] = key
        _write(prepared.directory / name, envelope)
        wire = json.dumps(envelope, allow_nan=False)
    events = _rows(json.loads((prepared.directory / "loader-events.json").read_bytes()))
    events[0]["result_json"] = wire
    prepared.mutate("loader-events.json", events)
    provenance = prepared.provenance()
    _mapping(provenance["runtime"])["native_key_model"] = {"status": "unavailable"}
    prepared.mutate("producer-provenance.json", provenance)
    candidate = prepared.load()
    assert historical._predictions_identity(
        candidate.predictions
    ) == historical._predictions_identity(prepared.predictions)
    assert "actual_native_key_model" not in {
        binding.role for binding in candidate.artifact_bindings
    }
