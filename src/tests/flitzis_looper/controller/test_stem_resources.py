"""Deterministic admission and active-command headroom policy, without processes."""

from dataclasses import dataclass
from unittest.mock import Mock

import pytest

from flitzis_looper.controller.stem_resources import StemResourceError, StemResourceGuard


@dataclass(slots=True)
class _Clock:
    now: float = 10.0

    def __call__(self) -> float:
        return self.now


def _headroom(
    *, caller: int = 1, system: int = 1, physical: int = 1, low_memory: int = 0
) -> dict[str, int]:
    return {
        "available_commit": caller,
        "system_available_commit_bytes": system,
        "available_physical": physical,
        "low_memory": low_memory,
    }


def test_initial_admission_accepts_positive_headroom_without_os_pressure() -> None:
    sample = Mock(return_value=_headroom())
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
    "metric", ["available_commit", "system_available_commit_bytes", "available_physical"]
)
def test_exhausted_headroom_stops_work_and_recovers_when_positive(metric: str) -> None:
    clock = _Clock()
    sample = Mock(return_value=_headroom() | {metric: 0})
    guard = StemResourceGuard(sample, clock)
    decision = guard.admission()
    assert decision.state == "pressure"
    assert not decision.allowed
    assert decision.message == "Waiting for available memory"
    assert guard.command_stop_reason() == "resource_pressure"

    clock.now += 0.25
    sample.return_value = _headroom()
    assert guard.admission().allowed
    assert guard.command_stop_reason() is None
    guard.entry()


@pytest.mark.parametrize("error", [OSError("query denied"), ValueError("bad query geometry")])
def test_query_failure_blocks_new_work_until_a_clear_observation(
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
    sample.return_value = _headroom()
    clock.now += 0.25
    assert guard.admission().allowed


def test_one_guard_shares_cache_across_admission_status_and_active_monitoring() -> None:
    clock = _Clock()
    sample = Mock(return_value=_headroom())
    guard = StemResourceGuard(sample, clock)
    initial = guard.admission()
    sample.return_value = _headroom(low_memory=1)
    for now in (10.0, 10.125, 10.249):
        clock.now = now
        assert guard.current is initial
        assert guard.admission() is initial
        assert guard.command_stop_reason() is None
    sample.assert_called_once()
    clock.now = 10.25
    assert guard.admission().state == "pressure"
    assert sample.call_count == 2
    assert guard.command_stop_reason() == "resource_pressure"
    assert sample.call_count == 2


def test_worker_entry_forces_fresh_query_and_publishes_its_observation() -> None:
    clock = _Clock()
    sample = Mock(return_value=_headroom())
    guard = StemResourceGuard(sample, clock)
    assert guard.admission().allowed
    sample.return_value = _headroom(system=0)
    clock.now = 10.001
    assert guard.admission().allowed
    with pytest.raises(StemResourceError, match="resource_pressure") as refused:
        guard.entry()
    assert refused.value.reason == "resource_pressure"
    assert not isinstance(refused.value, OSError)
    assert guard.current.state == "pressure"
    assert not guard.admission().allowed
    assert sample.call_count == 2
    sample.return_value = _headroom()
    guard.entry()
    assert guard.admission().allowed
    assert sample.call_count == 3


def test_os_low_memory_signal_stops_work_with_positive_measured_headroom() -> None:
    clock = _Clock()
    headroom = 4 * 1024**3
    image = _headroom(caller=headroom, system=headroom, physical=headroom)
    sample = Mock(return_value=image)
    guard = StemResourceGuard(sample, clock)
    assert guard.admission().allowed
    assert guard.command_stop_reason() is None
    clock.now += 0.25
    sample.return_value = image | {"low_memory": 1}
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
    sample.return_value = _headroom(low_memory=1)
    assert guard.admission().state == "pressure"
    assert guard.command_stop_reason() == "resource_pressure"
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
