param([Parameter(Mandatory = $true)][string] $Root, [switch] $Check)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$lock = $null
$result = 1
function Assert-PortableStartPath([string] $Path) {
    $current = [IO.Path]::GetFullPath($Path)
    while ($current) {
        if ((Test-Path -LiteralPath $current) -and ((Get-Item -LiteralPath $current -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'Portable-Pfade duerfen keine Verknuepfungen sein.'
        }
        $parent = [IO.Path]::GetDirectoryName($current)
        if ($parent -eq $current) { break }
        $current = $parent
    }
}
try {
    $Root = [IO.Path]::GetFullPath($Root).TrimEnd('\')
    foreach ($path in @($Root,(Join-Path $Root '.install.lock'),(Join-Path $Root 'current.json'))) {
        Assert-PortableStartPath $path
    }
    try { $lock = [IO.File]::Open((Join-Path $Root '.install.lock'), [IO.FileMode]::Open,
        [IO.FileAccess]::ReadWrite, [IO.FileShare]::None) }
    catch { throw 'Der Looper oder ein Installer laeuft bereits. Bitte schliessen und erneut starten.' }
    if (Test-Path -LiteralPath (Join-Path $Root 'portable-transaction.json')) {
        throw 'Eine Installation wurde unterbrochen. Bitte zuerst INSTALL-LOOPER-PORTABLE.bat erneut ausfuehren.'
    }
    $pointer = Get-Content -LiteralPath (Join-Path $Root 'current.json') -Raw | ConvertFrom-Json
    if ($pointer.git_commit -notmatch '^[a-f0-9]{40}$') { throw 'Ungueltiger Portable-Quellstand.' }
    if ($pointer.schema_version -eq 1) {
        $directory = [string]$pointer.directory
        if ($directory -notmatch '^main-[a-f0-9]{12}-[a-f0-9]{8}$') { throw 'Ungueltiger alter Versionszeiger.' }
        $runtime = Join-Path (Join-Path $Root 'versions') $directory
        $arguments = @('-Root',$Root,'-VersionDirectory',$directory)
    } elseif ($pointer.schema_version -eq 2 -and $pointer.source_kind -eq 'main') {
        $runtime = Join-Path $Root 'Runtime'
        $arguments = @('-Root',$Root,'-StartLockHeld')
    } else { throw 'Unbekanntes Portable-Installationsformat.' }
    foreach ($path in @($runtime,(Join-Path $runtime 'launch.ps1'))) {
        Assert-PortableStartPath $path
    }
    if ($Check) { $arguments += '-Check' }
    & powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File (Join-Path $runtime 'launch.ps1') @arguments
    $result = $LASTEXITCODE
} catch {
    Write-Host "Portable-Start fehlgeschlagen: $($_.Exception.Message)" -ForegroundColor Red
} finally { if ($lock) { $lock.Dispose() } }
exit $result
