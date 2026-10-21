"""Test-only counter truth, bounded minima and query lifetime; no memory policy.

Real Windows queries below inspect only the calling process/system. Injected
failures and arithmetic images exercise telemetry, never Native/model success.
"""

import ctypes
import os
from unittest.mock import Mock

import pytest

from flitzis_looper.system_resources import system_commit_bytes
from tests.flitzis_looper.controller.stem_batch_neural_native_probe import (
    _HostMinimumSnapshots,
    _WindowsCounters,
)


def test_commit_page_conversion_has_exact_system_byte_extent() -> None:
    assert system_commit_bytes(3, 9, 4096) == (12288, 36864, 24576)
    assert system_commit_bytes(9, 9, 4096) == (36864, 36864, 0)


@pytest.mark.parametrize(
    ("total", "limit", "page_size"),
    [(0, 1, 0), (2, 1, 4096), (-1, 1, 4096), (True, 1, 4096), (0, 1.0, 4096)],
)
def test_commit_page_conversion_rejects_invalid_or_coerced_geometry(
    total: int, limit: int, page_size: int
) -> None:
    with pytest.raises(ValueError, match="invalid commit geometry"):
        system_commit_bytes(total, limit, page_size)


def test_commit_byte_overflow_is_not_a_zero_or_unlimited_reading() -> None:
    with pytest.raises(ValueError, match="byte extent overflow"):
        system_commit_bytes(0, 2**64 - 1, 2)


def _host(caller: int, system: int, physical: int, stamp: int) -> dict[str, int]:
    return {
        "available_commit": caller,
        "system_available_commit_bytes": system,
        "available_physical": physical,
        "observed_at_unix_ns": stamp,
        "observation_started_perf_counter_ns": stamp * 2,
        "observation_span_ns": 3,
    }


def _context(index: int, *, omitted: int = 0) -> dict[str, int]:
    return {
        "sample_index": index,
        "parent_private_committed_bytes": 100 + index,
        "observed_owned_private_committed_bytes": 200 + index,
        "active_owned_scopes": 2,
        "omitted_owned_scopes": omitted,
    }


def test_each_minimum_retains_its_own_actual_observation_context() -> None:
    minima = _HostMinimumSnapshots()
    minima.observe(_host(900, 1000, 1100, 10), _context(0))
    minima.observe(_host(800, 1200, 1300, 20), _context(1, omitted=1))
    minima.observe(_host(850, 700, 1400, 30), _context(2))
    minima.observe(_host(860, 750, 600, 40), _context(3))
    report = minima.report()
    caller = report["available_commit"]
    assert caller["observed_at_unix_ns"] == 20
    assert caller["system_available_commit_bytes"] == 1200
    assert caller["omitted_owned_scopes"] == 1
    assert caller["parent_private_committed_bytes"] == 101
    system = report["system_available_commit_bytes"]
    assert system["observed_at_unix_ns"] == 30
    assert system["available_commit"] == 850
    assert system["observed_owned_private_committed_bytes"] == 202
    assert report["available_physical"]["observed_at_unix_ns"] == 40
    assert minima.minimum("system_available_commit_bytes") == 700


def test_minima_keep_three_detached_images_and_first_context_on_equal_values() -> None:
    minima = _HostMinimumSnapshots()
    original = _host(90, 100, 110, 10)
    context = _context(0)
    minima.observe(original, context)
    original["available_commit"] = 0
    context["sample_index"] = 999
    for stamp in range(20, 276):
        minima.observe(_host(90, 100, 110, stamp), _context(stamp))
    report = minima.report()
    assert len(report) == 3
    assert report["available_commit"]["available_commit"] == 90
    assert report["available_commit"]["sample_index"] == 0
    assert report["available_commit"]["observed_at_unix_ns"] == 10
    report["available_commit"]["available_commit"] = 1
    assert minima.minimum("available_commit") == 90


@pytest.mark.skipif(os.name != "nt", reason="actual Windows API instrumentation")
def test_actual_windows_host_query_preserves_caller_fields_and_reports_true_system_commit() -> None:
    counters = _WindowsCounters()
    host = counters.host()
    assert (
        host["system_commit_total_bytes"] + host["system_available_commit_bytes"]
        == (host["system_commit_limit_bytes"])
    )
    assert host["system_commit_limit_bytes"] > 0
    assert host["commit_limit"] > 0
    assert host["available_commit"] >= 0
    assert host["total_physical"] > 0
    assert host["observed_at_unix_ns"] > 0
    assert host["observation_span_ns"] >= 0
    # Cross-API caller/system values are deliberately not asserted simultaneous.


@pytest.mark.skipif(os.name != "nt", reason="actual Windows API instrumentation")
@pytest.mark.parametrize("api", ["GlobalMemoryStatusEx", "K32GetPerformanceInfo"])
def test_failed_windows_query_propagates_without_fabricated_numeric_image(
    monkeypatch: pytest.MonkeyPatch, api: str
) -> None:
    counters = _WindowsCounters()
    failed = Mock(return_value=0)
    monkeypatch.setattr(counters.kernel, api, failed)
    monkeypatch.setattr(ctypes, "get_last_error", lambda: 5)
    with pytest.raises(OSError, match=r"\[WinError 5\]"):
        counters.host()
    failed.assert_called_once()


@pytest.mark.skipif(os.name != "nt", reason="actual Windows API instrumentation")
def test_actual_repeated_host_queries_open_no_process_handles() -> None:
    # Run serially without other same-PID workers; these APIs create no handles.
    counters = _WindowsCounters()
    counters.host()
    process = counters.kernel.GetCurrentProcess()
    before = counters.snapshot(process)
    for _ in range(64):
        counters.host()
    after = counters.snapshot(process)
    assert after["creation_ticks"] == before["creation_ticks"]
    assert after["handle_count"] == before["handle_count"]
