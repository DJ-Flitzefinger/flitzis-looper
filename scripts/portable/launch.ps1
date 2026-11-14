param(
    [Parameter(Mandatory = $true)][string] $Root,
    [switch] $Check, [switch] $BuildRuntimeCheck, [string] $CheckoutDirectory, [string] $RuntimeDirectory,
    [switch] $InstallerOwned, [switch] $StartLockHeld,
    [ValidateSet('repair','check')][string] $ModelMode,
    [ValidateSet('repair','check')][string] $AnalysisMode,
    [string] $AnalysisReport,
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
    if ($BuildRuntimeCheck -and -not ($InstallerOwned -and $Check)) {
        throw 'Build-only runtime checks require the installer check interface.'
    }
    if ($InstallerOwned -and -not ($Check -or $ModelMode -or $AnalysisMode -or $RelocationAction)) {
        throw 'Installer check mode must not start an application.'
    }
    if (-not $InstallerOwned -and -not $StartLockHeld) {
        try { $startLock = [IO.File]::Open((Join-Path $Root '.install.lock'), [IO.FileMode]::Open,
            [IO.FileAccess]::ReadWrite, [IO.FileShare]::None) }
        catch { throw 'The Looper or an installer is already running. Please close it and start again.' }
    }
    if (-not $InstallerOwned -and (Test-Path -LiteralPath (Join-Path $Root 'portable-transaction.json'))) {
        throw 'An installation was interrupted. Please run INSTALL-LOOPER-PORTABLE.bat again first.'
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
        if (-not $path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'Portable path escapes the installation.' }
    }
    if (-not $InstallerOwned -and ($checkout -ne (Join-Path $Root 'flitzis-looper') -or $runtime -ne (Join-Path $Root 'Runtime'))) {
        throw 'Application launch must use the fixed checkout and private runtime.'
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
    if ($AnalysisMode) {
        if (-not $InstallerOwned) { throw 'Worker acquisition is an explicit installer operation.' }
        $analysisRoot = if ($runtimeDrive) { $runtimeDrive.Path } else { $Root }
        $helper = Join-Path $checkout 'scripts/portable/prepare_analysis.py'
        $mode = if ($AnalysisMode -eq 'repair') { 'stage' } else { 'check' }
        $arguments = @('-s','-B',$helper,'--root',$analysisRoot,'--checkout',$checkout,
            '--runtime',$runtime,'--mode',$mode)
        if ($AnalysisReport) { $arguments += @('--report-json',$AnalysisReport) }
        & (Join-Path $env:PYTHONHOME 'python.exe') @arguments
        $launchExit = $LASTEXITCODE
    } elseif ($ModelMode) {
        if ($ModelMode -eq 'repair') {
            $metadataArguments = @('-s','-B',(Join-Path $checkout 'scripts/portable/prepare_models.py'),
                '--root',$Root,'--checkout',$checkout,'--runtime',$runtime,'--mode','check','--asset-metadata')
            $metadataOutput = & (Join-Path $env:PYTHONHOME 'python.exe') @metadataArguments
            if ($LASTEXITCODE -ne 0) { throw 'Pinned Portable model metadata is unavailable.' }
            $modelMetadata = ($metadataOutput -join [Environment]::NewLine) | ConvertFrom-Json
            if ($modelMetadata.portable_models -ne 'passed' -or -not $modelMetadata.assets) { throw 'Invalid Portable model metadata.' }
            $modelRoot = if ($runtimeDrive) { $runtimeDrive.Path } else { $Root }
            foreach ($asset in $modelMetadata.assets) {
                if ($asset.relative_path -notmatch '^models/' -or
                    $asset.relative_path -match '(^|[/\\])\.\.([/\\]|$)|:|^[/\\]' -or
                    $asset.sha256 -notmatch '^[a-f0-9]{64}$' -or $asset.bytes -le 0) {
                    throw 'A model target or pin leaves the owned Portable contract.'
                }
                $destination = Join-Path $modelRoot $asset.relative_path
                Assert-PlainPath $destination
                [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($destination)) | Out-Null
                Read-Payload ([Uri]$asset.url) $destination $asset.sha256 $asset.bytes
            }
        }
        $arguments = @('-s','-B',(Join-Path $checkout 'scripts/portable/prepare_models.py'),
            '--root',$Root,'--checkout',$checkout,'--runtime',$runtime,'--mode',$ModelMode)
    } elseif ($RelocationAction) {
        $arguments = @('-s','-B',(Join-Path $checkout 'scripts/portable/relocate_data.py'),
            '--root',$Root,'--source',$RelocationSource,'--target',$RelocationTarget,
            '--record',$RelocationRecord,'--action',$RelocationAction)
    } else {
        $arguments = @('-s','-B',(Join-Path $runtime 'bootstrap.py'),'--root',$Root,
            '--checkout',$checkout,'--runtime',$runtime)
        if ($BuildRuntimeCheck) {
            $beatWorker = Join-Path $runtime 'runtime/beat-this/native-beat-this-worker.exe'
            Assert-PlainPath $beatWorker
            if (-not (Test-Path -LiteralPath $beatWorker -PathType Leaf)) {
                throw 'The private Runtime is missing its Release BeatThis worker executable.'
            }
            $arguments += '--check-build-runtime'
        }
        elseif ($Check) { $arguments += '--check-runtime' }
    }
    if (-not $AnalysisMode) {
        & (Join-Path $env:PYTHONHOME 'python.exe') @arguments
        $launchExit = $LASTEXITCODE
    }
} catch {
    Write-Host "Portable launch failed: $($_.Exception.Message)" -ForegroundColor Red
} finally {
    if ($runtimeDrive) {
        try { Remove-PortableBuildDrive $runtimeDrive | Out-Null }
        catch { Write-Warning "Temporary portable path could not be removed: $($_.Exception.Message)" }
    }
    if ($startLock) { $startLock.Dispose() }
}
exit $launchExit
