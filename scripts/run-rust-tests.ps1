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

function Get-UvPythonEnvironment {
    $PythonRuntimeProbe = @'
import json
import os
import sys
import sysconfig

runtime_dirs = [
    sys.base_prefix,
    os.path.dirname(sys.executable),
    sysconfig.get_config_var('BINDIR') or '',
]
module_dirs = [sysconfig.get_path('purelib'), sysconfig.get_path('platlib')]
print(json.dumps({
    'runtime_dirs': list(dict.fromkeys(path for path in runtime_dirs if path)),
    'module_dirs': list(dict.fromkeys(path for path in module_dirs if path)),
}))
'@

    $ProbeOutput = & uv run python -c $PythonRuntimeProbe
    if ($LASTEXITCODE -ne 0) {
        throw "Failed to discover uv's Python runtime and module directories."
    }

    return ($ProbeOutput -join "`n" | ConvertFrom-Json)
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
$PythonEnvironment = Get-UvPythonEnvironment

foreach ($PathEntry in $PythonEnvironment.runtime_dirs) {
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

$ManifestPath = Join-Path $RepoRoot "rust\Cargo.toml"
$OriginalRuntimePath = $env:PATH
$OriginalPythonPath = $env:PYTHONPATH
try {
    if ($RuntimeDirs.Count -gt 0) {
        $env:PATH = (@($RuntimeDirs.ToArray()) + @($OriginalRuntimePath)) -join [IO.Path]::PathSeparator
    }
    # Standalone PyO3 tests embed the base interpreter, which does not discover
    # uv's virtualenv site-packages from the cargo executable's location.
    $env:PYTHONPATH = (@($PythonEnvironment.module_dirs) + @(Split-PathList $OriginalPythonPath)) -join [IO.Path]::PathSeparator
    & uv run cargo test --manifest-path $ManifestPath --workspace @CargoArgs
    $TestExitCode = $LASTEXITCODE
} finally {
    $env:PATH = $OriginalRuntimePath
    $env:PYTHONPATH = $OriginalPythonPath
}
exit $TestExitCode
