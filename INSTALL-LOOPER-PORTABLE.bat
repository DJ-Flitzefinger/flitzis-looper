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
$DefaultChannel = '@PORTABLE_CHANNEL@'
$lock = $null

function Write-AtomicJson([string] $Path, $Value) {
    $temporary = "$Path.$([Guid]::NewGuid().ToString('N')).tmp"
    [IO.File]::WriteAllText($temporary, ($Value | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
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
                throw "Verknuepfung im Installationspfad: $current"
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
    } else { throw 'Nur lokale Dateien oder HTTPS-Downloads werden unterstuetzt.' }
}

function Assert-Hash([string] $Path, [string] $Expected) {
    $algorithm = [Security.Cryptography.SHA256]::Create()
    $stream = [IO.File]::OpenRead($Path)
    try { $actual = [BitConverter]::ToString($algorithm.ComputeHash($stream)).Replace('-', '').ToLowerInvariant() }
    finally { $stream.Dispose(); $algorithm.Dispose() }
    if ($Expected -notmatch '^[a-f0-9]{64}$' -or $actual -ne $Expected) {
        throw "SHA256-Pruefung fehlgeschlagen: $Path"
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
                throw "Unsicherer Archivpfad: $name"
            }
            foreach ($part in ($name.TrimEnd('\') -split '\\')) {
                if (-not $part -or $part -eq '.' -or $part.EndsWith('.') -or $part.EndsWith(' ') -or $part -match '^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(\.|$)') {
                    throw "Mehrdeutiger Windows-Archivpfad: $name"
                }
            }
            $path = [IO.Path]::GetFullPath((Join-Path $Destination $name))
            if (-not $path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) -or -not $seen.Add($path)) {
                throw "Archivpfad verlaesst das Ziel oder ist doppelt: $name"
            }
            $unixType = ($entry.ExternalAttributes -shr 16) -band 61440
            if ($unixType -eq 40960 -or ($entry.ExternalAttributes -band 1024)) {
                throw "Archiv enthaelt eine Verknuepfung: $name"
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

try {
    if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64' -and $env:PROCESSOR_ARCHITEW6432 -ne 'AMD64') {
        throw 'Dieses Paket benoetigt Windows x64.'
    }
    $installerFolder = [IO.Path]::GetDirectoryName($env:LOOPER_PORTABLE_FILE)
    $target = $env:LOOPER_PORTABLE_TARGET
    if (-not $target) {
        if (Test-Path -LiteralPath (Join-Path $installerFolder '.git')) {
            $target = Join-Path ([IO.Path]::GetDirectoryName($installerFolder)) 'Flitzis-Looper-Portable'
        } else { $target = Join-Path $installerFolder 'Flitzis-Looper' }
    }
    $target = [IO.Path]::GetFullPath($target)
    Assert-PlainPath $target
    $markerPath = Join-Path $target 'portable-root.json'
    Assert-PlainPath $markerPath
    if (Test-Path -LiteralPath $target) {
        if (-not (Test-Path -LiteralPath $markerPath)) {
            if (@(Get-ChildItem -LiteralPath $target -Force).Count) { throw 'Das Ziel ist nicht leer und gehoert keiner Portable-Installation.' }
        } else {
            $marker = Get-Content -LiteralPath $markerPath -Raw | ConvertFrom-Json
            if ($marker.app -ne 'flitzis-looper' -or $marker.schema_version -ne 1) { throw 'Unbekannter Installationsbesitzer.' }
        }
    }
    [IO.Directory]::CreateDirectory($target) | Out-Null
    $lockPath = Join-Path $target '.install.lock'
    Assert-PlainPath $lockPath
    $lock = [IO.File]::Open($lockPath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    if (-not (Test-Path -LiteralPath $markerPath)) { Write-AtomicJson $markerPath @{app='flitzis-looper'; schema_version=1} }
    foreach ($name in @('versions', 'data', 'models', 'cache', 'cache/temp', 'logs')) {
        $path = Join-Path $target $name
        Assert-PlainPath $path
        [IO.Directory]::CreateDirectory($path) | Out-Null
    }
    $source = $env:LOOPER_PORTABLE_CHANNEL
    if (-not $source -and $DefaultChannel -ne '@PORTABLE_CHANNEL@') { $source = $DefaultChannel }
    if (-not $source) {
        $local = Join-Path ([IO.Path]::GetDirectoryName($installerFolder)) 'portable-releases/portable-manifest.json'
        if (Test-Path -LiteralPath $local) { $source = [Uri]::new([IO.Path]::GetFullPath($local)).AbsoluteUri }
        else { $source = 'https://github.com/DJ-Flitzefinger/flitzis-looper/releases/latest/download/portable-manifest.json' }
    }
    $channelUri = [Uri]::new($source)
    $channelFile = Join-Path $target "cache/channel-$([Guid]::NewGuid().ToString('N')).json"
    Write-Host 'Pruefe auf eine neue Portable-Version ...'
    Read-Payload $channelUri $channelFile
    $manifest = Get-Content -LiteralPath $channelFile -Raw | ConvertFrom-Json
    if ($manifest.schema_version -ne 1 -or $manifest.release_id -notmatch '^[a-zA-Z0-9._-]{1,96}$' -or $manifest.git_commit -notmatch '^[a-f0-9]{40}$' -or $manifest.package_sha256 -notmatch '^[a-f0-9]{64}$' -or $manifest.package_size -le 0) {
        throw 'Ungueltiges Portable-Manifest.'
    }
    $pointerPath = Join-Path $target 'current.json'
    Assert-PlainPath $pointerPath
    if (Test-Path -LiteralPath $pointerPath) {
        $old = Get-Content -LiteralPath $pointerPath -Raw | ConvertFrom-Json
        if ($old.package_sha256 -eq $manifest.package_sha256) {
            & powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File (Join-Path $target 'launch.ps1') -Root $target -Check
            if ($LASTEXITCODE -ne 0) { throw 'Die vorhandene Installation ist nicht startbereit.' }
            Write-Host 'Die Portable-Installation ist bereits aktuell.'
            exit 0
        }
    }
    $archivePath = Join-Path $target "cache/package-$([Guid]::NewGuid().ToString('N')).zip"
    $packageUri = [Uri]::new($channelUri, [string]$manifest.package)
    Write-Host "Lade Version $($manifest.release_id) ($([Math]::Round($manifest.package_size / 1GB, 2)) GB) ..."
    Read-Payload $packageUri $archivePath
    if ((Get-Item -LiteralPath $archivePath).Length -ne $manifest.package_size) { throw 'Die Paketgroesse stimmt nicht.' }
    Assert-Hash $archivePath $manifest.package_sha256
    $directory = "$($manifest.release_id)-$([Guid]::NewGuid().ToString('N'))"
    $version = Join-Path (Join-Path $target 'versions') $directory
    [IO.Directory]::CreateDirectory($version) | Out-Null
    Write-Host 'Entpacke die private Laufzeit ...'
    Expand-ContainedZip $archivePath $version
    $release = Get-Content -LiteralPath (Join-Path $version 'release-manifest.json') -Raw | ConvertFrom-Json
    if ($release.release_id -ne $manifest.release_id -or $release.git_commit -ne $manifest.git_commit) { throw 'Paketidentitaet stimmt nicht.' }
    foreach ($name in @('python/python.exe', 'python/keynet.onnx', 'bootstrap.py', 'launch.ps1', 'dispatch.ps1', 'START-LOOPER.bat', 'runtime/ffmpeg/ffmpeg.exe', 'runtime/ffmpeg/ffprobe.exe')) {
        if (-not (Test-Path -LiteralPath (Join-Path $version $name) -PathType Leaf)) { throw "Paketdatei fehlt: $name" }
    }
    Write-Host 'Pruefe die neue Laufzeit ohne App- oder Audiostart ...'
    & powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File (Join-Path $version 'launch.ps1') -Root $target -Check -VersionDirectory $directory
    if ($LASTEXITCODE -ne 0) { throw 'Die neue Laufzeit hat die Startpruefung nicht bestanden.' }
    # Stable starters select a version at launch. They never rebuild or update.
    $launcher = Get-Content -LiteralPath (Join-Path $version 'START-LOOPER.bat') -Raw
    $launchScript = Get-Content -LiteralPath (Join-Path $version 'dispatch.ps1') -Raw
    foreach ($path in @((Join-Path $target 'launch.ps1'), (Join-Path $target 'START-LOOPER.bat'), (Join-Path $target 'start.bat'), (Join-Path $target 'start-release.bat'))) {
        Assert-PlainPath $path
        if (Test-Path -LiteralPath $path) { continue }
        $text = if ($path.EndsWith('.ps1')) { $launchScript } else { $launcher }
        $temporary = "$path.$([Guid]::NewGuid().ToString('N')).tmp"
        [IO.File]::WriteAllText($temporary, $text, [Text.UTF8Encoding]::new($false))
        if (Test-Path -LiteralPath $path) { [IO.File]::Replace($temporary, $path, [NullString]::Value) }
        else { [IO.File]::Move($temporary, $path) }
    }
    Write-AtomicJson $pointerPath @{schema_version=1; directory=$directory; release_id=$manifest.release_id; git_commit=$manifest.git_commit; package_sha256=$manifest.package_sha256}
    Write-Host "Portable-Version ist bereit: $target\START-LOOPER.bat" -ForegroundColor Green
    Write-Host 'Projekte und Modelle bleiben bei Updates erhalten. Alte Laufzeiten wurden behalten.'
    exit 0
} catch {
    Write-Host "Installation fehlgeschlagen: $($_.Exception.Message)" -ForegroundColor Red
    Write-Host 'Die bisherige Version bleibt erhalten. Details siehe obige Meldung; fehlgeschlagene Pakete bleiben im cache.'
    Write-Host 'Ein allgemeiner Internet-Download benoetigt ein veroeffentlichtes Portable-Release.'
    exit 1
} finally {
    if ($null -ne $lock) { $lock.Dispose() }
}
