"""Prove temporary Windows build aliases preserve owned files and foreign drive mappings."""

import json
import os
import subprocess
from pathlib import Path
from typing import cast

import pytest

pytestmark = pytest.mark.skipif(os.name != "nt", reason="Temporary Windows DOS build drives")
REPO = Path(__file__).resolve().parents[2]
RESULT_PREFIX = "PORTABLE_BUILD_PATH_RESULT:"
ROOT_NAME = "owned build ü spaces,semicolon;backtick`"


def _ps_string(value: Path | str) -> str:
    return "'" + str(value).replace("'", "''") + "'"


def _run_build_path(tmp_path: Path, commands: str) -> dict[str, object]:
    root = tmp_path / ROOT_NAME
    replacement = tmp_path / "other owned fixture root"
    root.mkdir()
    replacement.mkdir()
    (root / "marker.txt").write_text("original fixture", encoding="utf-8")
    (replacement / "marker.txt").write_text("replacement fixture", encoding="utf-8")
    script = tmp_path / "build-path-regression.ps1"
    script.write_text(
        "$ErrorActionPreference = 'Stop'\n"
        "Set-StrictMode -Version Latest\n"
        "[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)\n"
        "if ($PSVersionTable.PSVersion.Major -ne 5) { throw 'Expected Windows PowerShell 5' }\n"
        f". {_ps_string(REPO / 'scripts/portable/common.ps1')}\n"
        f". {_ps_string(REPO / 'scripts/portable/build-path.ps1')}\n"
        f"$root = {_ps_string(root)}\n"
        f"$replacement = {_ps_string(replacement)}\n"
        r"""
$fixtureReplacements = @()
function Read-AliasMarker([string] $Path) {
    return [IO.File]::ReadAllText((Join-Path $Path 'marker.txt'))
}
function Write-BuildPathResult($Value) {
    Write-Output ('PORTABLE_BUILD_PATH_RESULT:' + ($Value | ConvertTo-Json -Depth 8 -Compress))
}
try {
"""
        + commands
        + r"""
} finally {
    # Preserve production cleanup guards even if a regression assertion fails.
    $owners = Get-PortableBuildDriveOwners
    foreach ($owner in @($owners.Values)) {
        Remove-PortableBuildDrive $owner.Mapping | Out-Null
    }
    # These replacements belong only to this fixture, and must still point at
    # the fixture's exact other root before this harness may remove them.
    foreach ($fixture in $fixtureReplacements) {
        if ([string]::Equals((Get-PortableBuildDriveTarget $fixture.Drive), $fixture.Root,
            [StringComparison]::OrdinalIgnoreCase)) {
            Invoke-PortableBuildSubst @($fixture.Drive, '/D')
        }
    }
}
""",
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
    assert (root / "marker.txt").read_text(encoding="utf-8") == "original fixture"
    assert (replacement / "marker.txt").read_text(encoding="utf-8") == "replacement fixture"
    # The controlled PowerShell fixture emits one JSON object with named fields.
    return cast("dict[str, object]", json.loads(records[0]))


def test_real_alias_handles_unicode_and_shell_punctuation_and_removes_only_own_mapping(
    tmp_path: Path,
) -> None:
    result = _run_build_path(
        tmp_path,
        r"""
$mapping = New-PortableBuildDrive $root
$marker = Read-AliasMarker $mapping.Path
$target = Get-PortableBuildDriveTarget $mapping.Drive
$physicalMarker = Resolve-PortablePhysicalPath (Join-Path $mapping.Path 'marker.txt')
$forged = [pscustomobject]@{ Drive=$mapping.Drive; Path=$mapping.Path;
    OriginalRoot=$mapping.OriginalRoot }
$forgedRemoved = Remove-PortableBuildDrive $forged
$stillUsable = Read-AliasMarker $mapping.Path
$removed = Remove-PortableBuildDrive $mapping
Write-BuildPathResult @{ drive=$mapping.Drive; path=$mapping.Path;
    original_root=$mapping.OriginalRoot; target=$target; marker=$marker;
    physical_marker=$physicalMarker;
    forged_removed=$forgedRemoved; retained_marker=$stillUsable; removed=$removed;
    target_after=(Get-PortableBuildDriveTarget $mapping.Drive) }
""",
    )
    assert result["drive"] in [f"{letter}:" for letter in "ZYXWVUTSR"]
    assert result["path"] == str(result["drive"]) + "\\"
    assert Path(str(result["original_root"])) == tmp_path / ROOT_NAME
    assert Path(str(result["target"])) == tmp_path / ROOT_NAME
    assert Path(str(result["physical_marker"])) == tmp_path / ROOT_NAME / "marker.txt"
    assert result["marker"] == result["retained_marker"] == "original fixture"
    assert result["forged_removed"] is False
    assert result["removed"] is True
    assert result["target_after"] is None


def test_an_in_use_drive_is_preserved_while_another_free_drive_is_selected(tmp_path: Path) -> None:
    result = _run_build_path(
        tmp_path,
        r"""
$first = New-PortableBuildDrive $root
$second = New-PortableBuildDrive $replacement
$firstMarker = Read-AliasMarker $first.Path
$secondMarker = Read-AliasMarker $second.Path
$secondRemoved = Remove-PortableBuildDrive $second
$retained = Get-PortableBuildDriveTarget $first.Drive
$firstRemoved = Remove-PortableBuildDrive $first
Write-BuildPathResult @{ first=$first.Drive; second=$second.Drive;
    first_marker=$firstMarker; second_marker=$secondMarker; retained=$retained;
    first_removed=$firstRemoved; second_removed=$secondRemoved }
""",
    )
    assert result["first"] != result["second"]
    assert result["first_marker"] == "original fixture"
    assert result["second_marker"] == "replacement fixture"
    assert Path(str(result["retained"])) == tmp_path / ROOT_NAME
    assert result["first_removed"] is result["second_removed"] is True


def test_occupied_logical_drives_are_rejected_without_overriding_any_mapping(
    tmp_path: Path,
) -> None:
    result = _run_build_path(
        tmp_path,
        r"""
$script:substCalls = 0
function Get-PortableBuildLogicalDrives {
    return @('Z:\', 'Y:\', 'X:\', 'W:\', 'V:\', 'U:\', 'T:\', 'S:\', 'R:\')
}
function Invoke-PortableBuildSubst {
    $script:substCalls++
    throw 'No occupied drive may be assigned'
}
$caught = $null
try { New-PortableBuildDrive $root | Out-Null }
catch { $caught = $_.Exception.Message }
Write-BuildPathResult @{ error=$caught; subst_calls=$script:substCalls }
""",
    )
    assert "No free temporary build drive letter" in str(result["error"])
    assert result["subst_calls"] == 0


def test_failed_build_finally_releases_its_alias_and_preserves_the_original_root(
    tmp_path: Path,
) -> None:
    result = _run_build_path(
        tmp_path,
        r"""
$mapping = New-PortableBuildDrive $root
$caught = $null
$removed = $false
try { throw 'simulated failed build' }
catch { $caught = $_.Exception.Message }
finally { $removed = Remove-PortableBuildDrive $mapping }
Write-BuildPathResult @{ error=$caught; removed=$removed;
    target_after=(Get-PortableBuildDriveTarget $mapping.Drive);
    root_marker=(Read-AliasMarker $root) }
""",
    )
    assert result["error"] == "simulated failed build"
    assert result["removed"] is True
    assert result["target_after"] is None
    assert result["root_marker"] == "original fixture"


def test_replaced_alias_is_left_intact_instead_of_removing_an_unrelated_mapping(
    tmp_path: Path,
) -> None:
    result = _run_build_path(
        tmp_path,
        r"""
$mapping = New-PortableBuildDrive $root
if (-not [string]::Equals((Get-PortableBuildDriveTarget $mapping.Drive), $mapping.OriginalRoot,
    [StringComparison]::OrdinalIgnoreCase)) { throw 'Fixture lost ownership before replacement' }
Invoke-PortableBuildSubst @($mapping.Drive, '/D')
Invoke-PortableBuildSubst @($mapping.Drive, $replacement)
$fixtureReplacements += @{ Drive=$mapping.Drive; Root=$replacement }
$removed = Remove-PortableBuildDrive $mapping
Write-BuildPathResult @{ removed=$removed;
    target_after=(Get-PortableBuildDriveTarget $mapping.Drive);
    marker_after=(Read-AliasMarker $mapping.Path);
    owners_remaining=(Get-PortableBuildDriveOwners).Count }
""",
    )
    assert result["removed"] is False
    assert Path(str(result["target_after"])) == tmp_path / "other owned fixture root"
    assert result["marker_after"] == "replacement fixture"
    assert result["owners_remaining"] == 0


def test_physical_path_resolution_expands_nested_subst_and_rejects_cycles(tmp_path: Path) -> None:
    result = _run_build_path(
        tmp_path,
        r"""
function Get-PortableBuildDriveTarget([string] $Drive) {
    switch ($Drive) {
        'Z:' { return 'Y:\nested owned folder' }
        'Y:' { return 'C:\physical owned folder' }
        'C:' { return '\Device\HarddiskVolumeFixture' }
        default { throw 'Unexpected queried drive' }
    }
}
$physical = Resolve-PortablePhysicalPath 'Z:\cache\installer.exe'
$script:queries = 0
function Get-PortableBuildDriveTarget([string] $Drive) {
    $script:queries++
    if ($Drive -eq 'Z:') { return 'Y:\' }
    if ($Drive -eq 'Y:') { return 'Z:\' }
    throw 'Unexpected queried drive'
}
$caught = $null
try { Resolve-PortablePhysicalPath 'Z:\cache\installer.exe' | Out-Null }
catch { $caught = $_.Exception.Message }
Write-BuildPathResult @{ physical=$physical; error=$caught; queries=$script:queries }
""",
    )
    assert result["physical"] == (
        r"C:\physical owned folder\nested owned folder\cache\installer.exe"
    )
    assert "Cyclic SUBST mapping" in str(result["error"])
    assert result["queries"] == 2


def test_drive_claimed_after_recheck_causes_retry_without_touching_the_foreign_mapping(
    tmp_path: Path,
) -> None:
    result = _run_build_path(
        tmp_path,
        r"""
# Stub only the final native accessibility check so this collision fixture needs
# no real DOS mappings. The allocation/retry/ownership logic is the loaded code.
$definition = ${function:New-PortableBuildDrive}.Ast.Extent.Text
$nativeProof = '[IO.Directory]::Exists($path)'
if (($definition -split [regex]::Escape($nativeProof)).Count -ne 2) {
    throw 'Expected exactly one native alias-accessibility check'
}
$definition = $definition.Replace($nativeProof, '(Test-FixtureAliasAvailable $path)')
Invoke-Expression $definition
$script:targets = @{}
$script:queries = @{}
$script:substCalls = @()
$script:queriesBeforeCollision = 0
function Test-FixtureAliasAvailable([string] $Path) {
    $drive = $Path.Substring(0, 2)
    return $script:targets.ContainsKey($drive) -and $script:targets[$drive] -eq $root
}
function Get-PortableBuildLogicalDrives {
    foreach ($drive in $script:targets.Keys) { Write-Output ($drive + '\') }
}
function Get-PortableBuildDriveTarget([string] $Drive) {
    if (-not $script:queries.ContainsKey($Drive)) { $script:queries[$Drive] = 0 }
    $script:queries[$Drive]++
    return $script:targets[$Drive]
}
function Invoke-PortableBuildSubst([string[]] $Arguments) {
    $drive = $Arguments[0]
    $target = $Arguments[1]
    $script:substCalls += @{ drive=$drive; target=$target }
    if ($target -eq '/D') { $script:targets.Remove($drive); return }
    if ($drive -eq 'Z:') {
        $script:queriesBeforeCollision = $script:queries[$drive]
        $script:targets[$drive] = $replacement
        throw 'Command failed (1): subst.exe; drive is already SUBSTed'
    }
    if ($drive -ne 'Y:') { throw 'Retry must select the next available fixture drive' }
    $script:targets[$drive] = $target
}
$mapping = New-PortableBuildDrive $root
$removed = Remove-PortableBuildDrive $mapping
Write-BuildPathResult @{ selected=$mapping.Drive; removed=$removed;
    queries_before_collision=$script:queriesBeforeCollision;
    foreign_after=(Get-PortableBuildDriveTarget 'Z:');
    own_after=(Get-PortableBuildDriveTarget 'Y:'); calls=$script:substCalls;
    owners_remaining=(Get-PortableBuildDriveOwners).Count }
""",
    )
    root = str(tmp_path / ROOT_NAME)
    assert result["selected"] == "Y:"
    assert result["queries_before_collision"] == 2
    assert result["removed"] is True
    assert Path(str(result["foreign_after"])) == tmp_path / "other owned fixture root"
    assert result["own_after"] is None
    assert result["owners_remaining"] == 0
    assert result["calls"] == [
        {"drive": "Z:", "target": root},
        {"drive": "Y:", "target": root},
        {"drive": "Y:", "target": "/D"},
    ]


def test_subst_failure_without_a_competing_mapping_is_reported_without_retry(
    tmp_path: Path,
) -> None:
    result = _run_build_path(
        tmp_path,
        r"""
$script:substCalls = 0
function Get-PortableBuildLogicalDrives { return @() }
function Get-PortableBuildDriveTarget { return $null }
function Invoke-PortableBuildSubst {
    $script:substCalls++
    throw 'fixture SUBST failure without a collision'
}
$caught = $null
try { New-PortableBuildDrive $root | Out-Null }
catch { $caught = $_.Exception.Message }
Write-BuildPathResult @{ error=$caught; subst_calls=$script:substCalls;
    owners_remaining=(Get-PortableBuildDriveOwners).Count }
""",
    )
    assert result["error"] == "fixture SUBST failure without a collision"
    assert result["subst_calls"] == 1
    assert result["owners_remaining"] == 0
