"""Small Portable interpreter/namespace checks; no package/model acquisition."""

import importlib.util
import os
import shutil
import subprocess
import sys
from pathlib import Path
from types import ModuleType
from unittest.mock import Mock

import pytest

REPO = Path(__file__).resolve().parents[2]


def load_helper(name: str) -> ModuleType:
    specification = importlib.util.spec_from_file_location(
        "portable_" + name, REPO / "scripts/portable" / (name + ".py")
    )
    assert specification is not None
    assert specification.loader is not None
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module


@pytest.mark.skipif(os.name != "nt", reason="Windows private CPython layout")
def test_physical_worker_python_survives_move_without_its_provisioning_base(
    tmp_path: Path,
) -> None:
    helper = load_helper("prepare_analysis")
    candidate = os.environ.get("FLITZIS_TEST_BASE_PYTHON")
    if candidate is None:
        uv = shutil.which("uv")
        if uv is None:
            pytest.skip(
                "An existing registered CPython 3.12.13 is needed for this small copy fixture"
            )
        result = subprocess.run(
            [uv, "python", "find", "3.12.13", "--offline"],
            env=os.environ | {"UV_OFFLINE": "1", "UV_PYTHON_DOWNLOADS": "never"},
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode:
            pytest.skip("No existing CPython 3.12.13; fixture never downloads an interpreter")
        candidate = result.stdout.strip()
    base = Path(candidate).resolve(strict=True)
    original = tmp_path / "original"
    private = original / "tools/python"
    shutil.copytree(
        base.parent,
        private,
        ignore=shutil.ignore_patterns(
            "site-packages", "__pycache__", "include", "libs", "tcl", "Doc"
        ),
    )
    environment = original / "analysis-runtime/beat-this/installation-test/.venv"
    environment.joinpath("Lib/site-packages").mkdir(parents=True)
    helper.materialize_python(private / "python.exe", environment, original)
    private.rename(original / "tools/base-no-longer-available")
    moved = tmp_path / "moved Ã¼ different folder"
    moved.mkdir()
    relocated = moved / "worker"
    environment.rename(relocated)
    result = helper.verify_python(relocated, moved)
    assert Path(result["executable"]).resolve().is_relative_to(relocated.resolve())
    assert Path(result["base_prefix"]).resolve().is_relative_to(relocated.resolve())
    configuration = relocated.joinpath("pyvenv.cfg").read_text()
    assert "home" not in configuration
    assert "executable" not in configuration
    assert str(original) not in relocated.joinpath("Scripts/python312._pth").read_text()


def test_worker_python_inventory_requires_private_dll_and_relative_search_path(
    tmp_path: Path,
) -> None:
    helper = load_helper("prepare_analysis")
    environment = tmp_path / "worker"
    environment.mkdir()
    helper.write_new_json(
        environment / "portable-python.json",
        {
            "schema_version": 1,
            "python_version": "3.12.13",
            "files": {"Scripts/python.exe": "0" * 64},
        },
    )
    with pytest.raises(ValueError, match="Incomplete"):
        helper.verify_python(environment, tmp_path)


def test_bootstrap_isolates_known_developer_bindings(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    bootstrap = load_helper("bootstrap")
    checkout = tmp_path / "app/flitzis-looper"
    checkout.mkdir(parents=True)
    runtime = tmp_path / "app/Runtime"
    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr(bootstrap.os, "environ", dict(os.environ))
    monkeypatch.setattr(bootstrap.os, "add_dll_directory", lambda value: object(), raising=False)
    for name in (
        "FLITZIS_WORKSPACE_ENTRY",
        "FLITZIS_NATIVE_EXTENSION_DIR",
        "FLITZIS_APPLICATION_SOURCE_STAGE",
        "HF_HUB_CACHE",
        "CUDA_CACHE_PATH",
    ):
        monkeypatch.setenv(name, str(tmp_path / "foreign"))
    bootstrap.configure(checkout.parent, checkout, runtime)
    assert "FLITZIS_WORKSPACE_ENTRY" not in os.environ
    assert "FLITZIS_NATIVE_EXTENSION_DIR" not in os.environ
    assert "FLITZIS_APPLICATION_SOURCE_STAGE" not in os.environ
    assert Path(os.environ["HF_HUB_CACHE"]) == checkout.parent / "models/huggingface/hub"
    assert Path(os.environ["CUDA_CACHE_PATH"]) == checkout.parent / "cache/cuda"
    assert (
        Path(os.environ["FLITZIS_MODEL_CACHE_DIR"])
        == checkout.parent / "models/torch/hub/checkpoints"
    )


def test_missing_portable_worker_is_not_successful_readiness(tmp_path: Path) -> None:
    bootstrap = load_helper("bootstrap")
    with pytest.raises(RuntimeError, match="worker is missing"):
        bootstrap.check_beat_this_execution_api(object, tmp_path / "analysis-runtime/beat-this")


@pytest.mark.parametrize("check", [None, "--check", "--check-runtime", "--check-build-runtime"])
def test_actual_bootstrap_start_binds_worker_and_checks_never_construct_app(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, check: str | None
) -> None:
    bootstrap = load_helper("bootstrap")
    root = tmp_path / "Portable"
    checkout, runtime = root / "flitzis-looper", root / "Runtime"
    controller_module = ModuleType("flitzis_looper.controller")
    ui_module = ModuleType("flitzis_looper.ui")
    controller_factory, run_ui, check_runtime = Mock(), Mock(), Mock()
    monkeypatch.setattr(controller_module, "AppController", controller_factory, raising=False)
    monkeypatch.setattr(ui_module, "run_ui", run_ui, raising=False)
    monkeypatch.setitem(sys.modules, "flitzis_looper.controller", controller_module)
    monkeypatch.setitem(sys.modules, "flitzis_looper.ui", ui_module)
    monkeypatch.setattr(bootstrap, "configure", Mock(return_value=[object()]))
    monkeypatch.setattr(bootstrap, "check_runtime", check_runtime)
    arguments = ["bootstrap.py", "--root", str(root), "--runtime", str(runtime)]
    if check is not None:
        arguments.append(check)
    monkeypatch.setattr(sys, "argv", arguments)

    bootstrap.main()

    if check is None:
        controller_factory.assert_called_once_with(
            beat_this_installation_root=root.resolve() / "analysis-runtime/beat-this"
        )
        run_ui.assert_called_once_with(controller_factory.return_value)
        check_runtime.assert_not_called()
    else:
        controller_factory.assert_not_called()
        run_ui.assert_not_called()
        check_runtime.assert_called_once_with(
            root.resolve(), checkout, runtime, require_analysis=check != "--check-build-runtime"
        )
