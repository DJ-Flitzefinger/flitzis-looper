"""Mock source setup orchestration; no package/model downloads, inference or devices."""

import importlib.util
import json
import os
import subprocess
from pathlib import Path
from types import SimpleNamespace
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.analysis import beat_cuda_cache
from flitzis_looper.bs_onnx_cuda_cache import BsCudaCacheStatus

if TYPE_CHECKING:
    from types import ModuleType


@pytest.fixture
def source_setup() -> ModuleType:
    path = Path(__file__).resolve().parents[2] / "scripts/setup-runtime.py"
    spec = importlib.util.spec_from_file_location("source_runtime_setup", path)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    module.production_windows = lambda: True
    return module


def configuration(root: Path) -> SimpleNamespace:
    return SimpleNamespace(manifest=root / "manifest.json")


def test_source_main_defaults_to_repo_owned_required_worker_and_optional_stems(
    source_setup: ModuleType,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    root = tmp_path / "clone"
    monkeypatch.setattr(source_setup, "__file__", str(root / "scripts/setup-runtime.py"))
    worker = Mock(return_value=(configuration(root), {"warmup_executed": False}))
    stems = Mock(return_value=None)
    monkeypatch.setattr(source_setup, "prepare_windows_worker", worker)
    monkeypatch.setattr(source_setup, "prepare_stems", stems)
    assert source_setup.main(["--check", "--with-stems"]) == 0
    worker.assert_called_once_with(
        root, root / "models/beat-this", root / ".cache/runtime-setup", check=True
    )
    stems.assert_called_once_with(root, check=True)
    report = json.loads(capsys.readouterr().out)
    assert report["source_runtime"] == "passed"
    assert report["inference_executed"] is False
    assert report["bs_roformer_cuda_warmup_executed"] is False


def test_source_reports_bs_cuda_warmup_from_this_invocation(
    source_setup: ModuleType,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    monkeypatch.setattr(source_setup, "__file__", str(tmp_path / "scripts/setup-runtime.py"))
    monkeypatch.setattr(source_setup, "production_windows", lambda: True)
    monkeypatch.setattr(
        source_setup,
        "prepare_windows_worker",
        Mock(return_value=(configuration(tmp_path), {"warmup_executed": False})),
    )
    monkeypatch.setattr(
        source_setup,
        "prepare_stems",
        Mock(return_value=BsCudaCacheStatus("ready", "prepared", warmup_executed=True)),
    )

    assert source_setup.main(["--with-stems"]) == 0
    report = json.loads(capsys.readouterr().out)
    assert report["inference_executed"] is True
    assert report["bs_roformer_cuda_warmup_executed"] is True


def test_windows_check_uses_onnx_and_does_not_warm_cuda(
    source_setup: ModuleType, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    configuration = Mock()
    load = Mock(return_value=configuration)
    warm = Mock()
    status = {"status": "pending", "warmup_executed": False}
    monkeypatch.setattr(source_setup, "load_production_worker", load)
    monkeypatch.setattr(source_setup, "require_native_worker_capability", Mock())
    monkeypatch.setattr(beat_cuda_cache, "cuda_cache_status", Mock(return_value=status))
    monkeypatch.setattr(beat_cuda_cache, "ensure_cuda_cache", warm)
    worker_root = tmp_path / "models/beat-this"
    scratch = tmp_path / ".cache/runtime-setup"
    assert source_setup.prepare_windows_worker(tmp_path, worker_root, scratch, check=True) == (
        configuration,
        status,
    )
    load.assert_called_once_with(worker_root, scratch)
    warm.assert_not_called()


@pytest.mark.parametrize(
    ("aliased", "target_name"),
    [
        ("models", "record"),
        (".cache", "record"),
        ("record", "models"),
        ("record", ".cache"),
    ],
)
def test_source_model_and_scratch_aliases_fail_before_setup_or_directory_creation(
    source_setup: ModuleType,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    aliased: str,
    target_name: str,
) -> None:
    root = tmp_path / "clone"
    recordings = root / target_name
    recordings.mkdir(parents=True)
    sentinel = recordings / "original.wav"
    sentinel.write_bytes(b"unchanged original recording")
    alias = root / aliased
    if os.name == "nt":
        result = subprocess.run(
            ["cmd.exe", "/d", "/c", "mklink", "/J", str(alias), str(recordings)],
            capture_output=True,
            text=True,
            check=False,
        )
        assert result.returncode == 0, result.stdout + result.stderr
    else:
        alias.symlink_to(recordings, target_is_directory=True)
    monkeypatch.setattr(source_setup, "__file__", str(root / "scripts/setup-runtime.py"))
    prepare = Mock()
    monkeypatch.setattr(source_setup, "prepare_windows_worker", prepare)
    try:
        assert source_setup.main([]) == 1
        prepare.assert_not_called()
        assert list(recordings.iterdir()) == [sentinel]
        assert sentinel.read_bytes() == b"unchanged original recording"
        assert not (root / "models/beat-this").exists()
    finally:
        alias.rmdir() if os.name == "nt" else alias.unlink()


def test_source_stem_check_respects_external_override_and_uses_guarded_fresh_children(
    source_setup: ModuleType,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(source_setup, "check_stem_codecs", Mock())
    cache = tmp_path / "external-model-cache"
    monkeypatch.setenv("FLITZIS_MODEL_CACHE_DIR", str(cache))
    verifier = Mock()
    monkeypatch.setattr(source_setup, "verify_asset", verifier)
    bs_verify = Mock()
    monkeypatch.setattr(source_setup.bs_roformer_assets, "verify_model_assets", bs_verify)
    acquire = Mock(side_effect=AssertionError("check attempted download"))
    monkeypatch.setattr(source_setup, "acquire_pinned_asset", acquire)
    child = Mock(return_value=subprocess.CompletedProcess([], 0))
    monkeypatch.setattr(source_setup.subprocess, "run", child)
    source_setup.prepare_stems(tmp_path / "clone", check=True)
    assert verifier.call_args.args[0] == cache / "955717e8-8726e21a.th"
    bs_verify.assert_called_once_with(cache / "bs-roformer-musdb18hq")
    command = child.call_args.args[0]
    assert command[-1] == "--source-clone"
    assert command[command.index("--mode") + 1] == "check"
    assert child.call_args.kwargs["env"]["FLITZIS_MODEL_CACHE_DIR"] == os.fspath(cache)
    acquire.assert_not_called()


@pytest.mark.parametrize("has_shared_build", [False, True])
def test_source_stem_setup_preflights_the_capture_codecs_before_model_io(
    source_setup: ModuleType,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    *,
    has_shared_build: bool,
) -> None:
    monkeypatch.delenv("FLITZIS_MODEL_CACHE_DIR", raising=False)
    root = tmp_path / "clone"
    cache = root / "models/stem-checkpoints"
    executable = tmp_path / "codec/ffmpeg.exe"
    resolve = Mock(return_value=executable)
    contract = Mock(return_value="verified tool contract" if has_shared_build else None)
    prepare = Mock()
    launch = Mock(side_effect=[None, subprocess.CalledProcessError(1, "ffprobe.exe")])
    monkeypatch.setattr(source_setup, "resolved_demucs_encoder", resolve)
    monkeypatch.setattr(source_setup, "bs_input_contract", contract)
    monkeypatch.setattr(source_setup, "acquire_pinned_asset", prepare)
    monkeypatch.setattr(source_setup.subprocess, "run", launch)
    error = subprocess.CalledProcessError if has_shared_build else ValueError

    with pytest.raises(error):
        source_setup.prepare_stems(root, check=False)

    resolve.assert_called_once_with(cache)
    contract.assert_called_once_with(executable)
    prepare.assert_not_called()
    if has_shared_build:
        assert [call.args[0] for call in launch.call_args_list] == [
            [str(executable), "-version"],
            [str(executable.with_name("ffprobe.exe")), "-version"],
        ]
    else:
        launch.assert_not_called()


def test_windows_source_stem_setup_publishes_only_staged_worker(
    source_setup: ModuleType,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    root = tmp_path / "clone"
    monkeypatch.setattr(source_setup, "check_stem_codecs", Mock())
    scratch = root / ".cache/runtime-setup"
    model_root = root / "models/bs-roformer-onnx"
    cache = tmp_path / "external-weights"
    monkeypatch.setenv("FLITZIS_MODEL_CACHE_DIR", str(cache))
    monkeypatch.setattr(source_setup, "production_windows", lambda: True)
    monkeypatch.setattr(source_setup, "acquire_pinned_asset", Mock())
    monkeypatch.setattr(source_setup.bs_roformer_assets, "install_model_assets", Mock())
    worker = scratch / "bs-worker.exe"
    ht_worker = scratch / "htdemucs-worker.exe"
    crt = scratch / "crt"
    monkeypatch.setattr(source_setup, "build_htdemucs_onnx_worker", Mock(return_value=ht_worker))
    monkeypatch.setattr(source_setup, "htdemucs_source_crt", Mock(return_value=crt))
    stage_htdemucs = Mock()
    monkeypatch.setattr(source_setup, "stage_htdemucs_onnx_installation", stage_htdemucs)
    build = Mock(return_value=worker)
    stage = Mock()
    preparation = BsCudaCacheStatus("ready", "prepared", warmup_executed=True)

    def staged(*_args: object, **options: object) -> object:
        observe = options["cuda_preparation"]
        assert callable(observe)
        observe(preparation)
        return stage.return_value

    stage.side_effect = staged
    select = Mock()
    child = Mock()
    monkeypatch.setattr(source_setup, "build_bs_onnx_worker", build)
    monkeypatch.setattr(source_setup, "stage_bs_onnx_installation", stage)
    monkeypatch.setattr(source_setup, "select_bs_onnx_installation", select)
    monkeypatch.setattr(source_setup.subprocess, "run", child)

    assert source_setup.prepare_stems(root, check=False) is preparation

    build.assert_called_once_with(root, scratch)
    stage.assert_called_once()
    assert stage.call_args.args == (
        root,
        model_root,
        worker,
        cache / "bs-roformer-musdb18hq",
        scratch / "beat-this-downloads",
    )
    select.assert_called_once_with(model_root, stage.return_value)
    stage_htdemucs.assert_called_once_with(
        root, cache, ht_worker, scratch / "beat-this-downloads", crt_dir=crt, local_build=True
    )
    child.assert_not_called()

    stage.side_effect = RuntimeError("CPU load failed")
    select.reset_mock()
    child.reset_mock()
    with pytest.raises(RuntimeError, match="CPU load failed"):
        source_setup.prepare_stems(root, check=False)
    select.assert_not_called()
    child.assert_not_called()
