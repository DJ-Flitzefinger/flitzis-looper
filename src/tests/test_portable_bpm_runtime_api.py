"""Reject old extensions before any engine, worker or audio device is created."""

import runpy
from pathlib import Path
from types import SimpleNamespace

import pytest

REPO = Path(__file__).resolve().parents[2]


def _runtime_state(
    self: object,
    multi_loop: object,
    loaded: object,
    loop_starts: object,
    loop_ends: object,
    bindings: object = None,
    pitch_rows: object = None,
) -> None:
    pytest.fail("Preflight must never publish runtime state")


def bpm_runtime_native_module(engine_type: type = object) -> SimpleNamespace:
    engine = type(
        "BpmRuntimeEngine",
        (engine_type,),
        {"BPM_ONLY_RUNTIME_API_VERSION": 1, "set_input_runtime_state": _runtime_state},
    )
    return SimpleNamespace(AudioEngine=engine)


def test_bpm_runtime_preflight_creates_no_engine() -> None:
    native = bpm_runtime_native_module()
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))[
        "check_bpm_only_runtime_api"
    ]
    check(native)


@pytest.mark.parametrize("version", [None, False, True, 0, 2, "1", 1.0])
def test_old_or_ambiguous_native_requires_rebuild(version: object) -> None:
    native = bpm_runtime_native_module()
    native.AudioEngine.BPM_ONLY_RUNTIME_API_VERSION = version
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))[
        "check_bpm_only_runtime_api"
    ]
    with pytest.raises(AssertionError, match=r"start-release\.bat"):
        check(native)


def test_advertised_new_version_cannot_retain_map_or_adoption_api() -> None:
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))[
        "check_bpm_only_runtime_api"
    ]
    native = bpm_runtime_native_module()
    native.SourceBeatmap = object
    with pytest.raises(AssertionError, match="source-map editing"):
        check(native)
    del native.SourceBeatmap
    native.AudioEngine.request_input_runtime_policy_adoption = object()
    with pytest.raises(AssertionError, match="source-SYNC adoption"):
        check(native)


@pytest.mark.parametrize(
    "method", ["anchor_transport_phase_from_pad", "bootstrap_transport_from_pad"]
)
def test_bpm_runtime_rejects_unqualified_phase_control(method: str) -> None:
    native = bpm_runtime_native_module()
    setattr(native.AudioEngine, method, object())
    check = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))[
        "check_bpm_only_runtime_api"
    ]
    with pytest.raises(AssertionError, match="retired phase control"):
        check(native)
