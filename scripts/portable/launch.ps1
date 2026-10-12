param([Parameter(Mandatory = $true)][string] $Root, [switch] $Check, [string] $VersionDirectory)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$runtimeDrive = $null
$launchExit = 1
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
    $relativeVersion = Join-Path 'versions' $VersionDirectory
    $version = Join-Path $Root $relativeVersion
    foreach ($path in @($Root, (Join-Path $Root 'versions'), $version)) {
        if ((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw 'Portable-Pfade duerfen keine Verzeichnisverknuepfungen sein.'
        }
    }
    # Deep native-package paths need a new alias for this child process's entire
    # lifetime. Resolve possible installer aliases before selecting physical data
    # paths, so this launch never depends on the earlier build drive remaining.
    if ($version.Length -gt 140 -or $Root -match '^[R-Z]:\\') {
        $common = Join-Path $version 'common.ps1'
        $buildPath = Join-Path $version 'build-path.ps1'
        foreach ($helper in @($common, $buildPath)) {
            $item = Get-Item -LiteralPath $helper -Force
            if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                throw 'Portable-Build-Pfadhelfer fehlen oder sind ungueltig.'
            }
        }
        . $common
        . $buildPath
        $Root = Resolve-PortablePhysicalPath $Root
        $version = Join-Path $Root $relativeVersion
        foreach ($path in @($Root, (Join-Path $Root 'versions'), $version)) { Assert-PlainPath $path }
        if ($version.Length -gt 140) {
            $runtimeDrive = New-PortableBuildDrive $Root
            $version = Join-Path $runtimeDrive.Path $relativeVersion
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
    $launchExit = $LASTEXITCODE
} catch {
    Write-Host "Portable-Start fehlgeschlagen: $($_.Exception.Message)" -ForegroundColor Red
    $launchExit = 1
} finally {
    if ($runtimeDrive) {
        try { Remove-PortableBuildDrive $runtimeDrive | Out-Null }
        catch { Write-Warning "Temporaerer Portable-Pfad konnte nicht entfernt werden: $($_.Exception.Message)" }
    }
}
exit $launchExit
