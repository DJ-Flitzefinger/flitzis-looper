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
