"""Reject diagnostic ambiguity without running a learned model."""

import json
from types import SimpleNamespace

import pytest

from tests.flitzis_looper.controller.inference_overlap_probe import (
    PREFIX,
    _CallObservation,
    model_call,
)


@pytest.mark.parametrize(
    ("returncode", "reason"), [(101, "worker_crashed"), (1, "resource_pressure")]
)
def test_observer_leaves_original_worker_failure_visible(returncode: int, reason: str) -> None:
    observer = object.__new__(_CallObservation)
    observer.calls = []
    observer.command_results = []
    observer.diagnostic_errors = []
    result = SimpleNamespace(returncode=returncode, reason=reason, stdout=b"not a successful call")
    observer.returned(result)
    assert observer.command_results == [{"returncode": returncode, "reason": reason}]
    assert not observer.calls
    assert not observer.diagnostic_errors


def test_observer_defers_bad_success_diagnostic_to_main_verification() -> None:
    observer = object.__new__(_CallObservation)
    observer.calls = []
    observer.command_results = []
    observer.diagnostic_errors = []
    observer.returned(SimpleNamespace(returncode=0, reason="ok", stdout=b"missing"))
    assert not observer.calls
    assert len(observer.diagnostic_errors) == 1


@pytest.mark.parametrize(
    "damage",
    [
        "missing",
        "duplicate",
        "reverse",
        "bool",
        "cpu",
        "extra",
        "unterminated",
        "duplicate_key",
        "device",
        "nonobject",
    ],
)
def test_model_call_requires_one_complete_ordered_scalar_record(damage: str) -> None:
    call = {
        "pid": 1,
        "device": "cpu",
        "entered_ns": 2,
        "returned_ns": 3,
        "cpu_before_ns": 4,
        "cpu_after_ns": 5,
    }
    changes = {
        "reverse": ("returned_ns", 1),
        "bool": ("pid", True),
        "cpu": ("cpu_after_ns", 4),
        "extra": ("unknown", 7),
        "device": ("device", []),
    }
    if damage in changes:
        key, value = changes[damage]
        call[key] = value
    line = PREFIX + json.dumps(call).encode() + b"\n"
    if damage == "missing":
        line = b"model_ready\n"
    elif damage == "duplicate":
        line *= 2
    elif damage == "unterminated":
        line = line.rstrip(b"\n")
    elif damage == "duplicate_key":
        line = line.replace(b"{", b'{"pid":2,', 1)
    elif damage == "nonobject":
        line = PREFIX + b"[]\n"
    with pytest.raises(AssertionError):
        model_call(line)
