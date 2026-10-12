param(
    [Parameter(Mandatory = $true)][string] $Root,
    [switch] $Check, [string] $CheckoutDirectory, [string] $RuntimeDirectory,
    [switch] $InstallerOwned, [switch] $StartLockHeld,
    [ValidateSet('repair','check')][string] $ModelMode,
    [ValidateSet('capture','move','recover','rollback')][string] $RelocationAction,
    [string] $RelocationSource, [string] $RelocationTarget, [string] $RelocationRecord
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$runtimeDrive = $null
$startLock = $null
$launchExit = 1
try {
    $Root = [IO.Path]::GetFullPath($Root).TrimEnd('\')
    if ($InstallerOwned -and -not ($Check -or $ModelMode -or $RelocationAction)) {
        throw 'Installer-Pruefmodus darf keine Anwendung starten.'
    }
    if (-not $InstallerOwned -and -not $StartLockHeld) {
        try { $startLock = [IO.File]::Open((Join-Path $Root '.install.lock'), [IO.FileMode]::Open,
            [IO.FileAccess]::ReadWrite, [IO.FileShare]::None) }
        catch { throw 'Der Looper oder ein Installer laeuft bereits. Bitte schliessen und erneut starten.' }
    }
    if (-not $InstallerOwned -and (Test-Path -LiteralPath (Join-Path $Root 'portable-transaction.json'))) {
        throw 'Eine Installation wurde unterbrochen. Bitte zuerst INSTALL-LOOPER-PORTABLE.bat erneut ausfuehren.'
    }
    if (-not $CheckoutDirectory) { $CheckoutDirectory = Join-Path $Root 'flitzis-looper' }
    if (-not $RuntimeDirectory) { $RuntimeDirectory = Join-Path $Root 'Runtime' }
    . (Join-Path $RuntimeDirectory 'common.ps1')
    . (Join-Path $RuntimeDirectory 'build-path.ps1')
    $Root = Resolve-PortablePhysicalPath $Root
    $checkout = Resolve-PortablePhysicalPath $CheckoutDirectory
    $runtime = Resolve-PortablePhysicalPath $RuntimeDirectory
    $prefix = $Root.TrimEnd('\') + '\'
    foreach ($path in @($Root,$checkout,$runtime)) { Assert-PlainPath $path }
    foreach ($path in @($checkout,$runtime)) {
        if (-not $path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'Portable-Pfad verlaesst die Installation.' }
    }
    if (-not $InstallerOwned -and ($checkout -ne (Join-Path $Root 'flitzis-looper') -or $runtime -ne (Join-Path $Root 'Runtime'))) {
        throw 'Anwendungsstart muss den festen Checkout und die private Runtime verwenden.'
    }
    if ([Math]::Max($checkout.Length,$runtime.Length) -gt 140) {
        $runtimeDrive = New-PortableBuildDrive $Root
        $checkout = $runtimeDrive.Path + $checkout.Substring($prefix.Length)
        $runtime = $runtimeDrive.Path + $runtime.Substring($prefix.Length)
    }
    $env:PYTHONHOME = Join-Path $runtime 'python'
    $env:PYTHONPATH = ''
    $env:PYTHONNOUSERSITE = '1'
    $env:PYTHONDONTWRITEBYTECODE = '1'
    foreach ($name in @('VIRTUAL_ENV','VCPKG_ROOT','RUBBERBAND_DLL_DIR','RUBBERBAND_DLL_DIRS')) { Set-Item -LiteralPath ('Env:' + $name) -Value '' }
    $env:PATH = "$env:PYTHONHOME;$env:SystemRoot\System32;$env:SystemRoot"
    if ($ModelMode) {
        $arguments = @('-s','-B',(Join-Path $checkout 'scripts/portable/prepare_models.py'),
            '--root',$Root,'--checkout',$checkout,'--runtime',$runtime,'--mode',$ModelMode)
    } elseif ($RelocationAction) {
        $arguments = @('-s','-B',(Join-Path $checkout 'scripts/portable/relocate_data.py'),
            '--root',$Root,'--source',$RelocationSource,'--target',$RelocationTarget,
            '--record',$RelocationRecord,'--action',$RelocationAction)
    } else {
        $arguments = @('-s','-B',(Join-Path $runtime 'bootstrap.py'),'--root',$Root,
            '--checkout',$checkout,'--runtime',$runtime)
        if ($Check) { $arguments += '--check-runtime' }
    }
    & (Join-Path $env:PYTHONHOME 'python.exe') @arguments
    $launchExit = $LASTEXITCODE
} catch {
    Write-Host "Portable-Start fehlgeschlagen: $($_.Exception.Message)" -ForegroundColor Red
} finally {
    if ($runtimeDrive) {
        try { Remove-PortableBuildDrive $runtimeDrive | Out-Null }
        catch { Write-Warning "Temporaerer Portable-Pfad konnte nicht entfernt werden: $($_.Exception.Message)" }
    }
    if ($startLock) { $startLock.Dispose() }
}
exit $launchExit
