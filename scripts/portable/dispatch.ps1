param([Parameter(Mandatory = $true)][string] $Root, [switch] $Check)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
try {
    $Root = [IO.Path]::GetFullPath($Root)
    $pointer = Get-Content -LiteralPath (Join-Path $Root 'current.json') -Raw | ConvertFrom-Json
    $directory = [string]$pointer.directory
    if ($pointer.schema_version -ne 1 -or $directory -notmatch '^[a-zA-Z0-9._-]+$' -or $directory -in @('.', '..')) {
        throw 'Ungueltiger Portable-Versionszeiger.'
    }
    $version = Join-Path (Join-Path $Root 'versions') $directory
    foreach ($path in @($Root, (Join-Path $Root 'versions'), $version, (Join-Path $version 'launch.ps1'))) {
        if ((Get-Item -LiteralPath $path -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw 'Portable-Pfade duerfen keine Verknuepfungen sein.'
        }
    }
    $arguments = @('-Root', $Root, '-VersionDirectory', $directory)
    if ($Check) { $arguments += '-Check' }
    & powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File (Join-Path $version 'launch.ps1') @arguments
    exit $LASTEXITCODE
} catch {
    Write-Host "Portable-Start fehlgeschlagen: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
}
