"""Actual internal host roles and emitter bindings, without learned inference."""

from dataclasses import replace
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

from flitzis_looper import stem_execution as public
from flitzis_looper import stem_execution_protocol as protocol
from flitzis_looper.controller import (
    processing_code,
    stem_generation,
    stem_provenance,
    stem_provider,
)
from flitzis_looper.controller.processing_code import loaded_code_identity
from tests.flitzis_looper.controller.test_stem_identity import _request

if TYPE_CHECKING:
    from types import ModuleType


@pytest.mark.parametrize("separator", ["demucs:htdemucs", "bs-roformer:musdb18hq"])
def test_actual_receipt_roles_and_every_host_binding_match_held_sources(
    separator: str,
) -> None:
    paths = stem_provenance._host_processing_paths(replace(_request(), separator=separator))
    assert Path(protocol.__file__) in paths
    assert Path(public.__file__) not in paths
    manifests = {path.resolve(): processing_code._manifest(path) for path in paths}
    for module in (protocol, stem_generation, stem_provider):
        assert _namespace(module, manifests)
    assert loaded_code_identity((Path(protocol.__file__),)) is not None
    # No whole-tuple positive until every actual loaded role passes independently.
    for path in paths:
        modules = processing_code._loaded_at(path.resolve())
        assert modules
        for module in modules:
            assert _namespace(module, manifests)
    assert loaded_code_identity(paths) is not None


def _namespace(module: ModuleType, manifests: dict) -> list[object]:
    return processing_code._check_namespace(
        vars(module), manifests[Path(module.__file__).resolve()], manifests
    )


@pytest.mark.parametrize("binding", ["__init__", "__post_init__"])
def test_mutated_internal_constructor_and_validation_refuse_source_identity(
    monkeypatch: pytest.MonkeyPatch, binding: str
) -> None:
    paths = (Path(protocol.__file__),)
    assert loaded_code_identity(paths) is not None
    monkeypatch.setattr(protocol.ExecutionReceipt, binding, lambda *_args, **_kwargs: None)
    assert loaded_code_identity(paths) is None


@pytest.mark.parametrize(
    "binding", ["completed_execution", "completed_execution_line", "ExecutionReceipt"]
)
def test_mutated_codec_bindings_refuse_source_identity(
    monkeypatch: pytest.MonkeyPatch, binding: str
) -> None:
    paths = (Path(protocol.__file__),)
    assert loaded_code_identity(paths) is not None
    monkeypatch.setattr(protocol, binding, object())
    assert loaded_code_identity(paths) is None


def test_mutated_parser_defaults_and_imported_binding_refuse_identity(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    paths = (Path(protocol.__file__),)
    assert loaded_code_identity(paths) is not None
    monkeypatch.setattr(
        protocol.completed_execution_line,
        "__kwdefaults__",
        {"threads": (1, 1), "cuda_peak_bytes": None},
    )
    assert loaded_code_identity(paths) is None
    monkeypatch.undo()
    manifests = {
        path.resolve(): processing_code._manifest(path)
        for path in stem_provenance._host_processing_paths(_request())
    }
    assert _namespace(stem_provider, manifests)
    monkeypatch.setattr(stem_provider, "request_fingerprint", public.completed_execution)
    with pytest.raises(ValueError, match="outside the guarded producer roles"):
        _namespace(stem_provider, manifests)
