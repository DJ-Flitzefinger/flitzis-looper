# Dot-source common.ps1 and build-path.ps1 first. Public helpers write progress to
# the host and returns exactly one absolute path. Environment changes are process
# scoped; private Rust settings and caches belong to the installation Root.

function Get-PortableToolRoot([string] $Root) {
    $path = [IO.Path]::GetFullPath($Root)
    Assert-PlainPath $path
    $architecture = $env:PROCESSOR_ARCHITECTURE
    if ($env:PROCESSOR_ARCHITEW6432) { $architecture = $env:PROCESSOR_ARCHITEW6432 }
    if (-not [Environment]::Is64BitOperatingSystem -or -not [Environment]::Is64BitProcess -or
        [Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT -or $architecture -ne 'AMD64') {
        throw 'The portable toolchain requires Windows x64 and 64-bit PowerShell.'
    }
    return $path
}

function New-PortableToolDirectory([string] $Root, [string] $RelativePath) {
    $base = Get-PortableToolRoot $Root
    $path = [IO.Path]::GetFullPath((Join-Path $base $RelativePath))
    $prefix = $base.TrimEnd('\') + '\'
    if (-not $path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Tool path escapes the installation: $path"
    }
    Assert-PlainPath $path
    [IO.Directory]::CreateDirectory($path) | Out-Null
    return $path
}

function Get-PortableVerifiedTool([string] $Root, [string] $Name, [Uri] $Uri, [string] $Sha256) {
    $cache = New-PortableToolDirectory $Root 'cache\toolchain'
    if ($Name -ne [IO.Path]::GetFileName($Name)) { throw 'Tool download requires a plain filename.' }
    $destination = Join-Path $cache $Name
    Assert-PlainPath $destination
    if (-not (Test-Path -LiteralPath $destination -PathType Leaf)) {
        $temporary = "$destination.$([Guid]::NewGuid().ToString('N')).partial"
        Assert-PlainPath $temporary
        Write-Host "Download: $Name"
        Read-Payload $Uri $temporary
        Assert-Hash $temporary $Sha256
        [IO.File]::Move($temporary, $destination)
    }
    Assert-Hash $destination $Sha256
    return $destination
}

function ConvertTo-PortableNativeArgument([string] $Value) {
    # Windows native argv quoting, including quotes and trailing backslashes.
    $escaped = [Text.RegularExpressions.Regex]::Replace($Value, '(\\*)"', '$1$1\"')
    $escaped = [Text.RegularExpressions.Regex]::Replace($escaped, '(\\+)$', '$1$1')
    return '"' + $escaped + '"'
}

function Invoke-PortableCapturedProcess(
    [string] $Executable,
    [string] $ArgumentLine,
    [Text.Encoding] $Encoding = [Text.Encoding]::UTF8
) {
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $Executable
    $info.Arguments = $ArgumentLine
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.StandardOutputEncoding = $Encoding
    $info.StandardErrorEncoding = $Encoding
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $info
    try {
        if (-not $process.Start()) { throw "Process could not be started: $Executable" }
        $outputTask = $process.StandardOutput.ReadToEndAsync()
        $errorTask = $process.StandardError.ReadToEndAsync()
        $process.WaitForExit()
        $output = $outputTask.GetAwaiter().GetResult()
        $errorOutput = $errorTask.GetAwaiter().GetResult()
        if ($process.ExitCode -ne 0) {
            throw "Command failed ($($process.ExitCode)): $Executable`n$output`n$errorOutput"
        }
        if ($errorOutput.Trim()) { Write-Host $errorOutput.Trim() }
        return $output
    } finally { $process.Dispose() }
}

function Get-PortableUv([string] $Root) {
    $base = Get-PortableToolRoot $Root
    $archive = Get-PortableVerifiedTool $base 'uv-0.13.0-windows-x64.zip' `
        'https://github.com/astral-sh/uv/releases/download/0.13.0/uv-x86_64-pc-windows-msvc.zip' `
        '088962f9e7b7bd9ea740c04c650b2a21c8928c345bd99ac24350dc924dba656c'
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($archive)
    try {
        $entries = @($zip.Entries | Where-Object { [IO.Path]::GetFileName($_.FullName) -eq 'uv.exe' })
        if ($entries.Count -ne 1) { throw 'The verified uv archive does not contain a unique uv.exe.' }
        $entryName = $entries[0].FullName.Replace('/', '\')
        $algorithm = [Security.Cryptography.SHA256]::Create()
        $stream = $entries[0].Open()
        try {
            $exeHash = [BitConverter]::ToString($algorithm.ComputeHash($stream)).Replace('-', '').ToLowerInvariant()
        } finally { $stream.Dispose(); $algorithm.Dispose() }
    } finally { $zip.Dispose() }
    $tools = New-PortableToolDirectory $base 'tools'
    $directory = Join-Path $tools 'uv-0.13.0'
    Assert-PlainPath $directory
    if (-not (Test-Path -LiteralPath $directory)) {
        $staging = Join-Path $tools ("uv-0.13.0.$([Guid]::NewGuid().ToString('N')).partial")
        Assert-PlainPath $staging
        [IO.Directory]::CreateDirectory($staging) | Out-Null
        Expand-ContainedZip $archive $staging
        [IO.Directory]::Move($staging, $directory)
    }
    $executable = [IO.Path]::GetFullPath((Join-Path $directory $entryName))
    $prefix = $directory.TrimEnd('\') + '\'
    if (-not $executable.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'uv.exe is outside the tool directory.'
    }
    Assert-PlainPath $executable
    Assert-Hash $executable $exeHash
    $version = (Invoke-PortableCapturedProcess $executable '--version').Trim()
    if ($version -notmatch '^uv 0\.13\.0(?:\s|$)') { throw "Unexpected uv version: $version" }
    return $executable
}

function Get-PortableRust([string] $Root) {
    $base = Get-PortableToolRoot $Root
    $installerHash = '6f4bef66261261fcb43131be8720bab817d403a09edec7455c371974b90bdb7e'
    $installer = Get-PortableVerifiedTool $base 'rustup-init-1.29.1-windows-x64.exe' `
        'https://static.rust-lang.org/rustup/archive/1.29.1/x86_64-pc-windows-msvc/rustup-init.exe' `
        $installerHash
    $env:RUSTUP_HOME = New-PortableToolDirectory $base 'tools\rust-1.95.0\rustup'
    $env:CARGO_HOME = New-PortableToolDirectory $base 'tools\rust-1.95.0\cargo'
    $env:RUSTUP_TOOLCHAIN = '1.95.0-x86_64-pc-windows-msvc'
    $env:RUSTUP_DIST_SERVER = 'https://static.rust-lang.org'
    $env:RUSTUP_UPDATE_ROOT = 'https://static.rust-lang.org/rustup'
    $env:RUSTUP_INIT_SKIP_PATH_CHECK = 'yes'
    $env:RUSTUP_INIT_SKIP_MSVC_CHECK = 'yes'
    $bin = Join-Path $env:CARGO_HOME 'bin'
    Assert-PlainPath $bin
    $rustup = Join-Path $bin 'rustup.exe'
    $cargo = Join-Path $bin 'cargo.exe'
    Assert-PlainPath $rustup
    Assert-PlainPath $cargo
    if (-not (Test-Path -LiteralPath $rustup -PathType Leaf)) {
        Write-Host 'Preparing private Rust toolchain 1.95.0.'
        Invoke-Checked $installer @('-y', '--no-modify-path', '--profile', 'minimal',
            '--default-host', 'x86_64-pc-windows-msvc', '--default-toolchain', $env:RUSTUP_TOOLCHAIN) | Out-Host
    }
    # rustup's Windows proxies are copies/hardlinks of the verified bootstrapper.
    Assert-Hash $rustup $installerHash
    Assert-Hash $cargo $installerHash
    Invoke-Checked $rustup @('set', 'auto-self-update', 'disable') | Out-Host
    Invoke-Checked $rustup @('toolchain', 'install', $env:RUSTUP_TOOLCHAIN,
        '--profile', 'minimal', '--no-self-update') | Out-Host
    $version = (Invoke-PortableCapturedProcess $cargo '--version').Trim()
    if ($version -notmatch '^cargo 1\.95\.0(?:\s|$)') { throw "Unexpected Cargo version: $version" }
    # rustc normally discovers the physical executable path behind SUBST. Keep
    # its library/link paths short for both the application and host build scripts.
    $sysroot = Join-Path $env:RUSTUP_HOME ('toolchains\' + $env:RUSTUP_TOOLCHAIN)
    Assert-PlainPath $sysroot
    $env:CARGO_ENCODED_RUSTFLAGS = '--sysroot' + [char] 31 + $sysroot
    $env:PATH = $bin + ';' + (($env:PATH -split ';' | Where-Object {
        $_ -and -not $_.TrimEnd('\').Equals($bin.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)
    }) -join ';')
    return $cargo
}

function Get-PortableVisualStudioInstances([switch] $All) {
    $programFilesX86 = [Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFilesX86)
    $locator = Join-Path $programFilesX86 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path -LiteralPath $locator -PathType Leaf)) { return }
    Assert-PlainPath $locator
    $arguments = @('-products', '*', '-sort', '-format', 'json', '-utf8')
    if ($All) { $arguments += '-all' }
    else { $arguments += @('-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64') }
    $line = ($arguments | ForEach-Object { ConvertTo-PortableNativeArgument $_ }) -join ' '
    $json = Invoke-PortableCapturedProcess $locator $line
    if (-not $json.Trim()) { return }
    foreach ($instance in @($json | ConvertFrom-Json)) {
        if ($instance.installationPath) { Write-Output $instance }
    }
}

function Restore-PortableProcessEnvironment($Snapshot) {
    foreach ($name in @([Environment]::GetEnvironmentVariables('Process').Keys)) {
        if (-not $Snapshot.Contains($name)) {
            [Environment]::SetEnvironmentVariable([string] $name, $null, 'Process')
        }
    }
    foreach ($name in $Snapshot.Keys) {
        [Environment]::SetEnvironmentVariable([string] $name, [string] $Snapshot[$name], 'Process')
    }
}

function Import-PortableMsvcEnvironment([string] $InstallationPath) {
    $installation = [IO.Path]::GetFullPath($InstallationPath)
    $vcvars = Join-Path $installation 'VC\Auxiliary\Build\vcvars64.bat'
    Assert-PlainPath $vcvars
    if (-not (Test-Path -LiteralPath $vcvars -PathType Leaf)) { throw "vcvars64.bat is missing: $installation" }
    # CALL expands percent signs twice; do not execute an ambiguous custom path.
    if ($vcvars -match '[%"\r\n]') { throw "Unsupported Visual Studio path: $installation" }
    $command = '/d /u /s /c "call "' + $vcvars + '" >nul && set"'
    $cmd = Join-Path ([Environment]::GetFolderPath([Environment+SpecialFolder]::System)) 'cmd.exe'
    $output = Invoke-PortableCapturedProcess $cmd $command ([Text.Encoding]::Unicode)
    foreach ($line in ($output -split '\r?\n')) {
        $separator = $line.IndexOf('=')
        if ($separator -gt 0) {
            [Environment]::SetEnvironmentVariable($line.Substring(0, $separator),
                $line.Substring($separator + 1), 'Process')
        }
    }
    if ($env:VSCMD_ARG_TGT_ARCH -ne 'x64' -or -not $env:VCToolsInstallDir -or
        -not $env:WindowsSdkDir -or -not $env:WindowsSDKVersion) {
        throw "MSVC x64 or Windows SDK environment is missing: $installation"
    }
    $compiler = [IO.Path]::GetFullPath((Join-Path $env:VCToolsInstallDir 'bin\Hostx64\x64\cl.exe'))
    if (-not $compiler.StartsWith($installation.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) {
        throw "MSVC does not come from the selected installation: $compiler"
    }
    Assert-PlainPath $compiler
    if (-not (Test-Path -LiteralPath $compiler -PathType Leaf)) { throw "cl.exe is missing: $compiler" }
    $header = Join-Path $env:WindowsSdkDir ("Include\$($env:WindowsSDKVersion.TrimEnd('\'))\um\windows.h")
    Assert-PlainPath $header
    if (-not (Test-Path -LiteralPath $header -PathType Leaf)) { throw "Windows SDK header is missing: $header" }
    return $compiler
}

function Test-PortableMsvc([string] $Root, [string] $InstallationPath) {
    $compiler = Import-PortableMsvcEnvironment $InstallationPath
    $probe = New-PortableToolDirectory $Root ("cache\toolchain\msvc-probe-$([Guid]::NewGuid().ToString('N'))")
    $source = Join-Path $probe 'probe.c'
    $object = Join-Path $probe 'probe.obj'
    $executable = Join-Path $probe 'probe.exe'
    $content = "#include <windows.h>`r`n#ifndef _WIN64`r`n#error Expected Windows x64`r`n#endif`r`nint main(void) { return GetCurrentProcessId() == 0; }`r`n"
    [IO.File]::WriteAllText($source, $content, [Text.UTF8Encoding]::new($false))
    Invoke-Checked $compiler @('/nologo', '/W4', '/WX', '/MD', "/Fo$object", "/Fe$executable",
        $source, '/link', '/MACHINE:X64', 'kernel32.lib') | Out-Host
    if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) { throw 'MSVC compile/link test did not produce an EXE.' }
}

function Assert-PortableMicrosoftSignature([string] $Path) {
    Assert-PlainPath $Path
    # A caller may have inherited a PSModulePath from PowerShell 7. Import the
    # Windows PowerShell security module by its bundled path before verification.
    $securityModule = Join-Path $PSHOME 'Modules\Microsoft.PowerShell.Security\Microsoft.PowerShell.Security.psd1'
    Import-Module -Name $securityModule -ErrorAction Stop
    $signature = Microsoft.PowerShell.Security\Get-AuthenticodeSignature -LiteralPath $Path
    if ($signature.Status -ne [Management.Automation.SignatureStatus]::Valid -or
        -not $signature.SignerCertificate -or
        $signature.SignerCertificate.Subject -notmatch '(^|,\s*)O="?Microsoft Corporation"?(,|$)') {
        throw "No valid Microsoft signature: $Path"
    }
}

function Install-PortableMsvc([string] $Root) {
    $cache = New-PortableToolDirectory $Root 'cache\toolchain'
    $installer = Join-Path $cache 'vs_buildtools-17.exe'
    Assert-PlainPath $installer
    if (-not (Test-Path -LiteralPath $installer -PathType Leaf)) {
        $temporary = "$installer.$([Guid]::NewGuid().ToString('N')).partial.exe"
        Assert-PlainPath $temporary
        Write-Host 'Downloading Microsoft Build Tools and Windows SDK.'
        Read-Payload 'https://aka.ms/vs/17/release/vs_buildtools.exe' $temporary
        Assert-PortableMicrosoftSignature $temporary
        [IO.File]::Move($temporary, $installer)
    }
    # A per-logon SUBST name can disappear across UAC elevation. Verify and launch
    # the physical payload, with a physical working directory, before RunAs.
    $physicalInstaller = Resolve-PortablePhysicalPath $installer
    $physicalCache = Resolve-PortablePhysicalPath $cache
    Assert-PlainPath $physicalCache
    Assert-PortableMicrosoftSignature $physicalInstaller
    # The SDK is registered globally: a system installation must remain valid if
    # the portable Looper folder is moved. Never install it inside Root.
    $programFiles = [Environment]::GetFolderPath([Environment+SpecialFolder]::ProgramFiles)
    $installation = [IO.Path]::GetFullPath((Join-Path $programFiles 'LooperBuildTools'))
    Assert-PlainPath $installation
    $registered = @(Get-PortableVisualStudioInstances -All | Where-Object {
        [IO.Path]::GetFullPath($_.installationPath).TrimEnd('\').Equals(
            $installation.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)
    })
    if ((Test-Path -LiteralPath $installation) -and $registered.Count -eq 0 -and
        @(Get-ChildItem -LiteralPath $installation -Force).Count -gt 0) {
        throw "Unregistered, nonempty Build Tools destination will remain untouched: $installation"
    }
    if ($registered.Count -gt 0 -and
        $registered[0].productId -ne 'Microsoft.VisualStudio.Product.BuildTools') {
        throw "The reserved Build Tools destination belongs to another Visual Studio product: $installation"
    }
    $arguments = @('--wait', '--passive', '--norestart', '--installPath', $installation,
        '--add', 'Microsoft.VisualStudio.Workload.VCTools',
        '--add', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64',
        '--add', 'Microsoft.VisualStudio.Component.VC.Redist.14.Latest',
        '--add', 'Microsoft.VisualStudio.Component.Windows11SDK.26100')
    if ($registered.Count -gt 0) { $arguments = @('modify') + $arguments }
    $line = ($arguments | ForEach-Object { ConvertTo-PortableNativeArgument $_ }) -join ' '
    Write-Host 'Installing Microsoft C++ Build Tools; Windows may request administrator privileges.'
    $startArguments = @{
        FilePath = $physicalInstaller; ArgumentList = $line; WorkingDirectory = $physicalCache
        WindowStyle = 'Hidden'; Wait = $true; PassThru = $true; ErrorAction = 'Stop'
    }
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    try {
        $principal = [Security.Principal.WindowsPrincipal]::new($identity)
        if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
            $startArguments.Verb = 'RunAs'
        }
    } finally { $identity.Dispose() }
    try { $process = Start-Process @startArguments }
    catch {
        # Start-Process can wrap Win32Exception in InvalidOperationException.
        $failure = $_.Exception
        while ($failure) {
            if ($failure -is [ComponentModel.Win32Exception] -and $failure.NativeErrorCode -eq 1223) {
                throw 'Windows administrator approval was declined. The portable version was not activated.'
            }
            $failure = $failure.InnerException
        }
        throw
    }
    try { $code = $process.ExitCode }
    finally { $process.Dispose() }
    if ($code -eq 3010 -or $code -eq 1641) {
        throw 'Microsoft Build Tools require a Windows restart. Run the installer again afterward; the portable version was not activated.'
    }
    if ($code -eq 1602) { throw 'Microsoft Build Tools installation was cancelled. The portable version was not activated.' }
    if ($code -ne 0) { throw "Microsoft Build Tools installation failed (exit code $code)." }
}

function Enable-PortableMsvc([string] $Root) {
    $base = Get-PortableToolRoot $Root
    $snapshot = [Environment]::GetEnvironmentVariables('Process')
    foreach ($instance in @(Get-PortableVisualStudioInstances)) {
        try {
            Test-PortableMsvc $base $instance.installationPath
            Write-Host "MSVC and Windows SDK verified: $($instance.installationPath)"
            return [IO.Path]::GetFullPath($instance.installationPath)
        } catch {
            Restore-PortableProcessEnvironment $snapshot
            Write-Host "Build Tools candidate is not usable: $($_.Exception.Message)"
        }
    }
    Install-PortableMsvc $base
    foreach ($instance in @(Get-PortableVisualStudioInstances)) {
        try {
            Test-PortableMsvc $base $instance.installationPath
            Write-Host "MSVC and Windows SDK verified: $($instance.installationPath)"
            return [IO.Path]::GetFullPath($instance.installationPath)
        } catch {
            Restore-PortableProcessEnvironment $snapshot
            Write-Host "Build Tools candidate is not usable: $($_.Exception.Message)"
        }
    }
    throw 'Microsoft Build Tools were installed, but the MSVC x64/Windows SDK link test failed. See the installation log for details.'
}
