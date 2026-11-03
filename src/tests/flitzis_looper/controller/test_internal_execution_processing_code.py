"""Actual internal host roles and emitter bindings, without learned inference."""

import importlib
from contextlib import contextmanager
from dataclasses import replace
from pathlib import Path
from types import ModuleType, SimpleNamespace

import pytest

from flitzis_looper import demucs_streaming
from flitzis_looper import stem_execution as public
from flitzis_looper import stem_execution_protocol as protocol
from flitzis_looper.controller import (
    bs_roformer_generation,
    processing_code,
    stem_generation,
    stem_provenance,
    stem_provider,
)
from flitzis_looper.controller.bs_roformer_generation import BsRoformerStemGenerationBackend
from flitzis_looper.controller.processing_code import loaded_code_identity
from tests.conftest import write_mono_pcm16_wav
from tests.flitzis_looper.controller.test_stem_generation import _install_htdemucs_checkpoint
from tests.flitzis_looper.controller.test_stem_identity import _request
from tests.flitzis_looper.controller.test_stem_separators import _install_test_assets
from tests.flitzis_looper.controller.test_stem_separators import _request as _bs_request


@pytest.mark.parametrize("separator", ["demucs:htdemucs", "bs-roformer:musdb18hq"])
def test_actual_receipt_roles_bind_codec_while_unrelated_full_host_stays_unknown(
    separator: str,
) -> None:
    paths = stem_provenance._host_processing_paths(replace(_request(), separator=separator))
    assert Path(protocol.__file__) in paths
    assert Path(public.__file__) not in paths
    manifests = {path.resolve(): processing_code._manifest(path) for path in paths}
    for module in (protocol, stem_generation, stem_provider):
        assert _namespace(module, manifests)
    if separator == "bs-roformer:musdb18hq":
        assert _namespace(bs_roformer_generation, manifests)
    assert loaded_code_identity((Path(protocol.__file__),)) is not None
    assert loaded_code_identity(paths) is None
    for name, error in (
        ("system_resources.py", "host ctypes fields require supported direct tuple declarations"),
        ("stem_resources.py", "host default or constant requires unmeasured execution"),
    ):
        path = next(path for path in paths if path.name == name)
        module = processing_code._loaded_at(path.resolve())[0]
        with pytest.raises(ValueError, match=error):
            _namespace(module, manifests)
    del manifests[Path(protocol.__file__).resolve()]
    with pytest.raises(ValueError, match="outside the guarded producer roles"):
        _namespace(stem_generation, manifests)


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
    assert _namespace(stem_generation, manifests)
    monkeypatch.setattr(stem_generation, "completed_execution", public.completed_execution)
    with pytest.raises(ValueError, match="outside the guarded producer roles"):
        _namespace(stem_generation, manifests)


def _poison_public(monkeypatch: pytest.MonkeyPatch) -> None:
    def forbidden(*_args: object, **_kwargs: object) -> None:
        pytest.fail("Producer used the public Pydantic facade")

    for name in ("completed_execution", "completed_execution_line", "report_execution"):
        monkeypatch.setattr(public, name, forbidden)
    for name in ("model_validate", "model_validate_json", "model_dump_json", "model_copy"):
        monkeypatch.setattr(public.StemExecution, name, forbidden)


@pytest.mark.parametrize("separator", ["demucs:htdemucs", "bs-roformer:musdb18hq"])
def test_actual_backend_parser_and_internal_progress_ignore_public_facade(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, separator: str
) -> None:
    request = replace(_bs_request(tmp_path), separator=separator, device_policy="cpu")
    if separator.startswith("demucs"):
        _install_htdemucs_checkpoint(request.model_cache_dir)
        backend_name, model = "demucs", "htdemucs"
        backend_type = stem_generation.DemucsStemGenerationBackend
    else:
        _install_test_assets(request.model_cache_dir, monkeypatch)
        backend_name, model = "bs-roformer", "BS-RoFormer MUSDB18HQ"
        backend_type = BsRoformerStemGenerationBackend

    def runner(args: list[str], *, cwd: Path, env: dict[str, str]) -> stem_generation.CommandResult:
        if args[0] in {"ffmpeg", "ffprobe"} or "-c" in args:
            return stem_generation.CommandResult(0, "", "")
        root = Path(args[args.index("-o" if backend_name == "demucs" else "--output") + 1])
        for name in ("bass", "drums", "other", "vocals"):
            write_mono_pcm16_wav(root / f"{name}.wav", 44_100)
        return stem_generation.CommandResult(
            0, protocol.completed_execution_line(backend_name, model, "cpu"), ""
        )

    _poison_public(monkeypatch)
    events = []
    backend = backend_type(command_runner=runner)
    result = backend.generate(request, protocol.ExecutionProgress(lambda p, s, e: events.append(e)))
    assert isinstance(result.execution, protocol.ExecutionReceipt)
    assert result.execution in events
    assert result.execution.device == "cpu"
    assert not result.encoder_verified
    assert not result.decoder_verified


def test_actual_demucs_completion_emitter_ignores_public_facade(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    @contextmanager
    def cli(_cancellation: object):
        yield SimpleNamespace(devices={"cpu"}, model_name="htdemucs")

    _poison_public(monkeypatch)
    monkeypatch.setattr(demucs_streaming, "configure_child", lambda *_: (1, 1))
    monkeypatch.setattr(demucs_streaming, "_cancellable_cli", cli)
    monkeypatch.setattr(demucs_streaming._separate, "main", lambda: None)
    monkeypatch.setattr(demucs_streaming, "encoder_completion_line", lambda: "diagnostic only")
    demucs_streaming.main()
    assert (
        protocol.completed_execution(
            capsys.readouterr().out,
            backend="demucs",
            model_name="htdemucs",
            device="cpu",
            cpu_fallback=False,
        )
        is not None
    )


def test_actual_bs_completion_emitter_ignores_public_facade(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    emitter = importlib.import_module("flitzis_bs_roformer.__main__")
    _poison_public(monkeypatch)
    monkeypatch.setattr(emitter, "configure_child", lambda *_: (1, 1))
    monkeypatch.setattr(emitter, "load_model", lambda *_: object())
    monkeypatch.setattr(emitter, "decode_source", lambda *_args, **_kwargs: 2)
    monkeypatch.setattr(emitter, "separate_pcm", lambda *_args, **_kwargs: None)
    monkeypatch.setattr(emitter, "input_completion_line", lambda: "diagnostic only")
    monkeypatch.setattr(
        emitter.sys,
        "argv",
        [
            "bs",
            "--source",
            str(tmp_path / "source"),
            "--output",
            str(tmp_path / "out"),
            "--checkpoint",
            "unused",
            "--config",
            "unused",
            "--device",
            "cpu",
        ],
    )
    emitter.main()
    assert (
        protocol.completed_execution(
            capsys.readouterr().out,
            backend="bs-roformer",
            model_name="BS-RoFormer MUSDB18HQ",
            device="cpu",
            cpu_fallback=False,
        )
        is not None
    )
