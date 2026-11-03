"""Portable activation refuses Native builds lacking the prepared child owner."""

import runpy
from pathlib import Path
from types import SimpleNamespace

import pytest

REPO = Path(__file__).resolve().parents[2]


def _native() -> SimpleNamespace:
    native = SimpleNamespace(
        StemConversionReservation=type(
            "Converter", (), {"convert": lambda *_: None, "close": lambda *_: None}
        ),
        stem_conversion_identity=lambda: ("source-bound-aligned-pcm16-native-v3", "a" * 64),
        OfflineChildMemoryReservation=type(
            "Budget", (), {"limit_bytes": property(lambda _: 1), "close": lambda *_: None}
        ),
        PreparedWindowsProcess=type(
            "Prepared",
            (),
            dict.fromkeys(("start", "poll", "wait", "kill", "abort_startup"), lambda *_: None),
        ),
        offline_process_identity=lambda _: ("prepared-windows-process-v1", "b" * 64),
    )
    for name in ("stdout", "stderr", "pid", "returncode"):
        setattr(native.PreparedWindowsProcess, name, property(lambda _: None))
    return native


@pytest.mark.parametrize("missing", ["PreparedWindowsProcess", "offline_process_identity"])
def test_preflight_refuses_stale_native_before_worker_activation(missing: str) -> None:
    native = _native()
    delattr(native, missing)
    bootstrap = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))
    with pytest.raises(AssertionError, match="prepared Windows process owner missing"):
        bootstrap["check_stem_conversion_api"](native)


def test_preflight_checks_actual_constructor_identity_binding() -> None:
    native = _native()
    bindings = []

    def identity(actual: type) -> tuple[str, str]:
        bindings.append(actual)
        return "prepared-windows-process-v1", "b" * 64

    native.offline_process_identity = identity
    bootstrap = runpy.run_path(str(REPO / "scripts/portable/bootstrap.py"))
    bootstrap["check_stem_conversion_api"](native)
    assert bindings == [native.PreparedWindowsProcess]
