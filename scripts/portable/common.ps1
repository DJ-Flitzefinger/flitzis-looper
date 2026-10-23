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

function Assert-PortableInventory([string] $Directory, $Inventory, [switch] $Persistent, [switch] $VerifyHashes) {
    Assert-PortableTree $Directory
    if ($null -eq $Inventory) { throw "No complete file ownership inventory for $Directory; files will be preserved." }
    if (-not (Test-Path -LiteralPath $Directory)) {
        if ($VerifyHashes) { throw "Source revision is missing: $Directory" }
        return
    }
    $known = @{}
    foreach ($property in $Inventory.PSObject.Properties) {
        $relative = $property.Name
        if ($relative -match '(^[/\\]|:|(^|[/\\])\.\.([/\\]|$))' -or $property.Value -notmatch '^[a-f0-9]{64}$') {
            throw 'Invalid portable file ownership inventory.'
        }
        $known[$relative.Replace('\','/')] = $property.Value
    }
    foreach ($file in @(Get-ChildItem -LiteralPath $Directory -Recurse -Force -File)) {
        $relative = $file.FullName.Substring($Directory.TrimEnd('\').Length + 1).Replace('\','/')
        if ($Persistent -and ($relative.StartsWith('samples/') -or $relative.StartsWith('config/input/'))) { continue }
        if (-not $Persistent -and $relative -eq 'release-manifest.json') { continue }
        if (-not $known.ContainsKey($relative)) { throw "Unknown file will be preserved: $($file.FullName)" }
        # Present bytes must still belong to this installation, even when absent
        # Runtime leaves are allowed so that a damaged installation can be repaired.
        Assert-Hash $file.FullName $known[$relative]
    }
    if ($VerifyHashes) {
        foreach ($relative in $known.Keys) {
            $path = Join-Path $Directory $relative
            if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Source revision was modified: $path" }
        }
    }
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

function Get-PortableCandidateCustody($Paths) {
    $checkout = if (Test-Path -LiteralPath $Paths.source) { $Paths.source } else { $Paths.checkout }
    $runtime = if (Test-Path -LiteralPath $Paths.candidate) { $Paths.candidate } else { $Paths.runtime }
    foreach ($path in @($checkout,$runtime)) {
        Assert-PlainPath $path
        if (-not (Test-Path -LiteralPath $path -PathType Container)) { throw 'Transaction candidate is missing; resumption was safely stopped.' }
    }
    return @{checkout=$checkout; runtime=$runtime}
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
    $release = Get-Content -LiteralPath (Join-Path $paths.runtime 'release-manifest.json') -Raw | ConvertFrom-Json
    if ($release.git_commit -ne $Record.git_commit -or $release.source_dirty) { throw 'Activated source revision does not match the journal.' }
    Assert-PortableInventory $paths.checkout $release.checkout_files -Persistent -VerifyHashes
    Assert-PortableInventory $paths.runtime $release.runtime_files
    if (Test-Path -LiteralPath $paths.old_checkout) {
        Assert-PortableInventory $paths.old_checkout $Record.old_release.checkout_files -VerifyHashes
        Remove-PortableOwnedDirectory $Root $paths.old_checkout
    }
    if (Test-Path -LiteralPath $paths.old_runtime) {
        Assert-PortableInventory $paths.old_runtime $Record.old_release.runtime_files
        Remove-PortableOwnedDirectory $Root $paths.old_runtime
    }
    if ($Record.legacy_directory) {
        $legacy = Join-Path (Join-Path $Root 'versions') $Record.legacy_directory
        if (Test-Path -LiteralPath $legacy) {
            if ($Record.old_release.PSObject.Properties['runtime_files']) {
                Assert-PortableInventory $legacy $Record.old_release.runtime_files
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
        $custody = Get-PortableCandidateCustody $paths
        $sourceRoot = Get-PortableDataSource $Root $Record $paths
        Invoke-PortableRelocation $Root $custody.checkout $custody.runtime (Join-Path $sourceRoot 'samples') (Join-Path $paths.checkout 'samples') $paths.relocation 'rollback'
    }
    if ($Record.had_input) {
        $sourceRoot = Get-PortableDataSource $Root $Record $paths
        $inputSource = Join-Path $sourceRoot 'config/input'
        $inputTarget = Join-Path $paths.checkout 'config/input'
        if ($inputSource -ne $inputTarget -and -not (Test-Path -LiteralPath $inputSource) -and (Test-Path -LiteralPath $inputTarget)) {
            Assert-PortableInventory $inputTarget $Record.input_files -VerifyHashes
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
            Assert-PortableInventory $paths.source $release.checkout_files -VerifyHashes
            Assert-PortableInventory $paths.candidate $release.runtime_files
            $proven = $true
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
            if ($record.had_checkout) { Assert-PortableInventory $paths.checkout $record.old_release.checkout_files -Persistent -VerifyHashes }
            if ($record.had_runtime) { Assert-PortableInventory $paths.runtime $record.old_release.runtime_files }
            if ($record.legacy_directory) {
                Assert-PortableInventory (Join-Path (Join-Path $Root 'versions') $record.legacy_directory) $record.old_release.runtime_files
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
