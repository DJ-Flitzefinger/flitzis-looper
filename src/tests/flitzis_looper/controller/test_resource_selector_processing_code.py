"""Actual resource declarations and consulted selector roles, without inference."""

import ast
import ctypes
import os
import time
from ctypes import wintypes
from pathlib import Path
from types import ModuleType, SimpleNamespace

import pytest

from flitzis_looper import system_resources
from flitzis_looper.controller import (
    processing_code,
    stem_provenance,
    stem_resources,
    stem_separators,
)


def _manifests(separator: str) -> dict:
    paths = stem_provenance._host_processing_paths(SimpleNamespace(separator=separator))
    assert len(paths) == len(set(paths))
    return {path.resolve(): processing_code._manifest(path) for path in paths}


def _namespace(module: ModuleType, manifests: dict) -> list[object]:
    return processing_code._check_namespace(
        vars(module), manifests[Path(module.__file__).resolve()], manifests
    )


def _layout(kind: type[ctypes.Structure]) -> list[object]:
    tree, _codes = processing_code._manifest(Path(system_resources.__file__))
    declaration = next(
        item for item in tree.body if isinstance(item, ast.ClassDef) and item.name == kind.__name__
    )
    return processing_code._ctypes_layout(kind, declaration, vars(system_resources))


def test_actual_memory_layouts_match_original_win32_abi() -> None:
    # Independent original ABI declarations, including the original scalar name.
    memory = (
        ("length", wintypes.DWORD),
        ("load", wintypes.DWORD),
        *(
            (name, ctypes.c_ulonglong)
            for name in (
                "total_physical",
                "available_physical",
                "commit_limit",
                "available_commit",
                "total_virtual",
                "available_virtual",
                "available_extended_virtual",
            )
        ),
    )
    performance = (
        ("size", wintypes.DWORD),
        *(
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
        ),
        ("handles", wintypes.DWORD),
        ("processes", wintypes.DWORD),
        ("threads", wintypes.DWORD),
    )
    assert ctypes.c_uint64 is ctypes.c_ulonglong
    for kind, original in (
        (system_resources.HostMemory, memory),
        (system_resources.PerformanceInformation, performance),
    ):
        reference = type("OriginalMemoryLayout", (ctypes.Structure,), {"_fields_": original})
        assert _layout(kind)
        assert kind._fields_ == original
        assert ctypes.sizeof(kind) == ctypes.sizeof(reference)
        assert ctypes.alignment(kind) == ctypes.alignment(reference)
        for name, _field_type in original:
            actual, expected = getattr(kind, name), getattr(reference, name)
            assert (actual.offset, actual.size) == (expected.offset, expected.size)
    if os.name == "nt" and ctypes.sizeof(ctypes.c_void_p) == 8:
        assert ctypes.sizeof(system_resources.HostMemory) == 64
        assert ctypes.sizeof(system_resources.PerformanceInformation) == 104


@pytest.mark.parametrize("binding", ["_fields_", "available_commit"])
def test_changed_actual_memory_fields_or_descriptor_refuse_layout(
    monkeypatch: pytest.MonkeyPatch, binding: str
) -> None:
    kind = system_resources.HostMemory
    assert _layout(kind)
    if binding == "_fields_":
        kind = type(
            kind.__name__, (ctypes.Structure,), {"_fields_": tuple(reversed(kind._fields_))}
        )
    else:
        monkeypatch.setattr(kind, binding, kind.commit_limit)
    with pytest.raises(ValueError, match=r"loaded host ctypes (fields|descriptor) differ"):
        _layout(kind)


@pytest.mark.parametrize(
    "name",
    [
        "_MIB",
        "_ADMISSION_COMMIT",
        "_RECOVERY_COMMIT",
        "_ADMISSION_PHYSICAL",
        "_RECOVERY_PHYSICAL",
        "_CRITICAL_COMMIT",
        "_CRITICAL_PHYSICAL",
        "_SAMPLE_SECONDS",
        "_UNAVAILABLE_SECONDS",
    ],
)
def test_actual_resource_constants_positive_then_changed_policy_refuses(
    monkeypatch: pytest.MonkeyPatch, name: str
) -> None:
    tree, _codes = processing_code._manifest(Path(stem_resources.__file__))
    assert name in processing_code._check_constants(tree.body, vars(stem_resources))
    monkeypatch.setattr(stem_resources, name, getattr(stem_resources, name) + 1)
    with pytest.raises(ValueError, match="constant differs from held source"):
        processing_code._check_constants(tree.body, vars(stem_resources))


def test_resource_defaults_retain_original_captured_sampler_and_clock() -> None:
    assert stem_resources.StemResourceGuard.__init__.__defaults__ == (
        system_resources.host_memory_snapshot,
        time.monotonic,
    )


@pytest.mark.parametrize("separator", ["demucs:htdemucs", "bs-roformer:musdb18hq"])
@pytest.mark.parametrize(
    "missing", ["bs_roformer_generation.py", "bs_roformer_assets.py", "bs_input_provenance.py"]
)
def test_selector_consulted_roles_positive_then_missing_role_refuses(
    separator: str, missing: str
) -> None:
    manifests = _manifests(separator)
    assert _namespace(stem_separators, manifests)
    del manifests[next(path for path in manifests if path.name == missing)]
    with pytest.raises(ValueError, match="outside the guarded producer roles"):
        _namespace(stem_separators, manifests)


@pytest.mark.parametrize("separator", ["demucs:htdemucs", "bs-roformer:musdb18hq"])
def test_selector_changed_bs_binding_refuses_for_either_selection(
    monkeypatch: pytest.MonkeyPatch, separator: str
) -> None:
    manifests = _manifests(separator)
    assert _namespace(stem_separators, manifests)
    monkeypatch.setattr(stem_separators, "BsRoformerStemGenerationBackend", object())
    with pytest.raises(ValueError, match="no longer names its measured callable"):
        _namespace(stem_separators, manifests)
