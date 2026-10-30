"""Mock source setup orchestration; no package/model downloads, inference or devices."""

import importlib.util
import json
import os
import subprocess
from pathlib import Path
from typing import TYPE_CHECKING
from unittest.mock import Mock

import pytest

from flitzis_looper.analysis.contracts import BeatModelIdentity
from flitzis_looper.analysis.worker import WorkerConfiguration

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
    return module


def configuration(root: Path) -> WorkerConfiguration:
    return WorkerConfiguration(
        root / ".venv/Scripts/python.exe",
        root / "worker.py",
        root / "final0.ckpt",
        root / "scratch",
        BeatModelIdentity(),
        root / "manifest.json",
    )


def test_source_main_defaults_to_repo_owned_required_worker_and_optional_stems(
    source_setup: ModuleType,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    root = tmp_path / "clone"
    monkeypatch.setattr(source_setup, "__file__", str(root / "scripts/setup-runtime.py"))
    worker = Mock(return_value=configuration(root))
    stems = Mock()
    monkeypatch.setattr(source_setup, "prepare_worker", worker)
    monkeypatch.setattr(source_setup, "prepare_stems", stems)
    assert source_setup.main(["--check", "--with-stems"]) == 0
    worker.assert_called_once_with(
        root / "models/beat-this", root / ".cache/runtime-setup", check=True, profile=None
    )
    stems.assert_called_once_with(root, check=True)
    report = json.loads(capsys.readouterr().out)
    assert report["source_runtime"] == "passed"
    assert report["inference_executed"] is False


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
    monkeypatch.setattr(source_setup, "prepare_worker", prepare)
    try:
        assert source_setup.main([]) == 1
        prepare.assert_not_called()
        assert list(recordings.iterdir()) == [sentinel]
        assert sentinel.read_bytes() == b"unchanged original recording"
        assert not (root / "models/beat-this").exists()
    finally:
        alias.rmdir() if os.name == "nt" else alias.unlink()


def test_fresh_worker_inspection_has_only_the_existing_local_import_timeout(
    source_setup: ModuleType, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    inspector = Mock(side_effect=subprocess.TimeoutExpired("worker-inspection", 120))
    monkeypatch.setattr(source_setup.subprocess, "run", inspector)
    with pytest.raises(subprocess.TimeoutExpired):
        source_setup.inspect_worker(configuration(tmp_path), tmp_path)
    assert inspector.call_args.kwargs["timeout"] == 120


def test_worker_repair_checks_fresh_environment_before_selecting_new_pointer(
    source_setup: ModuleType,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    current = configuration(tmp_path / "installation-current")
    staged = configuration(tmp_path / "installation-staged")
    steps: list[str] = []
    monkeypatch.setattr(source_setup, "selected_profile", lambda _: "cuda-auto-v2")
    monkeypatch.setattr(
        source_setup.setup, "load_worker_configuration", Mock(side_effect=ValueError)
    )

    def install_worker(*_args: object, **_kwargs: object) -> WorkerConfiguration:
        steps.append("stage")
        return staged

    def activate_worker(*_args: object) -> WorkerConfiguration:
        steps.append("activate")
        return current

    install = Mock(side_effect=install_worker)
    monkeypatch.setattr(source_setup.setup, "install_worker", install)
    monkeypatch.setattr(source_setup, "inspect_worker", lambda *_: steps.append("inspect"))
    activate = Mock(side_effect=activate_worker)
    monkeypatch.setattr(source_setup.setup, "activate_worker_installation", activate)
    root, scratch = tmp_path / "models/beat-this", tmp_path / ".cache/setup"
    assert source_setup.prepare_worker(root, scratch, check=False, profile=None) == current
    assert steps == ["stage", "inspect", "activate"]
    install.assert_called_once_with(root, scratch, profile="cuda-auto-v2", stage_only=True)


def test_failed_fresh_inspection_never_activates_and_check_never_acquires(
    source_setup: ModuleType,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(source_setup, "selected_profile", lambda _: "legacy-cpu-v1")
    monkeypatch.setattr(
        source_setup.setup,
        "load_worker_configuration",
        Mock(side_effect=ValueError("missing worker")),
    )
    installer = Mock(return_value=configuration(tmp_path / "staged"))
    activation = Mock()
    monkeypatch.setattr(source_setup.setup, "install_worker", installer)
    monkeypatch.setattr(source_setup.setup, "activate_worker_installation", activation)
    monkeypatch.setattr(
        source_setup, "inspect_worker", Mock(side_effect=ValueError("invalid environment"))
    )
    with pytest.raises(ValueError, match="invalid environment"):
        source_setup.prepare_worker(
            tmp_path / "models", tmp_path / "scratch", check=False, profile=None
        )
    activation.assert_not_called()
    installer.reset_mock()
    with pytest.raises(ValueError, match="missing worker"):
        source_setup.prepare_worker(
            tmp_path / "models", tmp_path / "scratch", check=True, profile=None
        )
    installer.assert_not_called()
    activation.assert_not_called()


def test_source_stem_check_respects_external_override_and_uses_guarded_fresh_children(
    source_setup: ModuleType,
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    cache = tmp_path / "external-model-cache/torch/hub/checkpoints"
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
