"""Exercise model repair boundaries; fixtures do not claim actual CPU model loading."""

import hashlib
import importlib.util
import io
import json
import os
import subprocess
import sys
import urllib.request
from pathlib import Path
from types import ModuleType, SimpleNamespace
from typing import TYPE_CHECKING, cast
from unittest.mock import Mock

import pytest

from flitzis_looper.controller import bs_roformer_assets, stem_generation
from flitzis_looper.controller.stem_generation import MODEL_CACHE_DIR_ENV_VAR
from flitzis_looper.controller.stem_separators import separator_model_cache_dir
from flitzis_looper.models import STEM_SEPARATORS

if TYPE_CHECKING:
    from scripts.portable.prepare_models import ModelContext


@pytest.fixture
def helper() -> ModuleType:
    path = Path(__file__).resolve().parents[2] / "scripts" / "portable" / "prepare_models.py"
    spec = importlib.util.spec_from_file_location("portable_models_test_helper", path)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


@pytest.fixture
def context(tmp_path: Path, helper: ModuleType, monkeypatch: pytest.MonkeyPatch) -> ModelContext:
    root = tmp_path / "Portable"
    checkout = root / "flitzis-looper"
    checkout.mkdir(parents=True)
    monkeypatch.setenv(MODEL_CACHE_DIR_ENV_VAR, str(root / "models/torch/hub/checkpoints"))
    return cast("ModelContext", helper.ModelContext(root, checkout, root / "Runtime", "repair"))


def test_shared_resolver_keeps_both_portable_models_outside_checkout(tmp_path: Path) -> None:
    root = tmp_path / "Portable"
    cache = root / "models" / "torch" / "hub" / "checkpoints"
    environment = {MODEL_CACHE_DIR_ENV_VAR: str(cache)}
    paths = {
        separator: separator_model_cache_dir(
            separator, env=environment, project_root=root / "flitzis-looper"
        )
        for separator in STEM_SEPARATORS
    }

    assert paths["demucs:htdemucs"] == cache
    assert paths["bs-roformer:musdb18hq"] == cache / "bs-roformer-musdb18hq"


def test_parent_prepares_both_before_sequential_fresh_cpu_checks(
    helper: ModuleType, context: ModelContext, monkeypatch: pytest.MonkeyPatch
) -> None:
    calls: list[tuple[str, str]] = []

    def run(command: list[str], **kwargs: object) -> subprocess.CompletedProcess[str]:
        action = command[command.index("--child-action") + 1]
        separator = command[command.index("--separator") + 1]
        assert command[:3] == [sys.executable, "-s", "-B"]
        assert "--runtime" in command
        assert kwargs["cwd"] == context.checkout
        assert kwargs["env"] == dict(os.environ)
        calls.append((action, separator))
        return subprocess.CompletedProcess(command, 0, "", "")

    monkeypatch.setattr(helper.subprocess, "run", run)
    report = helper.check_models(context)

    assert calls == [
        ("prepare", "demucs:htdemucs"),
        ("prepare", "bs-roformer:musdb18hq"),
        ("load", "demucs:htdemucs"),
        ("load", "bs-roformer:musdb18hq"),
    ]
    assert report["portable_models"] == "passed"
    assert report["device"] == "cpu"


def test_check_mode_loads_without_preparation_and_blocks_after_cpu_failure(
    helper: ModuleType, context: ModelContext, monkeypatch: pytest.MonkeyPatch
) -> None:
    checked = helper.ModelContext(context.root, context.checkout, context.runtime, "check")
    runner = Mock(return_value=subprocess.CompletedProcess([], 1, "", "CPU load failed"))
    monkeypatch.setattr(helper.subprocess, "run", runner)

    with pytest.raises(helper.ModelCheckError, match="CPU load failed") as error:
        helper.check_models(checked)

    assert error.value.stage == "load"
    assert error.value.separator == "demucs:htdemucs"
    runner.assert_called_once()
    command = runner.call_args.args[0]
    assert command[command.index("--child-action") + 1] == "load"


def test_prepare_failure_never_starts_any_final_cpu_check(
    helper: ModuleType, context: ModelContext, monkeypatch: pytest.MonkeyPatch
) -> None:
    runner = Mock(
        side_effect=[
            subprocess.CompletedProcess([], 0, "", ""),
            subprocess.CompletedProcess([], 1, "", "pinned download failed"),
        ]
    )
    monkeypatch.setattr(helper.subprocess, "run", runner)

    with pytest.raises(helper.ModelCheckError, match="pinned download failed") as error:
        helper.check_models(context)

    assert error.value.stage == "prepare"
    assert error.value.separator == "bs-roformer:musdb18hq"
    assert runner.call_count == 2
    for call in runner.call_args_list:
        command = call.args[0]
        assert command[command.index("--child-action") + 1] == "prepare"


def test_helper_rejects_project_cache_redirect_before_starting_model_children(
    helper: ModuleType, context: ModelContext, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv(MODEL_CACHE_DIR_ENV_VAR, str(context.checkout / "models"))
    runner = Mock()
    monkeypatch.setattr(helper.subprocess, "run", runner)

    with pytest.raises(RuntimeError, match="leaves the owned sibling models directory"):
        helper.check_models(context)

    runner.assert_not_called()


def fake_demucs(
    helper: ModuleType, monkeypatch: pytest.MonkeyPatch, cache: Path
) -> tuple[bytes, Path, Mock]:
    payload = b"small pinned checkpoint fixture, never a real model"
    checksum = hashlib.sha256(payload).hexdigest()[:8]
    filename = f"955717e8-{checksum}.th"
    checkpoint = cache / filename
    monkeypatch.setattr(stem_generation, "DEMUCS_REQUIRED_CHECKPOINTS", {"htdemucs": (filename,)})

    class InvalidChecksumError(RuntimeError):
        __slots__ = ()

    def check_checksum(path: Path, expected: str) -> None:
        if hashlib.sha256(path.read_bytes()).hexdigest()[: len(expected)] != expected:
            msg = "invalid pinned checksum"
            raise InvalidChecksumError(msg)

    repo = SimpleNamespace(check_checksum=check_checksum, ModelLoadingError=InvalidChecksumError)

    def download(name: str) -> object:
        assert name == "htdemucs"
        assert not checkpoint.exists()
        checkpoint.write_bytes(payload)
        return object()

    downloader = Mock(side_effect=download)
    pretrained = SimpleNamespace(get_model=downloader)
    original_import = importlib.import_module

    def imported(name: str, package: str | None = None) -> ModuleType | SimpleNamespace:
        if name == "demucs.repo":
            return repo
        if name == "demucs.pretrained":
            return pretrained
        return original_import(name, package)

    monkeypatch.setattr(helper.importlib, "import_module", imported)
    return payload, checkpoint, downloader


@pytest.mark.parametrize("existing", ["missing", "valid", "corrupt"])
def test_demucs_repairs_only_required_invalid_checkpoint(
    tmp_path: Path, helper: ModuleType, monkeypatch: pytest.MonkeyPatch, existing: str
) -> None:
    cache = tmp_path / "checkpoints"
    cache.mkdir()
    payload, checkpoint, downloader = fake_demucs(helper, monkeypatch, cache)
    unrelated = cache / "other-user-model.th"
    unrelated.write_bytes(b"preserve this model")
    if existing != "missing":
        checkpoint.write_bytes(payload if existing == "valid" else b"x" * len(payload))

    helper.repair_demucs(cache)

    assert checkpoint.read_bytes() == payload
    assert unrelated.read_bytes() == b"preserve this model"
    assert downloader.call_count == (0 if existing == "valid" else 1)


@pytest.mark.parametrize("existing", ["missing", "corrupt"])
def test_demucs_check_rejects_bad_cache_before_loader_or_download(
    tmp_path: Path, helper: ModuleType, monkeypatch: pytest.MonkeyPatch, existing: str
) -> None:
    payload, checkpoint, downloader = fake_demucs(helper, monkeypatch, tmp_path)
    if existing == "corrupt":
        checkpoint.write_bytes(b"x" * len(payload))

    with pytest.raises(RuntimeError, match=r"no Model installed|invalid pinned checksum"):
        helper.operate_model("demucs:htdemucs", tmp_path, "load")

    downloader.assert_not_called()


def test_demucs_rejects_corrupt_download_before_final_cpu_check(
    tmp_path: Path, helper: ModuleType, monkeypatch: pytest.MonkeyPatch
) -> None:
    payload, checkpoint, downloader = fake_demucs(helper, monkeypatch, tmp_path)
    downloader.side_effect = lambda _name: checkpoint.write_bytes(b"x" * len(payload))

    with pytest.raises(RuntimeError, match="invalid pinned checksum"):
        helper.repair_demucs(tmp_path)

    downloader.assert_called_once_with("htdemucs")


def test_demucs_valid_cache_is_verified_and_loaded_on_cpu(
    tmp_path: Path, helper: ModuleType, monkeypatch: pytest.MonkeyPatch
) -> None:
    payload, checkpoint, loader = fake_demucs(helper, monkeypatch, tmp_path)
    checkpoint.write_bytes(payload)
    cpu = Mock()
    loader.side_effect = None
    loader.return_value = SimpleNamespace(to=cpu)

    helper.operate_model("demucs:htdemucs", tmp_path, "load")

    loader.assert_called_once_with("htdemucs")
    cpu.assert_called_once_with("cpu")


def test_bs_repair_uses_existing_pinned_atomic_installer(
    tmp_path: Path, helper: ModuleType, monkeypatch: pytest.MonkeyPatch
) -> None:
    installer = Mock()
    monkeypatch.setattr(bs_roformer_assets, "install_model_assets", installer)

    helper.operate_model("bs-roformer:musdb18hq", tmp_path, "prepare")

    installer.assert_called_once_with(tmp_path)


@pytest.mark.parametrize("existing", ["missing", "valid", "corrupt"])
def test_bs_asset_repair_reuses_size_digest_and_atomic_resolver(
    tmp_path: Path, helper: ModuleType, monkeypatch: pytest.MonkeyPatch, existing: str
) -> None:
    payloads = {
        filename: filename.encode("ascii")
        for filename in (bs_roformer_assets.CONFIG_FILENAME, bs_roformer_assets.CHECKPOINT_FILENAME)
    }
    pins = tuple(
        (filename, len(payload), hashlib.sha256(payload).hexdigest())
        for filename, payload in payloads.items()
    )
    monkeypatch.setattr(bs_roformer_assets, "MODEL_ASSETS", pins)
    for filename, payload in payloads.items():
        if existing != "missing":
            (tmp_path / filename).write_bytes(
                payload if existing == "valid" else b"x" * len(payload)
            )
    unrelated = tmp_path / "preserved-model.bin"
    unrelated.write_bytes(b"unchanged")
    downloads: list[str] = []

    def download(url: str, *, timeout: int) -> io.BytesIO:
        assert timeout == 120
        assert url.startswith(bs_roformer_assets.ASSET_BASE_URL)
        filename = url.rsplit("/", 1)[1]
        downloads.append(filename)
        return io.BytesIO(payloads[filename])

    monkeypatch.setattr(urllib.request, "urlopen", download)
    helper.operate_model("bs-roformer:musdb18hq", tmp_path, "prepare")

    bs_roformer_assets.verify_model_assets(tmp_path)
    assert unrelated.read_bytes() == b"unchanged"
    assert downloads == ([] if existing == "valid" else list(payloads))
    assert not list(tmp_path.glob(".install-*"))


def test_bs_failed_download_preserves_existing_asset_and_blocks_repair(
    tmp_path: Path, helper: ModuleType, monkeypatch: pytest.MonkeyPatch
) -> None:
    target = tmp_path / bs_roformer_assets.CONFIG_FILENAME
    target.write_bytes(b"old invalid config")
    payload = b"expected config"
    monkeypatch.setattr(
        bs_roformer_assets,
        "MODEL_ASSETS",
        ((target.name, len(payload), hashlib.sha256(payload).hexdigest()),),
    )
    monkeypatch.setattr(
        urllib.request,
        "urlopen",
        lambda _url, **_kwargs: io.BytesIO(b"x" * len(payload)),
    )

    with pytest.raises(RuntimeError, match="download integrity check failed"):
        helper.operate_model("bs-roformer:musdb18hq", tmp_path, "prepare")

    assert target.read_bytes() == b"old invalid config"
    assert not list(tmp_path.glob(".install-*"))


def test_bs_check_verifies_before_actual_cpu_loader_without_download(
    tmp_path: Path, helper: ModuleType, monkeypatch: pytest.MonkeyPatch
) -> None:
    calls: list[str] = []
    installer = Mock(side_effect=AssertionError("check must not repair"))
    monkeypatch.setattr(bs_roformer_assets, "install_model_assets", installer)
    monkeypatch.setattr(
        bs_roformer_assets, "verify_model_assets", lambda _path: calls.append("verify")
    )
    loader = Mock(side_effect=lambda *_args: calls.append("load"))
    original_import = importlib.import_module

    def imported(name: str, package: str | None = None) -> ModuleType | SimpleNamespace:
        if name == "flitzis_bs_roformer.model":
            assert calls == ["verify"]
            return SimpleNamespace(load_model=loader)
        return original_import(name, package)

    monkeypatch.setattr(helper.importlib, "import_module", imported)
    helper.operate_model("bs-roformer:musdb18hq", tmp_path, "load")

    loader.assert_called_once_with(
        tmp_path / bs_roformer_assets.CHECKPOINT_FILENAME,
        tmp_path / bs_roformer_assets.CONFIG_FILENAME,
        "cpu",
    )
    assert calls == ["verify", "load"]
    installer.assert_not_called()


def test_model_failure_is_reported_and_cannot_be_success(
    helper: ModuleType,
    context: ModelContext,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    monkeypatch.setattr(helper, "configure", lambda _context: [])
    failure = helper.ModelCheckError(
        "broken checkpoint", separator="bs-roformer:musdb18hq", stage="load"
    )
    monkeypatch.setattr(helper, "check_models", Mock(side_effect=failure))
    report_path = context.root / "cache" / "test" / "models.json"

    result = helper.main([
        "--root",
        str(context.root),
        "--checkout",
        str(context.checkout),
        "--runtime",
        str(context.runtime),
        "--mode",
        "check",
        "--report-json",
        str(report_path),
    ])

    report = json.loads(report_path.read_text(encoding="utf-8"))
    assert result == 1
    assert report["portable_models"] == "failed"
    assert report["separator"] == "bs-roformer:musdb18hq"
    assert report["stage"] == "load"
    assert json.loads(capsys.readouterr().out) == report
