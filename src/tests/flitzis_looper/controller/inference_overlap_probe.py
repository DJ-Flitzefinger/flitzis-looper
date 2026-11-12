"""Stock BS inference on a stopped target beside real software-rendered PADs."""

import json
import math
import os
import time
from contextlib import ExitStack
from dataclasses import asdict
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import patch

from flitzis_looper.controller import stem_process
from flitzis_looper.controller.stem_artifacts import read_pcm16_wav
from flitzis_looper.controller.stem_separators import SelectedStemGenerationBackend
from flitzis_looper.models import STEM_KINDS
from flitzis_looper.pcm16_wav import PcmWavReader
from tests.flitzis_looper.controller.stem_batch_216_native_probe import _ShapeAudio
from tests.flitzis_looper.controller.stem_batch_neural_native_probe import _OwnedProcessObservation

if TYPE_CHECKING:
    from types import FrameType

    from flitzis_looper.analysis.process import OwnedCommandResult
    from flitzis_looper.controller.stem_generation import (
        StemGenerationRequest,
        StemGenerationResult,
        StemProgressCallback,
    )
    from tests.flitzis_looper.controller.held_lifecycle_probe import HeldLifecycleProbe

PREFIX = b"FLITZIS_BS_MODEL_CALL_V1:"
_FIELDS = {"pid", "device", "entered_ns", "returned_ns", "cpu_before_ns", "cpu_after_ns"}


def _unique_fields(pairs: list[tuple[str, object]]) -> dict[str, object]:
    assert len({name for name, _ in pairs}) == len(pairs), "duplicate diagnostic field"
    return dict(pairs)


def model_call(stdout: bytes) -> dict[str, int | str]:
    """Require one complete scalar diagnostic, independently of execution receipts."""
    lines = [line for line in stdout.splitlines(keepends=True) if line.startswith(PREFIX)]
    assert len(lines) == 1, "missing/duplicate model call"
    assert len(lines[0]) < 512, "oversized model call"
    assert lines[0].endswith(b"\n"), "unterminated model-call record"
    value = json.loads(lines[0][len(PREFIX) :], object_pairs_hook=_unique_fields)
    assert isinstance(value, dict)
    assert set(value) == _FIELDS
    assert isinstance(value["device"], str)
    assert value["device"] in {"cpu", "cuda"}
    assert all(type(value[key]) is int and value[key] >= 0 for key in _FIELDS - {"device"})
    assert value["pid"] > 0
    assert value["entered_ns"] < value["returned_ns"]
    assert value["cpu_before_ns"] < value["cpu_after_ns"]
    return value


class _CallObservation(_OwnedProcessObservation):
    def __init__(self) -> None:
        self.calls: list[dict[str, int | str]] = []
        self.command_results: list[dict[str, object]] = []
        self.diagnostic_errors: list[str] = []
        self.commit_limits: dict[int, int | None] = {}
        super().__init__()

    def _profile(self, frame: FrameType, event: str, argument: object) -> None:
        super()._profile(frame, event, argument)
        if (
            event == "return"
            and frame.f_code is stem_process._run_owned.__code__
            and argument is not None
            and frame.f_locals["args"][1:3] == ["-m", "flitzis_bs_roformer"]
        ):
            # Observe the original command result before its diagnostic text is
            # compacted. This does not replace a command, stream or supervisor.
            self.returned(argument)

    def returned(self, result: OwnedCommandResult) -> None:
        """Observe a return without masking the original failure or fallback."""
        self.command_results.append({"returncode": result.returncode, "reason": result.reason})
        if result.reason == "ok" and result.returncode == 0:
            try:
                call = model_call(result.stdout)
            except (AssertionError, TypeError, ValueError) as error:
                self.diagnostic_errors.append(f"{type(error).__name__}: {error}")
            else:
                self.calls.append(call)

    def child_sample(self) -> tuple[int, int, int, int, int] | None:
        """Query only the retained actual root of a nonbusy original BS scope."""
        with self._lock:
            for scope, command in tuple(self._active.items()):
                if command != "flitzis_bs_roformer" or self._inflight.get(scope, 0):
                    continue
                job = scope._job
                if job is None or (retained := job._members.get(scope.pid)) is None:
                    continue
                self.commit_limits[scope.pid] = job._commit_limit_bytes
                began = time.perf_counter_ns()
                sample = self._member(scope.pid, retained, command)
                ended = time.perf_counter_ns()
                if sample is not None:
                    return (
                        began,
                        ended,
                        scope.pid,
                        sample["creation_ticks"],
                        sample["kernel_cpu_ns"] + sample["user_cpu_ns"],
                    )
        return None


class _Backend:
    def __init__(self, owner: InferenceOverlap) -> None:
        self.owner = owner
        self.delegate = SelectedStemGenerationBackend()
        self.requests: list[dict[str, object]] = []
        self.results: list[dict[str, object]] = []
        self.calibrations = 0

    def generate(
        self, request: StemGenerationRequest, progress: StemProgressCallback
    ) -> StemGenerationResult:
        """Delegate the original request and preserve all source/producer contracts."""
        assert request.sample_id == 6
        assert request.device_policy == "auto"
        assert request.separator == "bs-roformer:musdb18hq"
        assert request.producer_identity
        assert request.python_source_roots
        assert request.encoder_executable is not None
        assert request.encoder_contract
        job = next(
            job
            for job in tuple(self.owner.app.stems._jobs.values())
            if job.resolved_request is request
        )
        assert job.processing_files is not None
        assert job.producer_identity == request.producer_identity
        job.processing_files.verify()
        witnesses = job.processing_files.witnesses()
        assert self.owner.vendor_root in request.python_source_roots
        for name, digest in self.owner.vendor_digests.items():
            original = self.owner.vendor_root / "flitzis_bs_roformer" / name
            matched = [value for path, value, _size in witnesses if Path(path).samefile(original)]
            assert matched == [digest]
        self.requests.append({
            "sample_id": request.sample_id,
            "source_version": request.source_version,
            "producer_identity": request.producer_identity,
            "target_shape": asdict(request.target_shape),
            "original_bytes": request.original_bytes,
            "processing_files": witnesses,
        })
        result = self.delegate.generate(request, progress)
        assert result.decoder_verified
        assert result.artifact_count == 5
        assert result.execution is not None
        assert not result.cpu_fallback
        self.results.append(asdict(result))
        return result

    def accept_calibration(
        self, request: StemGenerationRequest, result: StemGenerationResult
    ) -> None:
        """Observe the original playback exclusion without changing its decision."""
        self.calibrations += 1
        self.delegate.accept_calibration(request, result)


class InferenceOverlap:
    """Use the existing App owners; no additional executor or inference route."""

    def __init__(self, app: HeldLifecycleProbe, *, cpu_only: bool, shape_audio: object) -> None:
        self.app = app
        self.vendor_root = Path(os.environ["FLITZIS_VENDOR_OVERLAY"]).resolve(strict=True)
        self.vendor_digests = json.loads(os.environ["FLITZIS_INFERENCE_VENDOR_DIGESTS"])
        assert isinstance(self.vendor_digests, dict)
        assert len(self.vendor_digests) == 8
        self.environment = ExitStack()
        settings = {"FLITZIS_BS_MODEL_CALL_DIAGNOSTICS": "1"}
        if cpu_only:
            settings["CUDA_VISIBLE_DEVICES"] = "-1"
        self.environment.enter_context(patch.dict(os.environ, settings))
        self.cpu_only = cpu_only
        self.meter = _CallObservation()
        self.backend = _Backend(self)
        self.previous_backend = app.stems._stem_backend
        # The device-free fixture has no stream_handle. Reuse the established
        # shape-only seam on this same engine; every authority still delegates.
        self.previous_audio = app.stems._audio
        app.stems._audio = _ShapeAudio(self.previous_audio, shape_audio)
        app.stems._stem_backend = self.backend
        app.project.stem_separator = "bs-roformer:musdb18hq"
        self.periods: list[tuple[object, ...]] = []
        self.closed = False

    def start(self) -> None:
        """Request one stopped target through the ordinary public controller."""
        assert self.app.session.active_sample_ids == set(range(6))
        assert not self.app.session.paused_sample_ids
        assert self.app.stems.generate_stems_async(6)

    def poll(self) -> bool:
        """Advance the ordinary original admission and delivery path."""
        self.app.stems.on_frame_render()
        self.app._assets.retry_retirements()
        self.meter.sample()
        assert not self.app.session.stem_generation_errors, {
            "errors": self.app.session.stem_generation_errors,
            "commands": self.meter.command_results,
        }
        selection = self.app.stems._committed_residency._selections.get(6)
        return bool(
            self.backend.results
            and not self.app.stems._jobs
            and not self.app.stems._stem_preparation_count()
            and not self.app.stems._disk_selection_settlements
            and not self.meter.pending()
            and selection is not None
            and selection.ticket.publication_status() == "accepted"
            and selection.ticket.is_current()
        )

    def sample(self) -> tuple[int, int, int, int, int]:
        """Return actual identity/time/CPU scalars for the enclosing render."""
        return self.meter.child_sample() or (
            time.perf_counter_ns(),
            time.perf_counter_ns(),
            0,
            0,
            0,
        )

    def period(
        self, frame: int, entered: int, returned: int, before: object, after: object
    ) -> None:
        """Retain a declared finite history; no resource-settlement claim uses it."""
        assert len(self.periods) < 56_250  # At most600 seconds of paced512 at48kHz.
        self.periods.append((frame, entered, returned, before, after))

    def finish(self) -> str:
        """Verify actual-call overlap, ordinary own publication and all five outputs."""
        assert self.poll()
        assert not self.meter.diagnostic_errors, self.meter.diagnostic_errors
        assert len(self.backend.requests) == len(self.backend.results) == len(self.meter.calls) == 1
        assert self.backend.calibrations == 0
        call = self.meter.calls[0]
        result = self.backend.results[0]
        assert call["device"] == result["device"]
        assert call["device"] == ("cpu" if self.cpu_only else "cuda")
        witnesses = []
        for index, (left, right) in enumerate(zip(self.periods, self.periods[1:], strict=False)):
            assert right[0] == left[0] + 512
            before, after = left[3], right[4]
            if before[2] == 0 or after[2] == 0:
                continue
            if (
                call["entered_ns"] <= before[0] <= left[1]
                and right[2] <= after[1] <= call["returned_ns"]
                and before[2] == after[2] == call["pid"]
                and before[3] == after[3]
                and after[4] > before[4]
            ):
                witnesses.append((index, left, right))
        assert witnesses, {"call": call, "periods": len(self.periods)}
        assert self.meter.commit_limits[call["pid"]] is None
        assert any(
            row.pid == call["pid"]
            and row.creation_ticks == witnesses[0][1][3][3]
            and row.command == "flitzis_bs_roformer"
            for row in self.meter._rows.values()
        )
        entry = self.app.project.stem_cache[6]
        selection = self.app.stems._committed_residency._selections[6]
        assert entry is not None
        assert entry.fingerprint_schema == "effective-stem-v3"
        assert entry.producer_identity == self.backend.requests[0]["producer_identity"]
        assert selection.content_id == self.app.project.pad_content[6].instance_id
        assert selection.source_path == self.app.project.sample_paths[6]
        outputs = {}
        for name in STEM_KINDS:
            path = Path(entry.cache_dir) / f"{name}.wav"
            with PcmWavReader(path) as reader:
                assert (reader.getnframes(), reader.getframerate(), reader.getnchannels()) == (
                    3 * 48_000,
                    48_000,
                    2,
                )
                digest = reader.verify()
            pcm = read_pcm16_wav(path)
            assert (pcm.frame_count, pcm.sample_rate_hz, pcm.channels) == (144_000, 48_000, 2)
            assert all(math.isfinite(value) for value in pcm.samples)
            outputs[name] = {"frames": 3 * 48_000, "digest": digest, "path": str(path.resolve())}
        metrics = self.meter.report()
        assert not self.meter.errors
        assert self.meter.started == self.meter.closed > 0
        report = {
            "cpu_only": self.cpu_only,
            "model_call": call,
            "periods": len(self.periods),
            "witness_count": len(witnesses),
            "first_witness": witnesses[0],
            "outputs": outputs,
            "backend_requests": self.backend.requests,
            "backend_results": self.backend.results,
            "processes": metrics,
            "observed_original_job_commit_limits": self.meter.commit_limits,
            "original_command_results": self.meter.command_results,
            "checked_vendor_digests": self.vendor_digests,
            "limits": "paced software rendering; CPU counters include runtime work inside host "
            "model-call bracket; CUDA host return is not kernel completion/occupancy/duration; "
            "no device deadline/quality/Human/wholeNative bound or complete task16 claim",
        }
        return json.dumps(report)

    def close(self) -> None:
        """Join original workers before observer/environment and fixture retirement."""
        if self.closed:
            return
        try:
            self.app.stems.shut_down()
            assert not self.meter.pending()
            self.meter.close()
        finally:
            self.app.stems._stem_backend = self.previous_backend
            self.app.stems._audio = self.previous_audio
            self.environment.close()
            self.closed = True
