"""Bounded passive calibration for the two existing offline separator routes.

No extra inference, sampler, executor or persistent state is created. Observations
are policy hints only; source, producer, publication and audio authority stay with
their existing owners. Unknown complete producer identity disables calibration.
"""

import json
import math
import re
import time
from collections import OrderedDict
from dataclasses import dataclass, replace
from threading import Lock
from typing import TYPE_CHECKING, Literal

from flitzis_looper.controller.stem_identity import request_fingerprint

if TYPE_CHECKING:
    from flitzis_looper.controller.stem_generation import StemGenerationRequest
    from flitzis_looper.stem_execution import StemExecution

type Device = Literal["cpu", "cuda"]
_MAX_KEYS = 32
_EXPIRY_SECONDS = 600.0
_CUDA_MARGIN_BYTES = 256 << 20


@dataclass(frozen=True, slots=True)
class CudaCapability:
    """Tentative support plus optional actual runtime/driver/UUID and free VRAM."""

    available: bool
    identity: str | None = None
    free_bytes: int | None = None


def parse_cuda_capability(returncode: int, stdout: str) -> CudaCapability:
    """Malformed or missing query fields cannot train the performance cache."""
    if returncode != 0 or len(stdout) > 4096:
        return CudaCapability(available=False)
    try:
        value = json.loads(stdout)
    except ValueError:
        return CudaCapability(available=False)
    if not isinstance(value, dict) or type(value.get("available")) is not bool:
        return CudaCapability(available=False)
    identity, free = value.get("identity"), value.get("free_bytes")
    if (
        not isinstance(identity, str)
        or re.fullmatch(r"[0-9a-f]{64}", identity) is None
        or type(free) is not int
        or not 0 <= free <= 2**63 - 1
    ):
        return CudaCapability(value["available"])
    return CudaCapability(value["available"], identity, free)


@dataclass(frozen=True, slots=True)
class ProviderPlan:
    """One fixed attempt order and its exact comparable observation key."""

    devices: tuple[Device, ...]
    key: tuple[str, str, str] | None = None


@dataclass(frozen=True, slots=True)
class _Cost:
    seconds: float
    observed_at: float
    cuda_peak_bytes: int | None


@dataclass(frozen=True, slots=True)
class ProviderCalibration:
    """Serializable candidate data; final provenance/read settlement permits promotion."""

    plan: ProviderPlan
    seconds: float
    verified: bool
    observed_at: float


class StemProviderSelector:
    """Keep at most 32 source/model/runtime-specific passive calibration rows."""

    def __init__(self) -> None:
        self._lock = Lock()
        self._costs: OrderedDict[tuple[str, str, str], dict[Device, _Cost]] = OrderedDict()
        self._last_capability: tuple[CudaCapability, float] | None = None

    def cpu_plan(
        self, request: StemGenerationRequest, model: str, *, now: float | None = None
    ) -> ProviderPlan:
        """Use a recent known identity without querying or initializing any GPU."""
        stamp = time.monotonic() if now is None else now
        capability = CudaCapability(available=False)
        with self._lock:
            previous = self._last_capability
            if previous is not None and 0 <= stamp - previous[1] < _EXPIRY_SECONDS:
                capability = previous[0]
        return self.plan(request, model, capability, now=stamp)

    def plan(
        self,
        request: StemGenerationRequest,
        model: str,
        capability: CudaCapability,
        *,
        now: float | None = None,
    ) -> ProviderPlan:
        """Retain cold CUDA preference until comparable measured costs justify CPU."""
        stamp = time.monotonic() if now is None else now
        fingerprint = request_fingerprint(
            replace(request, device_policy="auto"), request.producer_identity
        )
        key = (
            (fingerprint, model, capability.identity)
            if fingerprint is not None and capability.identity is not None
            else None
        )
        if request.device_policy != "cpu":
            with self._lock:
                self._last_capability = (
                    (capability, stamp) if capability.identity is not None else None
                )
        if request.device_policy == "cpu" or not capability.available:
            return ProviderPlan(("cpu",), key)
        with self._lock:
            self._expire(stamp)
            costs = self._costs.get(key, {}) if key is not None else {}
            cpu, cuda = costs.get("cpu"), costs.get("cuda")
            if (
                cuda is not None
                and capability.free_bytes is not None
                and (
                    cuda.cuda_peak_bytes is not None
                    and capability.free_bytes < cuda.cuda_peak_bytes + _CUDA_MARGIN_BYTES
                )
            ):
                return ProviderPlan(("cpu",), key)
            # Both totals cover real complete launches and artifact settlement.
            # A meaningful margin prevents route churn from timer noise.
            if cpu is not None and cuda is not None and cpu.seconds <= cuda.seconds * 0.85:
                return ProviderPlan(("cpu",), key)
        return ProviderPlan(("cuda", "cpu"), key)

    def observe(
        self,
        plan: ProviderPlan,
        execution: StemExecution | None,
        seconds: float,
        *,
        verified: bool,
        now: float | None = None,
        observed_at: float | None = None,
    ) -> None:
        """Train only complete verified successful artifacts, never a fallback cost."""
        if plan.key is None or execution is None or not verified or execution.cpu_fallback:
            return
        if execution.device not in plan.devices or not math.isfinite(seconds) or seconds <= 0:
            return
        stamp = time.monotonic() if now is None else now
        observed = stamp if observed_at is None else observed_at
        if not 0 <= stamp - observed < _EXPIRY_SECONDS:
            return
        with self._lock:
            self._expire(stamp)
            costs = self._costs.setdefault(plan.key, {})
            previous = costs.get(execution.device)
            if previous is not None and previous.observed_at >= observed:
                return
            costs[execution.device] = _Cost(seconds, observed, execution.cuda_peak_bytes)
            self._costs.move_to_end(plan.key)
            while len(self._costs) > _MAX_KEYS:
                self._costs.popitem(last=False)

    def accept(
        self,
        calibration: ProviderCalibration,
        execution: StemExecution | None,
        *,
        now: float | None = None,
    ) -> None:
        """Idempotently promote original-time data after the caller's final guards."""
        self.observe(
            calibration.plan,
            execution,
            calibration.seconds,
            verified=calibration.verified,
            now=now,
            observed_at=calibration.observed_at,
        )

    def _expire(self, now: float) -> None:
        for key, costs in tuple(self._costs.items()):
            fresh = {
                device: cost
                for device, cost in costs.items()
                if 0 <= now - cost.observed_at < _EXPIRY_SECONDS
            }
            if fresh:
                self._costs[key] = fresh
            else:
                del self._costs[key]
