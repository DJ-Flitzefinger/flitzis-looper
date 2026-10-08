"""Synthetic independently pinned producer packets exercise the complete strict reader."""

import hashlib
import json
import struct
from dataclasses import dataclass, replace
from typing import TYPE_CHECKING

import pytest

from flitzis_looper.analysis import corrected_legacy_reader as reader
from flitzis_looper.analysis.beat_candidate_models import ApprovedArtifact
from flitzis_looper.analysis.corrected_legacy_models import (
    CorrectedLegacyCandidate,
    CorrectedLegacyProfile,
)
from flitzis_looper.analysis.corrected_legacy_sources import COMPILED_SOURCES, PRODUCER_FILES
from flitzis_looper.analysis.reference_inputs_models import LoadedIdentity, PcmIdentity
from flitzis_looper.analysis.reference_inputs_validation import (
    MANIFEST_SHA256,
    FrozenCorpus,
    FrozenTrack,
)
from tests.flitzis_looper.analysis.test_corrected_legacy_models import encoded, legacy_fixture
from tests.flitzis_looper.analysis.test_reference_inputs import _reference_track

if TYPE_CHECKING:
    from pathlib import Path

    from flitzis_looper.analysis.reference_inputs_models import ReferenceTrack


def _digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _write(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, allow_nan=False), encoding="utf8")


@dataclass
class Packet:
    workspace: Path
    directory: Path
    track: ReferenceTrack
    profile: CorrectedLegacyProfile
    monkeypatch: pytest.MonkeyPatch

    def pin(self) -> None:
        """Repin only test-owned synthetic anchors; production exposes no registration API."""
        self.profile = replace(
            self.profile,
            provenance_sha256=_digest((self.directory / "producer-provenance.json").read_bytes()),
            artifacts=tuple(
                ApprovedArtifact(a.name, _digest((self.directory / a.name).read_bytes()))
                for a in self.profile.artifacts
            ),
        )
        self.monkeypatch.setattr(reader, "_PROFILES", (self.profile,))

    def mutate(self, name: str, value: object) -> None:
        _write(self.directory / name, value)
        self.pin()

    def load(self) -> CorrectedLegacyCandidate:
        return reader.load_corrected_legacy(self.workspace, self.track, self.profile.profile_id)

    def bind(self, path: Path) -> dict[str, object]:
        return {
            "path": path.relative_to(self.workspace).as_posix(),
            "sha256": _digest(path.read_bytes()),
            "bytes": path.stat().st_size,
        }


@pytest.fixture
def packet(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Packet:
    return _packet(tmp_path, monkeypatch)


def _packet(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> Packet:
    directory = tmp_path / "scratch" / "native-qm"
    directory.mkdir(parents=True)
    source = tmp_path / "test-audio" / "original.wav"
    source.parent.mkdir()
    source.write_bytes(b"synthetic original; no acoustic reference")
    frozen = FrozenTrack(
        id="T01",
        source_relative="test-audio/original.wav",
        source_sha256=_digest(source.read_bytes()),
        source_bytes=source.stat().st_size,
        split="development",
    )
    monkeypatch.setattr(reader, "frozen_corpus", lambda _w: FrozenCorpus(tracks=(frozen,)))
    wire = json.loads(encoded(legacy_fixture((0.0, 10.0, 20.0, 30.0), (0, 2), 1)))
    mono = struct.pack("<96000f", *([0.25] * 96000))
    mono_path = directory / "mono.f32le"
    mono_path.write_bytes(mono)
    loaded = directory / "loaded.f32le"
    loaded.write_bytes(mono)
    analyzer = directory / "analyzer.f64le"
    analyzer.write_bytes(struct.pack("<44100d", *([0.25] * 44100)))
    wire["loaded"]["mono_sha256"] = _digest(mono)
    wire["analyzer"]["sha256"] = _digest(analyzer.read_bytes())
    identity = LoadedIdentity(
        track_id="T01",
        source_sha256=frozen.source_sha256,
        provenance="synthetic full source binding",
        pcm=PcmIdentity(
            path=mono_path.relative_to(tmp_path).as_posix(),
            sha256=_digest(mono),
            sample_rate_hz=96000,
            frame_count=96000,
            origin_seconds=0.0,
            dtype="float32-le",
            channels=1,
        ),
    )
    fixture_profile = CorrectedLegacyProfile(
        "synthetic-qm", "T01", "scratch/native-qm", "a" * 64, ()
    )
    fixture = Packet(tmp_path, directory, _reference_track(identity), fixture_profile, monkeypatch)
    meta = wire["identity"] | {
        "sample_rate_hz": 96000,
        "frame_count": 96000,
        "channels": 1,
        "origin_seconds": 0.0,
        "mono_rule": wire["loaded"]["mono_rule"],
        "complete_source_identity": "c" * 64,
        "original_sha256": frozen.source_sha256,
        "complete_playback_sha256": _digest(mono),
        "complete_mono_sha256": _digest(mono),
        "source_zero_frame": 0,
    }
    manifest = directory / "cold-manifest.json"
    _write(
        manifest,
        {
            "identity": meta["complete_source_identity"],
            "descriptor": {
                "decoder": {
                    "original": {"sha256": frozen.source_sha256, "bytes": frozen.source_bytes}
                },
                "playback": {
                    "pcm": {
                        "rate_hz": 96000,
                        "channels": 1,
                        "full_frames": 96000,
                        "full_bytes": len(mono),
                        "interleaved_sha256": _digest(mono),
                        "mono_sha256": _digest(mono),
                        "source_zero_bits": "0000000000000000",
                        "mono_revision": meta["mono_rule"],
                    }
                },
            },
        },
    )
    dummy = _dummy_bindings(fixture)
    provenance = _provenance(fixture, frozen, meta, dummy)
    _write(directory / "producer-provenance.json", provenance)
    _packet_artifacts(fixture, wire, meta, analyzer)
    fixture.profile = replace(
        fixture.profile,
        artifacts=tuple(
            ApprovedArtifact(p.name, "a" * 64)
            for p in sorted(directory.glob("*.json"))
            if p.name != "producer-provenance.json"
        ),
    )
    fixture.pin()
    return fixture


def _dummy_bindings(fixture: Packet) -> list[dict[str, object]]:
    directory = fixture.directory
    dummy = []
    for name in (
        "native.exe",
        "installed.pyd",
        "dependency.dll",
        "producer.rs",
        "producer.py",
        "pipeline.rs",
        "protocol.txt",
    ):
        path = directory / name
        path.write_bytes(f"synthetic pinned {name}".encode())
        dummy.append(fixture.bind(path))
    return dummy


def _provenance(
    fixture: Packet, frozen: FrozenTrack, meta: dict[str, object], dummy: list[dict[str, object]]
) -> dict[str, object]:
    loaded = fixture.directory / "loaded.f32le"
    manifest = fixture.directory / "cold-manifest.json"
    analyzer = fixture.directory / "analyzer.f64le"
    mono_path = fixture.directory / "mono.f32le"
    mono = mono_path.read_bytes()
    sources = _producer_sources(fixture)
    return {
        "schema_version": 1,
        "producer": "hardware-free-native-corrected-qm-v1",
        "track_id": "T01",
        "original_source_relative": frozen.source_relative,
        "original_source_sha256": frozen.source_sha256,
        "original_source_bytes": frozen.source_bytes,
        "actual_source_path": frozen.source_relative,
        "finish_accepted": True,
        "diagnostic_only": True,
        "native_source": {
            "native_metadata": meta,
            "complete_mono_sha256": _digest(mono),
            "acknowledged_source_generation": 1,
            "complete_loaded_pcm": fixture.bind(loaded)
            | {"rate_hz": 96000, "channels": 1, "full_frames": 96000, "finite_values": True},
            "cold_manifest": fixture.bind(manifest),
            "cached_original": fixture.bind(fixture.workspace / frozen.source_relative),
            "resident_window": {"start_frame": 100, "end_frame": 200, "window_revision": 1},
        },
        "runtime": _fixture_runtime(fixture, dummy),
        **sources,
        "file_bindings": [dummy[6] | {"role": "frozen_protocol"}, *sources["file_bindings"]],
        "analyzer_input": fixture.bind(analyzer),
        "native_export": fixture.bind(mono_path),
    }


def _producer_sources(fixture: Packet) -> dict[str, list[dict[str, object]]]:
    repository = fixture.workspace / "repo"
    implementations: list[dict[str, object]] = []
    mappings: list[dict[str, object]] = []
    rows: list[dict[str, object]] = []
    for relative in PRODUCER_FILES:
        original = repository / relative
        original.parent.mkdir(parents=True, exist_ok=True)
        original.write_bytes(f"synthetic retained source {relative}\n".encode())
        retained = fixture.directory / "producer-implementation" / relative
        retained.parent.mkdir(parents=True, exist_ok=True)
        retained.write_bytes(original.read_bytes())
        bound = fixture.bind(retained)
        implementations.append(bound)
        rows.append(fixture.bind(original))
        mappings.append({
            "repository_relative": relative,
            "original_path": str(original),
            "binding": bound,
            "role": "checked_shared_qm_source_or_native_producer_support",
        })
    for name, relative in COMPILED_SOURCES.items():
        retained = fixture.directory / name
        retained.write_bytes((repository / relative).read_bytes())
        bound = fixture.bind(retained)
        implementations.append(bound)
        mappings.append({
            "repository_relative": relative,
            "original_path": str(repository / relative),
            "binding": bound,
            "role": "actual_exe_include_str_compiled_producer",
        })
    initializer = repository / "src/flitzis_looper/__init__.py"
    initializer.write_bytes(b"")
    rows.append(fixture.bind(initializer))
    initializer_copy = fixture.directory / "runtime-python" / "__init__.py"
    initializer_copy.parent.mkdir()
    initializer_copy.write_bytes(b"")
    before, after = (
        fixture.directory / "sources-before.json",
        fixture.directory / "sources-after.json",
    )
    _write(before, {"files": rows})
    _write(after, {"files": rows, "identical_to_preflight": True, "before": fixture.bind(before)})
    complete = _complete_snapshot_fixture(fixture, rows, before, after)
    return {
        "producer_implementation": implementations,
        "producer_source_files": mappings,
        "file_bindings": [
            fixture.bind(before) | {"role": "complete_native_source_preflight_manifest"},
            fixture.bind(after) | {"role": "same_native_source_postflight_manifest"},
            fixture.bind(initializer_copy)
            | {"role": "actual_imported_python_module_source_or_extension"},
            *complete,
        ],
    }


def _complete_snapshot_fixture(
    fixture: Packet, rows: list[dict[str, object]], before: Path, after: Path
) -> list[dict[str, object]]:
    copies = []
    for row in rows:
        relative = str(row["path"])
        retained = (
            fixture.directory / "runtime-python/__init__.py"
            if relative == "repo/src/flitzis_looper/__init__.py"
            else fixture.directory / "producer-implementation" / relative.removeprefix("repo/")
        )
        copies.append(
            fixture.bind(retained)
            | {
                "original_path": str(fixture.workspace / relative),
                "role": "complete_preflight_local_source_and_build_snapshot",
            }
        )
    manifest = fixture.directory / "complete-source-snapshot.json"
    _write(
        manifest, {"files": copies, "before": fixture.bind(before), "after": fixture.bind(after)}
    )
    return [fixture.bind(manifest) | {"role": "complete_source_snapshot_manifest"}, *copies]


def _fixture_runtime(fixture: Packet, dummy: list[dict[str, object]]) -> dict[str, object]:
    executable = str(fixture.directory / "native.exe")
    library = str(fixture.directory / "dependency.dll")
    python = fixture.directory / "stdlib.py"
    python.write_bytes(b"synthetic imported stdlib\n")
    imported: list[dict[str, object]] = [
        {"name": "stdlib", "path": str(python)},
        {"name": "stdlib.alias", "path": str(python)},
        {"name": "_native_helper", "path": library},
        {
            "name": "flitzis_looper",
            "path": str(fixture.workspace / "repo/src/flitzis_looper/__init__.py"),
        },
        {"name": "b2_native_candidate_probe", "path": "b2_native_candidate_probe.py"},
    ]
    embedded_python = {
        "base_prefix": str(fixture.directory / "synthetic-python-runtime"),
        "executable": executable,
        "version": "synthetic fixture version",
    }
    trace = fixture.directory / "loaded-runtime-modules.json"
    _write(
        trace,
        {
            "pid": 1,
            "native_observation": (
                "WinAPI EnumProcessModulesEx/GetModuleFileNameExW actual current process"
            ),
            "external_file_bytes_read": False,
            "loaded_native_modules": [{"path": executable}, {"path": library}],
            "imported_python_modules": imported,
        },
    )
    _write(
        fixture.directory / "native-runtime.json",
        {
            "build_profile": "release",
            "native_test_executable": dummy[0] | {"path": executable},
            "hardware_started": False,
            "keynet": "not_executed",
            "beat_this_worker": "not_executed",
            "repository": str(fixture.workspace / "repo"),
            "embedded_python": embedded_python,
        },
    )
    return {
        "build_profile": "release",
        "executed_procedure": "native-shared-analyze_bpm_raw",
        "native_exe_actually_executed": True,
        "key_worker_executed": False,
        "beat_this_worker_executed": False,
        "native_test_executable": dummy[0],
        "installed_native_extension": {
            "used_by_probe": False,
            "binding": dummy[1],
            "original_path": str(fixture.directory / "installed.pyd"),
        },
        "loaded_libraries": [dummy[2]],
        "loaded_library_sources": [
            {
                "source_path": library,
                "binding": dummy[2],
                "role": "actual_traced_workspace_native_runtime_library",
            }
        ],
        "actual_module_trace": fixture.bind(trace),
        "embedded_python": embedded_python,
        "external_runtime_module_metadata": [],
        "imported_python_sources": [
            row
            | {
                "binding": fixture.bind(fixture.directory / "compiled-producer.py")
                if row["name"] == "b2_native_candidate_probe"
                else dummy[2]
                if row["name"] == "_native_helper"
                else fixture.bind(fixture.directory / "runtime-python/__init__.py")
                if row["name"] == "flitzis_looper"
                else fixture.bind(python),
                "role": "actual_exe_embedded_python_source"
                if row["name"] == "b2_native_candidate_probe"
                else "actual_traced_imported_module_file_snapshot",
            }
            for row in imported
        ],
    }


def _packet_artifacts(
    fixture: Packet, wire: dict[str, object], meta: dict[str, object], analyzer: Path
) -> None:
    directory = fixture.directory
    for name in (
        "native-result.raw.json",
        "native-finish-input.raw.json",
        "result-envelope.raw.json",
        "completion-event-0.raw.json",
    ):
        (directory / name).write_bytes(encoded(wire))
    artifacts = {
        "native-request.raw.json": {
            "schema_version": 1,
            "backend": "corrected-qm-native-v1",
            "identity": wire["identity"],
            "native_metadata": meta,
            "pcm_export_path": "retired/mono.f32le",
            "analyzer_export_path": str(analyzer),
        },
        "summary.json": {
            "finish_accepted": True,
            "completion_event_count": 1,
            "retired": True,
            "subsequent_metadata": meta | {"request_id": 3},
        },
        "native-observations.json": [
            {
                "stage": stage,
                "metadata": meta,
                **({"finish_accepted": True} if stage == "after_finish" else {}),
            }
            for stage in reader._STAGES
        ],
        "loader-events.json": [
            {
                "type": "offline_analysis_completed",
                "id": 0,
                "request_id": 2,
                "result_json": encoded(wire).decode(),
            }
        ],
    }
    for name, value in artifacts.items():
        _write(directory / name, value)


def test_supported_full_chain_and_separate_unused_pyd(packet: Packet) -> None:
    result = reader.load_corrected_legacy(packet.workspace, packet.track, packet.profile.profile_id)
    assert len(result.result.beat_seconds) == 4
    assert result.result.downbeat_raw_indices == (0, 2)
    assert any(
        b.bytes == 0 and b.role == "actual_imported_python_source" for b in result.artifact_bindings
    )
    roles = {b.role for b in result.artifact_bindings}
    assert {
        "actual_executing_qm_native_test_executable",
        "separate_installed_pyd_unused_by_qm_probe",
        "actual_complete_qm_analyzer_input",
        "actual_complete_loaded_pcm",
    } <= roles
    assert reader.available_corrected_legacy_profiles() == [
        {"track_id": "T01", "profile_id": "synthetic-qm"}
    ]


def test_self_hashed_new_profile_is_never_supported(packet: Packet) -> None:
    with pytest.raises(ValueError, match="unsupported_corrected_legacy"):
        reader.load_corrected_legacy(packet.workspace, packet.track, "self-registered-qm")


@pytest.mark.parametrize(
    "name",
    [
        "native-result.raw.json",
        "native-finish-input.raw.json",
        "result-envelope.raw.json",
        "completion-event-0.raw.json",
        "native-observations.json",
        "native-request.raw.json",
        "summary.json",
        "loader-events.json",
    ],
)
def test_frozen_artifact_changes_are_rejected_without_registration(
    packet: Packet, name: str
) -> None:
    with (packet.directory / name).open("ab") as handle:
        handle.write(b" ")
    with pytest.raises(ValueError, match="artifact_changed"):
        packet.load()


@pytest.mark.parametrize(
    ("field", "value"),
    [
        ("finish_accepted", False),
        ("retired", False),
        ("completion_event_count", 2),
        ("completion_event_count", True),
    ],
)
def test_repinned_receipt_cannot_replace_actual_native_finish(
    packet: Packet, field: str, value: object
) -> None:
    summary = json.loads((packet.directory / "summary.json").read_bytes())
    summary[field] = value
    packet.mutate("summary.json", summary)
    with pytest.raises(ValueError, match="finish_retirement"):
        packet.load()


@pytest.mark.parametrize("stage", reader._STAGES)
def test_every_native_chain_stage_is_required_in_order(packet: Packet, stage: str) -> None:
    observations = json.loads((packet.directory / "native-observations.json").read_bytes())
    packet.mutate("native-observations.json", [r for r in observations if r["stage"] != stage])
    with pytest.raises(ValueError, match="chain_incomplete"):
        packet.load()


def test_repinned_unknown_stage_cannot_hide_inside_complete_native_observations(
    packet: Packet,
) -> None:
    observations = json.loads((packet.directory / "native-observations.json").read_bytes())
    observations.insert(5, {**observations[4], "stage": "invented_successful_analysis"})
    packet.mutate("native-observations.json", observations)
    with pytest.raises(ValueError, match="chain_incomplete"):
        packet.load()


@pytest.mark.parametrize(
    "kind",
    ["identity", "event_bytes", "duplicate_completion", "subsequent", "request", "late_finish"],
)
def test_repinned_packet_cannot_hide_stale_or_lossy_publication(packet: Packet, kind: str) -> None:
    if kind in {"identity", "event_bytes", "duplicate_completion"}:
        events = json.loads((packet.directory / "loader-events.json").read_bytes())
        if kind == "identity":
            events[0]["request_id"] = 3
        elif kind == "event_bytes":
            events[0]["result_json"] += " "
        else:
            events.append(events[0])
        packet.mutate("loader-events.json", events)
    elif kind == "subsequent":
        summary = json.loads((packet.directory / "summary.json").read_bytes())
        summary["subsequent_metadata"]["source_generation"] = 2
        packet.mutate("summary.json", summary)
    elif kind == "request":
        request = json.loads((packet.directory / "native-request.raw.json").read_bytes())
        request["native_metadata"]["request_id"] = 3
        packet.mutate("native-request.raw.json", request)
    else:
        packet.mutate(
            "native-finish-input.raw.json",
            json.loads((packet.directory / "native-result.raw.json").read_bytes()),
        )
    with pytest.raises(ValueError, match="corrected_legacy"):
        packet.load()


@pytest.mark.parametrize("field", ["native_export", "analyzer_input"])
def test_actual_pcm_and_analyzer_bytes_are_rehashed(packet: Packet, field: str) -> None:
    path = (
        packet.workspace
        / json.loads((packet.directory / "producer-provenance.json").read_bytes())[field]["path"]
    )
    with path.open("r+b") as handle:
        handle.write(b"\x00" * 8)
    with pytest.raises(ValueError, match="bound_file_changed"):
        packet.load()


@pytest.mark.parametrize(
    "change",
    [
        "key",
        "neural",
        "procedure",
        "exe",
        "untraced_library",
        "source",
        "pyd",
        "trace_bytes",
        "trace_import",
        "profile",
    ],
)
def test_repinned_receipt_requires_actual_runtime_and_source_mapping(
    packet: Packet, change: str
) -> None:
    data = json.loads((packet.directory / "producer-provenance.json").read_bytes())
    runtime = data["runtime"]
    if change in {"key", "neural"}:
        runtime["key_worker_executed" if change == "key" else "beat_this_worker_executed"] = True
    elif change == "procedure":
        runtime["executed_procedure"] = "synthetic_python_tracker"
    elif change == "exe":
        raw = json.loads((packet.directory / "native-runtime.json").read_bytes())
        raw["native_test_executable"]["sha256"] = "a" * 64
        packet.mutate("native-runtime.json", raw)
    elif change == "untraced_library":
        runtime["loaded_library_sources"][0]["source_path"] = "unobserved.dll"
    elif change == "source":
        data["producer_source_files"][0]["repository_relative"] = "../outside.rs"
    elif change == "pyd":
        runtime["installed_native_extension"]["original_path"] = str(
            packet.directory / "dependency.dll"
        )
    elif change in {"trace_bytes", "trace_import"}:
        trace = json.loads((packet.directory / "loaded-runtime-modules.json").read_bytes())
        if change == "trace_import":
            trace["imported_python_modules"][0]["name"] = "flitzis_looper_audio"
        else:
            trace["loaded_native_modules"] = []
        packet.mutate("loaded-runtime-modules.json", trace)
        runtime["actual_module_trace"] = packet.bind(
            packet.directory / "loaded-runtime-modules.json"
        )
    else:
        runtime["build_profile"] = "debug"
    packet.mutate("producer-provenance.json", data)
    with pytest.raises(ValueError, match="corrected_legacy"):
        packet.load()


@pytest.mark.parametrize("kind", ["provenance_schema", "event_id", "request_identity"])
def test_boolean_cannot_impersonate_native_integer_identity(packet: Packet, kind: str) -> None:
    if kind == "provenance_schema":
        data = json.loads((packet.directory / "producer-provenance.json").read_bytes())
        data["schema_version"] = True
        packet.mutate("producer-provenance.json", data)
    elif kind == "event_id":
        data = json.loads((packet.directory / "loader-events.json").read_bytes())
        data[0]["id"] = False
        packet.mutate("loader-events.json", data)
    else:
        data = json.loads((packet.directory / "native-request.raw.json").read_bytes())
        data["identity"]["pad_id"] = False
        packet.mutate("native-request.raw.json", data)
    with pytest.raises(ValueError, match=r"corrected_legacy|Input should be a valid integer"):
        packet.load()


@pytest.mark.parametrize(
    "change",
    [
        "missing_source",
        "duplicate_mapping",
        "original_path",
        "postflight",
        "retained_source",
        "preflight_link",
    ],
)
def test_source_support_and_actual_preflight_cannot_be_replaced_by_arbitrary_files(
    packet: Packet, change: str
) -> None:
    data = json.loads((packet.directory / "producer-provenance.json").read_bytes())
    if change == "missing_source":
        data["producer_implementation"].pop(0)
        data["producer_source_files"].pop(0)
    elif change == "duplicate_mapping":
        data["producer_source_files"][0] = data["producer_source_files"][1]
    elif change == "original_path":
        data["producer_source_files"][0]["original_path"] = str(packet.directory / "unrelated.rs")
    elif change == "retained_source":
        row = data["producer_source_files"][0]
        source = packet.workspace / row["binding"]["path"]
        source.write_bytes(b"arbitrary substitute producer source\n")
        row["binding"] = packet.bind(source)
        data["producer_implementation"][0] = row["binding"]
    else:
        path = packet.directory / "sources-after.json"
        after = json.loads(path.read_bytes())
        if change == "postflight":
            after["files"][0]["sha256"] = "a" * 64
        else:
            after["before"] = packet.bind(packet.directory / "protocol.txt")
        packet.mutate(path.name, after)
        next(
            row
            for row in data["file_bindings"]
            if row["role"] == "same_native_source_postflight_manifest"
        ).update(packet.bind(path))
    packet.mutate("producer-provenance.json", data)
    with pytest.raises(ValueError, match="corrected_legacy"):
        packet.load()


@pytest.mark.parametrize(
    "change",
    [
        "missing",
        "duplicate",
        "path",
        "embedded",
        "extension",
        "python_version",
        "duplicate_dll",
        "unaccounted_module",
    ],
)
def test_every_actual_import_and_loaded_module_has_one_correct_retained_identity(
    packet: Packet, change: str
) -> None:
    data = json.loads((packet.directory / "producer-provenance.json").read_bytes())
    runtime = data["runtime"]
    mappings = runtime["imported_python_sources"]
    if change == "missing":
        mappings.pop(0)
    elif change == "duplicate":
        mappings.append(mappings[0])
    elif change == "path":
        mappings[0]["path"] = str(packet.directory / "unobserved.py")
    elif change == "embedded":
        mappings[-1]["binding"] = data["producer_source_files"][4]["binding"]
    elif change == "extension":
        mappings[2]["binding"] = packet.bind(packet.directory / "protocol.txt")
    elif change == "python_version":
        runtime["embedded_python"]["version"] = "unobserved Python version"
    elif change == "duplicate_dll":
        runtime["loaded_libraries"].append(packet.bind(packet.directory / "protocol.txt"))
        row = dict(runtime["loaded_library_sources"][0])
        row["binding"] = runtime["loaded_libraries"][-1]
        runtime["loaded_library_sources"].append(row)
    else:
        trace_path = packet.directory / "loaded-runtime-modules.json"
        trace = json.loads(trace_path.read_bytes())
        trace["loaded_native_modules"].append({"path": str(packet.directory / "unaccounted.dll")})
        packet.mutate(trace_path.name, trace)
        runtime["actual_module_trace"] = packet.bind(trace_path)
    packet.mutate("producer-provenance.json", data)
    with pytest.raises(ValueError, match="corrected_legacy"):
        packet.load()


def test_external_os_modules_are_metadata_only_and_never_opened(packet: Packet) -> None:
    data = json.loads((packet.directory / "producer-provenance.json").read_bytes())
    trace_path = packet.directory / "loaded-runtime-modules.json"
    trace = json.loads(trace_path.read_bytes())
    external = str(packet.workspace.parent / "never-read-external-os-module.dll")
    assert not (packet.workspace.parent / "never-read-external-os-module.dll").exists()
    trace["loaded_native_modules"].append({"path": external})
    packet.mutate(trace_path.name, trace)
    data["runtime"]["actual_module_trace"] = packet.bind(trace_path)
    data["runtime"]["external_runtime_module_metadata"] = [
        {"path": external, "bytes_rehashed": False, "reason": "actual ambient OS module metadata"}
    ]
    packet.mutate("producer-provenance.json", data)
    assert packet.load() is not None
    data["runtime"]["external_runtime_module_metadata"][0]["bytes_rehashed"] = True
    packet.mutate("producer-provenance.json", data)
    with pytest.raises(ValueError, match="external_module_partition"):
        packet.load()


@pytest.mark.parametrize("change", ["missing", "duplicate", "hash", "original", "link"])
def test_complete_build_source_snapshot_cannot_omit_or_relabel_preflight_bytes(
    packet: Packet, change: str
) -> None:
    data = json.loads((packet.directory / "producer-provenance.json").read_bytes())
    path = packet.directory / "complete-source-snapshot.json"
    manifest = json.loads(path.read_bytes())
    if change == "missing":
        manifest["files"].pop(0)
    elif change == "duplicate":
        manifest["files"][0] = manifest["files"][1]
    elif change == "hash":
        manifest["files"][0]["sha256"] = "a" * 64
    elif change == "original":
        manifest["files"][0]["original_path"] = str(packet.directory / "unrelated.rs")
    else:
        manifest["before"] = packet.bind(packet.directory / "protocol.txt")
    packet.mutate(path.name, manifest)
    next(
        row for row in data["file_bindings"] if row["role"] == "complete_source_snapshot_manifest"
    ).update(packet.bind(path))
    packet.mutate("producer-provenance.json", data)
    with pytest.raises(ValueError, match="corrected_legacy"):
        packet.load()


def _relocated_source(packet: Packet) -> None:
    data = json.loads((packet.directory / "producer-provenance.json").read_bytes())
    original = packet.workspace / data["original_source_relative"]
    relocated = packet.workspace / "test-audio/explicit-renamed-source.wav"
    relocated.write_bytes(original.read_bytes())
    data["actual_source_path"] = relocated.relative_to(packet.workspace).as_posix()
    aliases = {
        "schema_version": 1,
        "manifest_sha256": MANIFEST_SHA256,
        "aliases": [
            {
                "track_id": "T01",
                "original_source_relative": data["original_source_relative"],
                "actual_source_path": data["actual_source_path"],
            }
        ],
    }
    alias_path = packet.directory / "source-aliases.json"
    _write(alias_path, aliases)
    data["file_bindings"].append(
        packet.bind(alias_path) | {"role": "unchanged_explicit_original_source_aliases"}
    )
    packet.mutate("producer-provenance.json", data)


def test_content_identical_t01_relocation_requires_actual_explicit_alias(packet: Packet) -> None:
    _relocated_source(packet)
    result = packet.load()
    assert any(
        b.role == "verified_corrected_legacy_source_aliases" for b in result.artifact_bindings
    )


@pytest.mark.parametrize("change", ["missing", "duplicate", "track", "path", "hash", "manifest"])
def test_same_source_hash_does_not_authorize_unrecorded_or_changed_alias(
    packet: Packet, change: str
) -> None:
    _relocated_source(packet)
    alias_path = packet.directory / "source-aliases.json"
    data = json.loads((packet.directory / "producer-provenance.json").read_bytes())
    aliases = json.loads(alias_path.read_bytes())
    if change == "missing":
        data["file_bindings"].pop()
    elif change == "duplicate":
        data["file_bindings"].append(data["file_bindings"][-1])
    elif change == "hash":
        (packet.workspace / data["actual_source_path"]).write_bytes(b"changed private source alias")
    else:
        if change == "track":
            # Supply a valid T02 alias against a two-track synthetic corpus. Its
            # presence must not authorize the distinct T01 relocation.
            first = FrozenTrack(
                id="T01",
                source_relative=data["original_source_relative"],
                source_sha256=data["original_source_sha256"],
                source_bytes=data["original_source_bytes"],
                split="development",
            )
            second = first.model_copy(update={"id": "T02"})
            packet.monkeypatch.setattr(
                reader, "frozen_corpus", lambda _w: FrozenCorpus(tracks=(first, second))
            )
            aliases["aliases"][0]["track_id"] = "T02"
        elif change == "path":
            other = packet.workspace / "test-audio/another-identical-copy.wav"
            other.write_bytes((packet.workspace / data["actual_source_path"]).read_bytes())
            aliases["aliases"][0]["actual_source_path"] = other.relative_to(
                packet.workspace
            ).as_posix()
        else:
            aliases["manifest_sha256"] = "a" * 64
        _write(alias_path, aliases)
        data["file_bindings"][-1].update(packet.bind(alias_path))
    packet.mutate("producer-provenance.json", data)
    with pytest.raises(ValueError, match=r"corrected_legacy|source_alias"):
        packet.load()
