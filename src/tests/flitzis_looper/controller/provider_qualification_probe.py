"""Explicit stock BS CUDA/CPU learning and genuine device-fault qualification.

No device is opened. The original Loader, stopped-target stem controller, source
guards, Native admission, owned children and final calibration promotion run.
Only streamless geometry and the recorded CPU/fault request seams are fixtures.
"""

import gc
import hashlib
import json
import math
import os
import time
from contextlib import ExitStack
from dataclasses import asdict, replace
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import patch
from weakref import ref

from flitzis_looper.analysis import process
from flitzis_looper.controller import stem_process
from flitzis_looper.controller.audio_output import AudioOutputController
from flitzis_looper.controller.audio_workload import AudioWorkloadCapture
from flitzis_looper.controller.bs_roformer_generation import BsRoformerStemGenerationBackend
from flitzis_looper.controller.stem_artifacts import read_pcm16_wav
from flitzis_looper.controller.stem_generation import demucs_cache_environment, run_command
from flitzis_looper.controller.stem_identity import request_fingerprint
from flitzis_looper.controller.stem_separators import SelectedStemGenerationBackend
from flitzis_looper.inference_provider import is_cuda_failure
from flitzis_looper.models import STEM_KINDS
from flitzis_looper.pcm16_wav import PcmWavReader
from tests.flitzis_looper.controller.inference_overlap_probe import model_call
from tests.flitzis_looper.controller.stem_batch_216_native_probe import _ShapeAudio
from tests.flitzis_looper.controller.stem_batch_neural_native_probe import _OwnedProcessObservation

if TYPE_CHECKING:
    from types import FrameType

    from flitzis_looper.controller.stem_generation import (
        CommandResult,
        StemGenerationRequest,
        StemGenerationResult,
        StemProgressCallback,
    )
    from tests.flitzis_looper.controller.held_lifecycle_probe import HeldLifecycleProbe


class _Observation(_OwnedProcessObservation):
    def __init__(self) -> None:
        self.commands: list[dict[str, object]] = []
        self.reader_ends: list[dict[str, object]] = []
        super().__init__()

    def _profile(self, frame: FrameType, event: str, argument: object) -> None:
        super()._profile(frame, event, argument)
        if event == "return" and frame.f_code is process.run_owned_command.__code__:
            owned = frame.f_locals.get("owned")
            if owned is not None:
                self.reader_ends.append({
                    "pid": owned.process.pid,
                    "retired": owned.retired.is_set(),
                    "reader_count": len(owned.readers),
                    "readers_ended": all(not reader.thread.is_alive() for reader in owned.readers),
                    "returned_ns": time.perf_counter_ns(),
                })
        if (
            event == "return"
            and frame.f_code is stem_process._run_admitted.__code__
            and argument is not None
        ):
            args = frame.f_locals["args"]
            self.commands.append({
                "args": tuple(args),
                "returned_ns": time.perf_counter_ns(),
                "returncode": argument.returncode,
                "reason": argument.reason,
                "stdout": argument.stdout.decode("utf-8", errors="replace"),
                "stderr": argument.stderr.decode("utf-8", errors="replace"),
                "pending_after_original_return": self.pending(),
            })


def _checkpoint(path: Path, value: object) -> None:
    assert not path.exists()
    temporary = path.with_suffix(".tmp")
    with temporary.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, indent=2)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)


class ProviderQualificationState:
    """Retain only pure backend policy and detached data across Native fixtures."""

    def __init__(self) -> None:
        self.selected = SelectedStemGenerationBackend()
        self.paired = self.selected._backends["bs-roformer:musdb18hq"]
        self.requests: list[StemGenerationRequest] = []
        self.results: list[StemGenerationResult] = []
        self.runs: list[dict[str, object]] = []
        self.paired_costs: dict[str, object] = {}
        self.policy_observation: dict[str, object] = {}
        self.root = Path(os.environ["FLITZIS_INFERENCE_PROFILE_ROOT"])
        self._owner_refs = {}
        self.worker_ends: dict[str, object] = {}

    def remember_owners(self, **owners: object) -> None:
        """Watch the actual Python Native holders without extending their lives."""
        assert all(owner() is None for owner in self._owner_refs.values())
        self._owner_refs = {name: ref(owner) for name, owner in owners.items()}

    def assert_fixture_owners_ended(self) -> dict[str, bool]:
        """Collect cycles only after Rust drops its original fixture references."""
        gc.collect()
        dead = {name: owner() is None for name, owner in self._owner_refs.items()}
        assert all(dead.values()), dead
        return dead

    def fixture_ended(self, mode: str, registry_report: str) -> None:
        """Supplement a provisional run after Rust drops its actual Native owners."""
        if mode == "preflight":
            assert not self.runs
            assert not self.requests
            assert not self.results
            assert not self.paired._provider_selector._costs
        else:
            assert self.runs[-1]["mode"] == mode
        dead = self.assert_fixture_owners_ended()
        registry = json.loads(registry_report)
        assert registry["settled"]
        lifecycle = {
            "mode": mode,
            "app_workers_joined": True,
            "cold_jobs": [0, 0, 0],
            "native_fixture_dropped_under_gil": True,
            "actual_known_python_native_holders_collected": dead,
            "original_asset_registry": registry,
            "callback_destructor_observed": True,
            "original_files_fully_rehashed_after_fixture_end": True,
            "saved_file_only_claims": "original persisted claims remain needed until EXE END",
            "original_files_exclusive_proof": "pending exact enclosing EXE and Job END",
            "enclosing_test_exe_and_outer_job_end": "pending final parent proof",
            "headless_output_unacquired_before_and_after_close": True,
            "original_stem_executor": self.worker_ends[mode],
        }
        _checkpoint(self.root / f"{mode}-fixture-end.json", lifecycle)
        if mode != "preflight":
            self.runs[-1]["fixture_end"] = lifecycle

    def finish(self) -> str:
        """Report observations after all three independent fixture owners ended."""
        assert len(self.runs) == 3
        assert all(isinstance(run["fixture_end"], dict) for run in self.runs)
        assert self.policy_observation
        commands = [
            row
            for run in self.runs
            for row in run["original_commands"]
            if row["args"][1:3] == ("-m", "flitzis_bs_roformer")
        ]
        calls = [model_call(row["stdout"].encode()) for row in commands if row["returncode"] == 0]
        assert [call["device"] for call in calls] == ["cuda", "cpu", "cpu"]
        return json.dumps({
            "runs": self.runs,
            "paired_costs": self.paired_costs,
            "paired_policy_observation": self.policy_observation,
            "model_calls": calls,
            "limits": "n=1 functionality in three sequential independent Native fixtures; "
            "identical source bytes/config/producer, distinct locator/content owners; "
            "CPU intent is a test backend-request seam; no speed/deadline/quality/physical "
            "acceptance or whole Native replacement; outer EXE/Job END still needs parent proof",
        })


class _Backend:
    def __init__(self, owner: ProviderQualification) -> None:
        self.owner = owner
        self.selected = owner.shared.selected
        self.paired = owner.shared.paired
        self.mode = owner.mode
        self.requests: list[StemGenerationRequest] = []
        self.jobs = []
        self.results: list[StemGenerationResult] = []
        self.receipts: list[dict[str, object]] = []
        self.accepted: list[dict[str, object]] = []

    def generate(
        self, request: StemGenerationRequest, progress: StemProgressCallback
    ) -> StemGenerationResult:
        """Preserve public AUTO capture; explicitly declare the CPU backend seam."""
        assert request.sample_id == 6
        assert request.device_policy == "auto"
        assert request.separator == "bs-roformer:musdb18hq"
        assert request.producer_identity
        assert request.encoder_contract
        assert request.encoder_executable is not None
        assert request.python_source_roots
        if self.owner.shared.requests:
            original = self.owner.shared.requests[0]
            assert request_fingerprint(request, request.producer_identity) == request_fingerprint(
                original, original.producer_identity
            )
        job = next(
            job
            for job in tuple(self.owner.app.stems._jobs.values())
            if job.resolved_request is request
        )
        assert job.processing_files is not None
        assert job.producer_identity == request.producer_identity
        job.processing_files.verify()
        witnesses = job.processing_files.witnesses()
        for name, digest in self.owner.vendor_digests.items():
            path = self.owner.vendor_root / "flitzis_bs_roformer" / name
            assert [d for p, d, _size in witnesses if Path(p).samefile(path)] == [digest]
        self.requests.append(request)
        self.jobs.append(job)
        executed = replace(request, device_policy="cpu") if self.mode == "cpu" else request
        result = self.selected.generate(executed, progress)
        assert result.decoder_verified
        assert result.artifact_count == 5
        assert result.execution is not None
        assert result.provider_calibration is not None
        assert (result.execution.intra_op_threads, result.execution.inter_op_threads) == (1, 1)
        self.results.append(result)
        self.receipts.append({
            "mode": self.mode,
            "captured_policy": request.device_policy,
            "executed_policy": executed.device_policy,
            "source_version": request.source_version,
            "original_bytes": request.original_bytes,
            "target_shape": asdict(request.target_shape),
            "producer_identity": request.producer_identity,
            "processing_files": witnesses,
            "result": asdict(result),
        })
        return result

    def accept_calibration(
        self, request: StemGenerationRequest, result: StemGenerationResult
    ) -> None:
        """Observe real final promotion after original source and reader settlement."""
        job = self.jobs[-1]
        assert request is self.requests[-1]
        assert result is self.results[-1]
        assert job.phase == "settled"
        assert job.source is None
        assert job.generation_fingerprint is not None
        assert not job.cancel_event.is_set()
        assert not self.owner.app.stems._stem_playback_active.is_set()
        assert not self.owner.meter.pending()
        assert len(self.owner.meter.reader_ends) == len(self.owner.meter.commands)
        assert all(row["retired"] and row["readers_ended"] for row in self.owner.meter.reader_ends)
        assert job.processing_files is not None
        job.processing_files.verify()
        self.selected.accept_calibration(request, result)
        if self.mode == "cpu":
            self.owner.capture_policy(request, job.cancel_event)
            job.processing_files.verify()
        self.accepted.append({
            "mode": self.mode,
            "phase": job.phase,
            "source_reader_released": job.source is None,
            "generation_fingerprint": job.generation_fingerprint,
            "pending_process_scopes": self.owner.meter.pending(),
        })


class ProviderQualification:
    """One public stopped-target generation with fresh fixture-specific owners."""

    def __init__(
        self,
        app: HeldLifecycleProbe,
        *,
        shape_audio: object,
        shared: ProviderQualificationState,
        mode: str,
    ) -> None:
        assert not app.session.active_sample_ids
        assert not app.session.paused_sample_ids
        assert os.environ.get("CUDA_VISIBLE_DEVICES") != "-1"
        # The older streamless fixture injects an existing Engine. Complete the
        # current App's real idle shutdown owners without starting output.
        app._init_audio_output_state()
        app.audio_workload = AudioWorkloadCapture(app.project, app.session, app._audio)
        app.audio_output = AudioOutputController(app)
        assert app.audio_output._operation is None
        assert not app.audio_output.available
        assert not app._runtime_activated
        self.shared = shared
        self.mode = mode
        self.app = app
        self.vendor_root = Path(os.environ["FLITZIS_VENDOR_OVERLAY"]).resolve(strict=True)
        self.vendor_digests = json.loads(os.environ["FLITZIS_INFERENCE_VENDOR_DIGESTS"])
        assert len(self.vendor_digests) == 8
        self.environment = ExitStack()
        self.environment.enter_context(
            patch.dict(os.environ, {"FLITZIS_BS_MODEL_CALL_DIAGNOSTICS": "1"})
        )
        self.meter = _Observation()
        self.backend = _Backend(self)
        self.previous_backend = app.stems._stem_backend
        self.previous_audio = app.stems._audio
        app.stems._audio = _ShapeAudio(self.previous_audio, shape_audio)
        app.stems._stem_backend = self.backend
        app.project.stem_separator = "bs-roformer:musdb18hq"
        self.source = (app.project.sample_paths[6], app.project.pad_content[6])
        self.closed = False
        self.fault_commands: list[dict[str, object]] = []
        shared.remember_owners(
            app=app,
            shape_wrapper=app.stems._audio,
            backend_wrapper=self.backend,
            observer=self.meter,
            run=self,
            audio_workload=app.audio_workload,
            audio_output=app.audio_output,
            audio_output_runtime=app.audio_output_runtime,
        )

    def preflight_files(self) -> list[tuple[str, str]]:
        """Prove the complete original fixture lifecycle before any model attempt."""
        assert self.mode == "preflight"
        assert not self.backend.requests
        assert not self.backend.results
        assert not self.backend.accepted
        assert not self.meter.commands
        path = Path(self.app.project.sample_paths[6]).resolve(strict=True)
        files = [(str(path), hashlib.sha256(path.read_bytes()).hexdigest())]
        _checkpoint(
            self.shared.root / "preflight-source-provisional.json",
            {
                "files": files,
                "model_inference_started": False,
                "fixture_end": "pending actual original shutdown/weakref/exclusive proof",
            },
        )
        return files

    def start(self, mode: str) -> None:
        """Generate once in a fresh original project, without completed-set reuse."""
        assert mode == ("cuda", "cpu", "fault")[len(self.shared.runs)]
        assert mode == self.mode
        assert not self.app.session.active_sample_ids
        assert not self.meter.pending()
        assert not self.app.stems._jobs
        assert (self.app.project.sample_paths[6], self.app.project.pad_content[6]) == self.source
        assert self.app.project.stem_cache[6] is None
        self.backend.mode = mode
        if mode == "fault":
            self.backend.selected = SelectedStemGenerationBackend({
                "bs-roformer:musdb18hq": BsRoformerStemGenerationBackend(
                    command_runner=self._fault_command
                )
            })
        assert self.app.stems.generate_stems_async(6)

    def poll(self) -> bool:
        """Let original admission, publication and own-ACK readers settle."""
        self.app.stems.on_frame_render()
        self.app._assets.retry_retirements()
        self.meter.sample()
        assert not self.app.session.stem_generation_errors, self.app.session.stem_generation_errors
        selection = self.app.stems._committed_residency._selections.get(6)
        return bool(
            len(self.backend.results) == 1
            and len(self.backend.accepted) == 1
            and not self.app.stems._jobs
            and not self.app.stems._stem_preparation_count()
            and not self.app.stems._disk_selection_settlements
            and not self.meter.pending()
            and selection is not None
            and selection.ticket.publication_status() == "accepted"
            and selection.ticket.is_current()
        )

    def finish_run(self) -> list[tuple[str, str]]:
        """Verify full outputs and actual learned rows, without calibration inference."""
        assert self.poll()
        result = self.backend.results[-1]
        execution, calibration = result.execution, result.provider_calibration
        assert execution is not None
        assert calibration is not None
        assert calibration.plan.key is not None
        mode = self.backend.mode
        assert result.device == ("cuda" if mode == "cuda" else "cpu")
        assert result.cpu_fallback == (mode == "fault")
        if mode == "cuda":
            assert execution.cuda_device_identity == calibration.plan.key[2]
            assert execution.cuda_peak_bytes is not None
            assert execution.cuda_peak_bytes > 0
        entry = self.app.project.stem_cache[6]
        assert entry is not None
        assert entry.fingerprint_schema == "effective-stem-v3"
        assert entry.producer_identity == self.backend.requests[-1].producer_identity
        outputs = self._outputs(Path(entry.cache_dir))
        assert not self.meter.errors
        assert not self.meter.pending()
        assert self.meter.started == self.meter.closed > 0
        selector = self.backend.paired._provider_selector
        key = calibration.plan.key
        if self.shared.results:
            assert key == self.shared.results[0].provider_calibration.plan.key
        costs = {device: asdict(cost) for device, cost in selector._costs[key].items()}
        if mode == "cpu":
            assert set(costs) == {"cpu", "cuda"}
            for index, device in enumerate(("cuda", "cpu")):
                observed = [*self.shared.results, result][index].provider_calibration
                assert costs[device]["seconds"] == observed.seconds
                assert costs[device]["observed_at"] == observed.observed_at
            self.shared.paired_costs = costs
            assert self.shared.policy_observation
        if mode == "fault":
            assert costs == self.shared.paired_costs
            fault = self.backend.selected._backends["bs-roformer:musdb18hq"]
            assert not fault._provider_selector._costs
            assert len(self.fault_commands) == 2
            assert self.fault_commands[0]["genuine_cuda_failure"]
            assert self.fault_commands[1]["device"] == "cpu"
        original = self.backend.requests[0]
        digest = hashlib.sha256(original.source_path.read_bytes()).hexdigest()
        files = [(str(original.source_path), digest)]
        files.extend((value["path"], value["digest"]) for value in outputs.values())
        record = {
            "mode": mode,
            "outputs": outputs,
            "receipt": self.backend.receipts[-1],
            "final_accept": self.backend.accepted[-1],
            "learned_costs": costs,
            "processes": self.meter.report(),
            "original_commands": self.meter.commands,
            "actual_reader_ends": self.meter.reader_ends,
            "fault_commands": self.fault_commands,
            "original_file_proofs": files,
            "fixture_end": "pending original app/Native/callback drop and exclusive file proof",
        }
        _checkpoint(self.shared.root / f"{mode}-run-provisional.json", record)
        self.shared.runs.append(record)
        self.shared.requests.append(original)
        self.shared.results.append(result)
        return files

    def capture_policy(self, request: StemGenerationRequest, cancel: object) -> None:
        """Inspect paired costs on the existing worker with fresh original headroom."""
        paired = self.backend.paired
        with stem_process.stem_command_scope(cancel, resources=self.app.stems._stem_resources):
            plan = paired._provider_plan(request, demucs_cache_environment(request.model_cache_dir))
        selector = paired._provider_selector
        capability, captured_at = selector._last_capability
        now = time.monotonic()
        assert 0 <= now - captured_at < 600
        assert plan.key == self.shared.results[0].provider_calibration.plan.key
        costs = selector._costs[plan.key]
        assert set(costs) == {"cpu", "cuda"}
        assert all(0 <= now - cost.observed_at < 600 for cost in costs.values())
        cpu_preferred = costs["cpu"].seconds <= costs["cuda"].seconds * 0.85
        headroom_veto = capability.free_bytes < costs["cuda"].cuda_peak_bytes + (256 << 20)
        assert plan.devices == (("cpu",) if cpu_preferred or headroom_veto else ("cuda", "cpu"))
        self.shared.policy_observation = {
            "plan": asdict(plan),
            "capability": asdict(capability),
            "capability_observation_age_seconds": now - captured_at,
            "scope": "fresh original nonneural capability query after paired final acceptance; "
            "same existing stems worker and original resources, no new executor/inference",
        }

    @staticmethod
    def _outputs(directory: Path) -> dict[str, object]:
        outputs = {}
        for name in STEM_KINDS:
            path = directory / f"{name}.wav"
            with PcmWavReader(path) as reader:
                assert (reader.getnframes(), reader.getframerate(), reader.getnchannels()) == (
                    144_000,
                    48_000,
                    2,
                )
                digest = reader.verify()
            pcm = read_pcm16_wav(path)
            assert (pcm.frame_count, pcm.sample_rate_hz, pcm.channels) == (144_000, 48_000, 2)
            assert all(math.isfinite(value) for value in pcm.samples)
            outputs[name] = {"path": str(path.resolve()), "frames": 144_000, "digest": digest}
        return outputs

    def _fault_command(self, args: list[str], *, cwd: Path, env: dict[str, str]) -> CommandResult:
        assert not self.meter.pending(), "previous actual tree/readers must end before launch"
        if args[1:3] != ["-m", "flitzis_bs_roformer"]:
            return run_command(args, cwd=cwd, env=env)
        device = args[args.index("--device") + 1]
        launched = time.perf_counter_ns()
        command = args
        if device == "cuda":
            shim = Path(os.environ["FLITZIS_BS_ALLOCATOR_FAULT_SHIM"]).resolve(strict=True)
            command = [args[0], str(shim), *args[3:]]
        result = run_command(command, cwd=cwd, env=env)
        reader_end = self.meter.reader_ends[-1]
        assert reader_end["retired"]
        assert reader_end["readers_ended"]
        identities = [row for row in self.meter._rows.values() if row.pid == reader_end["pid"]]
        assert len(identities) == 1
        identity = (identities[0].pid, identities[0].creation_ticks)
        genuine = is_cuda_failure(result.returncode, result.stdout)
        if device == "cuda":
            assert genuine, result
            assert "out of memory" in result.stderr.lower(), result
        else:
            assert result.returncode == 0
            assert not genuine
            assert len(self.fault_commands) == 1
            assert self.fault_commands[0]["returned_ns"] < launched
            assert self.fault_commands[0]["identity"] != identity
        assert not self.meter.pending()
        self.fault_commands.append({
            "device": device,
            "launched_ns": launched,
            "returned_ns": time.perf_counter_ns(),
            "genuine_cuda_failure": genuine,
            "identity": identity,
            "reader_end": reader_end,
            "returncode": result.returncode,
            "stdout": result.stdout,
            "stderr": result.stderr,
        })
        return result

    def close(self) -> None:
        """Join original workers before restoring observer and fixture seams."""
        if self.closed:
            return
        try:
            pool = self.app.stems._stem_worker_pool
            assert pool is not None
            self.app.stems.shut_down()
            # The normal UI close is nonblocking. This explicit offline profile
            # joins that same original executor before certifying owner END.
            pool._executor.shutdown(wait=True)
            threads = tuple(pool._executor._threads)
            assert all(not thread.is_alive() for thread in threads)
            assert pool.idle
            self.shared.worker_ends[self.mode] = {
                "pool_idle": pool.idle,
                "threads": [
                    {
                        "ident": thread.ident,
                        "native_id": thread.native_id,
                        "name": thread.name,
                        "alive": thread.is_alive(),
                    }
                    for thread in threads
                ],
            }
            assert not self.meter.pending()
            self.meter.close()
            assert all(job.phase == "disposed" for job in self.backend.jobs)
            assert all(job.processing_files is None for job in self.backend.jobs)
            assert all(job.source is None for job in self.backend.jobs)
        finally:
            self.app.stems._stem_backend = self.previous_backend
            self.app.stems._audio = self.previous_audio
            self.environment.close()
            self.backend.jobs.clear()
            self.backend.owner = None
            self.backend = None
            self.app = None
            self.previous_backend = None
            self.previous_audio = None
            self.meter = None
            self.closed = True
