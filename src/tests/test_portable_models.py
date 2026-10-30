"""Exercise model repair boundaries; fixtures do not claim actual CPU model loading."""

import base64
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


def test_source_clone_mode_allows_owned_models_without_relaxing_portable_guard(
    helper: ModuleType, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.delenv(MODEL_CACHE_DIR_ENV_VAR, raising=False)
    monkeypatch.chdir(tmp_path)
    source = helper.ModelContext(tmp_path, tmp_path, None, "check", source_clone=True)
    assert helper.model_cache_dirs(source)["demucs:htdemucs"] == (
        tmp_path / "models/torch/hub/checkpoints"
    )
    portable = helper.ModelContext(tmp_path, tmp_path, None, "check")
    with pytest.raises(RuntimeError, match="owned sibling models directory"):
        helper.model_cache_dirs(portable)


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
def test_demucs_prepare_only_verifies_preacquired_checkpoint(
    tmp_path: Path, helper: ModuleType, monkeypatch: pytest.MonkeyPatch, existing: str
) -> None:
    cache = tmp_path / "checkpoints"
    cache.mkdir()
    payload, checkpoint, downloader = fake_demucs(helper, monkeypatch, cache)
    unrelated = cache / "other-user-model.th"
    unrelated.write_bytes(b"preserve this model")
    if existing != "missing":
        checkpoint.write_bytes(payload if existing == "valid" else b"x" * len(payload))

    if existing == "valid":
        helper.repair_demucs(cache)
        assert checkpoint.read_bytes() == payload
    else:
        with pytest.raises(RuntimeError, match=r"no Model installed|invalid pinned checksum"):
            helper.repair_demucs(cache)
    assert unrelated.read_bytes() == b"preserve this model"
    downloader.assert_not_called()


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


def test_demucs_prepare_never_falls_back_to_library_download(
    tmp_path: Path, helper: ModuleType, monkeypatch: pytest.MonkeyPatch
) -> None:
    payload, checkpoint, downloader = fake_demucs(helper, monkeypatch, tmp_path)
    downloader.side_effect = lambda _name: checkpoint.write_bytes(b"x" * len(payload))

    with pytest.raises(RuntimeError, match="no Model installed"):
        helper.repair_demucs(tmp_path)

    downloader.assert_not_called()


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


def test_bs_prepare_verifies_without_legacy_temporary_downloads(
    tmp_path: Path, helper: ModuleType, monkeypatch: pytest.MonkeyPatch
) -> None:
    installer = Mock()
    verifier = Mock()
    monkeypatch.setattr(bs_roformer_assets, "install_model_assets", installer)
    monkeypatch.setattr(bs_roformer_assets, "verify_model_assets", verifier)

    helper.operate_model("bs-roformer:musdb18hq", tmp_path, "prepare")

    verifier.assert_called_once_with(tmp_path)
    installer.assert_not_called()


@pytest.mark.parametrize("existing", ["missing", "valid", "corrupt"])
def test_bs_prepare_rejects_invalid_preacquired_bytes_without_downloading(
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
    if existing == "valid":
        helper.operate_model("bs-roformer:musdb18hq", tmp_path, "prepare")
        bs_roformer_assets.verify_model_assets(tmp_path)
    else:
        with pytest.raises(RuntimeError, match=r"no Model installed|integrity check failed"):
            helper.operate_model("bs-roformer:musdb18hq", tmp_path, "prepare")
    assert unrelated.read_bytes() == b"unchanged"
    assert downloads == []
    assert not list(tmp_path.glob(".install-*"))


def test_bs_invalid_preacquired_asset_is_preserved_and_blocks_repair(
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

    with pytest.raises(RuntimeError, match="integrity check failed"):
        helper.operate_model("bs-roformer:musdb18hq", tmp_path, "prepare")

    assert target.read_bytes() == b"old invalid config"
    assert not list(tmp_path.glob(".install-*"))


def test_asset_metadata_has_complete_public_pins_and_owned_relative_targets(
    helper: ModuleType,
    context: ModelContext,
) -> None:
    metadata = helper.asset_metadata(context)
    assert len(metadata) == 3
    assert all(
        item["relative_path"].startswith("models/torch/hub/checkpoints/") for item in metadata
    )
    assert all(len(item["sha256"]) == 64 and item["bytes"] > 0 for item in metadata)
    assert all(item["url"].startswith("https://") for item in metadata)
    demucs = next(
        item for item in metadata if item["relative_path"].endswith("955717e8-8726e21a.th")
    )
    assert demucs["sha256"] == "8726e21a993978c7ba086d3872e7608d7d5bfca646ca4aca459ffda844faa8b4"
    assert demucs["bytes"] == 84141911


def test_model_child_network_guard_forbids_implicit_library_acquisition(helper: ModuleType) -> None:
    helper.deny_model_network("open", ())
    for event in ("socket.connect", "socket.getaddrinfo"):
        with pytest.raises(RuntimeError, match="pre-acquired pinned local assets"):
            helper.deny_model_network(event, ())


@pytest.mark.skipif(os.name != "nt", reason="Windows Portable launcher orchestration")
def test_actual_launcher_acquires_full_pins_before_model_checks(
    helper: ModuleType, context: ModelContext, tmp_path: Path
) -> None:
    """Use the real launch script with a tiny executable and network-free transfers."""
    portable = Path(__file__).resolve().parents[2] / "scripts" / "portable"
    runtime = context.runtime
    assert runtime is not None
    python = runtime / "python" / "python.exe"
    python.parent.mkdir(parents=True)
    trace = tmp_path / "model-orchestration.jsonl"
    metadata = helper.asset_metadata(context)
    (runtime / "common.ps1").write_text(
        (portable / "common.ps1").read_text(encoding="utf-8")
        + r"""
function Read-Payload([Uri] $Uri, [string] $Destination, [string] $Sha256,
    [long] $ExpectedBytes) {
    $entry = @{event='transfer'; url=$Uri.AbsoluteUri; destination=$Destination;
        sha256=$Sha256; bytes=$ExpectedBytes} | ConvertTo-Json -Compress
    [IO.File]::AppendAllText($env:LOOPER_TEST_MODEL_TRACE,$entry + "`n")
    [IO.File]::WriteAllText($Destination,'mock acquired bytes; never a real model')
}
""",
        encoding="utf-8-sig",
    )
    # Keep the actual script's long-path branch deterministic without SUBST side effects.
    (runtime / "build-path.ps1").write_text(
        r"""
function Resolve-PortablePhysicalPath([string] $Path) { return [IO.Path]::GetFullPath($Path) }
function New-PortableBuildDrive([string] $Root) { return @{Path=$Root.TrimEnd('\') + '\'} }
function Remove-PortableBuildDrive($Drive) {}
""",
        encoding="utf-8-sig",
    )
    fixture = r"""
using System;
using System.IO;
using System.Text;
public static class ModelLauncherFixture {
    static string Enc(string value) {
        return Convert.ToBase64String(Encoding.UTF8.GetBytes(value));
    }
    public static int Main(string[] args) {
        bool metadata = Array.IndexOf(args, "--asset-metadata") >= 0;
        string entry = "{\"event\":\"" + (metadata ? "metadata" : "load") +
            "\",\"home\":\"" + Enc(Environment.GetEnvironmentVariable("PYTHONHOME")) +
            "\",\"arguments\":\"" + Enc(String.Join("\u001f", args)) + "\"}";
        File.AppendAllText(Environment.GetEnvironmentVariable("LOOPER_TEST_MODEL_TRACE"),
            entry + "\n");
        Console.WriteLine(metadata ?
            Environment.GetEnvironmentVariable("LOOPER_TEST_MODEL_METADATA")
            : "{\"portable_models\":\"passed\"}");
        return 0;
    }
}
"""
    compiler = tmp_path / "compile-model-fixture.ps1"
    escaped = str(python).replace("'", "''")
    compiler.write_text(
        "$ErrorActionPreference = 'Stop'\nAdd-Type -TypeDefinition @'\n"
        + fixture
        + f"'@ -OutputAssembly '{escaped}' -OutputType ConsoleApplication\n",
        encoding="utf-8-sig",
    )
    command = ["powershell.exe", "-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass"]
    compiled = subprocess.run(
        [*command, "-File", str(compiler)], capture_output=True, text=True, check=False
    )
    assert compiled.returncode == 0, compiled.stdout + compiled.stderr
    launched = subprocess.run(
        [
            *command,
            "-File",
            str(portable / "launch.ps1"),
            "-Root",
            str(context.root),
            "-InstallerOwned",
            "-ModelMode",
            "repair",
        ],
        env=os.environ
        | {
            "LOOPER_TEST_MODEL_TRACE": str(trace),
            "LOOPER_TEST_MODEL_METADATA": json.dumps({
                "portable_models": "passed",
                "assets": metadata,
            }),
        },
        capture_output=True,
        text=True,
        check=False,
    )
    assert launched.returncode == 0, launched.stdout + launched.stderr
    events = [json.loads(line) for line in trace.read_text(encoding="utf-8").splitlines()]
    assert [event["event"] for event in events] == [
        "metadata",
        "transfer",
        "transfer",
        "transfer",
        "load",
    ]
    for event, asset in zip(events[1:-1], metadata, strict=True):
        assert event["url"] == asset["url"]
        assert event["sha256"] == asset["sha256"]
        assert event["bytes"] == asset["bytes"]
        assert Path(event["destination"]) == context.root / asset["relative_path"]
    for event in (events[0], events[-1]):
        arguments = base64.b64decode(event["arguments"]).decode().split("\x1f")
        assert arguments[arguments.index("--root") + 1] == str(context.root)
        assert arguments[arguments.index("--checkout") + 1] == str(context.checkout)
        assert arguments[arguments.index("--runtime") + 1] == str(runtime)
        assert Path(base64.b64decode(event["home"]).decode()) == python.parent
    assert "--asset-metadata" in base64.b64decode(events[0]["arguments"]).decode()
    assert base64.b64decode(events[-1]["arguments"]).decode().endswith("--mode\x1frepair")


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
