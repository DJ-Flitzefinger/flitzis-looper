param([Parameter(Mandatory = $true)][string] $Root, [switch] $Check, [string] $VersionDirectory)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
try {
    $Root = [IO.Path]::GetFullPath($Root)
    if (-not $VersionDirectory) {
        $pointer = Get-Content -LiteralPath (Join-Path $Root 'current.json') -Raw | ConvertFrom-Json
        if ($pointer.schema_version -ne 1) { throw 'Unbekanntes Zeigerformat.' }
        $VersionDirectory = $pointer.directory
    }
    if ($VersionDirectory -notmatch '^[a-zA-Z0-9._-]+$' -or $VersionDirectory -in @('.', '..')) {
        throw 'Ungueltiger Portable-Versionszeiger.'
    }
    $version = Join-Path (Join-Path $Root 'versions') $VersionDirectory
    foreach ($path in @($Root, (Join-Path $Root 'versions'), $version)) {
        if ((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw 'Portable-Pfade duerfen keine Verzeichnisverknuepfungen sein.'
        }
    }
    $env:PYTHONHOME = Join-Path $version 'python'
    $env:PYTHONPATH = ''
    $env:PYTHONNOUSERSITE = '1'
    $env:VIRTUAL_ENV = ''
    $env:VCPKG_ROOT = ''
    $env:RUBBERBAND_DLL_DIR = ''
    $env:RUBBERBAND_DLL_DIRS = ''
    $env:PATH = "$env:PYTHONHOME;$env:SystemRoot\System32;$env:SystemRoot"
    $arguments = @('-s', '-B', (Join-Path $version 'bootstrap.py'), '--root', $Root)
    if ($Check) { $arguments += '--check' }
    & (Join-Path $env:PYTHONHOME 'python.exe') @arguments
    exit $LASTEXITCODE
} catch {
    Write-Host "Portable-Start fehlgeschlagen: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
