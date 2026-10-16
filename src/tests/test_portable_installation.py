"""Exercise the real source-main installer without network, native builds or audio."""

import base64
import ctypes
import hashlib
import json
import os
import shutil
import stat
import subprocess
import time
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, cast
from zipfile import ZipFile, ZipInfo

import pytest

if TYPE_CHECKING:
    from collections.abc import Callable, Iterator

pytestmark = pytest.mark.skipif(os.name != "nt", reason="Windows portable installation")
REPO = Path(__file__).resolve().parents[2]
REPOSITORY = "https://github.com/DJ-Flitzefinger/flitzis-looper.git"
GIT_ARCHIVE = (
    "https://github.com/git-for-windows/git/releases/download/"
    "v2.56.0.windows.2/MinGit-2.56.0.2-64-bit.zip"
)
GIT_SHA256 = "da35e72aa21c005a5a0d298cfbae110bc1609a815730ea0dde84b01a1b3cd3be"
LAUNCH = r"""
param([string] $Root, [switch] $Check, [string] $CheckoutDirectory,
    [string] $RuntimeDirectory, [switch] $InstallerOwned, [switch] $StartLockHeld,
    [string] $ModelMode, [string] $RelocationAction, [string] $RelocationSource,
    [string] $RelocationTarget, [string] $RelocationRecord)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$runtime = $PSScriptRoot
. (Join-Path $runtime 'common.ps1')
if (-not $CheckoutDirectory) { $CheckoutDirectory = Join-Path $Root 'flitzis-looper' }
$release = Get-Content -LiteralPath (Join-Path $runtime 'release-manifest.json') -Raw |
    ConvertFrom-Json
if ($release.fixture_version -ne '@VERSION@') { throw 'Wrong private launch implementation' }
if ($RelocationAction) {
    function Inventory([string] $Path) {
        $result = @{}
        foreach ($file in @(Get-ChildItem -LiteralPath $Path -Recurse -Force -File)) {
            $relative = $file.FullName.Substring($Path.TrimEnd('\').Length + 1).Replace('\','/')
            $result[$relative] = Get-Sha256 $file.FullName
        }
        return $result
    }
    if ($RelocationAction -eq 'capture') {
        $capture = @{files=(Inventory $RelocationSource)} | ConvertTo-Json -Depth 6
        [IO.File]::WriteAllText($RelocationRecord, $capture)
        if ('@FAILURE@' -eq 'capture') { exit 33 }
    } elseif ($RelocationAction -eq 'rollback') {
        if ('@FAILURE@' -eq 'capture') {
            throw 'Unsealed capture rollback was incorrectly consumed'
        }
        if ((Test-Path -LiteralPath $RelocationTarget) -and
            -not (Test-Path -LiteralPath $RelocationSource)) {
            [IO.Directory]::Move($RelocationTarget,$RelocationSource)
        }
    } else {
        $proof = Get-Content -LiteralPath $RelocationRecord -Raw | ConvertFrom-Json
        $actual = if (Test-Path -LiteralPath $RelocationSource) {
            $RelocationSource
        } else { $RelocationTarget }
        $found = Inventory $actual
        foreach ($property in $proof.files.PSObject.Properties) {
            if ($found[$property.Name] -ne $property.Value) { throw 'Captured data changed' }
        }
        if ($RelocationAction -eq 'move' -and $actual -ne $RelocationTarget) {
            [IO.Directory]::Move($RelocationSource,$RelocationTarget)
        }
    }
    exit 0
}
if ($ModelMode) {
    foreach ($name in @('demucs-fixture.th','bs-fixture.ckpt')) {
        $path = Join-Path $Root "models/$name"
        if ($ModelMode -eq 'repair') { [IO.File]::WriteAllText($path,'verified fixture') }
        if (-not (Test-Path -LiteralPath $path) -or
            [IO.File]::ReadAllText($path) -ne 'verified fixture') { exit 31 }
    }
    [IO.File]::AppendAllText((Join-Path $Root 'cache/model-history.jsonl'),
        (@{mode=$ModelMode; fixture_version='@VERSION@'} | ConvertTo-Json -Compress) + "`n")
    if ('@FAILURE@' -eq 'model-cpu') { exit 32 }
    exit 0
}
if (-not $Check) { exit 90 }
if (-not (Test-Path -LiteralPath (Join-Path $runtime '@VERSION@-required.txt'))) {
    throw 'Unusable private runtime'
}
if ([IO.File]::ReadAllText((Join-Path $CheckoutDirectory 'fixture-version.txt')) -ne '@VERSION@') {
    throw 'Wrong checkout'
}
$previous = $null
if (Test-Path -LiteralPath (Join-Path $Root 'current.json')) {
    $pointer = Get-Content -LiteralPath (Join-Path $Root 'current.json') -Raw | ConvertFrom-Json
    $previous = $pointer.git_commit
}
$entry = @{fixture_version='@VERSION@'; git_commit=$release.git_commit;
    checkout=$CheckoutDirectory; previous_commit=$previous} | ConvertTo-Json -Compress
[IO.File]::AppendAllText((Join-Path $Root 'cache/preflight-history.jsonl'), $entry + "`n")
exit @EXIT@
"""
HELPER = r"""
param([string] $Root, [string] $Source, [string] $Git, [string] $Commit,
    [string] $RuntimeDirectory, [string] $OperationId)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'common.ps1')
$actual = & $Git -C $Source rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $actual -ne $Commit) { throw 'Expected exact pinned source' }
$dirty = @(& $Git -C $Source status --porcelain)
if ($LASTEXITCODE -ne 0 -or $dirty.Count) { throw 'Expected clean source before Native staging' }
$canonicalSource = [IO.Path]::GetFullPath((& $Git -C $Source rev-parse --show-toplevel))
$canonicalGit = & $Git -C $Source rev-parse --path-format=absolute --git-dir
$relativeSource = $Source.Substring($Root.TrimEnd('\').Length).TrimStart('\')
$canonicalRoot = $canonicalSource.Substring(
    0,$canonicalSource.Length-$relativeSource.Length).TrimEnd('\')
$relativeRuntime = $RuntimeDirectory.Substring($Root.TrimEnd('\').Length).TrimStart('\')
$canonicalRuntime = Join-Path $canonicalRoot $relativeRuntime
$expectedRuntime = Join-Path $Root ('cache/r-' + $OperationId.Substring(0,8))
if ($RuntimeDirectory -ne $expectedRuntime) { throw 'Wrong candidate runtime' }
$entry = @{fixture_version='@VERSION@'; git_commit=$Commit; source=$canonicalSource;
    runtime=$canonicalRuntime; git_directory=[IO.Path]::GetFullPath($canonicalGit);
    canonical_root=$canonicalRoot; build_root=$Root} | ConvertTo-Json -Compress
[IO.File]::AppendAllText((Join-Path $Root 'cache/helper-invocations.jsonl'), $entry + "`n")
if ('@FAILURE@' -eq 'helper') { exit 23 }
[IO.Directory]::CreateDirectory((Join-Path $RuntimeDirectory 'python')) | Out-Null
$python = Join-Path $RuntimeDirectory 'python/python.exe'
[IO.File]::WriteAllText($python,'immutable-runtime-@VERSION@')
[IO.File]::WriteAllText((Join-Path $RuntimeDirectory '@VERSION@-required.txt'),'required')
[IO.Directory]::CreateDirectory((Join-Path $RuntimeDirectory 'licenses')) | Out-Null
[IO.File]::WriteAllText((Join-Path $RuntimeDirectory 'licenses/samplerate.txt'),'owned-license')
[IO.File]::WriteAllText((Join-Path $RuntimeDirectory 'python/vcruntime140.dll'),'owned-runtime-dll')
foreach ($name in @('launch.ps1','dispatch.ps1','START-LOOPER.bat','common.ps1','build-path.ps1')) {
    $helperSource = Join-Path $Source "scripts/portable/$name"
    [IO.File]::Copy($helperSource,(Join-Path $RuntimeDirectory $name),$false)
}
function Inventory([string] $Path) {
    $result = @{}
    foreach ($file in @(Get-ChildItem -LiteralPath $Path -Recurse -Force -File)) {
        $relative = $file.FullName.Substring($Path.TrimEnd('\').Length + 1).Replace('\','/')
        $result[$relative] = Get-Sha256 $file.FullName
    }
    return $result
}
if ('@FAILURE@' -ne 'missing-manifest') {
    $runtimeCommit = if ('@FAILURE@' -eq 'wrong-commit') {
        'ffffffffffffffffffffffffffffffffffffffff'
    } else { $Commit }
    $release = @{schema_version=2; release_id=$Commit; git_commit=$runtimeCommit;
        source_dirty=('@FAILURE@' -eq 'dirty-manifest'); fixture_version='@VERSION@';
        checkout_files=(Inventory $Source); runtime_files=(Inventory $RuntimeDirectory)}
    $manifest = Join-Path $RuntimeDirectory 'release-manifest.json'
    [IO.File]::WriteAllText($manifest,($release | ConvertTo-Json -Depth 8))
}
if ('@FAILURE@' -eq 'prepared-interrupt') { exit 24 }
exit 0
"""
FAKE_GIT = """
using System;
using System.IO;
public static class PortableGitFixture {
    public static int Main(string[] arguments) {
        File.AppendAllText(Environment.GetEnvironmentVariable("LOOPER_TEST_GIT_TRACE"),
            string.Join(" ", arguments) + Environment.NewLine);
        File.AppendAllText(Environment.GetEnvironmentVariable("LOOPER_TEST_GIT_EXECUTABLE_TRACE"),
            typeof(PortableGitFixture).Assembly.Location + Environment.NewLine);
        if (arguments.Length == 1 && arguments[0] == "--version") {
            Console.WriteLine("git version portable-fixture");
            return 0;
        }
        return 77;
    }
}
"""


def _git_environment() -> dict[str, str]:
    environment = {name: value for name, value in os.environ.items() if not name.startswith("GIT_")}
    environment.update({
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_GLOBAL": "NUL",
        "GIT_TERMINAL_PROMPT": "0",
    })
    return environment


def _ps_string(value: Path | str) -> str:
    return "'" + str(value).replace("'", "''") + "'"


def _run_powershell(
    arguments: list[str], *, environment: dict[str, str] | None = None
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["powershell.exe", "-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", *arguments],
        env=environment,
        capture_output=True,
        text=True,
        errors="replace",
        timeout=30,
        check=False,
    )


def require_success(result: subprocess.CompletedProcess[str]) -> None:
    assert result.returncode == 0, result.stdout + result.stderr


@contextmanager
def exclusive_file(path: Path) -> Iterator[None]:
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.CreateFileW.argtypes = [
        ctypes.c_wchar_p,
        ctypes.c_uint,
        ctypes.c_uint,
        ctypes.c_void_p,
        ctypes.c_uint,
        ctypes.c_uint,
        ctypes.c_void_p,
    ]
    kernel.CreateFileW.restype = ctypes.c_void_p
    kernel.CloseHandle.argtypes = [ctypes.c_void_p]
    handle = kernel.CreateFileW(str(path), 0x80000000, 0, None, 3, 0, None)
    assert handle != ctypes.c_void_p(-1).value, ctypes.get_last_error()
    try:
        yield
    finally:
        assert kernel.CloseHandle(handle)


@dataclass
class Installation:
    installer: Path
    source: Path
    target: Path
    git: str

    def git_run(self, *arguments: str) -> subprocess.CompletedProcess[str]:
        result = subprocess.run(
            [
                self.git,
                "-c",
                f"core.hooksPath={self.source.parent / 'no-hooks'}",
                "-c",
                "user.name=Portable installer fixture",
                "-c",
                "user.email=portable-fixture@example.invalid",
                "-C",
                str(self.source),
                *arguments,
            ],
            env=_git_environment(),
            capture_output=True,
            text=True,
            errors="replace",
            timeout=15,
            check=False,
        )
        require_success(result)
        return result

    def publish(self, version: str, *, failure: str = "") -> str:
        portable = self.source / "scripts/portable"
        portable.mkdir(parents=True, exist_ok=True)
        helper = portable / "install-main.ps1"
        if failure == "missing-helper":
            helper.unlink(missing_ok=True)
        else:
            helper.write_text(
                HELPER.replace("@VERSION@", version).replace("@FAILURE@", failure),
                encoding="utf-8-sig",
            )
        (portable / "launch.ps1").write_text(
            LAUNCH
            .replace("@VERSION@", version)
            .replace("@FAILURE@", failure)
            .replace("@EXIT@", "17" if failure == "preflight" else "0"),
            encoding="utf-8-sig",
        )
        for name in ("dispatch.ps1", "common.ps1", "build-path.ps1"):
            shutil.copyfile(REPO / "scripts/portable" / name, portable / name)
        (portable / "START-LOOPER.bat").write_bytes(b"@echo off\r\nexit /b 90\r\n")
        (self.source / "fixture-version.txt").write_text(version, encoding="utf-8")
        self.git_run("add", "--all")
        self.git_run("commit", "--no-gpg-sign", "-m", version)
        return self.git_run("rev-parse", "HEAD").stdout.strip()

    def run(self, *, explicit_target: bool = True) -> subprocess.CompletedProcess[str]:
        arguments = f'"{self.target}" --no-pause' if explicit_target else "--no-pause"
        command = f'call "{self.installer}" {arguments}'
        processor = os.environ.get("COMSPEC", "cmd.exe")
        return subprocess.run(
            f'"{processor}" /d /s /c "{command}"',
            cwd=self.installer.parent,
            env=_git_environment(),
            capture_output=True,
            text=True,
            errors="replace",
            timeout=40,
            check=False,
        )

    def pointer(self) -> dict[str, object]:
        # The installer pointer is a JSON object; tests check its specific fields.
        return cast(
            "dict[str, object]",
            json.loads((self.target / "current.json").read_text(encoding="utf-8-sig")),
        )

    def trace(self, name: str) -> list[dict[str, object]]:
        path = self.target / "cache" / name
        return (
            [json.loads(line) for line in path.read_text(encoding="utf-8-sig").splitlines()]
            if path.exists()
            else []
        )


@pytest.fixture
def installation(tmp_path: Path) -> Installation:
    git = shutil.which("git.exe")
    if git is None:
        pytest.skip("The local main fixture requires Git; bootstrap is covered separately")
    source = tmp_path / "git-main"
    source.mkdir()
    caller = tmp_path / "Test ü ä with spaces"
    caller.mkdir()
    installer = caller / "INSTALL-LOOPER-PORTABLE.bat"
    content = (REPO / installer.name).read_text(encoding="utf-8-sig")
    declaration = f"$Repository = '{REPOSITORY}'"
    # A changed production declaration must fail before a test can contact the real remote.
    assert content.count(declaration) == 1
    content = content.replace(declaration, f"$Repository = {_ps_string(source.as_uri())}")
    installer.write_bytes(content.replace("\n", "\r\n").encode("utf-8"))
    fixture = Installation(installer, source, caller / "Flitzis-Looper", git)
    fixture.git_run("-c", "init.templateDir=", "init", "--initial-branch=main")
    return fixture


def test_first_install_builds_one_complete_main_checkout(installation: Installation) -> None:
    commit = installation.publish("first")
    require_success(installation.run(explicit_target=False))
    pointer = installation.pointer()
    assert pointer["schema_version"] == 2
    assert pointer["git_commit"] == pointer["release_id"] == commit
    assert pointer["source_kind"] == "main"
    checkout = installation.target / "flitzis-looper"
    runtime = installation.target / "Runtime"
    assert (checkout / ".git").is_dir()
    assert (checkout / "fixture-version.txt").read_text() == "first"
    assert (checkout / "samples").is_dir()
    assert (checkout / "config/input").is_dir()
    assert (runtime / "python/python.exe").read_text() == "immutable-runtime-first"
    assert not (runtime / "app").exists()
    assert not (installation.target / "versions").exists()
    assert not (installation.target / "data").exists()
    calls = installation.trace("helper-invocations.jsonl")
    assert len(calls) == 1
    assert calls[0]["git_commit"] == commit
    for field in ("source", "runtime"):
        assert Path(str(calls[0][field])).parent == installation.target / "cache"
        assert not Path(str(calls[0][field])).exists()
    assert installation.trace("preflight-history.jsonl")[0]["previous_commit"] is None
    assert not (installation.target / "portable-transaction.json").exists()
    assert len(installation.trace("model-history.jsonl")) == 2
    for name in ("START-LOOPER.bat", "start.bat", "start-release.bat", "launch.ps1"):
        assert (installation.target / name).is_file()


def test_deep_destination_keeps_source_and_final_git_paths_short(
    installation: Installation, tmp_path: Path
) -> None:
    assert len(str(installation.source / ".git").encode("utf-8")) < 180
    prefix = "Portable ü deep destination "
    padding = 190 - len(str(tmp_path / prefix).encode("utf-8"))
    assert padding >= 0, "Use a shorter basetemp for the intrinsic Git path bound"
    installation.target = tmp_path / (prefix + "d" * padding)
    commit = installation.publish("deep")
    require_success(installation.run())
    assert installation.pointer()["git_commit"] == commit
    call = installation.trace("helper-invocations.jsonl")[0]
    assert Path(str(call["canonical_root"])) == installation.target
    source = Path(str(call["source"]))
    assert source.parent == installation.target / "cache"
    assert Path(str(call["git_directory"])) == source / ".git"
    assert 200 <= len(str(source / ".git").encode("utf-8")) <= 220
    assert len(str(installation.target / "flitzis-looper/.git").encode("utf-8")) <= 220
    assert (installation.target / "flitzis-looper/fixture-version.txt").read_text() == "deep"
    assert (installation.target / "Runtime/python/python.exe").read_text() == (
        "immutable-runtime-deep"
    )


def _user_data(installation: Installation) -> dict[Path, bytes]:
    checkout = installation.target / "flitzis-looper"
    contents = {
        checkout / "samples/flitzis_looper.config.json": b'{"Human": "saved config"}',
        checkout / "samples/.pcm-cache/material/original.wav": b"original audio",
        checkout / "samples/.material-migrations/unknown.dat": b"history bytes",
        checkout / "config/input/keyboard.json": b'{"Human": "keyboard"}',
        checkout / "config/input/midi.json": b'{"Human": "MIDI"}',
        installation.target / "models/my-model.bin": b"Human model",
    }
    for path, content in contents.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
    return contents


def test_repeat_and_update_preserve_native_data_and_clean_owned_stages(
    installation: Installation,
) -> None:
    first = installation.publish("first")
    require_success(installation.run())
    original = installation.pointer()
    contents = _user_data(installation)
    require_success(installation.run())
    assert installation.pointer() == original
    assert len(installation.trace("helper-invocations.jsonl")) == 1
    second = installation.publish("second")
    require_success(installation.run())
    assert installation.pointer()["git_commit"] == second
    assert (installation.target / "Runtime/python/python.exe").read_text() == (
        "immutable-runtime-second"
    )
    for path, content in contents.items():
        assert path.read_bytes() == content
    assert not (installation.target / "versions").exists()
    assert not (installation.target / "portable-transaction.json").exists()
    for prefix in ("s-", "r-", "o-", "p-", "relocation-"):
        assert not list((installation.target / "cache").glob(prefix + "*"))
    checks = installation.trace("preflight-history.jsonl")
    assert checks[-1]["git_commit"] == second
    assert checks[-1]["previous_commit"] == first
    require_success(
        _run_powershell([
            "-File",
            str(installation.target / "launch.ps1"),
            "-Root",
            str(installation.target),
            "-Check",
        ])
    )
    assert installation.trace("preflight-history.jsonl")[-1]["fixture_version"] == "second"


def test_same_main_model_repair_does_not_invoke_native_builder(installation: Installation) -> None:
    installation.publish("first")
    require_success(installation.run())
    original = installation.pointer()
    (installation.target / "models/demucs-fixture.th").unlink()
    (installation.target / "models/bs-fixture.ckpt").write_bytes(b"corrupt")
    require_success(installation.run())
    assert installation.pointer() == original
    assert len(installation.trace("helper-invocations.jsonl")) == 1
    for name in ("demucs-fixture.th", "bs-fixture.ckpt"):
        assert (installation.target / "models" / name).read_text() == "verified fixture"


def test_same_main_runtime_defect_rebuilds_before_activation(installation: Installation) -> None:
    commit = installation.publish("first")
    require_success(installation.run())
    (installation.target / "Runtime/first-required.txt").unlink()
    require_success(installation.run())
    assert installation.pointer()["git_commit"] == commit
    assert len(installation.trace("helper-invocations.jsonl")) == 2
    assert (installation.target / "Runtime/first-required.txt").is_file()
    assert not list((installation.target / "cache").glob("o-*"))
    assert not list((installation.target / "cache").glob("p-*"))


@pytest.mark.parametrize("relative", ["licenses/samplerate.txt", "python/vcruntime140.dll"])
@pytest.mark.parametrize("update", [False, True])
def test_replaced_known_runtime_bytes_stop_before_activation_and_remain_intact(
    installation: Installation, relative: str, *, update: bool
) -> None:
    installation.publish("first")
    require_success(installation.run())
    pointer = (installation.target / "current.json").read_bytes()
    contents = _user_data(installation)
    changed = installation.target / "Runtime" / relative
    changed.write_bytes(b"Human replacement; preserve these bytes")
    if update:
        installation.publish("second")

    result = installation.run()

    assert result.returncode != 0, result.stdout + result.stderr
    assert changed.read_bytes() == b"Human replacement; preserve these bytes"
    assert (installation.target / "current.json").read_bytes() == pointer
    assert len(installation.trace("helper-invocations.jsonl")) == 1
    assert not (installation.target / "portable-transaction.json").exists()
    for path, content in contents.items():
        assert path.read_bytes() == content


@pytest.mark.parametrize("relative", ["licenses/samplerate.txt", "python/vcruntime140.dll"])
def test_retired_runtime_cleanup_preserves_replaced_known_bytes(
    installation: Installation, relative: str
) -> None:
    commit = installation.publish("first")
    require_success(installation.run())
    original = (installation.target / "Runtime" / relative).read_bytes()
    retired = installation.target / "cache/p-11111111" / relative
    retired.parent.mkdir(parents=True)
    retired.write_bytes(b"Human replacement after ownership capture")
    journal = installation.target / "portable-transaction.json"
    journal.write_text(
        json.dumps({
            "operation": "1" * 32,
            "git_commit": commit,
            "legacy_directory": None,
            "old_release": {"runtime_files": {relative: hashlib.sha256(original).hexdigest()}},
        }),
        encoding="utf-8",
    )
    captured_journal = journal.read_bytes()

    result = _run_embedded(
        installation.installer.parent,
        f"$record = Get-Content -LiteralPath {_ps_string(journal)} -Raw | ConvertFrom-Json\n"
        f"Complete-PortableTransaction {_ps_string(installation.target)} $record",
    )

    assert result.returncode != 0, result.stdout + result.stderr
    assert retired.read_bytes() == b"Human replacement after ownership capture"
    assert journal.read_bytes() == captured_journal
    assert (installation.target / "Runtime" / relative).read_bytes() == original


@pytest.mark.parametrize(
    "failure",
    [
        "helper",
        "preflight",
        "model-cpu",
        "capture",
        "wrong-commit",
        "dirty-manifest",
        "missing-manifest",
        "missing-helper",
    ],
)
def test_failed_update_preserves_pointer_runtime_and_user_bytes(
    installation: Installation, failure: str
) -> None:
    installation.publish("first")
    require_success(installation.run())
    original = (installation.target / "current.json").read_bytes()
    starter = (installation.target / "launch.ps1").read_bytes()
    contents = _user_data(installation)
    installation.publish("broken", failure=failure)
    result = installation.run()
    assert result.returncode != 0, result.stdout + result.stderr
    assert (installation.target / "current.json").read_bytes() == original
    assert (installation.target / "Runtime/python/python.exe").read_text() == (
        "immutable-runtime-first"
    )
    assert (installation.target / "launch.ps1").read_bytes() == starter
    for path, content in contents.items():
        assert path.read_bytes() == content


@pytest.mark.parametrize("failure", ["helper", "preflight", "model-cpu"])
def test_failed_initial_install_does_not_publish(installation: Installation, failure: str) -> None:
    installation.publish("broken", failure=failure)
    assert installation.run().returncode != 0
    for name in ("current.json", "launch.ps1", "START-LOOPER.bat"):
        assert not (installation.target / name).exists()


def test_failed_capture_is_preserved_without_native_rollback(installation: Installation) -> None:
    installation.publish("first")
    require_success(installation.run())
    contents = _user_data(installation)
    installation.publish("capture-failure", failure="capture")
    result = installation.run()
    assert result.returncode != 0
    assert "incorrectly consumed" not in result.stdout + result.stderr
    record = json.loads((installation.target / "portable-transaction.json").read_text())
    assert record["capture_completed"] is False
    next_commit = installation.publish("second")
    require_success(installation.run())
    assert installation.pointer()["git_commit"] == next_commit
    diagnostic = installation.target / f"cache/failed-relocation-{record['operation']}.json"
    assert diagnostic.is_file()
    for path, content in contents.items():
        assert path.read_bytes() == content


@pytest.mark.parametrize("location", ["flitzis-looper/human.txt", "Runtime/human.txt"])
def test_unknown_files_stop_update_before_swaps(installation: Installation, location: str) -> None:
    installation.publish("first")
    require_success(installation.run())
    original = (installation.target / "current.json").read_bytes()
    unknown = installation.target / location
    unknown.write_bytes(b"Human data")
    installation.publish("second")
    assert installation.run().returncode != 0
    assert unknown.read_bytes() == b"Human data"
    assert (installation.target / "current.json").read_bytes() == original
    assert len(installation.trace("helper-invocations.jsonl")) == 1


def test_changed_tracked_source_is_preserved(installation: Installation) -> None:
    installation.publish("first")
    require_success(installation.run())
    source = installation.target / "flitzis-looper/fixture-version.txt"
    source.write_text("Human edit")
    installation.publish("second")
    assert installation.run().returncode != 0
    assert source.read_text() == "Human edit"
    assert len(installation.trace("helper-invocations.jsonl")) == 1


def test_installer_rejects_unowned_nonempty_target(installation: Installation) -> None:
    installation.publish("first")
    installation.target.mkdir()
    sentinel = installation.target / "human.txt"
    sentinel.write_text("owned by Human")
    assert installation.run().returncode != 0
    assert sentinel.read_text() == "owned by Human"
    assert list(installation.target.iterdir()) == [sentinel]


def _junction(path: Path, destination: Path) -> None:
    command = f'mklink /J "{path}" "{destination}"'
    subprocess.run(f'cmd.exe /d /s /c "{command}"', check=True, capture_output=True)


def test_installer_rejects_reparse_target_without_writing_through_it(
    installation: Installation,
) -> None:
    installation.publish("first")
    outside = installation.target.parent / "other owned folder"
    outside.mkdir()
    _junction(installation.target, outside)
    try:
        assert installation.run().returncode != 0
        assert list(outside.iterdir()) == []
    finally:
        installation.target.rmdir()


@pytest.mark.parametrize("location", ["samples", "config/input"])
def test_installer_rejects_reparse_native_data_without_modifying_current(
    installation: Installation, location: str
) -> None:
    installation.publish("first")
    require_success(installation.run())
    original = (installation.target / "current.json").read_bytes()
    installation.publish("second")
    outside = installation.target.parent / "other owned data"
    outside.mkdir()
    sentinel = outside / "human.bin"
    sentinel.write_bytes(b"outside data")
    data = installation.target / "flitzis-looper" / location
    data.rmdir()
    _junction(data, outside)
    try:
        assert installation.run().returncode != 0
        assert (installation.target / "current.json").read_bytes() == original
        assert list(outside.iterdir()) == [sentinel]
        assert sentinel.read_bytes() == b"outside data"
    finally:
        data.rmdir()


def test_concurrent_installer_stops_before_build(installation: Installation) -> None:
    installation.publish("first")
    require_success(installation.run())
    original = (installation.target / "current.json").read_bytes()
    installation.publish("second")
    with exclusive_file(installation.target / ".install.lock"):
        assert installation.run().returncode != 0
    assert (installation.target / "current.json").read_bytes() == original
    assert len(installation.trace("helper-invocations.jsonl")) == 1


def test_portable_dispatch_rejects_escaping_legacy_pointer(tmp_path: Path) -> None:
    shutil.copyfile(REPO / "scripts/portable/dispatch.ps1", tmp_path / "launch.ps1")
    (tmp_path / ".install.lock").touch()
    (tmp_path / "current.json").write_text(
        json.dumps({
            "schema_version": 1,
            "directory": "../outside",
            "git_commit": "a" * 40,
        })
    )
    result = _run_powershell([
        "-File",
        str(tmp_path / "launch.ps1"),
        "-Root",
        str(tmp_path),
        "-Check",
    ])
    assert result.returncode != 0


def _is_owned_git_pack_leaf(leaf: Path, pack_directory: Path) -> bool:
    object_id = leaf.stem.removeprefix("pack-")
    return (
        leaf.parent == pack_directory
        and leaf.name.startswith("pack-")
        and leaf.suffix in {".idx", ".pack", ".rev"}
        and len(object_id) in {40, 64}
        and all(character in "0123456789abcdef" for character in object_id)
    )


def _remove_fixture_checkout(checkout: Path, root: Path) -> None:
    owned_checkout = checkout.resolve(strict=True)
    assert owned_checkout == root.resolve(strict=True) / "flitzis-looper"
    pack_directory = owned_checkout / ".git/objects/pack"
    assert pack_directory.resolve() == pack_directory

    def retry_readonly_pack(
        function: Callable[[str], None], path: str, error: BaseException
    ) -> None:
        leaf = Path(path)
        if (
            function is not os.unlink
            or not isinstance(error, PermissionError)
            or getattr(error, "winerror", None) != 5
            or not _is_owned_git_pack_leaf(leaf, pack_directory)
        ):
            raise error
        attributes = leaf.lstat()
        if (
            not stat.S_ISREG(attributes.st_mode)
            or attributes.st_file_attributes & stat.FILE_ATTRIBUTE_REPARSE_POINT
            or not attributes.st_file_attributes & stat.FILE_ATTRIBUTE_READONLY
        ):
            raise error
        # Windows chmod changes only the read-only bit of this owned pack leaf.
        os.chmod(leaf, attributes.st_mode | stat.S_IWRITE)
        function(path)

    shutil.rmtree(owned_checkout, onexc=retry_readonly_pack)


def test_legacy_native_data_moves_once_with_same_file_identities(
    installation: Installation,
) -> None:
    commit = installation.publish("first")
    require_success(installation.run())
    contents = _user_data(installation)
    checkout = installation.target / "flitzis-looper"
    legacy_name = f"main-{commit[:12]}-01234567"
    legacy = installation.target / "versions" / legacy_name
    legacy.parent.mkdir()
    (installation.target / "Runtime").rename(legacy)
    manifest = json.loads((legacy / "release-manifest.json").read_text())
    manifest["schema_version"] = 1
    (legacy / "release-manifest.json").write_text(json.dumps(manifest))
    data = installation.target / "data"
    (data / "config").mkdir(parents=True)
    samples_identity = (checkout / "samples").stat().st_ino
    original_identity = (checkout / "samples/.pcm-cache/material/original.wav").stat().st_ino
    (checkout / "samples").rename(data / "samples")
    (checkout / "config/input").rename(data / "config/input")
    assert checkout.resolve().is_relative_to(installation.target.resolve())
    _remove_fixture_checkout(checkout, installation.target)
    (data / "unknown-human.bin").write_bytes(b"unknown preserved")
    (installation.target / "current.json").write_text(
        json.dumps({
            "schema_version": 1,
            "directory": legacy_name,
            "release_id": commit,
            "git_commit": commit,
            "source_kind": "main",
        })
    )
    require_success(installation.run())
    for path, content in contents.items():
        assert path.read_bytes() == content
    assert (checkout / "samples").stat().st_ino == samples_identity
    assert (checkout / "samples/.pcm-cache/material/original.wav").stat().st_ino == (
        original_identity
    )
    assert (data / "unknown-human.bin").read_bytes() == b"unknown preserved"
    assert not legacy.exists()
    assert not (installation.target / "versions").exists()
    calls = len(installation.trace("helper-invocations.jsonl"))
    require_success(installation.run())
    assert len(installation.trace("helper-invocations.jsonl")) == calls


INTERRUPTION_SEAMS = (
    "captured",
    "old_checkout",
    "new_checkout",
    "old_runtime",
    "new_runtime",
    "samples",
    "input",
    "state",
    "accepted",
)


@dataclass(slots=True)
class ActivationPaths:
    source: Path
    candidate: Path
    checkout: Path
    runtime: Path
    old_checkout: Path
    old_runtime: Path
    relocation: Path

    @classmethod
    def for_transaction(cls, root: Path, operation: str) -> ActivationPaths:
        cache = root / "cache"
        short = operation[:8]
        return cls(
            cache / f"s-{short}",
            cache / f"r-{short}",
            root / "flitzis-looper",
            root / "Runtime",
            cache / f"o-{short}",
            cache / f"p-{short}",
            cache / f"relocation-{operation}.json",
        )


def _simulate_activation_interrupt(
    installation: Installation,
    seam: str,
    commit: str,
) -> ActivationPaths:
    journal = installation.target / "portable-transaction.json"
    record = json.loads(journal.read_text())
    paths = ActivationPaths.for_transaction(installation.target, str(record["operation"]))
    record["starters"] = {
        name: base64.b64encode((installation.target / name).read_bytes()).decode("ascii")
        for name in ("launch.ps1", "START-LOOPER.bat", "start.bat", "start-release.bat")
    }
    require_success(
        _run_powershell([
            "-File",
            str(paths.candidate / "launch.ps1"),
            "-Root",
            str(installation.target),
            "-CheckoutDirectory",
            str(paths.source),
            "-RuntimeDirectory",
            str(paths.candidate),
            "-InstallerOwned",
            "-RelocationAction",
            "capture",
            "-RelocationSource",
            str(paths.checkout / "samples"),
            "-RelocationTarget",
            str(paths.checkout / "samples"),
            "-RelocationRecord",
            str(paths.relocation),
        ])
    )
    record["capture_completed"] = True
    reached = INTERRUPTION_SEAMS.index(seam)
    if reached >= 1:
        paths.checkout.rename(paths.old_checkout)
    if reached >= 2:
        paths.source.rename(paths.checkout)
    if reached >= 3:
        paths.runtime.rename(paths.old_runtime)
    if reached >= 4:
        paths.candidate.rename(paths.runtime)
    if reached >= 5:
        (paths.old_checkout / "samples").rename(paths.checkout / "samples")
    if reached >= 6:
        (paths.checkout / "config").mkdir(exist_ok=True)
        (paths.old_checkout / "config/input").rename(paths.checkout / "config/input")
    record["phase"] = "accepted" if reached == 8 else "verifying" if reached >= 7 else "swapping"
    if reached >= 7:
        release = json.loads((paths.runtime / "release-manifest.json").read_text())
        (installation.target / "current.json").write_text(
            json.dumps({
                "schema_version": 2,
                "git_commit": commit,
                "release_id": commit,
                "source_kind": "main",
                "release": release,
            })
        )
    journal.write_text(json.dumps(record))
    return paths


@pytest.mark.parametrize("seam", INTERRUPTION_SEAMS)
def test_interrupted_activation_recovers_old_or_verified_new(
    installation: Installation,
    seam: str,
) -> None:
    first = installation.publish("first")
    require_success(installation.run())
    contents = _user_data(installation)
    second = installation.publish("second", failure="prepared-interrupt")
    assert installation.run().returncode != 0
    paths = _simulate_activation_interrupt(installation, seam, second)
    result = installation.run()
    expected = second if seam == "accepted" else first
    assert installation.pointer()["git_commit"] == expected
    assert (paths.runtime / "python/python.exe").read_text() == (
        "immutable-runtime-second" if seam == "accepted" else "immutable-runtime-first"
    )
    if seam == "accepted":
        require_success(result)
        assert not (installation.target / "portable-transaction.json").exists()
    else:
        # The deliberately interrupted new builder remains selected remotely.
        assert result.returncode != 0
    for path, content in contents.items():
        assert path.read_bytes() == content
    assert not paths.old_checkout.exists()
    assert not paths.old_runtime.exists()


LIVE_PYTHON = """
using System;
using System.IO;
using System.Threading;
public static class PortableLiveFixture {
    public static int Main(string[] arguments) {
        File.WriteAllText(Environment.GetEnvironmentVariable("LOOPER_TEST_READY"), "ready");
        while (!File.Exists(Environment.GetEnvironmentVariable("LOOPER_TEST_EXIT"))) {
            Thread.Sleep(20);
        }
        return 0;
    }
}
"""


@pytest.mark.parametrize("legacy", [False, True])
def test_running_app_stops_update_and_is_not_terminated(
    installation: Installation, tmp_path: Path, *, legacy: bool
) -> None:
    first = installation.publish("first")
    require_success(installation.run())
    original = (installation.target / "current.json").read_bytes()
    ready, leave = tmp_path / "ready", tmp_path / "leave"
    executable = installation.target / (
        f"versions/main-{first[:12]}-01234567/python/python.exe"
        if legacy
        else "Runtime/python/python.exe"
    )
    executable.parent.mkdir(parents=True, exist_ok=True)
    executable.unlink(missing_ok=True)
    compiled = tmp_path / "live-python.exe"
    compiler = tmp_path / "compile-live.ps1"
    compiler.write_text(
        "$ErrorActionPreference = 'Stop'\n"
        "Add-Type -TypeDefinition @'\n"
        + LIVE_PYTHON
        + "'@ -OutputAssembly "
        + _ps_string(compiled)
        + " -OutputType ConsoleApplication",
        encoding="utf-8-sig",
    )
    require_success(_run_powershell(["-File", str(compiler)]))
    assert compiled.is_file(), "Owned private live fixture was not compiled"
    compiled.replace(executable)
    environment = _git_environment() | {
        "LOOPER_TEST_READY": str(ready),
        "LOOPER_TEST_EXIT": str(leave),
    }
    if legacy:
        process = subprocess.Popen(
            [str(executable)],
            env=environment,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
    else:
        shutil.copyfile(
            REPO / "scripts/portable/launch.ps1",
            installation.target / "Runtime/launch.ps1",
        )
        process = subprocess.Popen(
            [
                "powershell.exe",
                "-NoLogo",
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                str(installation.target / "launch.ps1"),
                "-Root",
                str(installation.target),
            ],
            env=environment,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
    try:
        deadline = time.monotonic() + 15
        while not ready.exists() and time.monotonic() < deadline and process.poll() is None:
            time.sleep(0.05)
        assert ready.exists(), "Owned private child did not start"
        installation.publish("second")
        result = installation.run()
        assert result.returncode != 0
        assert "schliessen" in result.stdout.lower()
        assert process.poll() is None
        assert (installation.target / "current.json").read_bytes() == original
        assert len(installation.trace("helper-invocations.jsonl")) == 1
    finally:
        leave.touch()
        process.communicate(timeout=20)


def _embedded_functions() -> str:
    content = (REPO / "INSTALL-LOOPER-PORTABLE.bat").read_text(encoding="utf-8-sig")
    _, separator, powershell = content.partition(":__PORTABLE_PS__\n")
    assert separator
    functions, separator, _ = powershell.partition("\ntry {\n")
    assert separator
    return functions


def _run_embedded(tmp_path: Path, commands: str) -> subprocess.CompletedProcess[str]:
    script = tmp_path / "embedded-installer-functions.ps1"
    script.write_text(_embedded_functions() + "\n" + commands, encoding="utf-8-sig")
    return _run_powershell(["-File", str(script)])


def test_download_hash_requires_the_exact_sha256(tmp_path: Path) -> None:
    payload = tmp_path / "verified-tool.bin"
    payload.write_bytes(b"owned bootstrap fixture")
    digest = hashlib.sha256(payload.read_bytes()).hexdigest()
    require_success(_run_embedded(tmp_path, f"Assert-Hash {_ps_string(payload)} '{digest}'"))
    payload.write_bytes(b"modified bootstrap fixture")
    assert _run_embedded(tmp_path, f"Assert-Hash {_ps_string(payload)} '{digest}'").returncode != 0
    assert _run_embedded(tmp_path, f"Assert-Hash {_ps_string(payload)} 'not-a-sha'").returncode != 0


@pytest.mark.parametrize(
    "unsafe",
    ["escape", "absolute", "symlink", "windows-alias", "reserved", "case-duplicate"],
)
def test_bootstrap_archive_is_validated_before_any_extraction(tmp_path: Path, unsafe: str) -> None:
    archive_path = tmp_path / "tool.zip"
    destination = tmp_path / "private tool"
    destination.mkdir()
    with ZipFile(archive_path, "w") as archive:
        archive.writestr("safe.txt", "must not be extracted on failure")
        if unsafe == "symlink":
            link = ZipInfo("link")
            link.create_system = 3
            link.external_attr = 0o120777 << 16
            archive.writestr(link, "outside")
        else:
            name = {
                "escape": "../escaped.txt",
                "absolute": "C:/escaped.txt",
                "windows-alias": "python/python.exe.",
                "reserved": "NUL.txt",
                "case-duplicate": "SAFE.TXT",
            }[unsafe]
            archive.writestr(name, "unsafe")
    result = _run_embedded(
        tmp_path, f"Expand-ContainedZip {_ps_string(archive_path)} {_ps_string(destination)}"
    )
    assert result.returncode != 0, result.stdout + result.stderr
    assert list(destination.iterdir()) == []
    assert not (tmp_path / "escaped.txt").exists()


@pytest.mark.parametrize("integrity", ["verified", "corrupt"])
def test_missing_git_uses_the_pinned_verified_private_bootstrap(
    tmp_path: Path, integrity: str
) -> None:
    executable = tmp_path / "fixture-git.exe"
    compiler = tmp_path / "compile-fixture.ps1"
    compiler.write_text(
        "Add-Type -TypeDefinition @'\n"
        + FAKE_GIT
        + "'@ -OutputAssembly "
        + _ps_string(executable)
        + " -OutputType ConsoleApplication",
        encoding="utf-8-sig",
    )
    require_success(_run_powershell(["-File", str(compiler)]))
    archive_path = tmp_path / "owned-mingit.zip"
    with ZipFile(archive_path, "w") as archive:
        archive.write(executable, "ucrt64/bin/git.exe")
    fixture_hash = hashlib.sha256(archive_path.read_bytes()).hexdigest()
    digest = fixture_hash if integrity == "verified" else "0" * 64
    prefix = "Portable ü, comma; deep install "
    padding = 190 - len(str(tmp_path / prefix).encode("utf-8"))
    assert padding >= 0, "Use a shorter basetemp for the fixed-depth bootstrap fixture"
    root = tmp_path / (prefix + "d" * padding)
    assert len(str(root).encode("utf-8")) == 190
    (root / "cache").mkdir(parents=True)
    trace = tmp_path / "git-runs.txt"
    executable_trace = tmp_path / "git-executable-paths.txt"
    commands = rf"""
$originalHash = ${{function:Assert-Hash}}
function Get-Command {{ param($Name, $CommandType, $ErrorAction); return }}
function Read-Payload([Uri] $Uri, [string] $Destination) {{
    if ($Uri.AbsoluteUri -ne '{GIT_ARCHIVE}') {{ throw 'Bootstrap URL is not pinned' }}
    Copy-Item -LiteralPath {_ps_string(archive_path)} -Destination $Destination
}}
function Assert-Hash([string] $Path, [string] $Expected) {{
    if ($Expected -ne '{GIT_SHA256}') {{ throw 'Bootstrap SHA256 is not pinned' }}
    & $originalHash $Path '{digest}'
}}
$env:LOOPER_TEST_GIT_TRACE = {_ps_string(trace)}
$env:LOOPER_TEST_GIT_EXECUTABLE_TRACE = {_ps_string(executable_trace)}
$env:PATH = "$env:SystemRoot\System32;$env:SystemRoot;$PSHOME"
$mapping = $null
try {{
    $mapping = New-PortableBuildDrive {_ps_string(root)}
    $git = Get-PortableGit $mapping.Path
    $again = Get-PortableGit $mapping.Path
    if ($git -ne $again) {{ throw 'Verified private Git must be reused' }}
    if ($git -ne (Join-Path $mapping.Path 'tools/git-2.56.0.2/ucrt64/bin/git.exe')) {{
        throw 'Git must use the pinned real UCRT executable inside the installation'
    }}
}} finally {{
    if ($null -ne $mapping) {{ Remove-PortableBuildDrive $mapping | Out-Null }}
}}
"""
    result = _run_embedded(tmp_path, commands)
    published = root / "tools/git-2.56.0.2/ucrt64/bin/git.exe"
    if integrity == "verified":
        require_success(result)
        assert published.read_bytes() == executable.read_bytes()
        assert trace.read_text().splitlines() == ["--version"]
        executed = Path(executable_trace.read_text(encoding="utf-8").strip())
        assert executed.parts[-3:] == ("ucrt64", "bin", "git.exe")
        staging = executed.parent.parent.parent.name
        assert staging.startswith("g-")
        assert len(staging) == 10
        assert list((root / "tools").iterdir()) == [published.parents[2]]
    else:
        assert result.returncode != 0, result.stdout + result.stderr
        assert not published.exists()
        assert not trace.exists()
        assert not executable_trace.exists()
