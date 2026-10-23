@echo off
setlocal EnableExtensions DisableDelayedExpansion
set "LOOPER_PORTABLE_FILE=%~f0"
set "LOOPER_PORTABLE_TARGET=%~1"
set "LOOPER_PORTABLE_NO_PAUSE=%~2"
if /i "%~1"=="--no-pause" (
    set "LOOPER_PORTABLE_TARGET="
    set "LOOPER_PORTABLE_NO_PAUSE=--no-pause"
)
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -Command "$s=[IO.File]::ReadAllText($env:LOOPER_PORTABLE_FILE); & ([scriptblock]::Create(($s -split '(?m)^:__PORTABLE_PS__\r?\n',2)[1]))"
set "LOOPER_PORTABLE_EXIT=%errorlevel%"
if /i not "%LOOPER_PORTABLE_NO_PAUSE%"=="--no-pause" pause
exit /b %LOOPER_PORTABLE_EXIT%
:__PORTABLE_PS__
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$lock = $null
$portableDrive = $null
$transcript = $false
$record = $null

function Write-AtomicJson([string] $Path, $Value) {
    $temporary = "$Path.$([Guid]::NewGuid().ToString('N')).tmp"
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($Value | ConvertTo-Json -Depth 12))
    $stream = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true) } finally { $stream.Dispose() }
    if (Test-Path -LiteralPath $Path) {
        [IO.File]::Replace($temporary, $Path, [NullString]::Value)
    } else { [IO.File]::Move($temporary, $Path) }
}

function Assert-PlainPath([string] $Path) {
    $current = [IO.Path]::GetFullPath($Path)
    while ($current) {
        if (Test-Path -LiteralPath $current) {
            $item = Get-Item -LiteralPath $current -Force
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw "Link in installation path: $current"
            }
        }
        $parent = [IO.Path]::GetDirectoryName($current)
        if ($parent -eq $current) { break }
        $current = $parent
    }
}

function Read-Payload([Uri] $Uri, [string] $Destination) {
    if ($Uri.IsFile) {
        Copy-Item -LiteralPath $Uri.LocalPath -Destination $Destination
    } elseif ($Uri.Scheme -eq 'https') {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        Invoke-WebRequest -Uri $Uri.AbsoluteUri -UseBasicParsing -OutFile $Destination -TimeoutSec 1800
    } else { throw 'Only local files or HTTPS downloads are supported.' }
}

function Get-Sha256([string] $Path) {
    $algorithm = [Security.Cryptography.SHA256]::Create()
    $stream = [IO.File]::OpenRead($Path)
    try { return [BitConverter]::ToString($algorithm.ComputeHash($stream)).Replace('-', '').ToLowerInvariant() }
    finally { $stream.Dispose(); $algorithm.Dispose() }
}

function Assert-Hash([string] $Path, [string] $Expected) {
    $actual = Get-Sha256 $Path
    if ($Expected -notmatch '^[a-f0-9]{64}$' -or $actual -ne $Expected) {
        throw "SHA256 verification failed: $Path"
    }
}

function Expand-ContainedZip([string] $Archive, [string] $Destination) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($Archive)
    try {
        $prefix = [IO.Path]::GetFullPath($Destination).TrimEnd('\') + '\'
        $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
        foreach ($entry in $zip.Entries) {
            $name = $entry.FullName.Replace('/', '\')
            if ($name.StartsWith('\') -or $name.Contains(':') -or (($name -split '\\') -contains '..')) {
                throw "Unsafe archive path: $name"
            }
            foreach ($part in ($name.TrimEnd('\') -split '\\')) {
                if (-not $part -or $part -eq '.' -or $part.EndsWith('.') -or $part.EndsWith(' ') -or $part -match '^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(\.|$)') {
                    throw "Ambiguous Windows archive path: $name"
                }
            }
            $path = [IO.Path]::GetFullPath((Join-Path $Destination $name))
            if (-not $path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) -or -not $seen.Add($path)) {
                throw "Archive path escapes the destination or is duplicated: $name"
            }
            $unixType = ($entry.ExternalAttributes -shr 16) -band 61440
            if ($unixType -eq 40960 -or ($entry.ExternalAttributes -band 1024)) {
                throw "Archive contains a link: $name"
            }
        }
        foreach ($entry in $zip.Entries) {
            $path = Join-Path $Destination $entry.FullName.Replace('/', '\')
            if ($entry.FullName.EndsWith('/')) {
                [IO.Directory]::CreateDirectory($path) | Out-Null
            } else {
                [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($path)) | Out-Null
                [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $path, $false)
            }
        }
    } finally { $zip.Dispose() }
}

function Invoke-Checked([string] $Executable, [string[]] $Arguments) {
    & $Executable @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Command failed ($LASTEXITCODE): $Executable" }
}

function Assert-PortableTree([string] $Path) {
    Assert-PlainPath $Path
    if (-not (Test-Path -LiteralPath $Path)) { return }
    $pending = [Collections.Generic.Stack[string]]::new()
    $pending.Push($Path)
    while ($pending.Count) {
        foreach ($item in @(Get-ChildItem -LiteralPath $pending.Pop() -Force)) {
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Link in portable files: $($item.FullName)" }
            if ($item.PSIsContainer) { $pending.Push($item.FullName) }
        }
    }
}

function Open-PortableLock([string] $Root) {
    $path = Join-Path $Root '.install.lock'
    Assert-PlainPath $path
    try { return [IO.File]::Open($path, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None) }
    catch { throw 'The Looper or another installer is running. Please close the Looper and run the installer again; no process will be terminated.' }
}

function Test-PortableProcessExited($Process) {
    try {
        # Query this original Process object, never a new owner by recycled PID.
        $exited = $Process.HasExited
        return ($exited -is [bool] -and $exited)
    } catch { return $false }
}

function Assert-NoPortableProcess([string] $Root) {
    $prefix = [IO.Path]::GetFullPath($Root).TrimEnd('\') + '\'
    $processes = @(Get-Process -Name python,pythonw -ErrorAction SilentlyContinue)
    try {
        foreach ($process in $processes) {
            try { $executable = $process.Path }
            catch {
                if (Test-PortableProcessExited $process) { continue }
                throw 'A Python process owner cannot be verified. Please close the Looper and run the installer again.'
            }
            if (-not $executable) {
                if (Test-PortableProcessExited $process) { continue }
                throw 'A Python process owner cannot be verified. Please close the Looper and run the installer again.'
            }
            if ((Resolve-PortablePhysicalPath $executable).StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
                throw 'An installed Looper process is still running. Please close the Looper and run the installer again; no process will be terminated.'
            }
        }
    } finally {
        foreach ($process in $processes) { $process.Dispose() }
    }
}

function Get-PortablePaths([string] $Root, [string] $Operation) {
    if ($Operation -notmatch '^[a-f0-9]{32}$') { throw 'Invalid portable transaction identity.' }
    $short = $Operation.Substring(0,8)
    return @{
        checkout = Join-Path $Root 'flitzis-looper'; runtime = Join-Path $Root 'Runtime'
        source = Join-Path $Root "cache/s-$short"; candidate = Join-Path $Root "cache/r-$short"
        old_checkout = Join-Path $Root "cache/o-$short"; old_runtime = Join-Path $Root "cache/p-$short"
        relocation = Join-Path $Root "cache/relocation-$Operation.json"
        build_owned = Join-Path $Root "cache/build-owned-$Operation.json"
    }
}

function Read-PortableTransaction([string] $Root) {
    $path = Join-Path $Root 'portable-transaction.json'
    Assert-PlainPath $path
    if (-not (Test-Path -LiteralPath $path)) { return $null }
    $record = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
    if ($record.schema_version -ne 1 -or $record.operation -notmatch '^[a-f0-9]{32}$' -or
        $record.git_commit -notmatch '^[a-f0-9]{40}$' -or
        $record.capture_completed -isnot [bool] -or
        $record.phase -notin @('staging','prepared','captured','swapping','data','verifying','accepted','rolling_back','rolled_back')) {
        throw 'Unknown portable transaction journal; existing files will be preserved.'
    }
    if ($record.old -and ($record.old.schema_version -notin @(1,2) -or $record.old.git_commit -notmatch '^[a-f0-9]{40}$')) {
        throw 'Invalid previous source revision in transaction journal.'
    }
    if ($record.legacy_directory -and $record.legacy_directory -notmatch '^main-[a-f0-9]{12}-[a-f0-9]{8}$') {
        throw 'Invalid previous portable runtime.'
    }
    if ($record.had_samples -and -not $record.capture_completed -and $record.phase -in @('swapping','data','verifying','accepted')) {
        throw 'The data move has no completed Native capture; files will be preserved.'
    }
    $paths = Get-PortablePaths $Root $record.operation
    foreach ($entry in $paths.GetEnumerator()) { Assert-PlainPath $entry.Value }
    return $record
}

function Save-PortableTransaction([string] $Root, $Record, [string] $Phase) {
    $Record.phase = $Phase
    Write-AtomicJson (Join-Path $Root 'portable-transaction.json') $Record
}

function Invoke-PortableCheck([string] $Root, [string] $Checkout, [string] $Runtime) {
    Invoke-Checked 'powershell.exe' @('-NoLogo','-NoProfile','-ExecutionPolicy','Bypass','-File',
        (Join-Path $Runtime 'launch.ps1'),'-Root',$Root,'-CheckoutDirectory',$Checkout,
        '-RuntimeDirectory',$Runtime,'-Check','-InstallerOwned')
}

function Invoke-PortableModels([string] $Root, [string] $Checkout, [string] $Runtime, [string] $Mode) {
    Invoke-Checked 'powershell.exe' @('-NoLogo','-NoProfile','-ExecutionPolicy','Bypass','-File',
        (Join-Path $Runtime 'launch.ps1'),'-Root',$Root,'-CheckoutDirectory',$Checkout,
        '-RuntimeDirectory',$Runtime,'-ModelMode',$Mode,'-InstallerOwned')
}

function Invoke-PortableRelocation([string] $Root, [string] $Checkout, [string] $Runtime,
    [string] $Source, [string] $Target, [string] $Record, [string] $Action) {
    Invoke-Checked 'powershell.exe' @('-NoLogo','-NoProfile','-ExecutionPolicy','Bypass','-File',
        (Join-Path $Runtime 'launch.ps1'),'-Root',$Root,'-CheckoutDirectory',$Checkout,
        '-RuntimeDirectory',$Runtime,'-RelocationAction',$Action,'-RelocationSource',$Source,
        '-RelocationTarget',$Target,'-RelocationRecord',$Record,'-InstallerOwned')
}

function Move-PortableDirectory([string] $Source, [string] $Target) {
    Assert-PlainPath $Source; Assert-PlainPath $Target
    if (-not (Test-Path -LiteralPath $Source)) { return }
    if (Test-Path -LiteralPath $Target) { throw "Portable destination conflict: $Target" }
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($Target)) | Out-Null
    [IO.Directory]::Move($Source, $Target)
}

function Get-PortableCheckoutDirectories($Release) {
    if ($Release.PSObject.Properties['checkout_directories']) { return ,@($Release.checkout_directories) }
    return $null
}

function Assert-PortableInventory([string] $Directory, $Inventory, $Directories = $null,
    [switch] $Persistent, [switch] $VerifyHashes, [switch] $AllowExtras,
    [switch] $IgnoreUnrecordedDirectories, [switch] $ReleaseManifest) {
    Assert-PortableTree $Directory
    if ($null -eq $Inventory) { throw "No complete file ownership inventory for $Directory; files will be preserved." }
    if (-not (Test-Path -LiteralPath $Directory)) {
        if ($VerifyHashes) { throw "Source revision is missing: $Directory" }
        return
    }
    $known = @{}
    $knownDirectories = @{}
    foreach ($property in $Inventory.PSObject.Properties) {
        $relative = $property.Name
        if ($relative -match '(^[/\\]|:|(^|[/\\])\.\.([/\\]|$))' -or $property.Value -notmatch '^[a-f0-9]{64}$') {
            throw 'Invalid portable file ownership inventory.'
        }
        $relative = $relative.Replace('\','/')
        $known[$relative] = $property.Value
        while ($relative.Contains('/')) {
            $relative = $relative.Substring(0,$relative.LastIndexOf('/'))
            $knownDirectories[$relative] = $true
        }
    }
    if ($null -ne $Directories) {
        foreach ($relative in @($Directories)) {
            if ($relative -isnot [string] -or -not $relative -or
                $relative -match '(^[/\\]|:|(^|[/\\])\.{1,2}([/\\]|$)|[/\\]$)') {
                throw 'Invalid portable directory ownership inventory.'
            }
            $knownDirectories[$relative.Replace('\','/')] = $true
        }
    }
    $extras = $false
    foreach ($file in @(Get-ChildItem -LiteralPath $Directory -Recurse -Force -File)) {
        $relative = $file.FullName.Substring($Directory.TrimEnd('\').Length + 1).Replace('\','/')
        if ($Persistent -and ($relative.StartsWith('samples/') -or $relative.StartsWith('config/input/'))) { continue }
        if ($ReleaseManifest -and $relative -eq 'release-manifest.json') { continue }
        if (-not $known.ContainsKey($relative)) {
            if (-not $AllowExtras) { throw "Unknown file will be preserved: $($file.FullName)" }
            $extras = $true
            continue
        }
        # Present bytes must still belong to this installation, even when absent
        # Runtime leaves are allowed so that a damaged installation can be repaired.
        Assert-Hash $file.FullName $known[$relative]
    }
    if ($null -ne $Directories -or -not $IgnoreUnrecordedDirectories) {
        foreach ($folder in @(Get-ChildItem -LiteralPath $Directory -Recurse -Force -Directory)) {
            $relative = $folder.FullName.Substring($Directory.TrimEnd('\').Length + 1).Replace('\','/')
            if ($Persistent -and ($relative -in @('samples','config','config/input') -or
                $relative.StartsWith('samples/') -or $relative.StartsWith('config/input/'))) { continue }
            if (-not $knownDirectories.ContainsKey($relative)) {
                if (-not $AllowExtras) { throw "Unknown directory will be preserved: $($folder.FullName)" }
                $extras = $true
            }
        }
    }
    if ($VerifyHashes) {
        foreach ($relative in $known.Keys) {
            $path = Join-Path $Directory $relative
            if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Source revision was modified: $path" }
        }
    }
    if ($AllowExtras) { return $extras }
}

function Write-PortableBytes([string] $Path, [byte[]] $Bytes) {
    Assert-PlainPath $Path
    $temporary = "$Path.$([Guid]::NewGuid().ToString('N')).tmp"
    $stream = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
    try { $stream.Write($Bytes,0,$Bytes.Length); $stream.Flush($true) } finally { $stream.Dispose() }
    if (Test-Path -LiteralPath $Path) { [IO.File]::Replace($temporary,$Path,[NullString]::Value) }
    else { [IO.File]::Move($temporary,$Path) }
}

function Remove-PortableOwnedDirectory([string] $Root, [string] $Path) {
    $rootPrefix = [IO.Path]::GetFullPath((Join-Path $Root 'cache')).TrimEnd('\') + '\'
    $absolute = [IO.Path]::GetFullPath($Path)
    if (-not $absolute.StartsWith($rootPrefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'Cleanup destination escapes the private cache.' }
    Assert-PortableTree $absolute
    if (Test-Path -LiteralPath $absolute) { Remove-Item -LiteralPath $absolute -Recurse -Force }
}

function Get-PortableInputInventory([string] $Path) {
    $inventory = @{}
    Assert-PortableTree $Path
    if (Test-Path -LiteralPath $Path) {
        foreach ($file in @(Get-ChildItem -LiteralPath $Path -Recurse -Force -File)) {
            $relative = $file.FullName.Substring($Path.TrimEnd('\').Length + 1).Replace('\','/')
            $inventory[$relative] = Get-Sha256 $file.FullName
        }
    }
    return [pscustomobject]$inventory
}

function Get-PortableDataSource([string] $Root, $Record, $Paths) {
    if ($Record.legacy_directory) { return (Join-Path $Root 'data') }
    if (Test-Path -LiteralPath $Paths.old_checkout) { return $Paths.old_checkout }
    return $Paths.checkout
}

function Get-PortableCandidateCustody($Paths, $Record) {
    $checkout = if (Test-Path -LiteralPath $Paths.source) { $Paths.source } else { $Paths.checkout }
    $runtime = if (Test-Path -LiteralPath $Paths.candidate) { $Paths.candidate } else { $Paths.runtime }
    foreach ($path in @($checkout,$runtime)) {
        Assert-PlainPath $path
        if (-not (Test-Path -LiteralPath $path -PathType Container)) { throw 'Transaction candidate is missing; resumption was safely stopped.' }
    }
    $release = Get-Content -LiteralPath (Join-Path $runtime 'release-manifest.json') -Raw | ConvertFrom-Json
    if ($release.schema_version -ne 2 -or $release.git_commit -ne $Record.git_commit -or
        $release.source_dirty -isnot [bool] -or $release.source_dirty) {
        throw 'Transaction candidate source revision does not match the journal.'
    }
    # Active legacy manifests can validate executable files without claiming
    # unrecorded empty directories for cleanup. Staged candidates stay strict.
    $active = $checkout -eq $Paths.checkout
    Assert-PortableInventory $checkout $release.checkout_files -Directories (Get-PortableCheckoutDirectories $release) -Persistent:$active -VerifyHashes -IgnoreUnrecordedDirectories:$active
    Assert-PortableInventory $runtime $release.runtime_files -VerifyHashes -ReleaseManifest -IgnoreUnrecordedDirectories
    return @{checkout=$checkout; runtime=$runtime; release=$release}
}

function Remove-PortableBuildStages([string] $Root, $Record, $Paths) {
    if (-not (Test-Path -LiteralPath $Paths.build_owned)) { return }
    $owned = Get-Content -LiteralPath $Paths.build_owned -Raw | ConvertFrom-Json
    if ($owned.operation -ne $Record.operation -or $owned.schema_version -ne 1) { throw 'Unknown build file ownership inventory.' }
    foreach ($relative in $owned.directories) {
        if ($relative -notmatch ('^cache/(env|wheels|licenses|bundle)-' + $Record.operation.Substring(0,8) + '$')) { throw 'Unknown temporary build path.' }
        Remove-PortableOwnedDirectory $Root (Join-Path $Root $relative)
    }
    Remove-Item -LiteralPath $Paths.build_owned
}

function Complete-PortableTransaction([string] $Root, $Record) {
    $paths = Get-PortablePaths $Root $Record.operation
    $custody = Get-PortableCandidateCustody $paths $Record
    if ($custody.checkout -ne $paths.checkout -or $custody.runtime -ne $paths.runtime) { throw 'Candidate was not fully activated.' }
    if (Test-Path -LiteralPath $paths.old_checkout) {
        $extras = Assert-PortableInventory $paths.old_checkout $Record.old_release.checkout_files -Directories (Get-PortableCheckoutDirectories $Record.old_release) -VerifyHashes -AllowExtras
        if ($extras) {
            Write-Warning "Previous checkout contains unregistered files or directories and will be preserved in full: $(Resolve-PortablePhysicalPath $paths.old_checkout)"
        } else { Remove-PortableOwnedDirectory $Root $paths.old_checkout }
    }
    if (Test-Path -LiteralPath $paths.old_runtime) {
        Assert-PortableInventory $paths.old_runtime $Record.old_release.runtime_files -ReleaseManifest -IgnoreUnrecordedDirectories
        Remove-PortableOwnedDirectory $Root $paths.old_runtime
    }
    if ($Record.legacy_directory) {
        $legacy = Join-Path (Join-Path $Root 'versions') $Record.legacy_directory
        if (Test-Path -LiteralPath $legacy) {
            if ($Record.old_release.PSObject.Properties['runtime_files']) {
                Assert-PortableInventory $legacy $Record.old_release.runtime_files -ReleaseManifest -IgnoreUnrecordedDirectories
                Assert-PortableTree $legacy
                Remove-Item -LiteralPath $legacy -Recurse -Force
            } else {
                Write-Warning "Old runtime without a complete file ownership inventory will be preserved: $legacy"
            }
        }
        foreach ($directory in @('data/config/input','data/config','data','versions')) {
            $path = Join-Path $Root $directory
            Assert-PlainPath $path
            if ((Test-Path -LiteralPath $path -PathType Container) -and -not @(Get-ChildItem -LiteralPath $path -Force).Count) {
                Remove-Item -LiteralPath $path
            }
        }
    }
    Remove-PortableBuildStages $Root $Record $paths
    foreach ($path in @($paths.source,$paths.candidate)) {
        if (Test-Path -LiteralPath $path) { throw "Candidate was not fully activated: $path" }
    }
    if (Test-Path -LiteralPath $paths.relocation) { Remove-Item -LiteralPath $paths.relocation }
    Remove-Item -LiteralPath (Join-Path $Root 'portable-transaction.json')
    $sizes = @{}
    foreach ($name in @('cache/uv','cache/ort','cache/vcpkg-downloads','cache/vcpkg-binaries','b','tools')) {
        $path = Join-Path $Root $name
        try {
            Assert-PortableTree $path
            [long] $bytes = 0
            if (Test-Path -LiteralPath $path) {
                foreach ($file in @(Get-ChildItem -LiteralPath $path -Recurse -Force -File)) { $bytes += $file.Length }
            }
            $sizes[$name] = $bytes
        } catch { $sizes[$name] = 'not measured; cache will be preserved' }
    }
    Write-Host ('Retained tool/build caches (bytes; avoid repeated downloads and builds): ' + ($sizes | ConvertTo-Json -Compress))
}

function Undo-PortableTransaction([string] $Root, $Record) {
    $paths = Get-PortablePaths $Root $Record.operation
    Save-PortableTransaction $Root $Record 'rolling_back'
    if ($Record.had_samples -and $Record.capture_completed -and (Test-Path -LiteralPath $paths.relocation)) {
        $custody = Get-PortableCandidateCustody $paths $Record
        $sourceRoot = Get-PortableDataSource $Root $Record $paths
        Invoke-PortableRelocation $Root $custody.checkout $custody.runtime (Join-Path $sourceRoot 'samples') (Join-Path $paths.checkout 'samples') $paths.relocation 'rollback'
    }
    if ($Record.had_input) {
        $sourceRoot = Get-PortableDataSource $Root $Record $paths
        $inputSource = Join-Path $sourceRoot 'config/input'
        $inputTarget = Join-Path $paths.checkout 'config/input'
        if ($inputSource -ne $inputTarget -and -not (Test-Path -LiteralPath $inputSource) -and (Test-Path -LiteralPath $inputTarget)) {
            Assert-PortableInventory $inputTarget $Record.input_files -VerifyHashes -IgnoreUnrecordedDirectories
            Move-PortableDirectory $inputTarget $inputSource
        }
    }
    if (-not (Test-Path -LiteralPath $paths.source) -and (Test-Path -LiteralPath $paths.checkout) -and
        (-not $Record.had_checkout -or (Test-Path -LiteralPath $paths.old_checkout))) {
        Move-PortableDirectory $paths.checkout $paths.source
    }
    if (-not (Test-Path -LiteralPath $paths.candidate) -and (Test-Path -LiteralPath $paths.runtime) -and
        (-not $Record.had_runtime -or (Test-Path -LiteralPath $paths.old_runtime))) {
        Move-PortableDirectory $paths.runtime $paths.candidate
    }
    Move-PortableDirectory $paths.old_checkout $paths.checkout
    Move-PortableDirectory $paths.old_runtime $paths.runtime
    if ($Record.PSObject.Properties['starters']) {
        foreach ($property in $Record.starters.PSObject.Properties) {
            if ($property.Name -notin @('launch.ps1','START-LOOPER.bat','start.bat','start-release.bat')) { throw 'Invalid launcher in journal.' }
            $path = Join-Path $Root $property.Name
            if ($null -ne $property.Value) { Write-PortableBytes $path ([Convert]::FromBase64String($property.Value)) }
            elseif (Test-Path -LiteralPath $path) { Assert-PlainPath $path; Remove-Item -LiteralPath $path }
        }
    }
    if ($Record.old) { Write-AtomicJson (Join-Path $Root 'current.json') $Record.old }
    elseif (Test-Path -LiteralPath (Join-Path $Root 'current.json')) { Remove-Item -LiteralPath (Join-Path $Root 'current.json') }
    Save-PortableTransaction $Root $Record 'rolled_back'
    # Failed candidates remain recorded as diagnostic material. A repeat can
    # clean only these exact paths after the accepted installation is restored.
}

function Recover-PortableTransaction([string] $Root) {
    $record = Read-PortableTransaction $Root
    if (-not $record) { return }
    $paths = Get-PortablePaths $Root $record.operation
    if ($record.phase -eq 'accepted') {
        $custody = Get-PortableCandidateCustody $paths $record
        if ($custody.checkout -ne $paths.checkout -or $custody.runtime -ne $paths.runtime) { throw 'Accepted candidate was not fully activated.' }
        Invoke-PortableCheck $Root $paths.checkout $paths.runtime
        Invoke-PortableModels $Root $paths.checkout $paths.runtime 'check'
        Complete-PortableTransaction $Root $record
        return
    }
    if ($record.phase -notin @('staging','prepared','rolled_back')) { Undo-PortableTransaction $Root $record }
    $proven = $false
    if ((Test-Path -LiteralPath $paths.source) -and (Test-Path -LiteralPath (Join-Path $paths.candidate 'release-manifest.json'))) {
        $release = Get-Content -LiteralPath (Join-Path $paths.candidate 'release-manifest.json') -Raw | ConvertFrom-Json
        if ($release.schema_version -eq 2 -and $release.git_commit -eq $record.git_commit -and -not $release.source_dirty) {
            try {
                Get-PortableCandidateCustody $paths $record | Out-Null
                $proven = $true
            } catch { Write-Warning "Candidate ownership is incomplete; diagnostics will be preserved: $($_.Exception.Message)" }
        }
    }
    if ($proven) {
        foreach ($path in @($paths.source,$paths.candidate)) { Remove-PortableOwnedDirectory $Root $path }
        Remove-PortableBuildStages $Root $record $paths
    } else {
        Write-AtomicJson (Join-Path $Root ("cache/failed-" + $record.operation + '.json')) $record
        Write-Warning 'Incomplete candidates will be preserved as diagnostic material in the private cache.'
    }
    if ((Test-Path -LiteralPath $paths.old_checkout) -or (Test-Path -LiteralPath $paths.old_runtime)) {
        throw 'The previous installation was not fully restored.'
    }
    if (Test-Path -LiteralPath $paths.relocation) {
        if ($record.capture_completed) { Remove-Item -LiteralPath $paths.relocation }
        else {
            if ($record.had_checkout) { Assert-PortableInventory $paths.checkout $record.old_release.checkout_files -Directories (Get-PortableCheckoutDirectories $record.old_release) -Persistent -VerifyHashes -AllowExtras | Out-Null }
            if ($record.had_runtime) { Assert-PortableInventory $paths.runtime $record.old_release.runtime_files -ReleaseManifest -IgnoreUnrecordedDirectories }
            if ($record.legacy_directory) {
                Assert-PortableInventory (Join-Path (Join-Path $Root 'versions') $record.legacy_directory) $record.old_release.runtime_files -ReleaseManifest -IgnoreUnrecordedDirectories
            }
            $diagnostic = Join-Path $Root ('cache/failed-relocation-' + $record.operation + '.json')
            Assert-PlainPath $diagnostic
            if (Test-Path -LiteralPath $diagnostic) { throw "Capture diagnostic path is already occupied: $diagnostic" }
            [IO.File]::Move($paths.relocation,$diagnostic)
            Write-Warning "Incomplete Native capture will be preserved without moving data: $diagnostic"
        }
    }
    Remove-Item -LiteralPath (Join-Path $Root 'portable-transaction.json')
}

# BEGIN standalone build-path helpers
# Dot-source common.ps1 first. SUBST creates a temporary DOS name for the already
# validated installation directory; no files are copied, moved or deleted.

function Get-PortableBuildLogicalDrives {
    return [IO.Directory]::GetLogicalDrives()
}

function Get-PortableBuildDriveTarget([string] $Drive) {
    if ($Drive -notmatch '^[A-Z]:$') { throw "Invalid drive name: $Drive" }
    if (-not ('FlitziPortable.BuildDriveNative' -as [type])) {
        $utility = Join-Path $PSHOME 'Modules\Microsoft.PowerShell.Utility\Microsoft.PowerShell.Utility.psd1'
        Import-Module -Name $utility -ErrorAction Stop
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
namespace FlitziPortable {
    public static class BuildDriveNative {
        [DllImport("kernel32.dll", EntryPoint="QueryDosDeviceW", CharSet=CharSet.Unicode,
            SetLastError=true)]
        public static extern uint QueryDosDevice(string name, StringBuilder target, int capacity);
    }
}
'@
    }
    $capacity = 1024
    while ($capacity -le 32768) {
        $buffer = [Text.StringBuilder]::new($capacity)
        $length = [FlitziPortable.BuildDriveNative]::QueryDosDevice($Drive, $buffer, $capacity)
        if ($length -ne 0) {
            $target = $buffer.ToString().Split([char] 0)[0]
            if ($target.StartsWith('\??\UNC\', [StringComparison]::OrdinalIgnoreCase)) {
                return '\\' + $target.Substring(8).TrimEnd('\')
            }
            if ($target.StartsWith('\??\', [StringComparison]::OrdinalIgnoreCase)) {
                $target = $target.Substring(4)
                if ($target.Length -gt 3) { $target = $target.TrimEnd('\') }
            }
            return $target
        }
        $code = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
        if ($code -eq 2) { return $null }
        if ($code -ne 122) { throw [ComponentModel.Win32Exception]::new($code) }
        $capacity *= 2
    }
    throw "DOS drive mapping is too long: $Drive"
}

function Invoke-PortableBuildSubst([string[]] $Arguments) {
    $system = [Environment]::GetFolderPath([Environment+SpecialFolder]::System)
    $executable = Join-Path $system 'subst.exe'
    Invoke-Checked $executable $Arguments | Out-Host
}

function Resolve-PortablePhysicalPath([string] $Path) {
    $current = [IO.Path]::GetFullPath($Path)
    $visited = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    # Only drive-letter SUBST targets are expanded. Physical volumes and network
    # devices stop resolution; no filesystem junction or global setting changes.
    for ($depth = 0; $depth -lt 26; $depth++) {
        if ($current -notmatch '^[A-Z]:\\') { return $current }
        $drive = $current.Substring(0, 2).ToUpperInvariant()
        if (-not $visited.Add($drive)) { throw "Cyclic SUBST mapping: $drive" }
        $target = Get-PortableBuildDriveTarget $drive
        if (-not $target -or ($target -notmatch '^[A-Z]:\\' -and -not $target.StartsWith('\\'))) {
            return $current
        }
        $relative = $current.Substring(3)
        if ($relative) { $current = [IO.Path]::GetFullPath([IO.Path]::Combine($target, $relative)) }
        else { $current = [IO.Path]::GetFullPath($target) }
    }
    throw 'SUBST path resolution exceeds the maximum number of drives.'
}

function Get-PortableBuildDriveOwners {
    $existing = Get-Variable -Name PortableBuildDriveOwners -Scope Script -ErrorAction SilentlyContinue
    if (-not $existing) { $script:PortableBuildDriveOwners = @{} }
    return $script:PortableBuildDriveOwners
}

function New-PortableBuildDrive([string] $Root) {
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT -or
        -not [Environment]::Is64BitProcess) {
        throw 'The temporary build path requires Windows and 64-bit PowerShell.'
    }
    $original = [IO.Path]::GetFullPath($Root)
    if ($original.Length -gt 3) { $original = $original.TrimEnd('\') }
    Assert-PlainPath $original
    if (-not [IO.Directory]::Exists($original)) { throw "Installation directory is missing: $original" }
    $owners = Get-PortableBuildDriveOwners
    $logical = @(Get-PortableBuildLogicalDrives)
    foreach ($letter in ('Z', 'Y', 'X', 'W', 'V', 'U', 'T', 'S', 'R')) {
        $drive = $letter + ':'
        $path = $drive + '\'
        if ($logical -contains $path -or $owners.ContainsKey($drive) -or
            $null -ne (Get-PortableBuildDriveTarget $drive)) { continue }
        # Recheck immediately before SUBST. SUBST itself rejects existing names.
        if (@(Get-PortableBuildLogicalDrives) -contains $path -or
            $null -ne (Get-PortableBuildDriveTarget $drive)) { continue }
        try {
            Invoke-PortableBuildSubst @($drive, $original)
        } catch {
            # Another process may claim the name after our last check. SUBST
            # rejects that collision; retry without touching the other mapping.
            if ($null -ne (Get-PortableBuildDriveTarget $drive) -or
                @(Get-PortableBuildLogicalDrives) -contains $path) { continue }
            throw
        }
        $actual = Get-PortableBuildDriveTarget $drive
        if (-not [string]::Equals($actual, $original, [StringComparison]::OrdinalIgnoreCase)) {
            throw "Build drive does not point to the installation directory: $drive"
        }
        $mapping = [pscustomobject]@{ Drive=$drive; Path=$path; OriginalRoot=$original }
        # Keep the exact returned object and authoritative original path. A caller
        # cannot construct a mapping object to remove somebody else's drive.
        $owners[$drive] = @{ Mapping=$mapping; OriginalRoot=$original }
        try {
            if (-not [IO.Directory]::Exists($path)) { throw "Build drive is inaccessible: $drive" }
            Write-Host "Temporary build path: $path"
            return $mapping
        } catch {
            Remove-PortableBuildDrive $mapping | Out-Null
            throw
        }
    }
    throw 'No free temporary build drive letter is available between Z: and R:.'
}

function Remove-PortableBuildDrive($Mapping) {
    if ($null -eq $Mapping -or -not $Mapping.PSObject.Properties['Drive']) { return $false }
    $drive = [string] $Mapping.Drive
    $owners = Get-PortableBuildDriveOwners
    if (-not $owners.ContainsKey($drive) -or
        -not [object]::ReferenceEquals($owners[$drive].Mapping, $Mapping)) { return $false }
    $expected = [string] $owners[$drive].OriginalRoot
    $actual = Get-PortableBuildDriveTarget $drive
    if (-not [string]::Equals($actual, $expected, [StringComparison]::OrdinalIgnoreCase)) {
        $owners.Remove($drive)
        Write-Warning "Temporary build mapping was changed; drive will remain untouched: $drive"
        return $false
    }
    Invoke-PortableBuildSubst @($drive, '/D')
    $owners.Remove($drive)
    if ([string]::Equals((Get-PortableBuildDriveTarget $drive), $expected,
        [StringComparison]::OrdinalIgnoreCase)) {
        throw "Temporary build drive could not be removed: $drive"
    }
    return $true
}
# END standalone build-path helpers

$Repository = 'https://github.com/DJ-Flitzefinger/flitzis-looper.git'
$transcript = $false

function Get-PortableGit([string] $Root) {
    foreach ($existing in @(Get-Command git.exe -CommandType Application -ErrorAction SilentlyContinue)) {
        & $existing.Source --version | Out-Null
        if ($LASTEXITCODE -eq 0) { return $existing.Source }
    }
    $directory = Join-Path $Root 'tools/git-2.56.0.2'
    # Use the real pinned UCRT executable: cmd/git.exe reconstructs a physical
    # PATH that breaks for long folders or semicolons despite the build alias.
    $executable = Join-Path $directory 'ucrt64/bin/git.exe'
    Assert-PlainPath $directory
    if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
        $archive = Join-Path $Root "cache/git-$([Guid]::NewGuid().ToString('N')).zip"
        Write-Host 'Git is missing: downloading the private Git version ...'
        Read-Payload ([Uri]'https://github.com/git-for-windows/git/releases/download/v2.56.0.windows.2/MinGit-2.56.0.2-64-bit.zip') $archive
        Assert-Hash $archive 'da35e72aa21c005a5a0d298cfbae110bc1609a815730ea0dde84b01a1b3cd3be'
        # MinGit's cmd launcher also canonicalizes its executable directory.
        $fresh = Join-Path $Root "tools/g-$([Guid]::NewGuid().ToString('N').Substring(0,8))"
        [IO.Directory]::CreateDirectory($fresh) | Out-Null
        Expand-ContainedZip $archive $fresh
        $executable = Join-Path $fresh 'ucrt64/bin/git.exe'
        Invoke-Checked $executable @('--version') | Out-Host
        # Publish only a verified usable tool, without touching any existing tool.
        if (-not (Test-Path -LiteralPath $directory)) { [IO.Directory]::Move($fresh, $directory) }
        else { $directory = $fresh }
        $executable = Join-Path $directory 'ucrt64/bin/git.exe'
        Assert-PlainPath $archive
        Remove-Item -LiteralPath $archive
    }
    Assert-PlainPath $executable
    return $executable
}

try {
    if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64' -and $env:PROCESSOR_ARCHITEW6432 -ne 'AMD64') { throw 'The Looper requires Windows x64.' }
    $installerFolder = [IO.Path]::GetDirectoryName($env:LOOPER_PORTABLE_FILE)
    $target = $env:LOOPER_PORTABLE_TARGET
    if (-not $target) {
        if (Test-Path -LiteralPath (Join-Path $installerFolder '.git')) { $target = Join-Path ([IO.Path]::GetDirectoryName($installerFolder)) 'Flitzis-Looper-Portable' }
        else { $target = Join-Path $installerFolder 'Flitzis-Looper' }
    }
    $target = [IO.Path]::GetFullPath($target).TrimEnd('\')
    Assert-PlainPath $target
    $markerPath = Join-Path $target 'portable-root.json'
    Assert-PlainPath $markerPath
    if (Test-Path -LiteralPath $target) {
        if (-not (Test-Path -LiteralPath $markerPath)) {
            if (@(Get-ChildItem -LiteralPath $target -Force).Count) { throw 'The destination is not empty and does not belong to a portable installation.' }
        } else {
            $marker = Get-Content -LiteralPath $markerPath -Raw | ConvertFrom-Json
            if ($marker.app -ne 'flitzis-looper' -or $marker.schema_version -ne 1) { throw 'Unknown installation owner.' }
        }
    }
    [IO.Directory]::CreateDirectory($target) | Out-Null
    $lock = Open-PortableLock $target
    Assert-NoPortableProcess $target
    if (-not (Test-Path -LiteralPath $markerPath)) { Write-AtomicJson $markerPath @{app='flitzis-looper'; schema_version=1} }
    $actualTarget = $target
    $temporaryPath = Join-Path $target 'cache/temp'
    Assert-PlainPath $temporaryPath
    [IO.Directory]::CreateDirectory($temporaryPath) | Out-Null
    $env:TEMP = $temporaryPath; $env:TMP = $temporaryPath
    $portableDrive = New-PortableBuildDrive $target
    $target = $portableDrive.Path
    foreach ($name in @('models','cache','cache/temp','logs','tools','tools/no-hooks')) {
        $path = Join-Path $target $name; Assert-PlainPath $path
        [IO.Directory]::CreateDirectory($path) | Out-Null
    }
    $log = Join-Path $target "logs/install-$([DateTime]::Now.ToString('yyyyMMdd-HHmmss'))-$([Guid]::NewGuid().ToString('N')).txt"
    Start-Transcript -Path $log | Out-Null; $transcript = $true
    Recover-PortableTransaction $target
    foreach ($name in @('GIT_DIR','GIT_WORK_TREE','GIT_COMMON_DIR','GIT_INDEX_FILE','GIT_OBJECT_DIRECTORY','GIT_ALTERNATE_OBJECT_DIRECTORIES','GIT_CONFIG','GIT_CONFIG_PARAMETERS','GIT_TEMPLATE_DIR')) {
        Remove-Item -LiteralPath ('Env:' + $name) -ErrorAction SilentlyContinue
    }
    $env:GIT_TERMINAL_PROMPT = '0'; $env:GIT_CONFIG_NOSYSTEM = '1'; $env:GIT_CONFIG_GLOBAL = 'NUL'
    $env:GIT_CONFIG_COUNT = '2'; $env:GIT_CONFIG_KEY_0 = 'core.hooksPath'
    $env:GIT_CONFIG_VALUE_0 = Join-Path $target 'tools/no-hooks'
    $env:GIT_CONFIG_KEY_1 = 'core.longpaths'; $env:GIT_CONFIG_VALUE_1 = 'true'
    $git = Get-PortableGit $target
    Write-Host 'Checking the current version on main ...'
    $remote = & $git ls-remote $Repository 'refs/heads/main'
    if ($LASTEXITCODE -ne 0 -or $remote -notmatch '^([a-f0-9]{40})\s+refs/heads/main$') { throw 'main could not be read unambiguously from the repository.' }
    $commit = $Matches[1]
    $pointerPath = Join-Path $target 'current.json'; Assert-PlainPath $pointerPath
    $old = $null; $oldRelease = $null; $legacyDirectory = $null
    $checkout = Join-Path $target 'flitzis-looper'; $runtime = Join-Path $target 'Runtime'
    foreach ($path in @($checkout,$runtime,(Join-Path $target 'data'))) { Assert-PlainPath $path }
    if (Test-Path -LiteralPath $pointerPath) {
        $old = Get-Content -LiteralPath $pointerPath -Raw | ConvertFrom-Json
        if ($old.schema_version -notin @(1,2) -or $old.git_commit -notmatch '^[a-f0-9]{40}$' -or $old.source_kind -ne 'main') { throw 'Unknown previous installation state.' }
        if ($old.schema_version -eq 1) {
            $legacyDirectory = [string]$old.directory
            if ($legacyDirectory -notmatch '^main-[a-f0-9]{12}-[a-f0-9]{8}$') { throw 'Invalid legacy version pointer.' }
            $oldRuntime = Join-Path (Join-Path $target 'versions') $legacyDirectory
            Assert-PortableTree $oldRuntime
            $oldRelease = Get-Content -LiteralPath (Join-Path $oldRuntime 'release-manifest.json') -Raw | ConvertFrom-Json
            if ((Test-Path -LiteralPath $checkout) -or (Test-Path -LiteralPath $runtime)) { throw 'Old and new portable layouts conflict; files will be preserved.' }
        } else {
            $oldRelease = $old.release
            $checkoutExtras = Assert-PortableInventory $checkout $oldRelease.checkout_files -Directories (Get-PortableCheckoutDirectories $oldRelease) -Persistent -VerifyHashes -AllowExtras
            Assert-PortableInventory $runtime $oldRelease.runtime_files -ReleaseManifest -IgnoreUnrecordedDirectories
            if ($checkoutExtras) { Write-Host 'Unregistered checkout files or directories will be preserved; preparing a clean source/runtime without executing that checkout.' }
            if ($old.git_commit -eq $commit -and -not $checkoutExtras) {
                $healthy = $false
                try { Invoke-PortableCheck $actualTarget $checkout $runtime; $healthy = $true }
                catch { Write-Host 'The existing runtime is not ready to start; preparing it again.' }
                if ($healthy) {
                    Invoke-PortableModels $actualTarget $checkout $runtime 'repair'
                    Write-Host 'Current main and both models are ready to start; no Native build is required.' -ForegroundColor Green
                    exit 0
                }
            }
        }
    } elseif ((Test-Path -LiteralPath $checkout) -or (Test-Path -LiteralPath $runtime)) { throw 'Unregistered checkout/runtime will remain untouched.' }
    $operation = [Guid]::NewGuid().ToString('N')
    $paths = Get-PortablePaths $target $operation
    foreach ($name in @('source','candidate','old_checkout','old_runtime','relocation','build_owned')) {
        if (Test-Path -LiteralPath $paths[$name]) { throw "Transaction path is already occupied: $($paths[$name])" }
    }
    $dataRoot = if ($legacyDirectory) { Join-Path $target 'data' } else { $checkout }
    $hadSamples = Test-Path -LiteralPath (Join-Path $dataRoot 'samples')
    $hadInput = Test-Path -LiteralPath (Join-Path $dataRoot 'config/input')
    $record = [pscustomobject]@{schema_version=1; operation=$operation; git_commit=$commit; phase='staging';
        old=$old; old_release=$oldRelease; legacy_directory=$legacyDirectory;
        had_checkout=(Test-Path -LiteralPath $checkout); had_runtime=(Test-Path -LiteralPath $runtime);
        had_samples=$hadSamples; capture_completed=$false; had_input=$hadInput; input_files=(Get-PortableInputInventory (Join-Path $dataRoot 'config/input'))}
    Save-PortableTransaction $target $record 'staging'
    Write-Host "Downloading source revision $($commit.Substring(0,12)) from main ..."
    Invoke-Checked $git @('-c','init.templateDir=','clone','--no-checkout','--depth','1','--single-branch','--branch','main',$Repository,$paths.source)
    Invoke-Checked $git @('-C',$paths.source,'fetch','--depth','1','origin',$commit)
    Invoke-Checked $git @('-C',$paths.source,'checkout','-B','main',$commit)
    if ($old -and $legacyDirectory) { Invoke-Checked $git @('-C',$paths.source,'fetch','--depth','1','origin',$old.git_commit) }
    $actual = & $git -C $paths.source rev-parse HEAD
    if ($LASTEXITCODE -ne 0 -or $actual -ne $commit) { throw 'The source revision does not match the verified main.' }
    $helper = Join-Path $paths.source 'scripts/portable/install-main.ps1'; Assert-PlainPath $helper
    if (-not (Test-Path -LiteralPath $helper -PathType Leaf)) { throw 'The verified main contains no compatible portable installer.' }
    Write-Host 'Checking/installing components and building the private Release runtime ...'
    Invoke-Checked 'powershell.exe' @('-NoLogo','-NoProfile','-ExecutionPolicy','Bypass','-File',$helper,
        '-Root',$target,'-Source',$paths.source,'-Git',$git,'-Commit',$commit,'-RuntimeDirectory',$paths.candidate,'-OperationId',$operation)
    $release = Get-Content -LiteralPath (Join-Path $paths.candidate 'release-manifest.json') -Raw | ConvertFrom-Json
    if ($release.schema_version -ne 2 -or $release.git_commit -ne $commit -or $release.source_dirty) { throw 'The runtime does not belong to the unmodified main source revision.' }
    Get-PortableCandidateCustody $paths $record | Out-Null
    Invoke-PortableCheck $actualTarget $paths.source $paths.candidate
    Invoke-PortableModels $actualTarget $paths.source $paths.candidate 'repair'
    if ($legacyDirectory -and -not $oldRelease.PSObject.Properties['runtime_files']) {
        $proofPath = Join-Path $target "cache/legacy-owner-$operation.json"
        $savedPythonHome = $env:PYTHONHOME; $savedPythonPath = $env:PYTHONPATH
        try {
            $env:PYTHONHOME = Join-Path $paths.candidate 'python'; $env:PYTHONPATH = ''
            Invoke-Checked (Join-Path $paths.candidate 'python/python.exe') @('-s','-B',
                (Join-Path $paths.source 'scripts/portable/legacy_inventory.py'),'--source',$paths.source,
                '--candidate',$paths.candidate,'--legacy',$oldRuntime,'--git',$git,'--commit',$old.git_commit,'--output',$proofPath)
        } finally { $env:PYTHONHOME = $savedPythonHome; $env:PYTHONPATH = $savedPythonPath }
        $oldRelease | Add-Member -NotePropertyName runtime_files -NotePropertyValue (Get-Content -LiteralPath $proofPath -Raw | ConvertFrom-Json)
        $record.old_release = $oldRelease
        Remove-Item -LiteralPath $proofPath
    }
    if ($hadSamples -and (Test-Path -LiteralPath (Join-Path $paths.source 'samples'))) { throw 'New source samples conflict with existing samples.' }
    if ($hadInput -and (Test-Path -LiteralPath (Join-Path $paths.source 'config/input'))) { throw 'New input templates conflict with existing inputs.' }
    $starters = @{}
    foreach ($name in @('launch.ps1','START-LOOPER.bat','start.bat','start-release.bat')) {
        $path = Join-Path $target $name; Assert-PlainPath $path
        if (Test-Path -LiteralPath $path) {
            $template = if ($name -eq 'launch.ps1') { 'dispatch.ps1' } else { 'START-LOOPER.bat' }
            if (-not $oldRelease -or -not $oldRelease.runtime_files.PSObject.Properties[$template]) { throw "Launcher file ownership is unknown; file will be preserved: $path" }
            Assert-Hash $path $oldRelease.runtime_files.$template
            $starters[$name] = [Convert]::ToBase64String([IO.File]::ReadAllBytes($path))
        } else { $starters[$name] = $null }
    }
    $record | Add-Member -NotePropertyName starters -NotePropertyValue ([pscustomobject]$starters)
    Save-PortableTransaction $target $record 'prepared'
    Assert-NoPortableProcess $actualTarget
    Save-PortableTransaction $target $record 'captured'
    if ($hadSamples) {
        Get-PortableCandidateCustody $paths $record | Out-Null
        Invoke-PortableRelocation $actualTarget $paths.source $paths.candidate (Join-Path $dataRoot 'samples') (Join-Path $paths.checkout 'samples') $paths.relocation 'capture'
        $record.capture_completed = $true
        Save-PortableTransaction $target $record 'captured'
    }
    Save-PortableTransaction $target $record 'swapping'
    if ($record.had_checkout) { Move-PortableDirectory $paths.checkout $paths.old_checkout }
    Move-PortableDirectory $paths.source $paths.checkout
    if ($record.had_runtime) { Move-PortableDirectory $paths.runtime $paths.old_runtime }
    Move-PortableDirectory $paths.candidate $paths.runtime
    Save-PortableTransaction $target $record 'data'
    $sourceRoot = Get-PortableDataSource $target $record $paths
    Get-PortableCandidateCustody $paths $record | Out-Null
    if ($hadSamples) { Invoke-PortableRelocation $actualTarget $paths.checkout $paths.runtime (Join-Path $sourceRoot 'samples') (Join-Path $paths.checkout 'samples') $paths.relocation 'move' }
    else { [IO.Directory]::CreateDirectory((Join-Path $paths.checkout 'samples')) | Out-Null }
    if ($hadInput) {
        Move-PortableDirectory (Join-Path $sourceRoot 'config/input') (Join-Path $paths.checkout 'config/input')
        Assert-PortableInventory (Join-Path $paths.checkout 'config/input') $record.input_files -VerifyHashes -IgnoreUnrecordedDirectories
    } else { [IO.Directory]::CreateDirectory((Join-Path $paths.checkout 'config/input')) | Out-Null }
    Save-PortableTransaction $target $record 'verifying'
    Get-PortableCandidateCustody $paths $record | Out-Null
    if ($hadSamples) { Invoke-PortableRelocation $actualTarget $paths.checkout $paths.runtime (Join-Path $sourceRoot 'samples') (Join-Path $paths.checkout 'samples') $paths.relocation 'recover' }
    Invoke-PortableCheck $actualTarget $paths.checkout $paths.runtime
    Invoke-PortableModels $actualTarget $paths.checkout $paths.runtime 'check'
    foreach ($name in @('launch.ps1','START-LOOPER.bat','start.bat','start-release.bat')) {
        $path = Join-Path $target $name; Assert-PlainPath $path
        $template = if ($name -eq 'launch.ps1') { 'dispatch.ps1' } else { 'START-LOOPER.bat' }
        Write-PortableBytes $path ([IO.File]::ReadAllBytes((Join-Path $paths.runtime $template)))
    }
    Write-AtomicJson $pointerPath @{schema_version=2; git_commit=$commit; release_id=$commit; source_kind='main'; release=$release}
    Save-PortableTransaction $target $record 'accepted'
    Complete-PortableTransaction $target $record
    Write-Host "Looper is ready: $actualTarget\START-LOOPER.bat" -ForegroundColor Green
    Write-Host 'Samples, PCM caches, configuration, inputs and both models were preserved/verified.'
    exit 0
} catch {
    $failure = $_.Exception.Message
    if ($null -ne $record -and $record.phase -notin @('staging','prepared','accepted','rolled_back')) {
        try { Undo-PortableTransaction $target $record }
        catch { Write-Warning "Recovery stopped: $($_.Exception.Message). Please run the same installer again." }
    }
    Write-Host "Installation failed: $failure" -ForegroundColor Red
    Write-Host 'Existing data and recovery evidence will be preserved. The same installer can be run again.'
    if ($transcript) { Write-Host "Installation log: $(Join-Path $actualTarget ('logs/' + [IO.Path]::GetFileName($log)))" }
    exit 1
} finally {
    try {
        if ($transcript) { Stop-Transcript | Out-Null }
        if ($null -ne $lock) { $lock.Dispose() }
    } finally { if ($portableDrive) { Remove-PortableBuildDrive $portableDrive | Out-Null } }
}
