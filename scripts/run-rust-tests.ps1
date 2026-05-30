param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]] $CargoArgs
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Split-PathList {
    param([string] $Value)

    if ([string]::IsNullOrWhiteSpace($Value)) {
        return @()
    }

    return $Value -split [regex]::Escape([IO.Path]::PathSeparator) |
        Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
}

function Add-CandidateDirectory {
    param(
        [System.Collections.Generic.List[string]] $Candidates,
        [string] $Path
    )

    if ([string]::IsNullOrWhiteSpace($Path)) {
        return
    }

    $Candidates.Add($Path)
}

function Get-UvPythonRuntimeDirectories {
    $PythonRuntimeProbe = @'
import os
import sys
import sysconfig

paths = [
    sys.base_prefix,
    os.path.dirname(sys.executable),
    sysconfig.get_config_var('BINDIR') or '',
]
print(os.pathsep.join(dict.fromkeys(path for path in paths if path)))
'@

    $ProbeOutput = & uv run python -c $PythonRuntimeProbe
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to discover uv's Python runtime directory."
    }

    return Split-PathList ($ProbeOutput -join [IO.Path]::PathSeparator)
}

$ScriptDir = Split-Path -Parent $PSCommandPath
$RepoRoot = (Resolve-Path (Join-Path $ScriptDir "..")).Path
$Triplet = if ($env:RUBBERBAND_VCPKG_TRIPLET) {
    $env:RUBBERBAND_VCPKG_TRIPLET
} elseif ($env:VCPKG_DEFAULT_TRIPLET) {
    $env:VCPKG_DEFAULT_TRIPLET
} else {
    "x64-windows"
}

$Candidates = [System.Collections.Generic.List[string]]::new()

foreach ($PathEntry in Get-UvPythonRuntimeDirectories) {
    Add-CandidateDirectory $Candidates $PathEntry
}

foreach ($PathEntry in Split-PathList $env:RUBBERBAND_DLL_DIRS) {
    Add-CandidateDirectory $Candidates $PathEntry
}

foreach ($PathEntry in Split-PathList $env:RUBBERBAND_DLL_DIR) {
    Add-CandidateDirectory $Candidates $PathEntry
}

if ($env:RUBBERBAND_LIB_DIR) {
    Add-CandidateDirectory $Candidates (Join-Path (Split-Path -Parent $env:RUBBERBAND_LIB_DIR) "bin")
}

if ($env:VCPKG_ROOT) {
    Add-CandidateDirectory $Candidates (Join-Path $env:VCPKG_ROOT "installed\$Triplet\bin")
}

if ($env:LOCALAPPDATA) {
    Add-CandidateDirectory $Candidates (Join-Path $env:LOCALAPPDATA "vcpkg\installed\$Triplet\bin")
}

foreach ($PathEntry in Split-PathList $env:PATH) {
    if (Test-Path -LiteralPath (Join-Path $PathEntry "rubberband-3.dll") -PathType Leaf) {
        Add-CandidateDirectory $Candidates $PathEntry
    }
}

$Seen = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
$RuntimeDirs = [System.Collections.Generic.List[string]]::new()

foreach ($Candidate in $Candidates) {
    if (-not (Test-Path -LiteralPath $Candidate -PathType Container)) {
        continue
    }

    $Resolved = (Resolve-Path -LiteralPath $Candidate).Path
    if (-not $Seen.Add($Resolved)) {
        continue
    }

    $RuntimeDirs.Add($Resolved)
}

if ($RuntimeDirs.Count -gt 0) {
    $env:PATH = (@($RuntimeDirs.ToArray()) + @($env:PATH)) -join [IO.Path]::PathSeparator
}

$ManifestPath = Join-Path $RepoRoot "rust\Cargo.toml"
& uv run cargo test --manifest-path $ManifestPath @CargoArgs
exit $LASTEXITCODE
