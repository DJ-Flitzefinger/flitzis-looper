"""Validate UI accounting without constructing Native or predicting allocation success."""

import pytest

from flitzis_looper.controller.committed_stem_residency import resident_pcm_budget_pressure
from flitzis_looper.controller.stems import StemController


def _resident_status() -> dict[str, object]:
    return {
        "budget_mib": 0,
        "budget_bytes": 0,
        "held_unique_pcm_bytes": 4096,
        "pending_resident_bytes": 2048,
        "admitted_total_bytes": 6144,
        "unique_backing_count": 1,
        "pending_reservation_count": 1,
        "available_bytes": 0,
        "over_budget": False,
    }


@pytest.mark.parametrize(
    "host",
    [
        {
            "automatic": True,
            "host_pending_allocation_bytes": 1024,
            "host_available_physical_bytes": 987654321,
            "host_available_commit_bytes": 876543210,
            "host_allocation_allowance_bytes": 765432109,
            "host_memory_error": None,
        },
        {
            "automatic": False,
            "host_pending_allocation_bytes": 0,
            "host_available_physical_bytes": None,
            "host_available_commit_bytes": None,
            "host_allocation_allowance_bytes": None,
            "host_memory_error": "OS memory query failed",
        },
        {},
    ],
)
def test_residency_summary_preserves_exact_optional_host_status(host: dict[str, object]) -> None:
    original = _resident_status() | host
    sanitized = StemController._resident_accounting_counters(original)
    assert sanitized == original
    assert sanitized is not original
    assert sanitized["admitted_total_bytes"] == 6144
    if "host_memory_error" in host:
        assert sanitized["host_memory_error"] == host["host_memory_error"]
    else:
        assert "host_available_physical_bytes" not in sanitized


@pytest.mark.parametrize(
    ("key", "value"),
    [
        ("host_pending_allocation_bytes", None),
        ("host_pending_allocation_bytes", True),
        ("host_pending_allocation_bytes", -1),
        ("host_available_physical_bytes", False),
        ("host_available_physical_bytes", -1),
        ("host_available_commit_bytes", 1.5),
        ("host_available_commit_bytes", "1024"),
        ("host_allocation_allowance_bytes", -1),
        ("host_allocation_allowance_bytes", True),
        ("automatic", 0),
        ("automatic", "true"),
        ("automatic", None),
        ("host_memory_error", 0),
        ("host_memory_error", False),
    ],
)
def test_residency_summary_rejects_invalid_optional_status_without_inventing_counters(
    key: str, value: object
) -> None:
    with pytest.raises(TypeError, match=r"Native.*(accounting|memory)"):
        StemController._resident_accounting_counters(_resident_status() | {key: value})


@pytest.mark.parametrize(
    "message",
    [
        "[resident_pcm_budget_exceeded] requested=8192 budget=4096",
        "[resident_memory_pressure] insufficient host allocation headroom",
        "[resident_memory_unavailable] OS memory query failed",
    ],
)
def test_declared_resident_memory_refusal_uses_existing_recoverable_admission(message: str) -> None:
    assert resident_pcm_budget_pressure(message)


@pytest.mark.parametrize(
    "message",
    [
        "memory allocation failed",
        "I/O error reading complete PCM",
        "corrupt path contains [resident_memory_pressure]",
        "corrupt marker contains [resident_memory_unavailable]",
        "[resident_memory_pressure_other] invalid codec",
        None,
        0,
        MemoryError("allocation failed"),
    ],
)
def test_real_allocation_io_and_untyped_errors_remain_terminal(message: object) -> None:
    assert not resident_pcm_budget_pressure(message)
