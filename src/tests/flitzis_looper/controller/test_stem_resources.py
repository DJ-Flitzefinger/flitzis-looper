"""Deterministic admission and active-command headroom policy, without processes."""

from dataclasses import dataclass
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.stem_resources import StemResourceError, StemResourceGuard

_MIB = 1024**2
_GIB = 1024**3


@dataclass(slots=True)
class _Clock:
    now: float = 10.0

    def __call__(self) -> float:
        return self.now


def _headroom(
    *, caller: int = 4 * _GIB, system: int = 4 * _GIB, physical: int = _GIB
) -> dict[str, int]:
    return {
        "available_commit": caller,
        "system_available_commit_bytes": system,
        "available_physical": physical,
    }


def test_initial_admission_accepts_exact_commit_and_physical_floors() -> None:
    sample = Mock(return_value=_headroom(caller=2 * _GIB, system=2 * _GIB, physical=512 * _MIB))
    guard = StemResourceGuard(sample, _Clock())
    assert not guard.current.allowed
    sample.assert_not_called()
    decision = guard.admission()
    assert decision.allowed
    assert decision.state == "clear"
    assert decision.message is None
    guard.entry()
    assert guard.command_stop_reason() is None
    assert sample.call_count == 2


@pytest.mark.parametrize(
    ("metric", "initial_floor", "recovery_floor"),
    [
        ("available_commit", 2 * _GIB, 3 * _GIB),
        ("system_available_commit_bytes", 2 * _GIB, 3 * _GIB),
        ("available_physical", 512 * _MIB, 768 * _MIB),
    ],
)
def test_each_low_headroom_channel_latches_until_its_inclusive_recovery_floor(
    metric: str, initial_floor: int, recovery_floor: int
) -> None:
    clock = _Clock()
    image = _headroom()
    image[metric] = initial_floor - 1
    sample = Mock(return_value=image)
    guard = StemResourceGuard(sample, clock)
    decision = guard.admission()
    assert decision.state == "pressure"
    assert not decision.allowed
    assert decision.message == "Waiting for available memory"
    for amount in (initial_floor, recovery_floor - 1):
        clock.now += 0.25
        sample.return_value = _headroom() | {metric: amount}
        assert not guard.admission().allowed
        # Admission pressure alone does not terminate an already active command.
        assert guard.command_stop_reason() is None
    clock.now += 0.25
    sample.return_value = _headroom() | {metric: recovery_floor}
    assert guard.admission().allowed
    guard.entry()


@pytest.mark.parametrize("error", [OSError("query denied"), ValueError("bad query geometry")])
def test_query_failure_blocks_new_work_and_requires_recovery_headroom(
    error: OSError | ValueError,
) -> None:
    clock = _Clock()
    sample = Mock(side_effect=error)
    guard = StemResourceGuard(sample, clock)
    decision = guard.admission()
    assert decision.state == "unavailable"
    assert not decision.allowed
    assert decision.message == "Waiting for memory monitoring"
    assert decision.detail == str(error)
    with pytest.raises(StemResourceError, match="resource_unavailable") as refused:
        guard.entry()
    assert refused.value.reason == "resource_unavailable"
    # Neither a zero image nor an unlimited image may replace a failed query.
    assert guard.current.state == "unavailable"
    sample.side_effect = None
    sample.return_value = _headroom(caller=2 * _GIB, system=2 * _GIB, physical=512 * _MIB)
    clock.now += 0.25
    assert guard.admission().state == "pressure"
    sample.return_value = _headroom(caller=3 * _GIB, system=3 * _GIB, physical=768 * _MIB)
    clock.now += 0.25
    assert guard.admission().allowed


def test_one_guard_shares_cache_across_admission_status_and_active_monitoring() -> None:
    clock = _Clock()
    sample = Mock(return_value=_headroom())
    guard = StemResourceGuard(sample, clock)
    initial = guard.admission()
    sample.return_value = _headroom(caller=2 * _GIB - 1)
    for now in (10.0, 10.125, 10.249):
        clock.now = now
        assert guard.current is initial
        assert guard.admission() is initial
        assert guard.command_stop_reason() is None
    sample.assert_called_once()
    clock.now = 10.25
    assert guard.admission().state == "pressure"
    assert sample.call_count == 2
    assert guard.command_stop_reason() is None
    assert sample.call_count == 2


def test_worker_entry_forces_fresh_query_and_shares_its_recovery_latch() -> None:
    clock = _Clock()
    sample = Mock(return_value=_headroom())
    guard = StemResourceGuard(sample, clock)
    assert guard.admission().allowed
    sample.return_value = _headroom(system=2 * _GIB - 1)
    clock.now = 10.001
    assert guard.admission().allowed
    with pytest.raises(StemResourceError, match="resource_pressure") as refused:
        guard.entry()
    assert refused.value.reason == "resource_pressure"
    assert not isinstance(refused.value, OSError)
    assert guard.current.state == "pressure"
    assert not guard.admission().allowed
    assert sample.call_count == 2
    sample.return_value = _headroom(caller=2 * _GIB, system=2 * _GIB, physical=512 * _MIB)
    with pytest.raises(StemResourceError, match="resource_pressure"):
        guard.entry()
    sample.return_value = _headroom(caller=3 * _GIB, system=3 * _GIB, physical=768 * _MIB)
    guard.entry()
    assert guard.admission().allowed
    assert sample.call_count == 4


@pytest.mark.parametrize(
    ("metric", "critical_floor"),
    [
        ("available_commit", 512 * _MIB),
        ("system_available_commit_bytes", 512 * _MIB),
        ("available_physical", 128 * _MIB),
    ],
)
def test_active_command_stops_at_inclusive_critical_floor_but_not_ordinary_pressure(
    metric: str, critical_floor: int
) -> None:
    clock = _Clock()
    sample = Mock(return_value=_headroom() | {metric: critical_floor + 1})
    guard = StemResourceGuard(sample, clock)
    assert not guard.admission().allowed
    assert guard.command_stop_reason() is None
    clock.now += 0.25
    sample.return_value = _headroom() | {metric: critical_floor}
    assert guard.command_stop_reason() == "resource_pressure"
    assert not guard.admission().allowed
    error = guard.failure("resource_pressure")
    assert isinstance(error, StemResourceError)
    assert error.reason == "resource_pressure"
    assert guard.current.detail in str(error)
    # Failure construction uses the settled current context, without another query.
    assert sample.call_count == 2


def test_consecutive_query_failures_have_one_fixed_deadline_across_all_callers() -> None:
    clock = _Clock()
    sample = Mock(side_effect=OSError("query unavailable"))
    guard = StemResourceGuard(sample, clock)
    assert guard.command_stop_reason() is None
    for now in (10.25, 10.5, 10.75):
        clock.now = now
        assert not guard.admission().allowed
        with pytest.raises(StemResourceError, match="resource_unavailable"):
            guard.entry()
        assert guard.command_stop_reason() is None
    clock.now = 10.999
    assert guard.command_stop_reason() is None
    clock.now = 11.0
    assert guard.command_stop_reason() == "resource_unavailable"
    clock.now = 11.125
    assert guard.command_stop_reason() == "resource_unavailable"
    error = guard.failure("resource_unavailable")
    assert error.reason == "resource_unavailable"
    assert "query unavailable" in str(error)


def test_valid_low_observation_resets_unknown_deadline_without_opening_admission() -> None:
    clock = _Clock()
    sample = Mock(side_effect=OSError("query unavailable"))
    guard = StemResourceGuard(sample, clock)
    assert guard.command_stop_reason() is None
    clock.now = 10.75
    sample.side_effect = None
    sample.return_value = _headroom(caller=_GIB)
    assert guard.admission().state == "pressure"
    assert guard.command_stop_reason() is None
    clock.now = 11.0
    sample.side_effect = ValueError("new invalid query")
    assert guard.command_stop_reason() is None
    clock.now = 11.75
    assert guard.command_stop_reason() is None
    clock.now = 12.0
    assert guard.command_stop_reason() == "resource_unavailable"


def test_explicit_unsupported_platform_allows_the_existing_bounded_pool() -> None:
    clock = _Clock()
    sample = Mock(return_value=None)
    guard = StemResourceGuard(sample, clock)
    decision = guard.admission()
    assert decision.allowed
    assert decision.state == "unsupported"
    assert decision.message is not None
    assert "unsupported" in decision.message
    guard.entry()
    assert guard.command_stop_reason() is None
    assert guard.current.state == "unsupported"
    # Unsupported support is explicit; it does not pretend that headroom is known.
    assert not guard.current.detail
