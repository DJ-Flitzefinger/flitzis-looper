"""Check Portable worker selection and namespace without package/model acquisition."""

import base64
import importlib.util
import json
import os
import sys
from pathlib import Path
from types import ModuleType
from typing import Literal
from unittest.mock import Mock, call

import pytest

from flitzis_looper.analysis.native_beat_worker import NativeWorkerConfiguration

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


def test_onnx_worker_pointer_is_relative_and_bound_to_retained_manifest(tmp_path: Path) -> None:
    helper = load_helper("prepare_analysis")
    installation = tmp_path / "installation-0123456789abcdef0123456789abcdef"
    installation.mkdir()
    manifest = installation / "manifest.json"
    manifest.write_bytes(b"fixture")
    configuration = NativeWorkerConfiguration(
        manifest, tmp_path / "cache", "0" * 64, "auto", startup_fault_protocol=1
    )

    result = helper.worker_result(configuration, {"status": "unavailable"})
    pointer = json.loads(base64.b64decode(result["pointer_bytes"]))

    assert result["execution_policy"] == "auto"
    assert result["inference_executed"] is False
    assert result["cuda_cache"] == {"status": "unavailable"}
    assert pointer == {
        "schema_version": 1,
        "directory": installation.name,
        "manifest_sha256": helper.digest(manifest),
    }


def test_worker_for_previous_release_is_not_reused(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    helper = load_helper("prepare_analysis")
    executable = tmp_path / "native-beat-this-worker.exe"
    executable.write_bytes(b"new release")
    configuration = NativeWorkerConfiguration(
        tmp_path / "manifest.json",
        tmp_path / "cache",
        "0" * 64,
        "auto",
        startup_fault_protocol=1,
    )
    monkeypatch.setattr(
        "flitzis_looper.analysis.native_beat_worker.is_production_worker", lambda _: True
    )

    with pytest.raises(ValueError, match="differs from this Release"):
        helper.verify_release_worker(configuration, executable)


@pytest.mark.parametrize("policy", ["cpu", "auto"])
def test_production_policies_require_matching_native_release(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, policy: Literal["cpu", "auto"]
) -> None:
    helper = load_helper("prepare_analysis")
    executable = tmp_path / "native-beat-this-worker.exe"
    executable.write_bytes(b"matching release")
    configuration = NativeWorkerConfiguration(
        tmp_path / "manifest.json",
        tmp_path / "cache",
        helper.digest(executable),
        policy,
        startup_fault_protocol=1,
    )
    require_capability = Mock()
    engine_type = object()
    monkeypatch.setattr(
        "flitzis_looper.analysis.native_beat_worker.require_native_worker_capability",
        require_capability,
    )

    helper.verify_release_worker(configuration, executable, engine_type=engine_type)

    require_capability.assert_called_once_with(configuration, engine_type=engine_type)


@pytest.mark.parametrize(
    ("mode", "policy", "cache_status", "warmup_executed"),
    [
        ("check", "auto", "pending", False),
        ("check", "auto", "failed", False),
        ("stage", "cpu", "unavailable", False),
        ("stage", "auto", "ready", True),
        ("stage", "auto", "failed", True),
        ("stage", "auto", "pending", False),
        ("bundle", "auto", "pending", False),
    ],
)
def test_portable_stage_and_checks_share_production_worker_without_selecting_it(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    mode: str,
    policy: str,
    cache_status: str,
    *,
    warmup_executed: bool,
) -> None:
    helper = load_helper("prepare_analysis")
    root = tmp_path / "Portable"
    runtime, checkout = root / "Runtime", root / "flitzis-looper"
    installation_root = root / "analysis-runtime/beat-this"
    manifest = installation_root / ("installation-" + "1" * 32) / "manifest.json"
    manifest.parent.mkdir(parents=True)
    manifest.write_bytes(b"new worker")
    pointer = installation_root / "current.json"
    pointer.write_bytes(b"prior pointer")
    configuration = Mock(manifest=manifest, execution_policy=policy)
    observed = {
        "status": cache_status,
        "detail": "fixture detail",
        "warmup_executed": warmup_executed,
    }
    cache = Mock(status=Mock(return_value=observed), warm=Mock(return_value=observed))
    worker = Mock(load=Mock(return_value=configuration), stage=Mock(return_value=configuration))
    monkeypatch.setattr(helper, "configure", lambda *_: [object()])
    monkeypatch.setattr(helper, "verify_release_worker", worker.verify)
    monkeypatch.setattr(helper, "load_verified_selection", worker.load)
    monkeypatch.setattr("flitzis_looper.analysis.beat_cuda_cache.cuda_cache_status", cache.status)
    monkeypatch.setattr("flitzis_looper.analysis.beat_cuda_cache.ensure_cuda_cache", cache.warm)
    monkeypatch.setattr(
        "flitzis_looper.analysis.native_provisioning.stage_production_worker", worker.stage
    )
    report = root / "cache/worker.json"
    arguments = [
        "--root",
        str(root),
        "--checkout",
        str(checkout),
        "--runtime",
        str(runtime),
        "--mode",
        mode,
        "--report-json",
        str(report),
    ]
    if mode == "bundle":
        arguments += ["--asset-cache", str(tmp_path / "assets")]

    if mode == "stage" and cache_status in {"failed", "pending"}:
        with pytest.raises(ValueError, match=f"CUDA preparation {cache_status}: fixture detail"):
            helper.main(arguments)
        assert not report.exists()
    else:
        assert helper.main(arguments) == 0

        result = json.loads(report.read_bytes())
        assert result["execution_policy"] == policy
        assert result["cuda_cache"]["status"] == cache_status
        assert result["inference_executed"] is warmup_executed
    assert pointer.read_bytes() == b"prior pointer"
    if mode == "check":
        worker.load.assert_called_once_with(
            installation_root, root / "cache/analysis", runtime / helper.WORKER_EXECUTABLE
        )
        cache.status.assert_called_once_with(configuration)
        worker.stage.assert_not_called()
        worker.verify.assert_not_called()
    else:
        worker.load.assert_not_called()
        cache.status.assert_not_called()
        worker.stage.assert_called_once_with(
            installation_root,
            root / "cache/analysis",
            repository=checkout,
            executable=runtime / helper.WORKER_EXECUTABLE,
            crt_dir=checkout / "src/flitzis_looper_audio",
            include_cuda=True if mode == "bundle" else None,
            asset_cache=tmp_path / "assets" if mode == "bundle" else None,
        )
        worker.verify.assert_called_once_with(configuration, runtime / helper.WORKER_EXECUTABLE)
    assert cache.warm.call_args_list == (
        [call(configuration, retry_failed=True)] if mode == "stage" else []
    )


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
        "FLITZIS_UI_STATE_DIR",
    ):
        monkeypatch.setenv(name, str(tmp_path / "foreign"))
    bootstrap.configure(checkout.parent, checkout, runtime)
    assert "FLITZIS_WORKSPACE_ENTRY" not in os.environ
    assert "FLITZIS_NATIVE_EXTENSION_DIR" not in os.environ
    assert "FLITZIS_APPLICATION_SOURCE_STAGE" not in os.environ
    assert Path(os.environ["HF_HUB_CACHE"]) == checkout.parent / "models/huggingface/hub"
    assert Path(os.environ["CUDA_CACHE_PATH"]) == checkout.parent / "cache/cuda"
    assert Path(os.environ["FLITZIS_UI_STATE_DIR"]) == checkout.parent / "cache/ui"
    assert (
        Path(os.environ["FLITZIS_MODEL_CACHE_DIR"])
        == checkout.parent / "models/torch/hub/checkpoints"
    )


def test_missing_portable_worker_is_not_successful_readiness(tmp_path: Path) -> None:
    bootstrap = load_helper("bootstrap")
    with pytest.raises(RuntimeError, match="worker is missing"):
        bootstrap.check_beat_this_execution_api(object, tmp_path / "analysis-runtime/beat-this")


def test_portable_gate_rejects_a_retained_torch_selection(tmp_path: Path) -> None:
    bootstrap = load_helper("bootstrap")
    helper = load_helper("prepare_analysis")
    installation_root = tmp_path / "analysis-runtime/beat-this"
    manifest = installation_root / ("installation-" + "1" * 32) / "manifest.json"
    manifest.parent.mkdir(parents=True)
    manifest.write_bytes(b'{"profile":"legacy-cpu-v1"}')
    helper.write_new_json(
        installation_root / "current.json",
        {
            "schema_version": 1,
            "directory": manifest.parent.name,
            "manifest_sha256": helper.digest(manifest),
        },
    )

    with pytest.raises(ValueError, match="requires the ONNX worker"):
        bootstrap.check_beat_this_execution_api(object, installation_root)


@pytest.mark.parametrize("check", [None, "--check", "--check-runtime", "--check-build-runtime"])
def test_actual_bootstrap_start_binds_worker_and_checks_never_construct_app(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, check: str | None
) -> None:
    bootstrap = load_helper("bootstrap")
    root = tmp_path / "Portable"
    checkout, runtime = root / "flitzis-looper", root / "Runtime"
    controller_module = ModuleType("flitzis_looper.controller")
    ui_module = ModuleType("flitzis_looper.ui")
    ui_run_module = ModuleType("flitzis_looper.ui.run")
    controller_factory, run_ui, check_runtime, prepare_ui = Mock(), Mock(), Mock(), Mock()
    sequence = Mock()
    sequence.attach_mock(prepare_ui, "prepare")
    sequence.attach_mock(controller_factory, "controller")
    sequence.attach_mock(run_ui, "ui")
    monkeypatch.setattr(controller_module, "AppController", controller_factory, raising=False)
    monkeypatch.setattr(ui_module, "run_ui", run_ui, raising=False)
    monkeypatch.setattr(ui_run_module, "prepare_ui_state_directory", prepare_ui, raising=False)
    monkeypatch.setitem(sys.modules, "flitzis_looper.controller", controller_module)
    monkeypatch.setitem(sys.modules, "flitzis_looper.ui", ui_module)
    monkeypatch.setitem(sys.modules, "flitzis_looper.ui.run", ui_run_module)
    monkeypatch.setattr(bootstrap, "configure", Mock(return_value=[object()]))
    monkeypatch.setattr(bootstrap, "check_runtime", check_runtime)
    manifest = root / "models/bs-roformer-onnx/installation-fixture/manifest.json"
    model_check = Mock(return_value={"manifest": str(manifest)})
    monkeypatch.setattr(bootstrap, "check_bs_roformer_runtime", model_check)
    monkeypatch.delenv("FLITZIS_BS_ROFORMER_ONNX_MANIFEST", raising=False)
    arguments = ["bootstrap.py", "--root", str(root), "--runtime", str(runtime)]
    if check is not None:
        arguments.append(check)
    monkeypatch.setattr(sys, "argv", arguments)

    bootstrap.main()

    if check is None:
        model_check.assert_called_once_with(root, runtime, require_model=True)
        assert os.environ["FLITZIS_BS_ROFORMER_ONNX_MANIFEST"] == str(manifest)
        prepare_ui.assert_called_once_with()
        controller_factory.assert_called_once_with(
            beat_this_installation_root=root.resolve() / "analysis-runtime/beat-this"
        )
        run_ui.assert_called_once_with(controller_factory.return_value)
        assert sequence.mock_calls == [
            call.prepare(),
            call.controller(
                beat_this_installation_root=root.resolve() / "analysis-runtime/beat-this"
            ),
            call.ui(controller_factory.return_value),
        ]
        check_runtime.assert_not_called()
    else:
        model_check.assert_not_called()
        prepare_ui.assert_not_called()
        controller_factory.assert_not_called()
        run_ui.assert_not_called()
        check_runtime.assert_called_once_with(
            root.resolve(), checkout, runtime, require_analysis=check != "--check-build-runtime"
        )
