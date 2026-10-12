"""Exercise Windows toolchain bootstrap decisions without running installers or compilers."""

import json
import os
import shlex
import subprocess
from pathlib import Path

import pytest

pytestmark = pytest.mark.skipif(os.name != "nt", reason="Windows PowerShell toolchain bootstrap")
REPO = Path(__file__).resolve().parents[2]
RESULT_PREFIX = "PORTABLE_TOOLCHAIN_RESULT:"
BOOTSTRAP_URL = "https://aka.ms/vs/17/release/vs_buildtools.exe"
MSVC_COMPONENTS = (
    "Microsoft.VisualStudio.Workload.VCTools",
    "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
    "Microsoft.VisualStudio.Component.VC.Redist.14.Latest",
    "Microsoft.VisualStudio.Component.Windows11SDK.26100",
)
RUSTUP_URL = (
    "https://static.rust-lang.org/rustup/archive/1.29.1/x86_64-pc-windows-msvc/rustup-init.exe"
)
RUSTUP_SHA256 = "6f4bef66261261fcb43131be8720bab817d403a09edec7455c371974b90bdb7e"
SAFE_SEAMS = r"""
function Read-Payload { throw 'Unexpected real network download' }
function Start-Process { throw 'Unexpected real process launch' }
function Invoke-Checked { throw 'Unexpected real compiler/tool execution' }
function Invoke-PortableCapturedProcess { throw 'Unexpected real captured process' }
function Write-ToolchainResult($Value) {
    Write-Output ('PORTABLE_TOOLCHAIN_RESULT:' + ($Value | ConvertTo-Json -Depth 8 -Compress))
}
"""
SIGNED_BOOTSTRAP = r"""
$script:signaturePaths = @()
$script:launches = @()
$script:installed = $false
function Get-PortableVisualStudioInstances([switch] $All) {
    if ($script:installed) {
        [pscustomobject]@{ installationPath = $script:systemInstall }
    }
}
function Assert-PortableMicrosoftSignature([string] $Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw 'Signature must inspect a payload'
    }
    $script:signaturePaths += $Path
}
$script:systemInstall = Join-Path ([Environment]::GetFolderPath(
    [Environment+SpecialFolder]::ProgramFiles)) 'LooperBuildTools'
function Test-Path {
    param([string] $LiteralPath, [string] $PathType)
    if ($LiteralPath -eq $script:systemInstall) { return $false }
    if ($PathType) {
        return Microsoft.PowerShell.Management\Test-Path -LiteralPath $LiteralPath `
            -PathType $PathType
    }
    return Microsoft.PowerShell.Management\Test-Path -LiteralPath $LiteralPath
}
"""


def _ps_string(value: Path | str) -> str:
    return "'" + str(value).replace("'", "''") + "'"


def _run_toolchain(tmp_path: Path, commands: str) -> dict[str, object]:
    root = tmp_path / "owned installation ü with spaces"
    root.mkdir(exist_ok=True)
    script = tmp_path / "toolchain-regression.ps1"
    script.write_text(
        "$ErrorActionPreference = 'Stop'\n"
        "Set-StrictMode -Version Latest\n"
        "[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)\n"
        "if ($PSVersionTable.PSVersion.Major -ne 5) { throw 'Expected Windows PowerShell 5' }\n"
        f". {_ps_string(REPO / 'scripts/portable/common.ps1')}\n"
        f". {_ps_string(REPO / 'scripts/portable/build-path.ps1')}\n"
        f". {_ps_string(REPO / 'scripts/portable/toolchain.ps1')}\n"
        f"$root = {_ps_string(root)}\n" + SAFE_SEAMS + commands,
        encoding="utf-8-sig",
    )
    result = subprocess.run(
        [
            "powershell.exe",
            "-NoLogo",
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            str(script),
        ],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    records = [
        line.removeprefix(RESULT_PREFIX)
        for line in result.stdout.splitlines()
        if line.startswith(RESULT_PREFIX)
    ]
    assert len(records) == 1, result.stdout + result.stderr
    return json.loads(records[0])


def test_missing_msvc_downloads_verified_microsoft_tools_and_explicit_components(
    tmp_path: Path,
) -> None:
    result = _run_toolchain(
        tmp_path,
        SIGNED_BOOTSTRAP
        + r"""
$script:downloads = @()
$script:probes = @()
function Read-Payload([Uri] $Uri, [string] $Destination) {
    $script:downloads += $Uri.AbsoluteUri
    if (-not $Destination.StartsWith($root + '\', [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Fixture download escaped its installation'
    }
    [IO.File]::WriteAllText($Destination, 'signed bootstrap fixture')
}
function Start-Process {
    param($FilePath, $ArgumentList, $WorkingDirectory, $WindowStyle, $Wait, $PassThru,
        $ErrorAction, $Verb)
    if ($script:signaturePaths -notcontains $FilePath) { throw 'Execution before verification' }
    $script:launches += @{ executable=$FilePath; arguments=$ArgumentList;
        working_directory=$WorkingDirectory; window_style=$WindowStyle;
        wait=$Wait; pass_thru=$PassThru }
    $script:installed = $true
    $process = [pscustomobject]@{ ExitCode=0 }
    $process | Add-Member -MemberType ScriptMethod -Name Dispose -Value { }
    return $process
}
function Test-PortableMsvc([string] $Root, [string] $InstallationPath) {
    $script:probes += $InstallationPath
}
$selected = @(Enable-PortableMsvc $root)
Write-ToolchainResult @{ selected=$selected; downloads=$script:downloads;
    signatures=$script:signaturePaths; launches=$script:launches; probes=$script:probes;
    system_install=$script:systemInstall }
""",
    )
    root = tmp_path / "owned installation ü with spaces"
    installer = root / "cache/toolchain/vs_buildtools-17.exe"
    assert result["downloads"] == [BOOTSTRAP_URL]
    assert result["selected"] == result["probes"] == [result["system_install"]]
    assert not Path(str(result["system_install"])).is_relative_to(root)
    signatures = result["signatures"]
    assert isinstance(signatures, list)
    assert len(signatures) == 2
    assert str(signatures[0]).endswith(".partial.exe")
    assert Path(str(signatures[1])) == installer
    launches = result["launches"]
    assert isinstance(launches, list)
    assert len(launches) == 1
    launch = launches[0]
    assert Path(launch["executable"]) == installer
    assert Path(launch["working_directory"]) == root / "cache/toolchain"
    assert launch["window_style"] == "Hidden"
    assert launch["wait"] is launch["pass_thru"] is True
    arguments = shlex.split(launch["arguments"])
    for component in MSVC_COMPONENTS:
        assert arguments[arguments.index(component) - 1] == "--add"
    assert {"--wait", "--passive", "--norestart"}.issubset(arguments)
    assert arguments[arguments.index("--installPath") + 1] == result["system_install"]


def test_usable_installed_msvc_is_probed_and_reused_without_installation(tmp_path: Path) -> None:
    result = _run_toolchain(
        tmp_path,
        r"""
$script:probes = @()
$installation = Join-Path $root 'registered existing Visual Studio'
function Get-PortableVisualStudioInstances([switch] $All) {
    [pscustomobject]@{ installationPath=$installation }
}
function Test-PortableMsvc([string] $Root, [string] $InstallationPath) {
    $script:probes += $InstallationPath
}
function Install-PortableMsvc { throw 'A usable existing toolchain must not be installed again' }
$selected = @(Enable-PortableMsvc $root)
Write-ToolchainResult @{ selected=$selected; probes=$script:probes; existing=$installation }
""",
    )
    assert result["selected"] == result["probes"] == [result["existing"]]


def test_microsoft_installer_uses_physical_paths_when_root_has_a_real_subst_alias(
    tmp_path: Path,
) -> None:
    root = tmp_path / "owned installation ü with spaces"
    cache = root / "cache/toolchain"
    cache.mkdir(parents=True)
    installer = cache / "vs_buildtools-17.exe"
    installer.write_bytes(b"signed bootstrap fixture")
    result = _run_toolchain(
        tmp_path,
        SIGNED_BOOTSTRAP
        + r"""
function Start-Process {
    param($FilePath, $ArgumentList, $WorkingDirectory, $WindowStyle, $Wait, $PassThru,
        $ErrorAction, $Verb)
    $script:launches += @{ executable=$FilePath; working_directory=$WorkingDirectory }
    if ($script:signaturePaths -notcontains $FilePath) { throw 'Physical payload was not verified' }
    $process = [pscustomobject]@{ ExitCode=0 }
    $process | Add-Member -MemberType ScriptMethod -Name Dispose -Value { }
    return $process
}
# Permit only the real DOS-drive helper; the native installer seam stays mocked.
$originalChecked = ${function:Invoke-Checked}
function Invoke-Checked([string] $Executable, [string[]] $Arguments) {
    $expected = Join-Path ([Environment]::GetFolderPath(
        [Environment+SpecialFolder]::System)) 'subst.exe'
    if ($Executable -ne $expected) { & $originalChecked $Executable $Arguments; return }
    & $Executable @Arguments
    if ($LASTEXITCODE -ne 0) { throw 'Fixture SUBST command failed' }
}
$mapping = New-PortableBuildDrive $root
try {
    Install-PortableMsvc $mapping.Path
    $observed = @{ alias=$mapping.Path; launches=$script:launches;
        signatures=$script:signaturePaths }
} finally { Remove-PortableBuildDrive $mapping | Out-Null }
Write-ToolchainResult $observed
""",
    )
    launches = result["launches"]
    assert isinstance(launches, list)
    assert len(launches) == 1
    assert Path(launches[0]["executable"]) == installer
    assert Path(launches[0]["working_directory"]) == cache
    assert result["signatures"] == [str(installer)]
    assert not str(launches[0]["executable"]).startswith(str(result["alias"]))


@pytest.mark.parametrize(
    ("failure", "message"),
    [("uac", "Administratorfreigabe wurde abgelehnt"), ("reboot", "Windows-Neustart")],
)
def test_msvc_declined_elevation_or_required_reboot_stops_before_adoption(
    tmp_path: Path, failure: str, message: str
) -> None:
    root = tmp_path / "owned installation ü with spaces"
    cache = root / "cache/toolchain"
    cache.mkdir(parents=True)
    (cache / "vs_buildtools-17.exe").write_bytes(b"signed bootstrap fixture")
    result = _run_toolchain(
        tmp_path,
        SIGNED_BOOTSTRAP
        + f"$failure = {_ps_string(failure)}\n"
        + r"""
$script:probeCalls = 0
function Test-PortableMsvc {
    $script:probeCalls++
    throw 'Probe must not follow failed installation'
}
function Start-Process {
    param($FilePath, $ArgumentList, $WorkingDirectory, $WindowStyle, $Wait, $PassThru,
        $ErrorAction, $Verb)
    $script:launches += $FilePath
    if ($failure -eq 'uac') {
        throw [InvalidOperationException]::new('wrapped UAC rejection',
            [ComponentModel.Win32Exception]::new(1223))
    }
    $process = [pscustomobject]@{ ExitCode=3010 }
    $process | Add-Member -MemberType ScriptMethod -Name Dispose -Value { }
    return $process
}
$caught = $null
$continued = $false
try { Enable-PortableMsvc $root | Out-Null; $continued = $true }
catch { $caught = $_.Exception.Message }
Write-ToolchainResult @{ error=$caught; continued=$continued; probes=$script:probeCalls;
    launches=$script:launches }
""",
    )
    assert message in str(result["error"])
    assert "nicht aktiviert" in str(result["error"])
    assert result["continued"] is False
    assert result["probes"] == 0
    assert isinstance(result["launches"], list)
    assert len(result["launches"]) == 1


def test_unsigned_build_tools_payload_is_rejected_before_execution(tmp_path: Path) -> None:
    root = tmp_path / "owned installation ü with spaces"
    cache = root / "cache/toolchain"
    cache.mkdir(parents=True)
    (cache / "vs_buildtools-17.exe").write_bytes(b"unsigned downloaded executable")
    result = _run_toolchain(
        tmp_path,
        r"""
$script:launches = 0
function Start-Process { $script:launches++; throw 'Unsigned payload must never execute' }
function Get-PortableVisualStudioInstances([switch] $All) { }
$caught = $null
try { Install-PortableMsvc $root }
catch { $caught = $_.Exception.Message }
Write-ToolchainResult @{ error=$caught; launches=$script:launches }
""",
    )
    assert "Keine gueltige Microsoft-Signatur" in str(result["error"])
    assert result["launches"] == 0


def test_private_rust_bootstrap_pins_toolchain_without_changing_developer_homes(
    tmp_path: Path,
) -> None:
    root = tmp_path / "owned installation ü with spaces"
    developer = root / "developer-cargo/config.toml"
    developer.parent.mkdir(parents=True)
    developer.write_bytes(b"Human cargo configuration")
    result = _run_toolchain(
        tmp_path,
        r"""
$env:CARGO_HOME = Join-Path $root 'developer-cargo'
$env:RUSTUP_HOME = Join-Path $root 'developer-rustup'
$env:RUSTUP_TOOLCHAIN = 'nightly'
$userPath = [Environment]::GetEnvironmentVariable('PATH', 'User')
$machinePath = [Environment]::GetEnvironmentVariable('PATH', 'Machine')
$script:downloads = @()
$script:commands = @()
$script:hashes = @()
function Get-PortableVerifiedTool([string] $Root, [string] $Name, [Uri] $Uri, [string] $Sha256) {
    $script:downloads += @{ name=$Name; uri=$Uri.AbsoluteUri; sha256=$Sha256 }
    return Join-Path $Root 'cache/rustup-init-fixture.exe'
}
function Assert-Hash([string] $Path, [string] $Expected) {
    $script:hashes += @{ path=$Path; sha256=$Expected }
}
function Invoke-Checked([string] $Executable, [string[]] $Arguments) {
    $script:commands += @{ executable=$Executable; arguments=$Arguments }
}
function Invoke-PortableCapturedProcess([string] $Executable, [string] $ArgumentLine) {
    if ($Executable -ne (Join-Path $env:CARGO_HOME 'bin/cargo.exe') -or
        $ArgumentLine -ne '--version') { throw 'Rust verification escaped the private cargo proxy' }
    return 'cargo 1.95.0 (fixture)'
}
$selected = @(Get-PortableRust $root)
Write-ToolchainResult @{ selected=$selected; cargo_home=$env:CARGO_HOME;
    rustup_home=$env:RUSTUP_HOME;
    toolchain=$env:RUSTUP_TOOLCHAIN; path=$env:PATH; downloads=$script:downloads;
    encoded_rustflags=$env:CARGO_ENCODED_RUSTFLAGS;
    commands=$script:commands; hashes=$script:hashes;
    user_path_unchanged=($userPath -eq [Environment]::GetEnvironmentVariable('PATH', 'User'));
    machine_path_unchanged=($machinePath -eq
        [Environment]::GetEnvironmentVariable('PATH', 'Machine')) }
""",
    )
    cargo_home = root / "tools/rust-1.95.0/cargo"
    rustup_home = root / "tools/rust-1.95.0/rustup"
    cargo = cargo_home / "bin/cargo.exe"
    assert result["selected"] == [str(cargo)]
    assert Path(str(result["cargo_home"])) == cargo_home
    assert Path(str(result["rustup_home"])) == rustup_home
    assert result["toolchain"] == "1.95.0-x86_64-pc-windows-msvc"
    assert str(result["encoded_rustflags"]).split(chr(31)) == [
        "--sysroot",
        str(rustup_home / "toolchains/1.95.0-x86_64-pc-windows-msvc"),
    ]
    assert Path(str(result["path"]).split(";", maxsplit=1)[0]) == cargo.parent
    assert result["user_path_unchanged"] is result["machine_path_unchanged"] is True
    assert result["downloads"] == [
        {
            "name": "rustup-init-1.29.1-windows-x64.exe",
            "uri": RUSTUP_URL,
            "sha256": RUSTUP_SHA256,
        }
    ]
    commands = result["commands"]
    assert isinstance(commands, list)
    bootstrap = commands[0]["arguments"]
    assert "--no-modify-path" in bootstrap
    assert bootstrap[bootstrap.index("--profile") + 1] == "minimal"
    assert bootstrap[bootstrap.index("--default-host") + 1] == "x86_64-pc-windows-msvc"
    assert bootstrap[bootstrap.index("--default-toolchain") + 1] == result["toolchain"]
    assert commands[1]["arguments"] == ["set", "auto-self-update", "disable"]
    assert commands[2]["arguments"] == [
        "toolchain",
        "install",
        result["toolchain"],
        "--profile",
        "minimal",
        "--no-self-update",
    ]
    hashes = result["hashes"]
    assert isinstance(hashes, list)
    assert {Path(entry["path"]) for entry in hashes} == {cargo, cargo.parent / "rustup.exe"}
    assert {entry["sha256"] for entry in hashes} == {RUSTUP_SHA256}
    assert developer.read_bytes() == b"Human cargo configuration"
