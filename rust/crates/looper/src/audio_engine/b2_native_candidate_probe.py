"""Embedded private probe of productive native preparation and offline publication."""
# Compiled into an explicitly ignored Rust test; never imported by the app.
# ruff: noqa: INP001

import hashlib
import json
import shutil
import struct
import sys
import time
from dataclasses import asdict
from pathlib import Path
from threading import Lock
from typing import Any
from unittest.mock import patch

from flitzis_looper.analysis import process as worker_process
from flitzis_looper.analysis import worker as worker_module
from flitzis_looper.analysis.contracts import WorkerLimits, decode_response, encode_request
from flitzis_looper.analysis.jobs import OfflineAnalysisService
from flitzis_looper.analysis.publication import decode_result
from flitzis_looper.analysis.reference_inputs_validation import (
    frozen_corpus,
    loaded_identities,
    private_path,
    read_source_aliases,
    sha256_file,
)
from flitzis_looper.analysis.setup import load_worker_configuration
from flitzis_looper.analysis.worker import BeatWorkerAdapter

ARRAYS = ("beat_seconds", "downbeat_seconds", "beat_logits", "downbeat_logits")
PRODUCER_FILES = (
    "rust/crates/looper/src/audio_engine/b2_native_candidate_probe.rs",
    "rust/crates/looper/src/audio_engine/b2_native_candidate_probe.py",
    "rust/crates/looper/src/audio_engine/cold_residency_tests.rs",
    "rust/crates/looper/src/audio_engine/cold_load.rs",
    "rust/crates/looper/src/audio_engine/cold_store.rs",
    "rust/crates/looper/src/audio_engine/cold_store/staging.rs",
    "rust/crates/looper/src/audio_engine/sample_loader.rs",
    "rust/crates/looper/src/audio_engine/complete_context.rs",
    "rust/crates/looper/src/audio_engine/analysis_jobs.rs",
    "rust/crates/looper/src/audio_engine/analysis_pcm.rs",
    "rust/crates/looper/src/audio_engine/analysis_pcm/streamed.rs",
    "rust/crates/looper/src/audio_engine/analysis_pcm/fft.rs",
    "src/flitzis_looper/analysis/jobs.py",
    "src/flitzis_looper/analysis/publication.py",
    "src/flitzis_looper/analysis/contracts.py",
    "src/flitzis_looper/analysis/worker.py",
    "src/flitzis_looper/analysis/process.py",
    "src/flitzis_looper/analysis/windows_job.py",
    "src/flitzis_looper/analysis/setup.py",
    "src/flitzis_looper/analysis/artifacts.py",
)


def write_json(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, indent=2, allow_nan=False), encoding="utf-8")


def read_json(path: Path) -> Any:
    return json.loads(path.read_bytes())


def array_identity(predictions: Any) -> dict[str, dict[str, int | str]]:
    result = {}
    for name in ARRAYS:
        values = getattr(predictions, name)
        raw = struct.pack(f"<{len(values)}d", *values)
        result[name] = {"count": len(values), "float64_le_sha256": hashlib.sha256(raw).hexdigest()}
    return result


class NativeRecorder:
    """Delegate every productive operation unchanged and retain observations."""

    def __init__(self, native: Any, output: Path) -> None:
        self.native = native
        self.output = output
        self.observations: list[dict[str, object]] = []
        self.lock = Lock()
        self.finish_accepted = None
        self.observe("admitted")

    def __getattr__(self, name: str) -> Any:
        return getattr(self.native, name)

    def observe(self, stage: str, **extra: object) -> None:
        row = {
            "stage": stage,
            "monotonic_seconds": time.monotonic(),
            "metadata": self.native.metadata(),
            "staging_stats": self.native.staging_stats(),
            **extra,
        }
        with self.lock:
            self.observations.append(row)
            write_json(self.output / "native-observations.json", self.observations)

    def prepare_export(self, path: str) -> None:
        self.observe("before_prepare_export")
        self.native.prepare_export(path)
        self.observe("after_prepare_export")

    def analyze_key(self) -> str:
        self.observe("before_analyze_key")
        raw = self.native.analyze_key()
        (self.output / "native-key.raw.json").write_text(raw, encoding="utf-8")
        self.observe("after_analyze_key")
        return raw

    def retire_pcm(self) -> None:
        self.observe("before_retire_pcm")
        self.native.retire_pcm()
        self.observe("after_retire_pcm")

    def finish(self, result_json: str) -> bool:
        self.observe("before_finish")
        (self.output / "native-finish-input.raw.json").write_text(result_json, encoding="utf-8")
        self.finish_accepted = self.native.finish(result_json)
        self.observe(
            "after_finish", finish_return=self.finish_accepted, finish_accepted=self.finish_accepted
        )
        return self.finish_accepted


class RecordingEngine:
    def __init__(self, bridge: Any, output: Path) -> None:
        self.bridge = bridge
        self.output = output
        self.native: NativeRecorder | None = None

    def begin_offline_analysis(self, pad_id: int) -> NativeRecorder:
        self.native = NativeRecorder(self.bridge.begin_offline_analysis(pad_id), self.output)
        return self.native


class RecordingAdapter(BeatWorkerAdapter):
    def __init__(self, configuration: Any, output: Path, source_proof: dict[str, Any]) -> None:
        super().__init__(configuration)
        self.output = output
        self.source_proof = source_proof
        self.request = None
        self.export = None
        self.component = None

    def run(self, request: Any, cancel: Any) -> Any:
        self.request = request
        (self.output / "worker-request.raw.json").write_bytes(
            encode_request(request, WorkerLimits())
        )
        retained = self.output / "complete-native-export.f32le"
        shutil.copyfile(request.pcm.path, retained)
        digest = sha256_file(retained, pcm=True)
        assert digest == self.source_proof["complete_mono_sha256"]
        assert retained.stat().st_size == request.pcm.frame_count * 4
        with retained.open("rb") as stream:
            first = struct.unpack("<f", stream.read(4))[0]
            stream.seek(-4, 2)
            last = struct.unpack("<f", stream.read(4))[0]
        self.export = {
            **asdict(request.pcm),
            "path": str(request.pcm.path),
            "sha256": digest,
            "bytes": retained.stat().st_size,
            "first_frame": first,
            "last_frame": last,
            "scope": "complete native loaded-rate mono; source zero through exclusive end",
        }
        write_json(self.output / "complete-native-export-identity.json", self.export)
        self.component = super().run(request, cancel)
        write_json(self.output / "beat-component.raw.json", asdict(self.component))
        return self.component


class Probe:
    """Own one actual service until native/worker retirement and completed event."""

    def __init__(self, bridge: Any, config_path: str) -> None:
        self.config_path = Path(config_path)
        self.config = read_json(self.config_path)
        self.workspace = Path(self.config["workspace"]).resolve(strict=True)
        self.output = Path(self.config["output_directory"]).resolve(strict=True)
        self.repository = self.workspace / "repo"
        self.runtime = read_json(self.output / "native-runtime.json")
        self.retain_key_model()
        self.source_proof = read_json(self.output / "native-source-proof.json")
        self.track_id = self.config["track_id"]
        assert self.track_id in {"T01", "T02", "T03"}
        corpus = frozen_corpus(self.workspace)
        self.track = next(track for track in corpus.tracks if track.id == self.track_id)
        self.identity = next(
            item
            for item in loaded_identities(
                self.workspace, self.config["loaded_identities_path"]
            ).tracks
            if item.track_id == self.track_id
        )
        aliases = read_source_aliases(self.workspace, self.config["source_aliases_path"], corpus)
        alias = next((item for item in aliases if item.track_id == self.track_id), None)
        actual_path = alias.actual_source_path if alias else self.track.source_relative
        self.source = private_path(self.workspace, actual_path)
        assert self.source == private_path(self.workspace, self.config["actual_source_path"])
        assert sha256_file(self.source) == self.track.source_sha256 == self.config["source_sha256"]
        assert self.source.stat().st_size == self.track.source_bytes == self.config["source_bytes"]
        assert self.source_proof["complete_mono_sha256"] == self.identity.pcm.sha256
        assert (
            self.source_proof["complete_loaded_pcm"]["full_frames"] == self.identity.pcm.frame_count
        )
        assert (
            self.source_proof["complete_loaded_pcm"]["rate_hz"] == self.identity.pcm.sample_rate_hz
        )
        self.jobs_directory = self.output / "jobs"
        self.worker_directory = self.output / "worker-requests"
        self.worker_directory.mkdir()
        self.configuration = load_worker_configuration(
            self.workspace / self.config["worker_installation"], self.worker_directory
        )
        self.bridge = bridge
        self.engine = RecordingEngine(bridge, self.output)
        self.adapter = RecordingAdapter(self.configuration, self.output, self.source_proof)
        self.service = OfflineAnalysisService()
        self.events: list[dict[str, object]] = []
        self.processes: list[Any] = []
        self.wire_outcome = None
        self.original_start = worker_process._start
        self.original_run = worker_module.run_process
        self.start_patch = patch.object(worker_process, "_start", self.record_start)
        self.run_patch = patch.object(worker_module, "run_process", self.record_run)
        self.start_patch.start()
        self.run_patch.start()
        self.started = time.monotonic()
        self.job = self.service.start(
            self.engine,
            0,
            self.jobs_directory,
            model=self.configuration.model,
            adapter=self.adapter,
        )

    def retain_key_model(self) -> None:
        # Mirror KeyNet's documented resolver order before native inference. The
        # retained digest describes actual selected bytes; no model is installed.
        executable = Path(self.runtime["native_test_executable"]["path"])
        candidates = [executable.parent / "keynet.onnx", Path.cwd() / "keynet.onnx"]
        candidates.extend(
            Path.cwd() / name / "keynet.onnx" for name in ("assets/models", "models", "assets")
        )
        selected = next((path for path in candidates if path.is_file()), None)
        if selected is None:
            self.runtime["native_key_model"] = {"status": "unavailable"}
            return
        assert selected.resolve().is_relative_to(self.workspace)
        retained = self.output / "native-key-model.onnx"
        shutil.copyfile(selected, retained)
        self.runtime["native_key_model"] = {
            "binding": self.binding(retained),
            "original_path": str(selected.resolve()),
            "resolver": "KeyNet documented executable/cwd/assets model resolution order",
        }

    def record_start(self, command: Any, request_path: Path) -> Any:
        raw = request_path.read_bytes()
        assert raw == (self.output / "worker-request.raw.json").read_bytes()
        (self.output / "worker-process-request.raw.json").write_bytes(raw)
        process = self.original_start(command, request_path)
        self.processes.append(process)
        write_json(
            self.output / "worker-process-start.json",
            {
                "pid": process.pid,
                "request_path": str(request_path),
                "command": {name: str(value) for name, value in asdict(command).items()},
                "monotonic_seconds": time.monotonic(),
            },
        )
        return process

    def record_run(self, *arguments: Any, **keywords: Any) -> Any:
        result = self.original_run(*arguments, **keywords)
        (self.output / "worker-response.raw.json").write_bytes(result.response)
        self.wire_outcome = {
            "reason": result.reason,
            "bytes": len(result.response),
            "sha256": hashlib.sha256(result.response).hexdigest(),
            "resources_released": result.resources_released,
        }
        write_json(self.output / "worker-wire-outcome.json", self.wire_outcome)
        return result

    def poll(self) -> bool:
        while (event := self.bridge.engine.poll_loader_events()) is not None:
            self.events.append(event)
            if event["type"] == "offline_analysis_completed":
                completions = sum(
                    row["type"] == "offline_analysis_completed" for row in self.events
                )
                (self.output / f"completion-event-{completions - 1}.raw.json").write_text(
                    event["result_json"], encoding="utf-8"
                )
        write_json(self.output / "loader-events.json", self.events)
        return self.job.done.is_set()

    def binding(self, path: Path, role: str | None = None) -> dict[str, object]:
        resolved = path.resolve(strict=True)
        assert resolved.is_relative_to(self.workspace)
        return {
            "path": resolved.relative_to(self.workspace).as_posix(),
            **({"role": role} if role else {}),
            "sha256": sha256_file(resolved),
            "bytes": resolved.stat().st_size,
        }

    def provenance(self, admitted: dict[str, object]) -> dict[str, object]:
        configuration = self.configuration
        assert configuration.manifest is not None
        installed = configuration.manifest.parent
        files = [
            self.binding(self.source, "original_source"),
            self.binding(self.output / "native-original-snapshot.bin", "native_original_snapshot"),
            self.binding(self.output / "cold-source-manifest.json", "cold_source_manifest"),
            self.binding(self.output / "native-test-executable.exe", "native_test_executable"),
            self.binding(self.output / "complete-native-loaded-pcm.f32le", "native_loaded_pcm"),
            self.binding(self.output / "complete-native-export.f32le", "native_export_pcm"),
            self.binding(self.config_path, "probe_configuration"),
            self.binding(self.workspace / "scratch/b2a/frozen-manifest.json", "frozen_manifest"),
            self.binding(
                self.workspace / self.config["loaded_identities_path"], "loaded_identities"
            ),
            self.binding(self.workspace / self.config["source_aliases_path"], "source_aliases"),
            self.binding(installed.parent / "current.json", "installation_pointer"),
            self.binding(configuration.manifest, "installation_manifest"),
            self.binding(configuration.interpreter, "worker_interpreter"),
            self.binding(configuration.script, "worker_script"),
            self.binding(configuration.checkpoint, "worker_checkpoint"),
            self.binding(installed / "uv.lock", "worker_lock"),
            self.binding(installed / "pyproject.toml", "worker_pyproject"),
            self.binding(installed / "environment.json", "worker_environment"),
        ]
        implementation = []
        implementation_sources = []
        for name in PRODUCER_FILES:
            original = self.repository / name
            retained = self.output / "producer-implementation" / name
            retained.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(original, retained)
            implementation.append(self.binding(retained, "producer_implementation"))
            implementation_sources.append({
                "original_path": original.relative_to(self.workspace).as_posix(),
                "retained_path": retained.relative_to(self.workspace).as_posix(),
                "sha256": sha256_file(retained),
                "bytes": retained.stat().st_size,
            })
        write_json(self.output / "producer-sources.json", implementation_sources)
        for suffix in ("rs", "py"):
            compiled = self.output / f"compiled-producer.{suffix}"
            original = (
                self.repository
                / f"rust/crates/looper/src/audio_engine/b2_native_candidate_probe.{suffix}"
            )
            assert sha256_file(compiled) == sha256_file(original), (
                "producer source changed after compilation"
            )
            files.append(self.binding(compiled, "compiled_producer"))
        files.append(self.binding(self.output / "producer-sources.json", "producer_sources"))
        pyds = list((self.repository / "src/flitzis_looper_audio").glob("*.pyd"))
        assert len(pyds) == 1
        self.runtime["embedded_python"] = {
            "version": sys.version,
            "executable": sys.executable,
            "base_prefix": sys.base_prefix,
        }
        retained_pyd = self.output / "installed-native-extension.pyd"
        shutil.copyfile(pyds[0], retained_pyd)
        self.runtime["original_native_test_executable"] = self.runtime["native_test_executable"]
        self.runtime["native_test_executable"] = self.binding(
            self.output / "native-test-executable.exe"
        )
        self.runtime["installed_native_extension"] = {
            "binding": self.binding(retained_pyd),
            "used_by_probe": False,
            "original_path": str(pyds[0]),
        }
        files.append(self.binding(retained_pyd, "installed_pyd"))
        worker_configuration = {
            name: self.binding(getattr(configuration, name))
            for name in ("interpreter", "script", "checkpoint", "manifest")
        }
        worker_configuration.update({
            "pointer": self.binding(installed.parent / "current.json"),
            "lock": self.binding(installed / "uv.lock"),
            "environment": self.binding(installed / "environment.json"),
            "pyproject": self.binding(installed / "pyproject.toml"),
            "scratch_dir": str(configuration.scratch_dir),
        })
        worker_configuration.update(
            model=asdict(configuration.model), worker_limits=asdict(WorkerLimits())
        )
        return {
            "schema_version": 1,
            "producer": "hardware-free-native-b2-v1",
            "track_id": self.track_id,
            "original_source_relative": self.track.source_relative,
            "actual_source_path": self.source.relative_to(self.workspace).as_posix(),
            "original_source_sha256": self.track.source_sha256,
            "original_source_bytes": self.track.source_bytes,
            "frozen_manifest": self.binding(self.workspace / "scratch/b2a/frozen-manifest.json"),
            "probe_config": self.binding(self.config_path),
            "native_source": {
                **self.source_proof,
                "native_metadata": admitted,
                "cold_manifest": self.binding(self.output / "cold-source-manifest.json"),
                "cached_original": self.binding(self.output / "native-original-snapshot.bin"),
            },
            "worker_configuration": worker_configuration,
            "runtime": self.runtime,
            "producer_implementation": [
                self.binding(self.output / "compiled-producer.rs"),
                self.binding(self.output / "compiled-producer.py"),
            ],
            "file_bindings": files + implementation,
            "finish_accepted": self.engine.native.finish_accepted,
            "diagnostic_only": True,
        }

    def publication_evidence(self, raw: str, request: Any) -> dict[str, Any]:
        published = decode_result(raw, request)
        worker = decode_response(
            (self.output / "worker-response.raw.json").read_bytes(), request, WorkerLimits()
        )
        component = self.adapter.component
        assert component is not None
        assert component.status == "ready"
        assert component.resources_released
        assert component.predictions is not None
        assert published.beat.predictions is not None
        identities = [
            array_identity(value)
            for value in (worker, component.predictions, published.beat.predictions)
        ]
        completions = [
            event for event in self.events if event["type"] == "offline_analysis_completed"
        ]
        assert len(completions) == 1
        assert completions[0]["id"] == request.identity.pad_id
        assert completions[0]["request_id"] == request.identity.request_id
        completed = decode_result(completions[0]["result_json"], request)
        assert completed.beat.predictions is not None
        identities.append(array_identity(completed.beat.predictions))
        assert all(value == identities[0] for value in identities)
        assert (self.output / "native-finish-input.raw.json").read_text(encoding="utf-8") == raw
        assert published.schema_version == completed.schema_version == 2
        rounded = (2 * request.pcm.frame_count * 22050 + request.pcm.sample_rate_hz) // (
            2 * request.pcm.sample_rate_hz
        )
        expected_frames = rounded // 441 + 1
        assert len(worker.beat_logits) == len(worker.downbeat_logits) == expected_frames
        return {
            "beat": asdict(component),
            "key": published.key,
            "completion_event_count": len(completions),
            "expected_full_frontend_frames": expected_frames,
            "prediction_counts": {name: identities[0][name]["count"] for name in ARRAYS},
            "prediction_arrays": identities[0],
            "final_schema_version": published.schema_version,
        }

    def retirement_evidence(self, request: Any, native: NativeRecorder) -> dict[str, Any]:
        assert self.adapter.retired.is_set()
        exit_codes = [process.poll() for process in self.processes]
        assert exit_codes == [0]
        remaining_pcm = [str(path) for path in self.jobs_directory.iterdir()]
        remaining_requests = [str(path) for path in self.worker_directory.iterdir()]
        assert not remaining_pcm
        assert not remaining_requests
        reservation = self.bridge.begin_offline_analysis(0)
        metadata = reservation.metadata()
        admitted = native.observations[0]["metadata"]
        assert metadata["request_id"] == request.identity.request_id + 1
        assert all(metadata[name] == admitted[name] for name in metadata if name != "request_id")
        reservation.abort_unstarted()
        # The abort is asynchronous; Rust waits for actual admission retirement.
        return {
            "worker_retired": self.adapter.retired.is_set(),
            "worker_exit_codes": exit_codes,
            "remaining_pcm_directories": remaining_pcm,
            "remaining_request_directories": remaining_requests,
            "native_reservation_after_done": metadata,
        }

    def finish(self) -> None:
        self.poll()
        self.start_patch.stop()
        self.run_patch.stop()
        native = self.engine.native
        assert native is not None
        native.observe("job_done")
        assert self.job.snapshot().stage == "finished"
        assert native.finish_accepted is True
        request = self.adapter.request
        assert request is not None
        raw = self.job.snapshot().result_json
        assert isinstance(raw, str)
        (self.output / "result-envelope.raw.json").write_text(raw, encoding="utf-8")
        publication = self.publication_evidence(raw, request)
        retirement = self.retirement_evidence(request, native)
        admitted = native.observations[0]["metadata"]
        pyd = self.provenance(admitted)
        write_json(self.output / "producer-provenance.json", pyd)
        summary = {
            "schema_version": 1,
            "measurement_slice": "b2-fresh-hardware-free-native-lineage",
            "track_id": self.track_id,
            "mode": "ready",
            "source": str(self.source),
            "source_sha256": self.track.source_sha256,
            "source_bytes": self.track.source_bytes,
            "native_extension_sha256": (
                pyd["runtime"]["installed_native_extension"]["binding"]["sha256"]
            ),
            "native_extension_scope": (
                "installed PYD on disk; native harness executes retained test EXE"
            ),
            "loaded_shape_rate_channels_frames": [
                request.pcm.sample_rate_hz,
                admitted["channels"],
                request.pcm.frame_count,
            ],
            "loaded_duration_seconds": request.pcm.frame_count / request.pcm.sample_rate_hz,
            "model": asdict(request.model),
            "export": self.adapter.export,
            **publication,
            **retirement,
            "wire_outcome": self.wire_outcome,
            "job_wall_seconds": time.monotonic() - self.started,
            "worker_limits": asdict(WorkerLimits()),
            "native_observations": native.observations,
            "limitations": [
                (
                    "Hardware-free engineering lineage only; no musical reference, score, hearing "
                    "or default acceptance."
                ),
                (
                    "Native staging capacities exclude model/FFT/CQT allocations and are "
                    "not process RSS measurements."
                ),
            ],
        }
        write_json(self.output / "summary.json", summary)
