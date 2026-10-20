"""Actual stock neural batch/Config/own-ACK profile, without a device or voices.

Only the headless warning envelope and streamless shape accessor are fixtures.
Producer capture, executable lineage, model files, backend commands, cancellation,
artifact verification and Native admission remain the production implementations.
"""

import ctypes
import json
import os
import threading
import time
from contextlib import ExitStack
from ctypes import wintypes
from dataclasses import asdict, dataclass, field
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING, cast
from unittest.mock import patch
from weakref import WeakKeyDictionary

from flitzis_looper.analysis import process
from flitzis_looper.controller.asset_lifecycle import ProjectAssetLifecycle
from flitzis_looper.controller.committed_stem_residency import resident_stem_admission_pressure
from flitzis_looper.controller.performance_confirmation import (
    PerformanceConfirmationController,
    StemBatchConfirmation,
)
from flitzis_looper.controller.persistence import ProjectPersistence
from flitzis_looper.controller.stem_batch import StemBatchController
from flitzis_looper.controller.stem_identity import source_digest
from flitzis_looper.controller.stem_separators import SelectedStemGenerationBackend
from flitzis_looper.controller.stems import StemController
from flitzis_looper.models import PadContentIdentity, ProjectState, SessionState
from tests.flitzis_looper.controller.stem_batch_216_native_probe import (
    _PoolObservation,
    _ShapeAudio,
)

if TYPE_CHECKING:
    from types import FrameType

    from flitzis_looper.controller import AppController
    from flitzis_looper.controller.stem_generation import (
        StemGenerationRequest,
        StemGenerationResult,
        StemProgressCallback,
    )
    from flitzis_looper.models import StemSeparator
    from flitzis_looper_audio import AudioEngine

_INFERENCE_MODULES = frozenset({"flitzis_looper.demucs_streaming", "flitzis_bs_roformer"})
_MEMORY_KEYS = ("working_set_bytes", "private_committed_bytes")


class _Memory(ctypes.Structure):
    _fields_ = [
        ("size", wintypes.DWORD),
        ("faults", wintypes.DWORD),
        ("values", ctypes.c_size_t * 9),
    ]


class _FileTime(ctypes.Structure):
    _fields_ = [("low", wintypes.DWORD), ("high", wintypes.DWORD)]


class _HostMemory(ctypes.Structure):
    _fields_ = [
        ("length", wintypes.DWORD),
        ("load", wintypes.DWORD),
        ("total_physical", ctypes.c_ulonglong),
        ("available_physical", ctypes.c_ulonglong),
        ("commit_limit", ctypes.c_ulonglong),
        ("available_commit", ctypes.c_ulonglong),
        ("total_virtual", ctypes.c_ulonglong),
        ("available_virtual", ctypes.c_ulonglong),
        ("available_extended_virtual", ctypes.c_ulonglong),
    ]


class _PerformanceInformation(ctypes.Structure):
    _fields_ = [
        ("size", wintypes.DWORD),
        *[
            (name, ctypes.c_size_t)
            for name in (
                "commit_total",
                "commit_limit",
                "commit_peak",
                "physical_total",
                "physical_available",
                "system_cache",
                "kernel_total",
                "kernel_paged",
                "kernel_nonpaged",
                "page_size",
            )
        ],
        ("handles", wintypes.DWORD),
        ("processes", wintypes.DWORD),
        ("threads", wintypes.DWORD),
    ]


def _system_commit_bytes(total: int, limit: int, page_size: int) -> tuple[int, int, int]:
    """Convert one real PERFORMANCE_INFORMATION page image without coercion."""
    if (
        any(type(value) is not int for value in (total, limit, page_size))
        or page_size <= 0
        or total < 0
        or total > limit
    ):
        message = "GetPerformanceInfo returned invalid commit geometry"
        raise ValueError(message)
    values = total * page_size, limit * page_size, (limit - total) * page_size
    if any(value > 2**64 - 1 for value in values):
        message = "GetPerformanceInfo commit byte extent overflow"
        raise ValueError(message)
    return values


class _HostMinimumSnapshots:
    """Three associated minimum images; never a growing sample history."""

    _metrics = ("available_commit", "system_available_commit_bytes", "available_physical")

    def __init__(self) -> None:
        self._snapshots: dict[str, dict[str, int]] = {}

    def observe(self, host: dict[str, int], context: dict[str, int]) -> None:
        """Retain the exact timestamped host/context image for each strict minimum."""
        image = host | context
        for metric in self._metrics:
            previous = self._snapshots.get(metric)
            if previous is None or host[metric] < previous[metric]:
                self._snapshots[metric] = image.copy()

    def minimum(self, metric: str) -> int:
        """Read a scalar only after the baseline observation exists."""
        return self._snapshots[metric][metric]

    def report(self) -> dict[str, dict[str, int]]:
        """Detach three images so report consumers cannot alter retained minima."""
        return {metric: dict(image) for metric, image in self._snapshots.items()}


class _WindowsCounters:
    """Read the same Win32 counters as C3, with creation identity for each handle."""

    def __init__(self) -> None:
        assert os.name == "nt", "this explicit resource profile requires Windows"
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        signatures: tuple[tuple[str, list[object], object], ...] = (
            ("OpenProcess", [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD], wintypes.HANDLE),
            ("CloseHandle", [wintypes.HANDLE], wintypes.BOOL),
            ("GetCurrentProcess", [], wintypes.HANDLE),
            (
                "GetProcessMemoryInfo",
                [wintypes.HANDLE, ctypes.POINTER(_Memory), wintypes.DWORD],
                wintypes.BOOL,
            ),
            ("GetProcessTimes", [wintypes.HANDLE] + [ctypes.POINTER(_FileTime)] * 4, wintypes.BOOL),
            (
                "GetProcessIoCounters",
                [wintypes.HANDLE, ctypes.POINTER(ctypes.c_ulonglong * 6)],
                wintypes.BOOL,
            ),
            (
                "GetProcessHandleCount",
                [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)],
                wintypes.BOOL,
            ),
            ("WaitForSingleObject", [wintypes.HANDLE, wintypes.DWORD], wintypes.DWORD),
            ("GlobalMemoryStatusEx", [ctypes.POINTER(_HostMemory)], wintypes.BOOL),
            (
                "GetPerformanceInfo",
                [ctypes.POINTER(_PerformanceInformation), wintypes.DWORD],
                wintypes.BOOL,
            ),
        )
        for name, arguments, result in signatures:
            function = getattr(
                self.kernel,
                f"K32{name}" if name in {"GetProcessMemoryInfo", "GetPerformanceInfo"} else name,
            )
            function.argtypes = arguments
            function.restype = result

    def identity(self, handle: int) -> tuple[int, int, int]:
        """Return creation, kernel and user ticks from the actual retained handle."""
        times = [_FileTime() for _ in range(4)]
        if not self.kernel.GetProcessTimes(handle, *(ctypes.byref(value) for value in times)):
            raise ctypes.WinError(ctypes.get_last_error())
        ticks = [(value.high << 32) | value.low for value in times]
        return ticks[0], ticks[2] * 100, ticks[3] * 100

    def snapshot(self, handle: int) -> dict[str, int]:
        """Read one process; independent lifetime maxima are never summed as RAM."""
        created, kernel, user = self.identity(handle)
        memory = _Memory()
        memory.size = ctypes.sizeof(memory)
        io = (ctypes.c_ulonglong * 6)()
        handles = wintypes.DWORD()
        calls = (
            self.kernel.K32GetProcessMemoryInfo(handle, ctypes.byref(memory), memory.size),
            self.kernel.GetProcessIoCounters(handle, ctypes.byref(io)),
            self.kernel.GetProcessHandleCount(handle, ctypes.byref(handles)),
        )
        if not all(calls):
            raise ctypes.WinError(ctypes.get_last_error())
        return {
            "creation_ticks": created,
            "kernel_cpu_ns": kernel,
            "user_cpu_ns": user,
            "working_set_bytes": memory.values[1],
            "peak_working_set_bytes": memory.values[0],
            "private_committed_bytes": memory.values[8],
            "peak_commit_charge_bytes": memory.values[7],
            "page_faults": memory.faults,
            "handle_count": handles.value,
            "read_operations": io[0],
            "write_operations": io[1],
            "other_operations": io[2],
            "read_transfer_bytes": io[3],
            "write_transfer_bytes": io[4],
            "other_transfer_bytes": io[5],
        }

    def host(self) -> dict[str, int]:
        """Read sequential caller/system headroom; this is not an atomic OS snapshot."""
        started = time.perf_counter_ns()
        observed_at = time.time_ns()
        status = _HostMemory()
        status.length = ctypes.sizeof(status)
        if not self.kernel.GlobalMemoryStatusEx(ctypes.byref(status)):
            raise ctypes.WinError(ctypes.get_last_error())
        system = _PerformanceInformation()
        system.size = ctypes.sizeof(system)
        if not self.kernel.K32GetPerformanceInfo(ctypes.byref(system), system.size):
            raise ctypes.WinError(ctypes.get_last_error())
        total, limit, available = _system_commit_bytes(
            system.commit_total, system.commit_limit, system.page_size
        )
        caller = {
            name: int(getattr(status, name))
            for name in ("total_physical", "available_physical", "commit_limit", "available_commit")
        }
        return caller | {
            "system_commit_total_bytes": total,
            "system_commit_limit_bytes": limit,
            "system_available_commit_bytes": available,
            "observed_at_unix_ns": observed_at,
            "observation_started_perf_counter_ns": started,
            "observation_span_ns": time.perf_counter_ns() - started,
        }


@dataclass(slots=True)
class _ProcessRow:
    pid: int
    creation_ticks: int
    command: str
    samples: int = 0
    maxima: dict[str, int] = field(default_factory=dict)


class _OwnedProcessObservation:
    """Observe original code events on existing workers, without patching sealed code.

    Weak references identify only currently executing original command scopes.
    WindowsJob remains the sole process owner. No timer thread, executor, extra
    process supervisor or authoritative process registry is introduced.
    """

    def __init__(self) -> None:
        self.counters = _WindowsCounters()
        self._lock = threading.RLock()
        self._active: WeakKeyDictionary[process._OwnedTreeProcess, str] = WeakKeyDictionary()
        self._rows: dict[tuple[int, int], _ProcessRow] = {}
        self._inflight: WeakKeyDictionary[process._OwnedTreeProcess, int] = WeakKeyDictionary()
        self._codes = {
            getattr(process._OwnedTreeProcess, name).__code__: name
            for name in ("__init__", "activate", "observe_tree", "kill", "close_tree")
        }
        self._previous = threading.getprofile()
        self._enabled = True
        self._last_sample = 0.0
        self._previous_cpu: dict[tuple[int, int], int] = {}
        self.started = self.closed = self.samples = self.exit_races = 0
        self.peak_members = self.peak_scopes = self.peak_inference_trees = 0
        self.overlap_cpu_samples = 0
        self.skipped_inflight_scopes = 0
        self.samples_omitting_inflight = 0
        self.max_sample_span_ns = 0
        self.errors: list[str] = []
        self.peaks = {
            f"{domain}_{key}": 0 for domain in ("owned", "combined") for key in _MEMORY_KEYS
        }
        baseline_started = time.perf_counter_ns()
        self.root_before = self.counters.snapshot(self.counters.kernel.GetCurrentProcess())
        self.root_before["process_id"] = os.getpid()
        self.host_before = self.counters.host()
        self.minimum_available_physical = self.host_before["available_physical"]
        self.minimum_available_commit = self.host_before["available_commit"]
        self.host_minima = _HostMinimumSnapshots()
        self.host_minima.observe(
            self.host_before,
            {
                "sample_index": 0,
                "parent_private_committed_bytes": self.root_before["private_committed_bytes"],
                "observed_owned_private_committed_bytes": 0,
                "active_owned_scopes": 0,
                "omitted_owned_scopes": 0,
                "sample_observation_started_perf_counter_ns": baseline_started,
                "sample_observation_span_ns": time.perf_counter_ns() - baseline_started,
            },
        )
        threading.setprofile(self._profile)

    def _error(self, error: OSError | RuntimeError | ValueError | TypeError | KeyError) -> None:
        message = f"{type(error).__name__}: {error}"
        if message not in self.errors and len(self.errors) < 16:
            self.errors.append(message)

    def _profile(self, frame: FrameType, event: str, argument: object) -> None:
        if self._previous is not None:
            self._previous(frame, event, argument)
        name = self._codes.get(frame.f_code)
        if name is None or event not in {"call", "return"}:
            return
        scope = frame.f_locals.get("self")
        if not isinstance(scope, process._OwnedTreeProcess):
            return
        with self._lock:
            if event == "call":
                if self._enabled:
                    self._inflight[scope] = self._inflight.get(scope, 0) + 1
                return
            depth = self._inflight.get(scope, 0)
            if not depth:
                return
            if depth == 1:
                self._inflight.pop(scope)
            else:
                self._inflight[scope] = depth - 1
            try:
                self._profile_return(scope, name, frame)
                self._safe_sample(force=True)
            except (OSError, RuntimeError, ValueError, TypeError, KeyError) as error:
                self._error(error)

    @staticmethod
    def _command(arguments: list[str]) -> str:
        if len(arguments) > 2 and arguments[1] == "-m":
            return arguments[2]
        if len(arguments) > 1 and arguments[1] == "-c":
            return "python-detector"
        return Path(arguments[0]).name if arguments else "unknown"

    def _profile_return(
        self, scope: process._OwnedTreeProcess, name: str, frame: FrameType
    ) -> None:
        if (
            name == "__init__"
            and getattr(scope, "pid", None) is not None
            and scope._job is not None
            and scope._job._handle is not None
        ):
            self._active[scope] = self._command(frame.f_locals.get("arguments", []))
            self.started += 1
        if name == "close_tree" and scope in self._active:
            if scope._job is not None and scope._job._handle is not None:
                self.errors.append("original close_tree returned without closing its WindowsJob")
            self._active.pop(scope, None)
            self.closed += 1

    def _safe_sample(self, *, force: bool = False) -> None:
        try:
            self._sample(force=force)
        except (OSError, RuntimeError, ValueError, TypeError, KeyError) as error:
            self._error(error)

    def _retained_exited(self, retained: int) -> bool:
        state = self.counters.kernel.WaitForSingleObject(retained, 0)
        if state == 0:
            self.exit_races += 1
            return True
        if state != 258:
            raise ctypes.WinError(ctypes.get_last_error())
        return False

    def _member(self, pid: int, retained: int, command: str) -> dict[str, int] | None:
        # Compare a query handle with the actual job-retained process creation
        # identity. An exited/reused PID can never be counted as an owned member.
        if self._retained_exited(retained):
            return None
        created, _, _ = self.counters.identity(retained)
        handle = self.counters.kernel.OpenProcess(0x1010, wintypes.BOOL(0), pid)
        if not handle:
            if self._retained_exited(retained):
                return None
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            current = self.counters.snapshot(handle)
            if current["creation_ticks"] != created:
                if self._retained_exited(retained):
                    return None
                message = "a queried PID no longer names its actual owned job member"
                raise RuntimeError(message)
        except OSError:
            if self._retained_exited(retained):
                return None
            raise
        finally:
            if not self.counters.kernel.CloseHandle(handle):
                raise ctypes.WinError(ctypes.get_last_error())
        key = pid, created
        row = self._rows.setdefault(key, _ProcessRow(pid, created, command))
        row.samples += 1
        for name, value in current.items():
            row.maxima[name] = max(row.maxima.get(name, 0), value)
        cpu = current["kernel_cpu_ns"] + current["user_cpu_ns"]
        current["cpu_advanced"] = int(cpu > self._previous_cpu.get(key, cpu))
        self._previous_cpu[key] = cpu
        return current

    def _sample_scope(
        self, scope: process._OwnedTreeProcess, command: str, seen: set[tuple[int, int]]
    ) -> tuple[dict[str, int], bool, bool]:
        owned = dict.fromkeys(_MEMORY_KEYS, 0)
        live = advanced = False
        job = scope._job
        if job is None:
            return owned, live, advanced
        for pid, retained in tuple(job._members.items()):
            current = self._member(pid, retained, command)
            if current is None:
                continue
            key = pid, current["creation_ticks"]
            if key in seen:
                message = "one current process was observed in two independent owned scopes"
                raise RuntimeError(message)
            seen.add(key)
            live = True
            advanced |= bool(current["cpu_advanced"])
            for name in _MEMORY_KEYS:
                owned[name] += current[name]
        return owned, live, advanced

    def _sample(self, *, force: bool = False) -> None:
        began = time.perf_counter_ns()
        if not force and time.monotonic() - self._last_sample < 0.02:
            return
        self._last_sample = time.monotonic()
        owned = dict.fromkeys(_MEMORY_KEYS, 0)
        seen: set[tuple[int, int]] = set()
        inference_trees = advancing_trees = 0
        skipped = 0
        for scope, command in tuple(self._active.items()):
            if scope in self._inflight:
                skipped += 1
                continue
            amounts, live, advanced = self._sample_scope(scope, command, seen)
            for name in _MEMORY_KEYS:
                owned[name] += amounts[name]
            inference_trees += int(live and command in _INFERENCE_MODULES)
            advancing_trees += int(live and advanced and command in _INFERENCE_MODULES)
        parent = self.counters.snapshot(self.counters.kernel.GetCurrentProcess())
        host = self.counters.host()
        self.samples += 1
        self.skipped_inflight_scopes += skipped
        self.samples_omitting_inflight += int(skipped > 0)
        self.peak_members = max(self.peak_members, len(seen))
        self.peak_scopes = max(self.peak_scopes, len(self._active))
        self.peak_inference_trees = max(self.peak_inference_trees, inference_trees)
        self.overlap_cpu_samples += int(inference_trees >= 2 and advancing_trees >= 2)
        for name in _MEMORY_KEYS:
            self.peaks[f"owned_{name}"] = max(self.peaks[f"owned_{name}"], owned[name])
            self.peaks[f"combined_{name}"] = max(
                self.peaks[f"combined_{name}"], parent[name] + owned[name]
            )
        self.minimum_available_physical = min(
            self.minimum_available_physical, host["available_physical"]
        )
        self.minimum_available_commit = min(self.minimum_available_commit, host["available_commit"])
        self.host_minima.observe(
            host,
            {
                "sample_index": self.samples,
                "parent_private_committed_bytes": parent["private_committed_bytes"],
                "observed_owned_private_committed_bytes": owned["private_committed_bytes"],
                "active_owned_scopes": len(self._active),
                "omitted_owned_scopes": skipped,
                "sample_observation_started_perf_counter_ns": began,
                "sample_observation_span_ns": time.perf_counter_ns() - began,
            },
        )
        self.max_sample_span_ns = max(self.max_sample_span_ns, time.perf_counter_ns() - began)

    def sample(self) -> None:
        """Sample all existing active scopes plus the parent in one observation interval."""
        with self._lock:
            self._safe_sample()

    def pending(self) -> bool:
        """Require original close_tree settlement, never just launcher exit."""
        with self._lock:
            return bool(self._active) or bool(self._inflight) or self.started != self.closed

    def cpu_progress(self) -> int:
        """Expose monotone actual child CPU progress for the engineering watchdog."""
        with self._lock:
            return (
                sum(
                    row.maxima.get("kernel_cpu_ns", 0) + row.maxima.get("user_cpu_ns", 0)
                    for row in self._rows.values()
                )
                // 100_000_000
            )

    def report(self) -> dict[str, object]:
        """Keep sampled concurrent sums separate from individual OS lifetime peaks."""
        with self._lock:
            self._safe_sample(force=True)
            return {
                "scope": (
                    "sampled current parent+nonbusy retained owned members; "
                    "individual lifetime peaks separate"
                ),
                "poll_sampling_minimum_seconds": 0.02,
                "samples": self.samples,
                "observer": "original supervisor method call/return boundaries plus host polls",
                "maximum_observation_span_ns": self.max_sample_span_ns,
                "root_before": self.root_before,
                "root_after": self.counters.snapshot(self.counters.kernel.GetCurrentProcess())
                | {"process_id": os.getpid()},
                "host_before": self.host_before,
                "host_after": self.counters.host(),
                "minimum_available_physical": self.minimum_available_physical,
                "minimum_available_commit": self.minimum_available_commit,
                "minimum_system_available_commit": self.host_minima.minimum(
                    "system_available_commit_bytes"
                ),
                "host_minimum_snapshots": self.host_minima.report(),
                "host_counter_scope": {
                    "commit_limit/available_commit": (
                        "GlobalMemoryStatusEx caller limit/headroom; "
                        "not unqualified system-wide commit"
                    ),
                    "system_commit_*": "GetPerformanceInfo pages multiplied by PageSize",
                    "timing": (
                        "sequential API reads within recorded observation span; "
                        "Unix timestamp is wall clock, perf_counter is monotone"
                    ),
                    "minimum_context": (
                        "parent and observed nonbusy owned amounts share the sample interval; "
                        "omitted scopes remain explicit; no causal attribution"
                    ),
                },
                "sampled_concurrent_peaks": dict(self.peaks),
                "peak_owned_members": self.peak_members,
                "peak_owned_scopes": self.peak_scopes,
                "peak_actual_inference_trees": self.peak_inference_trees,
                "two_inference_tree_cpu_progress_samples": self.overlap_cpu_samples,
                "scope_starts": self.started,
                "original_scope_closes": self.closed,
                "pending_scopes": len(self._active),
                "inflight_supervisor_scopes": len(self._inflight),
                "skipped_inflight_scope_observations": self.skipped_inflight_scopes,
                "samples_omitting_inflight_scopes": self.samples_omitting_inflight,
                "exited_sample_races": self.exit_races,
                "sampling_errors": tuple(self.errors),
                "processes": [asdict(row) for row in self._rows.values()],
                "limits": "sampled aggregate maxima are observations, not hard process RAM limits; "
                "CPU/I/O are observed per-identity cumulative counters, not exact exit totals",
            }

    def close(self) -> None:
        """Restore the previous observer only after actual original scopes close."""
        with self._lock:
            assert not self._active
            assert self.started == self.closed
            self._enabled = False
            threading.setprofile(self._previous)


class _ObservedSelectedBackend:
    """Delegate once to the unmodified backend; observe metadata and original progress."""

    def __init__(self, expected: set[str], meter: _OwnedProcessObservation) -> None:
        self.delegate = SelectedStemGenerationBackend()
        self.expected = expected
        self.meter = meter
        self.stems: StemController | None = None
        self.requests: list[dict[str, object]] = []
        self.results: list[dict[str, object]] = []
        self.progress_events = 0
        self._lock = threading.Lock()

    def generate(
        self, request: StemGenerationRequest, progress: StemProgressCallback
    ) -> StemGenerationResult:
        """Preserve request, callback values, backend cancellation and returned receipts."""
        digest = source_digest(request.source_version)
        assert digest in self.expected
        assert request.device_policy == "auto"
        assert request.target_shape.sample_rate_hz == request.target_shape.frame_count == 48_000
        assert request.target_shape.channels == 2
        assert request.encoder_executable is not None
        assert request.encoder_contract
        assert request.python_source_roots
        assert self.stems is not None
        job = next(
            job for job in tuple(self.stems._jobs.values()) if job.resolved_request is request
        )
        assert job.processing_files is not None
        assert job.producer_identity is not None
        assert len(job.producer_identity) == 64
        with self._lock:
            assert all(row["digest"] != digest for row in self.requests), (
                "duplicate physical inference"
            )
            self.requests.append({
                "pad_id": request.sample_id,
                "digest": digest,
                "producer_identity": job.producer_identity,
                "started_ns": time.perf_counter_ns(),
            })

        def observe(value: float, stage: str) -> None:
            progress(value, stage)
            with self._lock:
                self.progress_events += 1
            self.meter.sample()

        result = self.delegate.generate(request, observe)
        assert result.device == "cpu"
        assert not result.cpu_fallback
        assert result.artifact_count == 5
        assert (
            result.decoder_verified
            if request.separator == "bs-roformer:musdb18hq"
            else result.encoder_verified
        ), "actual executed encoder/decoder receipt required"
        with self._lock:
            self.results.append({
                "pad_id": request.sample_id,
                "digest": digest,
                "finished_ns": time.perf_counter_ns(),
                **asdict(result),
            })
        return result


class StemBatchNeuralProbe:
    """Compose stock neural inference with the actual original all-slot control path."""

    def __init__(
        self,
        audio: AudioEngine,
        shape_audio: object,
        input_json: str,
        separator: str,
        case_label: str,
        expected_jobs: int,
    ) -> None:
        assert separator in {"demucs:htdemucs", "bs-roformer:musdb18hq"}
        rows = json.loads(input_json)
        assert len(rows) == 216
        self.occupied = tuple(slot for slot, row in enumerate(rows) if row is not None)
        assert self.occupied in {(0, 215), tuple(range(216))}
        self.case_label = case_label
        self.separator = separator
        self.expected_jobs = expected_jobs
        self.audio = cast("AudioEngine", _ShapeAudio(audio, shape_audio))
        self.project = ProjectState(
            key_lock=False,
            resident_pcm_budget_mib=512,
            selected_pad=215,
            selected_bank=5,
            stem_separator=cast("StemSeparator", separator),
        )
        for slot in self.occupied:
            row = rows[slot]
            self.project.sample_paths[slot] = row["new_reference"]
            self.project.sample_durations[slot] = 1.0
            self.project.pad_content[slot] = PadContentIdentity(
                instance_id=f"{slot + 1:032x}",
                material_id=row["material_id"],
                material_creation=row["material_creation"],
            )
            self.project.pad_key_lock[slot] = False
        self._contents = tuple(self.project.pad_content)
        self._paths = tuple(self.project.sample_paths)
        self._expected = {rows[slot]["original"]["sha256"] for slot in self.occupied}
        assert len(self._expected) == expected_jobs
        self._patches = ExitStack()
        # Public control captures auto. This explicit test environment makes the
        # unmodified fresh Torch detector choose CPU; it does not rewrite intent.
        self._patches.enter_context(patch.dict(os.environ, {"CUDA_VISIBLE_DEVICES": "-1"}))
        self.meter = _OwnedProcessObservation()
        self.backend = _ObservedSelectedBackend(self._expected, self.meter)
        self.session = SessionState(settings_open=False)
        self.persistence = ProjectPersistence(self.project)
        self.persistence.config_path = Path("samples/neural-batch.json")
        self.persistence.bind_audio(self.audio)
        self.assets = ProjectAssetLifecycle(self.project, self.audio)
        self.assets.sync_assignments()
        self.persistence.mark_dirty()
        self.persistence.flush()
        self.stems = StemController(
            self.project,
            self.session,
            self.audio,
            on_project_changed=self.persistence.mark_dirty,
            stem_backend=self.backend,
            asset_lifecycle=self.assets,
        )
        self.backend.stems = self.stems
        self.stems.bind_disk_selection_commit(
            self.persistence.commit_stem_selection,
            self.persistence.flush,
            writer_blocked=self.persistence.stem_selection_write_blocked,
        )
        self.batch = StemBatchController(self.stems)
        idle = SimpleNamespace(active=False)
        envelope = SimpleNamespace(
            project=self.project,
            stems=self.stems,
            stem_batch=self.batch,
            stem_delete=idle,
            _close_requested=False,
            banks=idle,
            pairs=idle,
            arrangement=idle,
        )
        self.warning = PerformanceConfirmationController(cast("AppController", envelope))
        self.observation = _PoolObservation(self.stems, self._patches)
        self._before = self.persistence.config_path.read_bytes()
        self._all_cursor = 0
        self._closing = self._closed = False

    def start(self) -> None:
        """Accept the exact immutable public warning, without producer substitutes."""
        self.warning.request_all_loaded_stems()
        warning = self.warning.pending
        assert isinstance(warning, StemBatchConfirmation)
        assert len(warning.inventory.items) == 216
        assert tuple(item.pad_id for item in warning.inventory.items if item) == self.occupied
        assert all(
            item is None or item.unavailable_reason is None for item in warning.inventory.items
        )
        assert not self.stems._jobs
        assert not self.stems._requested_generations
        assert self.assets._reserved == 0
        assert self.persistence.config_path.read_bytes() == self._before
        self.warning.accept(warning)
        self.warning.accept(warning)
        assert self.warning.pending is None

    def poll(self) -> None:
        """Advance original controllers and collect observations, including during close."""
        if self._closed:
            return
        before = len(self.observation.grants)
        self.stems.on_frame_render()
        assert len(self.observation.grants) - before <= 6
        self.batch.poll()
        self.assets.retry_retirements()
        self.observation.observe()
        self.meter.sample()
        if self._closing:
            if not self.process_pending():
                self._finish_close()
            return
        for slot, error in tuple(self.session.stem_generation_errors.items()):
            demand = self.stems._demands.get(slot)
            assert self._all_cursor > 0, self.diagnostics()
            assert resident_stem_admission_pressure(error), self.diagnostics()
            assert demand is not None, self.diagnostics()
            assert demand.explicit, self.diagnostics()
            assert demand.state in {"queued", "deferred", "preparing"}, self.diagnostics()
        assert (self.project.selected_pad, self.project.selected_bank) == (215, 5)
        assert tuple(self.project.pad_content) == self._contents
        assert tuple(self.project.sample_paths) == self._paths
        assert not self.session.settings_open

    def complete(self) -> bool:
        """Require every original durable result, actual provenance and own registration ACK."""
        summary = self.batch.summary
        if summary is None or summary.active:
            return False
        assert summary.done == summary.total == len(self.occupied), self.diagnostics()
        assert summary.skipped == summary.failed == summary.cancelled == 0
        assert tuple(item.pad_id for item in summary.items) == self.occupied
        assert len(self.backend.requests) == len(self.backend.results) == self.expected_jobs
        if (
            self.stems._jobs
            or self.stems._stem_preparation_count()
            or self.observation.submitted
            or self.meter.pending()
        ):
            return False
        selections = self.stems._committed_residency._selections
        if len(selections) != len(self.occupied):
            return False
        for slot in self.occupied:
            selection = selections[slot]
            if selection.ticket.publication_status() != "accepted":
                return False
            assert selection.ticket.is_current()
            content = self._contents[slot]
            assert content is not None
            assert selection.content_id == content.instance_id
            assert selection.source_path == self._paths[slot]
            assert selection.entry == self.project.stem_cache[slot]
            assert selection.entry.fingerprint_schema == "effective-stem-v3"
            assert selection.entry.generation_fingerprint
            assert selection.entry.producer_identity
            assert selection.entry.pair is not None
            assert selection.entry.pair.provenance is not None
            assert (
                selection.entry.pair.provenance.producer_identity
                == selection.entry.producer_identity
            )
        return True

    def adopt_all(self) -> None:
        """Request at most eight stopped-PAD ALL residencies per host poll."""
        end = min(self._all_cursor + 8, len(self.occupied))
        for slot in self.occupied[self._all_cursor : end]:
            assert self.stems.set_stem_mix_mode(slot, "all_stems"), self.diagnostics()
        self._all_cursor = end

    def all_ready(self) -> bool:
        """Require each original selection's own current ALL component receipt."""
        if self._all_cursor != len(self.occupied):
            return False
        selections = self.stems._committed_residency._selections
        for slot in self.occupied:
            selection = selections[slot]
            assert selection.ticket.is_current()
            receipt = selection.receipt
            if receipt is None or receipt.residency_status() != "accepted":
                return False
            assert receipt.is_current()
            assert selection.receipt_intent == (
                "all_stems",
                self.project.pad_stem_intent[slot].enabled_mask,
            )
        budget = self.audio.get_resident_pcm_budget_status()
        assert budget["budget_mib"] == 512
        assert not budget["over_budget"]
        return budget["pending_reservation_count"] == budget["pending_resident_bytes"] == 0

    def finish(self) -> str:
        """Return one actual profile receipt after exact Config reopen and own ACKs."""
        assert self.complete()
        assert self.all_ready()
        assert not self.session.stem_generation_errors, self.diagnostics()
        assert not self.process_pending(), self.diagnostics()
        self.persistence.flush()
        saved = ProjectPersistence.from_config_path(self.persistence.config_path)
        assert saved.load_error is None
        for slot in range(216):
            assert saved.project.pad_content[slot] == self._contents[slot]
            assert saved.project.sample_paths[slot] == self._paths[slot]
            if slot in self.occupied:
                entry = self.project.stem_cache[slot]
                reopened = saved.project.stem_cache[slot]
                assert entry is not None
                assert reopened is not None
                assert reopened.pair == entry.pair
                assert saved.project.pad_stem_mix_mode[slot] == "all_stems"
            else:
                assert saved.project.stem_cache[slot] is None
        assert (saved.project.selected_pad, saved.project.selected_bank) == (215, 5)
        assert (
            len({entry.cache_dir for entry in self.project.stem_cache if entry})
            == self.expected_jobs
        )
        metrics = self.meter.report()
        assert not self.meter.errors, self.diagnostics()
        assert self.meter.started == self.meter.closed > 0
        assert self.meter.peak_inference_trees <= 2
        if self.expected_jobs >= 2:
            assert self.meter.peak_inference_trees >= 2, self.diagnostics()
            assert self.meter.overlap_cpu_samples > 0, self.diagnostics()
        assert self.observation.peak_running == 2
        assert self.observation.peak_queued <= 32
        assert self.observation.peak_submitted <= 34
        return json.dumps({
            "case": self.case_label,
            "separator": self.separator,
            "scope": "actual stock neural/source/controller/Config/own registration+ALL ACK; "
            "stopped one-second48000Hz stereo inputs; no216voices/music/device acceptance",
            "requested_device_policy": "auto",
            "effective_device": "cpu",
            "test_environment": {"CUDA_VISIBLE_DEVICES": "-1"},
            "processing_settings": {
                "demucs_shifts": self.project.demucs_shifts,
                "demucs_overlap": self.project.demucs_overlap,
                "processing_revision": self.project.stem_processing_revision,
            },
            "controller": json.loads(self.diagnostics()),
            "backend_requests": self.backend.requests,
            "backend_results": self.backend.results,
            "process_observations": metrics,
        })

    def diagnostics(self) -> str:
        """Expose bounded current state without recursive source or lease dumps."""
        summary = self.batch.summary
        return json.dumps({
            "occupied": len(self.occupied),
            "expected_jobs": self.expected_jobs,
            "inferences": len(self.backend.requests),
            "completed_backends": len(self.backend.results),
            "jobs": len(self.stems._jobs),
            "preparations": self.stems._stem_preparation_count(),
            "requested": len(self.stems._requested_generations),
            "done": summary.done if summary else None,
            "failed": summary.failed if summary else None,
            "errors": self.session.stem_generation_errors,
            "peaks": {
                "workers": self.observation.peak_running,
                "queued": self.observation.peak_queued,
                "admitted": self.observation.peak_submitted,
                "jobs": self.observation.peak_jobs,
                "preparations": self.observation.peak_preparations,
                "requested": self.observation.peak_requested,
            },
            "inference_tree_peak": self.meter.peak_inference_trees,
            "overlap_cpu_samples": self.meter.overlap_cpu_samples,
            "process_scopes": {"started": self.meter.started, "closed": self.meter.closed},
            "sampling_errors": self.meter.errors,
            "closing": self._closing,
            "closed": self._closed,
        })

    def progress_marker(self) -> tuple[int, int, int, int, int]:
        """Track actual child CPU/backend/semantic progress, without extending product deadlines."""
        summary = self.batch.summary
        selections = tuple(self.stems._committed_residency._selections.values())
        return (
            len(self.observation.grants),
            len(self.backend.requests),
            summary.done + summary.skipped + summary.failed + summary.cancelled if summary else 0,
            sum(row.ticket.publication_status() == "accepted" for row in selections)
            + sum(
                row.receipt is not None and row.receipt.residency_status() == "accepted"
                for row in selections
            ),
            self.backend.progress_events + self.meter.cpu_progress(),
        )

    def process_pending(self) -> bool:
        """Retain cleanup until original child, worker, Pair and cancellation owners settle."""
        if self._closed:
            return False
        self.meter.sample()
        return bool(
            self.meter.pending()
            or self.observation.submitted
            or self.stems._jobs
            or self.stems._stem_preparation_count()
            or self.stems._disk_selection_settlements
            or self.batch.active
        )

    def close(self) -> None:
        """Initiate bounded cooperative close; Rust continues poll/callback settlement."""
        if self._closing:
            return
        self._closing = True
        self.batch.cancel()
        if not self.process_pending():
            self._finish_close()

    def _finish_close(self) -> None:
        self.stems.shut_down()
        self.meter.close()
        self._patches.close()
        self.assets.release_saved_assignments()
        self._closed = True
