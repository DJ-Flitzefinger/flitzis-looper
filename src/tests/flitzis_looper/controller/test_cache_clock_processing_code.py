"""Actual cache and captured-clock binding evidence before causal negatives."""

import os
import time
from pathlib import Path
from types import FunctionType, SimpleNamespace

import pytest

from flitzis_looper import system_resources
from flitzis_looper.controller import processing_code, stem_provenance, stem_resources


def _manifests() -> dict:
    paths = stem_provenance._host_processing_paths(SimpleNamespace(separator="demucs:htdemucs"))
    return {path.resolve(): processing_code._manifest(path) for path in paths}


def _namespace(module, manifests):
    return processing_code._check_namespace(
        vars(module), manifests[Path(module.__file__).resolve()], manifests
    )


@pytest.mark.skipif(os.name != "nt", reason="actual Windows configured wrapper")
def test_actual_kernel_cache_identity_is_stable_when_warmed_and_cleared() -> None:
    manifests = _manifests()
    system_resources._WINDOWS_KERNEL.clear()
    try:
        cold = _namespace(system_resources, manifests)
        assert cold
        system_resources._windows_kernel()
        assert _namespace(system_resources, manifests) == cold
        system_resources._WINDOWS_KERNEL.clear()
        assert _namespace(system_resources, manifests) == cold
    finally:
        system_resources._WINDOWS_KERNEL.clear()


@pytest.mark.parametrize("method", ["__init__", "get", "clear"])
def test_actual_cache_method_mutation_rejects_after_same_component_positive(
    monkeypatch: pytest.MonkeyPatch, method: str
) -> None:
    manifests = _manifests()
    assert _namespace(system_resources, manifests)
    original = getattr(system_resources._WindowsKernelCache, method)
    changed = FunctionType((lambda _self: None).__code__, vars(system_resources))
    changed.__qualname__ = original.__qualname__
    monkeypatch.setattr(system_resources._WindowsKernelCache, method, changed)
    with pytest.raises(ValueError, match="loaded host function differs"):
        _namespace(system_resources, manifests)


def test_actual_initializer_mutation_rejects_after_same_component_positive(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    manifests = _manifests()
    assert _namespace(system_resources, manifests)
    original = system_resources._configure_windows_kernel
    changed = FunctionType(
        (lambda: None).__code__.replace(co_filename=original.__code__.co_filename),
        vars(system_resources),
    )
    changed.__qualname__ = system_resources._configure_windows_kernel.__qualname__
    monkeypatch.setattr(system_resources, "_configure_windows_kernel", changed)
    with pytest.raises(ValueError, match="loaded host function differs"):
        _namespace(system_resources, manifests)


@pytest.mark.parametrize(
    "binding",
    [
        "carrier",
        "lock",
        "constructor",
        "method-globals",
        "initializer-globals",
        "kernel-slot",
        "lock-slot",
    ],
)
def test_actual_cache_carrier_binding_rejects_after_same_component_positive(
    monkeypatch: pytest.MonkeyPatch, binding: str
) -> None:
    manifests = _manifests()
    assert _namespace(system_resources, manifests)
    if binding == "carrier":
        original = system_resources._WINDOWS_KERNEL
        monkeypatch.setattr(
            system_resources,
            "_WINDOWS_KERNEL",
            SimpleNamespace(get=original.get, clear=original.clear),
        )
    elif binding == "lock":
        monkeypatch.setattr(system_resources._WINDOWS_KERNEL, "_lock", object())
    elif binding == "constructor":
        monkeypatch.setattr(system_resources, "Lock", lambda: None)
    elif binding in {"kernel-slot", "lock-slot"}:
        kind = system_resources._WindowsKernelCache
        target, donor = ("_kernel", "_lock") if binding == "kernel-slot" else ("_lock", "_kernel")
        monkeypatch.setattr(kind, target, getattr(kind, donor))
    else:
        original = (
            system_resources._configure_windows_kernel
            if binding == "initializer-globals"
            else system_resources._WindowsKernelCache.get
        )
        cloned = FunctionType(original.__code__, dict(original.__globals__), original.__name__)
        cloned.__qualname__ = original.__qualname__
        if binding == "initializer-globals":
            monkeypatch.setattr(system_resources, "_configure_windows_kernel", cloned)
        else:
            monkeypatch.setattr(system_resources._WindowsKernelCache, "get", cloned)
    with pytest.raises(ValueError, match="kernel cache"):
        _namespace(system_resources, manifests)


def test_captured_default_remains_original_after_module_clock_is_changed(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    manifests = _manifests()
    captured = stem_resources.StemResourceGuard.__init__.__defaults__[1]
    before = _namespace(stem_resources, manifests)
    assert before
    monkeypatch.setattr(time, "monotonic", lambda: -100.0)
    assert stem_resources.StemResourceGuard(sampler=lambda: None)._clock is captured
    assert _namespace(stem_resources, manifests) == before

    def injected() -> float:
        return 7.0

    assert stem_resources.StemResourceGuard(sampler=lambda: None, clock=injected)._clock is injected


def test_replaced_actual_clock_default_rejects_after_same_component_positive(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    manifests = _manifests()
    assert _namespace(stem_resources, manifests)
    constructor = stem_resources.StemResourceGuard.__init__
    sampler, _clock = constructor.__defaults__
    monkeypatch.setattr(constructor, "__defaults__", (sampler, lambda: 0.0))
    with pytest.raises(ValueError, match="independently captured binding"):
        _namespace(stem_resources, manifests)


def test_clock_binding_policy_does_not_admit_arbitrary_builtin_defaults() -> None:
    with pytest.raises(ValueError, match="unsupported mutable or executable host binding"):
        processing_code._binding(time.monotonic, _manifests())
