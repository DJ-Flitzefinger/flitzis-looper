"""Deterministic process-snapshot races in the real Portable guard functions.

Only Process query objects are substituted. No installer launch, Native result,
lock or path resolver is substituted. These predicate-level cases do not claim
real application startup acceptance. Both production function copies run.
"""

import os
import subprocess
from pathlib import Path

import pytest

pytestmark = pytest.mark.skipif(os.name != "nt", reason="Windows Portable process guard")
REPO = Path(__file__).resolve().parents[2]


def guard_functions(source: str) -> str:
    if source == "embedded":
        content = (REPO / "INSTALL-LOOPER-PORTABLE.bat").read_text(encoding="utf-8-sig")
        _, separator, content = content.partition(":__PORTABLE_PS__\n")
        assert separator
        content, separator, _ = content.partition("\ntry {\n")
        assert separator
        return content
    common = (REPO / "scripts/portable/common.ps1").read_text(encoding="utf-8-sig")
    build_path = (REPO / "scripts/portable/build-path.ps1").read_text(encoding="utf-8-sig")
    return common + "\n" + build_path


@pytest.mark.parametrize("source", ["embedded", "common"])
@pytest.mark.parametrize("path_query", ["throws", "null"])
@pytest.mark.parametrize("exit_query", ["exited", "alive", "throws", "null", "nonboolean"])
def test_only_same_original_positive_exit_can_resolve_unreadable_path(
    tmp_path: Path,
    source: str,
    path_query: str,
    exit_query: str,
) -> None:
    """A positive exit releases the race; alive and uncertain owners still block."""
    script = tmp_path / "same-original-process-query.ps1"
    script.write_text(
        "Set-StrictMode -Version Latest\n$ErrorActionPreference = 'Stop'\n"
        + guard_functions(source)
        + f"\n$pathQuery = '{path_query}'\n$exitQuery = '{exit_query}'\n"
        + r"""
$original = [pscustomobject]@{
    PathQueries=0; ExitQueries=0; Disposals=0; Terminations=0; Identity='original-object'
}
$original | Add-Member -MemberType ScriptProperty -Name Path -Value {
    $this.PathQueries++
    if ($script:pathQuery -eq 'throws') { throw 'explicit path query failure' }
    return $null
}
$original | Add-Member -MemberType ScriptProperty -Name HasExited -Value {
    $this.ExitQueries++
    switch ($script:exitQuery) {
        'exited' { return $true }
        'alive' { return $false }
        'throws' { throw 'explicit liveness query failure' }
        'null' { return $null }
        'nonboolean' { return 'True' }
        default { throw 'unknown liveness fixture' }
    }
}
$original | Add-Member -MemberType ScriptMethod -Name Dispose -Value { $this.Disposals++ }
$original | Add-Member -MemberType ScriptMethod -Name Kill -Value {
    $this.Terminations++
    throw 'installer must never terminate a process'
}
$script:enumerations = 0
function Get-Process {
    param($Name, $ErrorAction)
    $script:enumerations++
    if (@($Name).Count -ne 2 -or $Name[0] -ne 'python' -or $Name[1] -ne 'pythonw') {
        throw 'expected one complete Python/Pythonw snapshot'
    }
    if ($script:enumerations -ne 1) {
        throw 'liveness must use the original object, never another PID lookup'
    }
    return $script:original
}
$failure = $null
try { Assert-NoPortableProcess $PSScriptRoot }
catch { $failure = $_.Exception.Message }
if ($exitQuery -eq 'exited') {
    if ($failure) { throw "positive original exit did not resolve query race: $failure" }
} else {
    if (-not $failure -or $failure -notlike '*Python-Prozessbesitzer*') {
        throw "live/unqueryable owner did not fail closed: $failure"
    }
}
if ($original.PathQueries -ne 1 -or $original.ExitQueries -ne 1) {
    throw 'query order changed or a different owner supplied liveness'
}
if ($script:enumerations -ne 1 -or $original.Disposals -ne 1 -or $original.Terminations -ne 0) {
    throw 'process snapshot was leaked, requeried or terminated'
}
""",
        encoding="utf-8-sig",
        newline="\n",
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
        errors="replace",
        timeout=15,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr


@pytest.mark.parametrize("source", ["embedded", "common"])
@pytest.mark.parametrize("owned", [False, True])
def test_known_live_owner_keeps_root_guard_and_disposes_entire_snapshot(
    tmp_path: Path,
    source: str,
    *,
    owned: bool,
) -> None:
    """One known root-owned process blocks; later untouched objects still close."""
    script = tmp_path / "known-process-owner-query.ps1"
    script.write_text(
        "Set-StrictMode -Version Latest\n$ErrorActionPreference = 'Stop'\n"
        + guard_functions(source)
        + f"\n$owned = {'$true' if owned else '$false'}\n"
        + r"""
$root = Join-Path $PSScriptRoot 'owned-installation'
$outside = Join-Path $PSScriptRoot 'unrelated-python/python.exe'
$inside = Join-Path $root 'Runtime/python/python.exe'
$first = [pscustomobject]@{
    Executable=$(if ($owned) { $inside } else { $outside });
    PathQueries=0; ExitQueries=0; Disposals=0; Terminations=0
}
$second = [pscustomobject]@{
    Executable=$outside; PathQueries=0; ExitQueries=0; Disposals=0; Terminations=0
}
foreach ($process in @($first, $second)) {
    $process | Add-Member -MemberType ScriptProperty -Name Path -Value {
        $this.PathQueries++
        return $this.Executable
    }
    $process | Add-Member -MemberType ScriptProperty -Name HasExited -Value {
        $this.ExitQueries++
        return $false
    }
    $process | Add-Member -MemberType ScriptMethod -Name Dispose -Value { $this.Disposals++ }
    $process | Add-Member -MemberType ScriptMethod -Name Kill -Value {
        $this.Terminations++
        throw 'installer must never terminate a process'
    }
}
$script:enumerations = 0
function Get-Process {
    param($Name, $ErrorAction)
    $script:enumerations++
    return @($script:first, $script:second)
}
$failure = $null
try { Assert-NoPortableProcess $root }
catch { $failure = $_.Exception.Message }
if ($owned) {
    if (-not $failure -or $failure -notlike '*installierter Looper-Prozess*') {
        throw "known live root owner did not block: $failure"
    }
    if ($second.PathQueries -ne 0) { throw 'guard continued after rejecting original owner' }
} else {
    if ($failure) { throw "known unrelated Python owner incorrectly blocked: $failure" }
    if ($second.PathQueries -ne 1) { throw 'complete snapshot was not checked' }
}
if ($first.PathQueries -ne 1 -or $first.ExitQueries -ne 0 -or $second.ExitQueries -ne 0) {
    throw 'known executable path bypassed root guard through a liveness claim'
}
if ($script:enumerations -ne 1) { throw 'process owner was replaced by another enumeration' }
foreach ($process in @($first, $second)) {
    if ($process.Disposals -ne 1 -or $process.Terminations -ne 0) {
        throw 'original process snapshot handle was leaked or terminated'
    }
}
""",
        encoding="utf-8-sig",
        newline="\n",
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
        errors="replace",
        timeout=15,
        check=False,
    )
    assert result.returncode == 0, result.stdout + result.stderr
