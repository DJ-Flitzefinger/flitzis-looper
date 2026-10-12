"""Exercise the real source-main installer without network, native builds or audio."""

import ctypes
import hashlib
import json
import os
import shutil
import subprocess
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING
from zipfile import ZipFile, ZipInfo

import pytest

if TYPE_CHECKING:
    from collections.abc import Iterator

pytestmark = pytest.mark.skipif(os.name != "nt", reason="Windows portable installation")
REPO = Path(__file__).resolve().parents[2]
REPOSITORY = "https://github.com/DJ-Flitzefinger/flitzis-looper.git"
GIT_ARCHIVE = (
    "https://github.com/git-for-windows/git/releases/download/"
    "v2.56.0.windows.2/MinGit-2.56.0.2-64-bit.zip"
)
GIT_SHA256 = "da35e72aa21c005a5a0d298cfbae110bc1609a815730ea0dde84b01a1b3cd3be"
LAUNCH = """
param([string] $Root, [switch] $Check, [string] $VersionDirectory)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $Check) { exit 90 }
if (-not $VersionDirectory) { throw 'Staging must name its actual candidate version' }
if ((Split-Path -Leaf $PSScriptRoot) -ne $VersionDirectory) { throw 'Wrong candidate directory' }
# Read this immutable fixture through the path used to execute the candidate;
# the BAT may intentionally provide a short build alias while Root is canonical.
$version = $PSScriptRoot
$releasePath = Join-Path $version 'release-manifest.json'
$release = Get-Content -LiteralPath $releasePath -Raw | ConvertFrom-Json
if ($release.fixture_version -ne '@VERSION@') { throw 'Wrong launch implementation' }
$required = Join-Path $version '@VERSION@-required.txt'
if (-not (Test-Path -LiteralPath $required)) { throw 'New launcher required' }
$previous = $null
$pointer = Join-Path $Root 'current.json'
if (Test-Path -LiteralPath $pointer) {
    $previous = (Get-Content -LiteralPath $pointer -Raw | ConvertFrom-Json).git_commit
}
$entry = @{fixture_version='@VERSION@'; git_commit=$release.git_commit;
    version_directory=$VersionDirectory; previous_commit=$previous} | ConvertTo-Json -Compress
[IO.File]::AppendAllText((Join-Path $Root 'cache/preflight-history.jsonl'), $entry + "`n")
exit @EXIT@
"""
HELPER = r"""
param([string] $Root, [string] $Source, [string] $Git, [string] $Commit,
    [string] $RuntimeDirectory)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$actual = & $Git -C $Source rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $actual -ne $Commit) { throw 'Expected the pinned source commit' }
$dirty = @(& $Git -C $Source status --porcelain)
if ($LASTEXITCODE -ne 0 -or $dirty.Count) { throw 'Expected a clean private source checkout' }
$canonicalSource = [IO.Path]::GetFullPath((& $Git -C $Source rev-parse --show-toplevel))
if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve the physical source checkout' }
$canonicalGit = & $Git -C $Source rev-parse --path-format=absolute --git-dir
if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve the physical Git directory' }
$rootPrefix = $Root.TrimEnd('\')
$relativeSource = $Source.Substring($rootPrefix.Length).TrimStart('\')
$canonicalRoot = $canonicalSource.Substring(0,
    $canonicalSource.Length - $relativeSource.Length).TrimEnd('\')
$relativeRuntime = $RuntimeDirectory.Substring($rootPrefix.Length).TrimStart('\')
$canonicalRuntime = Join-Path $canonicalRoot $relativeRuntime
$prefix = [IO.Path]::GetFullPath((Join-Path $Root 'versions')).TrimEnd('\') + '\'
if (-not [IO.Path]::GetFullPath($RuntimeDirectory).StartsWith(
    $prefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'Candidate leaves versions' }
$entry = @{fixture_version='@VERSION@'; git_commit=$Commit; source=$canonicalSource;
    runtime=$canonicalRuntime; git_directory=[IO.Path]::GetFullPath($canonicalGit);
    canonical_root=$canonicalRoot; build_root=$Root} | ConvertTo-Json -Compress
[IO.File]::AppendAllText((Join-Path $Root 'cache/helper-invocations.jsonl'), $entry + "`n")
if ('@FAILURE@' -eq 'helper') { exit 23 }
[IO.Directory]::CreateDirectory((Join-Path $RuntimeDirectory 'python')) | Out-Null
[IO.File]::WriteAllText((Join-Path $RuntimeDirectory 'python/python.exe'),
    'immutable-runtime-@VERSION@')
[IO.File]::WriteAllText((Join-Path $RuntimeDirectory '@VERSION@-required.txt'), 'required')
foreach ($name in @('launch.ps1', 'dispatch.ps1', 'START-LOOPER.bat')) {
    [IO.File]::Copy((Join-Path $Source "scripts/portable/$name"),
        (Join-Path $RuntimeDirectory $name), $false)
}
if ('@FAILURE@' -ne 'missing-manifest') {
    $runtimeCommit = $Commit
    if ('@FAILURE@' -eq 'wrong-commit') {
        $runtimeCommit = 'ffffffffffffffffffffffffffffffffffffffff'
    }
    $release = @{release_id=$Commit; git_commit=$runtimeCommit;
        source_dirty=('@FAILURE@' -eq 'dirty-manifest'); fixture_version='@VERSION@'}
    [IO.File]::WriteAllText((Join-Path $RuntimeDirectory 'release-manifest.json'),
        ($release | ConvertTo-Json))
}
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
            LAUNCH.replace("@VERSION@", version).replace(
                "@EXIT@", "17" if failure == "preflight" else "0"
            ),
            encoding="utf-8-sig",
        )
        shutil.copyfile(REPO / "scripts/portable/dispatch.ps1", portable / "dispatch.ps1")
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
        return json.loads((self.target / "current.json").read_text(encoding="utf-8-sig"))

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


def test_first_install_builds_current_main_without_a_release_bundle(
    installation: Installation,
) -> None:
    commit = installation.publish("first")
    require_success(installation.run(explicit_target=False))
    pointer = installation.pointer()
    assert pointer["git_commit"] == pointer["release_id"] == commit
    assert pointer["source_kind"] == "main"
    assert str(pointer["directory"]).startswith(f"main-{commit[:12]}-")
    candidate = installation.target / "versions" / str(pointer["directory"])
    assert (candidate / "python/python.exe").read_text() == "immutable-runtime-first"
    assert json.loads((candidate / "release-manifest.json").read_text())["source_dirty"] is False
    calls = installation.trace("helper-invocations.jsonl")
    assert len(calls) == 1
    assert calls[0]["git_commit"] == commit
    assert Path(str(calls[0]["runtime"])) == candidate
    source = Path(str(calls[0]["source"]))
    assert source.is_relative_to(installation.target / "cache")
    assert (source / "fixture-version.txt").read_text() == "first"
    assert installation.trace("preflight-history.jsonl")[0]["previous_commit"] is None
    assert (installation.target / "portable-root.json").is_file()
    for name in ("START-LOOPER.bat", "start.bat", "start-release.bat", "launch.ps1"):
        assert (installation.target / name).is_file()


def test_deep_destination_builds_from_a_short_local_remote(
    installation: Installation, tmp_path: Path
) -> None:
    # Git canonicalizes SUBST paths and limits inherited GIT_DIR by UTF-8 bytes.
    # Keep the remote independently short so a transport-side limit cannot mask
    # the installer's destination layout. The target remains below that intrinsic
    # limit, while the former source-commit12-GUID32 layout would exceed it.
    remote_git = installation.source / ".git"
    assert len(str(remote_git).encode("utf-8")) < 180
    prefix = "Portable ü deep destination "
    padding = 190 - len(str(tmp_path / prefix).encode("utf-8"))
    assert padding >= 0, "Use a shorter basetemp; the remote must stay independently short"
    installation.target = tmp_path / (prefix + "d" * padding)
    assert len(str(installation.target).encode("utf-8")) == 190
    commit = installation.publish("deep")
    require_success(installation.run())
    assert installation.pointer()["git_commit"] == commit
    calls = installation.trace("helper-invocations.jsonl")
    assert len(calls) == 1
    assert Path(str(calls[0]["canonical_root"])) == installation.target
    source = Path(str(calls[0]["source"]))
    assert source.parent == installation.target / "cache"
    assert Path(str(calls[0]["git_directory"])) == source / ".git"
    assert 200 <= len(str(source / ".git").encode("utf-8")) <= 220
    assert (source / "fixture-version.txt").read_text() == "deep"
    candidate = installation.target / "versions" / str(installation.pointer()["directory"])
    assert Path(str(calls[0]["runtime"])) == candidate
    assert (candidate / "python/python.exe").read_text() == "immutable-runtime-deep"


def test_repeat_and_new_main_keep_old_locked_runtime_and_user_data(
    installation: Installation,
) -> None:
    first = installation.publish("first")
    require_success(installation.run())
    original = installation.pointer()
    old_runtime = (
        installation.target / "versions" / str(original["directory"]) / "python/python.exe"
    )
    data = installation.target / "data/user-settings.json"
    model = installation.target / "models/my-model.bin"
    data.write_text("Human settings", encoding="utf-8")
    model.write_bytes(b"Human model")
    starter = (installation.target / "launch.ps1").read_bytes()
    sources = set((installation.target / "cache").glob("source-*"))
    require_success(installation.run())
    assert installation.pointer() == original
    assert len(installation.trace("helper-invocations.jsonl")) == 1
    assert set((installation.target / "cache").glob("source-*")) == sources
    second = installation.publish("second")
    with exclusive_file(old_runtime):
        require_success(installation.run())
    assert installation.pointer()["git_commit"] == second
    assert installation.pointer()["directory"] != original["directory"]
    assert old_runtime.read_text() == "immutable-runtime-first"
    assert data.read_text() == "Human settings"
    assert model.read_bytes() == b"Human model"
    assert (installation.target / "launch.ps1").read_bytes() == starter
    checks = installation.trace("preflight-history.jsonl")
    assert checks[-1]["git_commit"] == second
    assert checks[-1]["previous_commit"] == first
    # The unchanged dispatcher must select the newly activated candidate launcher.
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


def test_same_main_rebuilds_an_unusable_runtime_before_activation(
    installation: Installation,
) -> None:
    commit = installation.publish("first")
    require_success(installation.run())
    original = installation.pointer()
    candidate = installation.target / "versions" / str(original["directory"])
    (candidate / "first-required.txt").unlink()
    require_success(installation.run())
    assert installation.pointer()["git_commit"] == commit
    assert installation.pointer()["directory"] != original["directory"]
    assert len(installation.trace("helper-invocations.jsonl")) == 2
    assert (candidate / "python/python.exe").read_text() == "immutable-runtime-first"


@pytest.mark.parametrize(
    "failure",
    ["helper", "preflight", "wrong-commit", "dirty-manifest", "missing-manifest", "missing-helper"],
)
def test_failed_source_update_preserves_accepted_pointer_runtime_and_data(
    installation: Installation, failure: str
) -> None:
    installation.publish("first")
    require_success(installation.run())
    pointer = installation.target / "current.json"
    original = pointer.read_bytes()
    runtime = (
        installation.target
        / "versions"
        / str(installation.pointer()["directory"])
        / "python/python.exe"
    )
    launcher = (installation.target / "launch.ps1").read_bytes()
    data = installation.target / "data/project.bin"
    data.write_bytes(b"saved project")
    installation.publish("broken", failure=failure)
    result = installation.run()
    assert result.returncode != 0, result.stdout + result.stderr
    assert pointer.read_bytes() == original
    assert runtime.read_text() == "immutable-runtime-first"
    assert (installation.target / "launch.ps1").read_bytes() == launcher
    assert data.read_bytes() == b"saved project"


@pytest.mark.parametrize("failure", ["helper", "preflight"])
def test_failed_initial_install_never_publishes_a_current_runtime(
    installation: Installation, failure: str
) -> None:
    installation.publish("broken", failure=failure)
    result = installation.run()
    assert result.returncode != 0, result.stdout + result.stderr
    assert not (installation.target / "current.json").exists()
    assert not (installation.target / "launch.ps1").exists()
    assert not (installation.target / "START-LOOPER.bat").exists()


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


def test_installer_rejects_reparse_data_directory_without_modifying_current(
    installation: Installation,
) -> None:
    installation.publish("first")
    require_success(installation.run())
    original = (installation.target / "current.json").read_bytes()
    installation.publish("second")
    outside = installation.target.parent / "other owned data"
    outside.mkdir()
    sentinel = outside / "human.bin"
    sentinel.write_bytes(b"outside data")
    data = installation.target / "data"
    data.rmdir()
    _junction(data, outside)
    try:
        assert installation.run().returncode != 0
        assert (installation.target / "current.json").read_bytes() == original
        assert list(outside.iterdir()) == [sentinel]
        assert sentinel.read_bytes() == b"outside data"
    finally:
        data.rmdir()


def test_concurrent_installer_fails_without_changing_current_version(
    installation: Installation,
) -> None:
    installation.publish("first")
    require_success(installation.run())
    original = (installation.target / "current.json").read_bytes()
    installation.publish("second")
    with exclusive_file(installation.target / ".install.lock"):
        assert installation.run().returncode != 0
    assert (installation.target / "current.json").read_bytes() == original
    assert len(installation.trace("helper-invocations.jsonl")) == 1


def test_portable_launch_rejects_escaping_current_pointer(tmp_path: Path) -> None:
    shutil.copyfile(REPO / "scripts/portable/dispatch.ps1", tmp_path / "launch.ps1")
    (tmp_path / "current.json").write_text(
        json.dumps({"schema_version": 1, "directory": "../outside"})
    )
    result = _run_powershell([
        "-File",
        str(tmp_path / "launch.ps1"),
        "-Root",
        str(tmp_path),
        "-Check",
    ])
    assert result.returncode != 0


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
