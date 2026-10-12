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

function Invoke-Checked([string] $Executable, [string[]] $Arguments) {
    & $Executable @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Befehl fehlgeschlagen ($LASTEXITCODE): $Executable" }
}

# BEGIN standalone build-path helpers
# Dot-source common.ps1 first. SUBST creates a temporary DOS name for the already
# validated installation directory; no files are copied, moved or deleted.

function Get-PortableBuildLogicalDrives {
    return [IO.Directory]::GetLogicalDrives()
}

function Get-PortableBuildDriveTarget([string] $Drive) {
    if ($Drive -notmatch '^[A-Z]:$') { throw "Ungueltiger Laufwerksname: $Drive" }
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
    throw "DOS-Laufwerkszuordnung ist zu lang: $Drive"
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
        if (-not $visited.Add($drive)) { throw "Zyklische SUBST-Zuordnung: $drive" }
        $target = Get-PortableBuildDriveTarget $drive
        if (-not $target -or ($target -notmatch '^[A-Z]:\\' -and -not $target.StartsWith('\\'))) {
            return $current
        }
        $relative = $current.Substring(3)
        if ($relative) { $current = [IO.Path]::GetFullPath([IO.Path]::Combine($target, $relative)) }
        else { $current = [IO.Path]::GetFullPath($target) }
    }
    throw 'SUBST-Pfadaufloesung ueberschreitet die maximale Anzahl von Laufwerken.'
}

function Get-PortableBuildDriveOwners {
    $existing = Get-Variable -Name PortableBuildDriveOwners -Scope Script -ErrorAction SilentlyContinue
    if (-not $existing) { $script:PortableBuildDriveOwners = @{} }
    return $script:PortableBuildDriveOwners
}

function New-PortableBuildDrive([string] $Root) {
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT -or
        -not [Environment]::Is64BitProcess) {
        throw 'Der temporaere Build-Pfad benoetigt Windows und 64-Bit-PowerShell.'
    }
    $original = [IO.Path]::GetFullPath($Root)
    if ($original.Length -gt 3) { $original = $original.TrimEnd('\') }
    Assert-PlainPath $original
    if (-not [IO.Directory]::Exists($original)) { throw "Installationsverzeichnis fehlt: $original" }
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
            throw "Build-Laufwerk zeigt nicht auf das Installationsverzeichnis: $drive"
        }
        $mapping = [pscustomobject]@{ Drive=$drive; Path=$path; OriginalRoot=$original }
        # Keep the exact returned object and authoritative original path. A caller
        # cannot construct a mapping object to remove somebody else's drive.
        $owners[$drive] = @{ Mapping=$mapping; OriginalRoot=$original }
        try {
            if (-not [IO.Directory]::Exists($path)) { throw "Build-Laufwerk ist nicht erreichbar: $drive" }
            Write-Host "Temporaerer Build-Pfad: $path"
            return $mapping
        } catch {
            Remove-PortableBuildDrive $mapping | Out-Null
            throw
        }
    }
    throw 'Kein freier temporaerer Build-Laufwerksbuchstabe zwischen Z: und R: verfuegbar.'
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
        Write-Warning "Temporaere Build-Zuordnung wurde geaendert; Laufwerk bleibt unangetastet: $drive"
        return $false
    }
    Invoke-PortableBuildSubst @($drive, '/D')
    $owners.Remove($drive)
    if ([string]::Equals((Get-PortableBuildDriveTarget $drive), $expected,
        [StringComparison]::OrdinalIgnoreCase)) {
        throw "Temporaeres Build-Laufwerk konnte nicht entfernt werden: $drive"
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
    $executable = Join-Path $directory 'cmd/git.exe'
    Assert-PlainPath $directory
    if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
        $archive = Join-Path $Root "cache/git-$([Guid]::NewGuid().ToString('N')).zip"
        Write-Host 'Git fehlt: lade die private Git-Version ...'
        Read-Payload ([Uri]'https://github.com/git-for-windows/git/releases/download/v2.56.0.windows.2/MinGit-2.56.0.2-64-bit.zip') $archive
        Assert-Hash $archive 'da35e72aa21c005a5a0d298cfbae110bc1609a815730ea0dde84b01a1b3cd3be'
        $fresh = "$directory-$([Guid]::NewGuid().ToString('N'))"
        [IO.Directory]::CreateDirectory($fresh) | Out-Null
        Expand-ContainedZip $archive $fresh
        $executable = Join-Path $fresh 'cmd/git.exe'
        Invoke-Checked $executable @('--version') | Out-Host
        # Publish only a verified usable tool, without touching any existing tool.
        if (-not (Test-Path -LiteralPath $directory)) { [IO.Directory]::Move($fresh, $directory) }
        else { $directory = $fresh }
        $executable = Join-Path $directory 'cmd/git.exe'
    }
    Assert-PlainPath $executable
    return $executable
}

try {
    if ($env:PROCESSOR_ARCHITECTURE -ne 'AMD64' -and $env:PROCESSOR_ARCHITEW6432 -ne 'AMD64') { throw 'Der Looper benoetigt Windows x64.' }
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
    $actualTarget = $target
    $temporaryPath = Join-Path $target 'cache/temp'
    Assert-PlainPath $temporaryPath
    [IO.Directory]::CreateDirectory($temporaryPath) | Out-Null
    $env:TEMP = $temporaryPath
    $env:TMP = $temporaryPath
    $portableDrive = New-PortableBuildDrive $target
    $target = $portableDrive.Path
    foreach ($name in @('versions','data','models','cache','cache/temp','logs','tools','tools/no-hooks')) {
        $path = Join-Path $target $name
        Assert-PlainPath $path
        [IO.Directory]::CreateDirectory($path) | Out-Null
    }
    $log = Join-Path $target "logs/install-$([DateTime]::Now.ToString('yyyyMMdd-HHmmss'))-$([Guid]::NewGuid().ToString('N')).txt"
    Start-Transcript -Path $log | Out-Null
    $transcript = $true
    foreach ($name in @('GIT_DIR','GIT_WORK_TREE','GIT_COMMON_DIR','GIT_INDEX_FILE','GIT_OBJECT_DIRECTORY','GIT_ALTERNATE_OBJECT_DIRECTORIES','GIT_CONFIG','GIT_CONFIG_PARAMETERS','GIT_TEMPLATE_DIR')) {
        Remove-Item -LiteralPath ('Env:' + $name) -ErrorAction SilentlyContinue
    }
    $env:GIT_TERMINAL_PROMPT = '0'
    $env:GIT_CONFIG_NOSYSTEM = '1'
    $env:GIT_CONFIG_GLOBAL = 'NUL'
    $env:GIT_CONFIG_COUNT = '2'
    $env:GIT_CONFIG_KEY_0 = 'core.hooksPath'
    $env:GIT_CONFIG_VALUE_0 = Join-Path $target 'tools/no-hooks'
    $env:GIT_CONFIG_KEY_1 = 'core.longpaths'
    $env:GIT_CONFIG_VALUE_1 = 'true'
    $git = Get-PortableGit $target
    Write-Host 'Pruefe die aktuelle Version auf main ...'
    $remote = & $git ls-remote $Repository 'refs/heads/main'
    if ($LASTEXITCODE -ne 0 -or $remote -notmatch '^([a-f0-9]{40})\s+refs/heads/main$') { throw 'main konnte nicht eindeutig vom Repository gelesen werden.' }
    $commit = $Matches[1]
    $pointerPath = Join-Path $target 'current.json'
    Assert-PlainPath $pointerPath
    if (Test-Path -LiteralPath $pointerPath) {
        $old = Get-Content -LiteralPath $pointerPath -Raw | ConvertFrom-Json
        if ($old.git_commit -eq $commit -and $old.PSObject.Properties['source_kind'] -and $old.source_kind -eq 'main') {
            & powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File (Join-Path $target 'launch.ps1') -Root $actualTarget -Check
            if ($LASTEXITCODE -eq 0) { Write-Host 'Die Installation entspricht bereits dem aktuellen main.'; exit 0 }
            Write-Host 'Vorhandene Laufzeit ist nicht startbereit; bereite sie neu vor.'
        }
    }
    # Git canonicalizes SUBST paths and limits its internal gitdir to ~220 bytes.
    # Keep the disposable checkout name short; its identity is verified below.
    $source = Join-Path $target "cache/source-$([Guid]::NewGuid().ToString('N').Substring(0,8))"
    Write-Host "Lade den Quellstand $($commit.Substring(0,12)) von main ..."
    Invoke-Checked $git @('-c','init.templateDir=', 'clone','--no-checkout','--depth','1','--single-branch','--branch','main',$Repository,$source)
    Invoke-Checked $git @('-C',$source,'fetch','--depth','1','origin',$commit)
    Invoke-Checked $git @('-C',$source,'checkout','--detach',$commit)
    $actual = & $git -C $source rev-parse HEAD
    if ($LASTEXITCODE -ne 0 -or $actual -ne $commit) { throw 'Der Quellstand stimmt nicht mit dem geprueften main ueberein.' }
    $helper = Join-Path $source 'scripts/portable/install-main.ps1'
    Assert-PlainPath $helper
    if (-not (Test-Path -LiteralPath $helper -PathType Leaf)) { throw 'Der gepruefte main enthaelt keinen kompatiblen Portable-Installer.' }
    $directory = "main-$($commit.Substring(0,12))-$([Guid]::NewGuid().ToString('N').Substring(0,8))"
    $version = Join-Path (Join-Path $target 'versions') $directory
    Write-Host 'Pruefe/installiere Komponenten und baue die private Release-Version ...'
    & powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File $helper -Root $target -Source $source -Git $git -Commit $commit -RuntimeDirectory $version
    if ($LASTEXITCODE -ne 0) { throw 'Vorbereitung oder Build fehlgeschlagen. Details stehen im Installationslog.' }
    $release = Get-Content -LiteralPath (Join-Path $version 'release-manifest.json') -Raw | ConvertFrom-Json
    if ($release.git_commit -ne $commit -or $release.source_dirty) { throw 'Die Laufzeit gehoert nicht zum unveraenderten main-Quellstand.' }
    & powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File (Join-Path $version 'launch.ps1') -Root $actualTarget -Check -VersionDirectory $directory
    if ($LASTEXITCODE -ne 0) { throw 'Die neue private Laufzeit hat die Startpruefung nicht bestanden.' }
    foreach ($name in @('launch.ps1','START-LOOPER.bat','start.bat','start-release.bat')) {
        $path = Join-Path $target $name
        Assert-PlainPath $path
        if (Test-Path -LiteralPath $path) { continue }
        $template = if ($name -eq 'launch.ps1') { 'dispatch.ps1' } else { 'START-LOOPER.bat' }
        [IO.File]::Copy((Join-Path $version $template), $path, $false)
    }
    Write-AtomicJson $pointerPath @{schema_version=1; directory=$directory; release_id=$commit; git_commit=$commit; source_kind='main'}
    Write-Host "Looper ist bereit: $actualTarget\START-LOOPER.bat" -ForegroundColor Green
    Write-Host 'Projekte/Modelle und bisherige Laufzeiten wurden erhalten.'
    exit 0
} catch {
    Write-Host "Installation fehlgeschlagen: $($_.Exception.Message)" -ForegroundColor Red
    Write-Host 'Eine vorhandene Version und Ihre Daten bleiben erhalten. Der gleiche Installer kann erneut ausgefuehrt werden.'
    if ($transcript) { Write-Host "Installationslog: $(Join-Path $actualTarget ('logs/' + [IO.Path]::GetFileName($log)))" }
    exit 1
} finally {
    try {
        if ($transcript) { Stop-Transcript | Out-Null }
        if ($null -ne $lock) { $lock.Dispose() }
    } finally {
        if ($portableDrive) { Remove-PortableBuildDrive $portableDrive | Out-Null }
    }
}
