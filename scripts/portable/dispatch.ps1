param([Parameter(Mandatory = $true)][string] $Root, [switch] $Check)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$lock = $null
$result = 1
function Assert-PortableStartPath([string] $Path) {
    $current = [IO.Path]::GetFullPath($Path)
    while ($current) {
        if ((Test-Path -LiteralPath $current) -and ((Get-Item -LiteralPath $current -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'Portable paths must not be links.'
        }
        $parent = [IO.Path]::GetDirectoryName($current)
        if ($parent -eq $current) { break }
        $current = $parent
    }
}
try {
    $Root = [IO.Path]::GetFullPath($Root).TrimEnd('\')
    $markerPath = Join-Path $Root 'portable-root.json'
    foreach ($path in @($Root,$markerPath,(Join-Path $Root '.install.lock'),(Join-Path $Root 'current.json'))) {
        Assert-PortableStartPath $path
    }
    try { $lock = [IO.File]::Open((Join-Path $Root '.install.lock'), [IO.FileMode]::Open,
        [IO.FileAccess]::ReadWrite, [IO.FileShare]::None) }
    catch { throw 'The Looper or an installer is already running. Please close it and start again.' }
    if (Test-Path -LiteralPath (Join-Path $Root 'portable-transaction.json')) {
        throw 'An installation was interrupted. Please run INSTALL-LOOPER-PORTABLE.bat again first.'
    }
    $pointer = Get-Content -LiteralPath (Join-Path $Root 'current.json') -Raw | ConvertFrom-Json
    if ($pointer.git_commit -notmatch '^[a-f0-9]{40}$') { throw 'Invalid portable source revision.' }
    $candidate = $false
    if (Test-Path -LiteralPath $markerPath) {
        $marker = Get-Content -LiteralPath $markerPath -Raw | ConvertFrom-Json
        $candidate = $marker.app -eq 'flitzis-looper-candidate'
    }
    $candidatePointer = $pointer.schema_version -eq 2 -and $pointer.source_kind -eq 'candidate'
    if ($candidate -ne $candidatePointer -or ($candidate -and $marker.schema_version -ne 1)) {
        throw 'Portable candidate owner does not match its source pointer.'
    }
    if ($pointer.schema_version -eq 1) {
        $directory = [string]$pointer.directory
        if ($directory -notmatch '^main-[a-f0-9]{12}-[a-f0-9]{8}$') { throw 'Invalid legacy version pointer.' }
        $runtime = Join-Path (Join-Path $Root 'versions') $directory
        $arguments = @('-Root',$Root,'-VersionDirectory',$directory)
    } elseif ($candidate) {
        if (-not $Check) { throw 'This candidate provides a build preflight. Run START-LOOPER.bat --check.' }
        $runtime = Join-Path $Root 'Runtime'
        $arguments = @('-Root',$Root,'-InstallerOwned','-BuildRuntimeCheck')
    } elseif ($pointer.schema_version -eq 2 -and $pointer.source_kind -eq 'main') {
        $runtime = Join-Path $Root 'Runtime'
        $arguments = @('-Root',$Root,'-StartLockHeld')
    } else { throw 'Unknown portable installation format.' }
    foreach ($path in @($runtime,(Join-Path $runtime 'launch.ps1'))) {
        Assert-PortableStartPath $path
    }
    if ($Check) { $arguments += '-Check' }
    & powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File (Join-Path $runtime 'launch.ps1') @arguments
    $result = $LASTEXITCODE
} catch {
    Write-Host "Portable launch failed: $($_.Exception.Message)" -ForegroundColor Red
} finally { if ($lock) { $lock.Dispose() } }
exit $result
