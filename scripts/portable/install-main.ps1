param(
    [Parameter(Mandatory = $true)][string] $Root,
    [Parameter(Mandatory = $true)][string] $Source,
    [Parameter(Mandatory = $true)][string] $Git,
    [Parameter(Mandatory = $true)][string] $Commit,
    [Parameter(Mandatory = $true)][string] $RuntimeDirectory,
    [Parameter(Mandatory = $true)][string] $OperationId
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
. (Join-Path $PSScriptRoot 'common.ps1')
. (Join-Path $PSScriptRoot 'toolchain.ps1')
. (Join-Path $PSScriptRoot 'build-path.ps1')
$buildDrive = $null

function Get-VerifiedArchive([string] $Name, [string] $Url, [string] $Hash) {
    $directory = Join-Path $Root "tools/$Name"
    Assert-PlainPath $directory
    if (-not (Test-Path -LiteralPath (Join-Path $directory 'verified.json'))) {
        $archive = Join-Path $Root "cache/$Name.zip"
        Read-Payload ([Uri]$Url) $archive $Hash
        Assert-Hash $archive $Hash
        $directory = "$directory-$([Guid]::NewGuid().ToString('N'))"
        [IO.Directory]::CreateDirectory($directory) | Out-Null
        Expand-ContainedZip $archive $directory
        Write-AtomicJson (Join-Path $directory 'verified.json') @{sha256=$Hash}
        $canonical = Join-Path $Root "tools/$Name"
        if (-not (Test-Path -LiteralPath $canonical)) { [IO.Directory]::Move($directory, $canonical); $directory = $canonical }
        Assert-PlainPath $archive
        Remove-Item -LiteralPath $archive
    }
    return $directory
}

function Get-PortableVcpkg {
    $pin = '020c8202737fb3ae3bfef5dd78ca537137be0ea0'
    $directory = Join-Path $Root "tools/vcpkg-$($pin.Substring(0,12))"
    Assert-PlainPath $directory
    if (-not (Test-Path -LiteralPath $directory)) {
        # Git canonicalizes the build drive; avoid its internal gitdir limit.
        $fresh = Join-Path $Root "tools/v-$([Guid]::NewGuid().ToString('N').Substring(0,8))"
        Invoke-Checked $Git @('-c','init.templateDir=','clone','--filter=blob:none','--no-checkout','https://github.com/microsoft/vcpkg.git',$fresh) | Out-Host
        Invoke-Checked $Git @('-C',$fresh,'fetch','--depth','1','origin',$pin) | Out-Host
        Invoke-Checked $Git @('-C',$fresh,'checkout','--detach',$pin) | Out-Host
        [IO.Directory]::Move($fresh, $directory)
    }
    $actual = & $Git -C $directory rev-parse HEAD
    if ($LASTEXITCODE -ne 0 -or $actual -ne $pin) { throw 'The private vcpkg version does not match the verified revision.' }
    $env:VCPKG_ROOT = $directory
    $env:VCPKG_DOWNLOADS = Join-Path $Root 'cache/vcpkg-downloads'
    $env:VCPKG_DEFAULT_BINARY_CACHE = Join-Path $Root 'cache/vcpkg-binaries'
    $env:VCPKG_BINARY_SOURCES = 'clear;default,readwrite'
    $env:VCPKG_DISABLE_METRICS = '1'
    foreach ($path in @($env:VCPKG_DOWNLOADS, (Join-Path $Root 'cache/vcpkg-binaries'))) {
        Assert-PlainPath $path
        [IO.Directory]::CreateDirectory($path) | Out-Null
    }
    $executable = Join-Path $directory 'vcpkg.exe'
    $usable = $false
    if (Test-Path -LiteralPath $executable) {
        & $executable version | Out-Host
        $usable = $LASTEXITCODE -eq 0
    }
    if (-not $usable) {
        Invoke-Checked (Join-Path $directory 'bootstrap-vcpkg.bat') @('-disableMetrics') | Out-Host
    }
    Invoke-Checked $executable @('install','rubberband:x64-windows','--disable-metrics') | Out-Host
    return $directory
}

function Get-RuntimeLicenses([string] $Vcpkg, [string] $Ffmpeg) {
    $directory = Join-Path $Root "cache/licenses-$($OperationId.Substring(0,8))"
    [IO.Directory]::CreateDirectory($directory) | Out-Null
    $urls = [ordered]@{
        'antlr-4.9.3.txt' = 'https://raw.githubusercontent.com/antlr/antlr4/4.9.3/LICENSE.txt'
        'onnxruntime-1.24.2.txt' = 'https://raw.githubusercontent.com/microsoft/onnxruntime/v1.24.2/LICENSE'
        'onnxruntime-1.24.2-notices.txt' = 'https://raw.githubusercontent.com/microsoft/onnxruntime/v1.24.2/ThirdPartyNotices.txt'
        'directml-1.15.4.html' = 'https://www.nuget.org/packages/Microsoft.AI.DirectML/1.15.4/License'
        'msvc-2026.docx' = 'https://visualstudio.microsoft.com/wp-content/uploads/2025/10/Visual-C-V14-License-Redistributable_and_Runtime_ENU.docx'
    }
    foreach ($entry in $urls.GetEnumerator()) { Read-Payload ([Uri]$entry.Value) (Join-Path $directory $entry.Key) }
    foreach ($entry in @(@('rubberband','rubberband.txt'), @('libsamplerate','samplerate.txt'), @('sleef','sleef.txt'))) {
        [IO.File]::Copy((Join-Path $Vcpkg "installed/x64-windows/share/$($entry[0])/copyright"), (Join-Path $directory $entry[1]))
    }
    [IO.File]::Copy((Join-Path $Ffmpeg 'LICENSE'), (Join-Path $directory 'ffmpeg.txt'))
    [IO.File]::Copy((Join-Path $Ffmpeg 'README.txt'), (Join-Path $directory 'ffmpeg-README.txt'))
    $urls['ffmpeg.txt'] = 'https://github.com/GyanD/codexffmpeg/releases/download/8.1.1/ffmpeg-8.1.1-full_build-shared.zip'
    $urls['vcpkg'] = 'https://github.com/microsoft/vcpkg/tree/020c8202737fb3ae3bfef5dd78ca537137be0ea0'
    Write-AtomicJson (Join-Path $directory 'source-urls.json') $urls
    return $directory
}

try {
    $Root = [IO.Path]::GetFullPath($Root).TrimEnd('\')
    $Source = [IO.Path]::GetFullPath($Source)
    $RuntimeDirectory = [IO.Path]::GetFullPath($RuntimeDirectory)
    $paths = Get-PortablePaths $Root $OperationId
    foreach ($path in @($Root,$Source,$RuntimeDirectory)) { Assert-PlainPath $path }
    $prefix = $Root + '\'
    if (-not $Source.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase) -or
        $Source -ne $paths.source -or $RuntimeDirectory -ne $paths.candidate -or
        (Test-Path -LiteralPath $RuntimeDirectory) -or $Commit -notmatch '^[a-f0-9]{40}$') { throw 'Invalid private build paths or source identity.' }
    $marker = Get-Content -LiteralPath (Join-Path $Root 'portable-root.json') -Raw | ConvertFrom-Json
    if ($marker.app -ne 'flitzis-looper' -or $marker.schema_version -ne 1) { throw 'The build requires its own portable installation.' }
    $actual = & $Git -C $Source rev-parse HEAD
    if ($LASTEXITCODE -ne 0 -or $actual -ne $Commit) { throw 'The build source revision was modified.' }
    # MSVC/CMake still reject long paths; a temporary DOS drive keeps all files
    # in the chosen root while giving build tools short ASCII paths.
    if ($Root -match '^[A-Za-z]:$') {
        $Root += '\'
    } else {
        $buildDrive = New-PortableBuildDrive $Root
        $Source = $buildDrive.Path + $Source.Substring($prefix.Length)
        $RuntimeDirectory = $buildDrive.Path + $RuntimeDirectory.Substring($prefix.Length)
        if ($Git.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
            $Git = $buildDrive.Path + $Git.Substring($prefix.Length)
        }
        $Root = $buildDrive.Path
    }
    # Inherited developer overrides must never redirect a beginner's private build.
    foreach ($item in @(Get-ChildItem Env: | Where-Object { $_.Name -match '^(UV_|CARGO_|RUSTUP_|RUSTFLAGS$|RUSTC(?:_|$)|RUSTDOC(?:_|$)|PYO3_|MATURIN_|PYTHON|VIRTUAL_ENV$|ORT_|VCPKG_|RUBBERBAND_|PKG_CONFIG_|CL$|_CL_$|LINK$|_LINK_$)' })) {
        Remove-Item -LiteralPath ('Env:' + $item.Name)
    }
    $env:UV_CACHE_DIR = Join-Path $Root 'cache/uv'
    $env:UV_PYTHON_INSTALL_DIR = Join-Path $Root 'tools/python'
    $env:UV_TOOL_DIR = Join-Path $Root 'tools/uv-tools'
    $shortOperation = $OperationId.Substring(0,8)
    $ownedDirectories = @("cache/env-$shortOperation", "cache/wheels-$shortOperation",
        "cache/licenses-$shortOperation", "cache/bundle-$shortOperation")
    foreach ($relative in $ownedDirectories) {
        $path = Join-Path $Root $relative
        Assert-PlainPath $path
        if (Test-Path -LiteralPath $path) { throw "Build staging conflict: $path" }
    }
    Write-AtomicJson (Join-Path $Root "cache/build-owned-$OperationId.json") @{
        schema_version=1; operation=$OperationId; directories=$ownedDirectories
    }
    $env:UV_PROJECT_ENVIRONMENT = Join-Path $Root "cache/env-$shortOperation"
    $env:UV_LINK_MODE = 'copy'
    $env:UV_PYTHON_PREFERENCE = 'only-managed'
    $env:UV_NO_PROGRESS = '1'
    # Cargo can embed canonical physical dependency paths in rlibs despite SUBST.
    # Keep the owned target directory short enough for MSVC LINK after relocation.
    $env:CARGO_TARGET_DIR = Join-Path $Root 'b'
    $env:ORT_CACHE_DIR = Join-Path $Root 'cache/ort'
    $env:TEMP = Join-Path $Root 'cache/temp'
    $env:TMP = $env:TEMP
    foreach ($path in @($env:UV_CACHE_DIR,$env:UV_PYTHON_INSTALL_DIR,$env:UV_TOOL_DIR,$env:UV_PROJECT_ENVIRONMENT,$env:CARGO_TARGET_DIR,$env:ORT_CACHE_DIR,$env:TEMP)) {
        Assert-PlainPath $path
        [IO.Directory]::CreateDirectory($path) | Out-Null
    }
    $env:PATH = [IO.Path]::GetDirectoryName($Git) + ';' + $env:PATH
    Write-Host 'Checking C++ Build Tools and Windows SDK ...'
    $visualStudio = Enable-PortableMsvc $Root
    $env:VCPKG_VISUAL_STUDIO_PATH = $visualStudio
    Write-Host 'Checking private Python and Rust tools ...'
    $uv = Get-PortableUv $Root
    $cargo = Get-PortableRust $Root
    Invoke-Checked $uv @('python','install','3.14.2','--no-bin','--no-registry') | Out-Host
    Write-Host 'Installing the pinned Python dependencies ...'
    Push-Location -LiteralPath $Source
    try {
        Invoke-Checked $uv @('sync','--locked','--python','3.14.2','--no-install-project','--no-editable') | Out-Host
        Write-Host 'Preparing RubberBand and its private libraries ...'
        $vcpkg = Get-PortableVcpkg
        $wheelDirectory = Join-Path $Root "cache/wheels-$shortOperation"
        [IO.Directory]::CreateDirectory($wheelDirectory) | Out-Null
        Write-Host 'Building the native audio module in the Release profile ...'
        Invoke-Checked $uv @('run','--no-sync','maturin','build','--locked','--release','--strip','--interpreter',(Join-Path $env:UV_PROJECT_ENVIRONMENT 'Scripts/python.exe'),'--out',$wheelDirectory) | Out-Host
        $wheels = @(Get-ChildItem -LiteralPath $wheelDirectory -Filter '*.whl')
        if ($wheels.Count -ne 1) { throw 'The build must produce exactly one Release wheel.' }
        Write-Host 'Preparing FFmpeg and the standalone runtime ...'
        $ffmpegArchive = Get-VerifiedArchive 'ffmpeg-8.1.1' 'https://github.com/GyanD/codexffmpeg/releases/download/8.1.1/ffmpeg-8.1.1-full_build-shared.zip' '4296b396bdfd5fbc3dfc75ab4c8703354a56963232d65c4182993543df2d2f45'
        $ffmpeg = Join-Path $ffmpegArchive 'ffmpeg-8.1.1-full_build-shared'
        $licenses = Get-RuntimeLicenses $vcpkg $ffmpeg
        $redistributables = @(Get-ChildItem -LiteralPath (Join-Path $visualStudio 'VC/Redist/MSVC') -Directory |
            Where-Object { $_.Name -match '^\d+\.\d+\.\d+(?:\.\d+)?$' } |
            Sort-Object { [version]$_.Name } -Descending)
        if ($redistributables.Count -eq 0) { throw 'The MSVC installation contains no versioned x64 runtime.' }
        $redist = $redistributables[0].FullName
        $dllDirectories = @((Join-Path $vcpkg 'installed/x64-windows/bin'), (Join-Path $env:CARGO_TARGET_DIR 'release'))
        $dllDirectories += @(Get-ChildItem -LiteralPath (Join-Path $redist 'x64') -Directory | Where-Object { $_.Name -match '^Microsoft\.VC\d+\.(CRT|OpenMP)$' } | ForEach-Object { $_.FullName })
        $arguments = @('run','--no-sync','python','-B',(Join-Path $Source 'scripts/portable/build_bundle.py'),'--native-wheel',$wheels[0].FullName,'--native-source-commit',$Commit,'--ffmpeg-dir',(Join-Path $ffmpeg 'bin'),'--output',(Join-Path $Root "cache/bundle-$shortOperation"),'--runtime-directory',$RuntimeDirectory,'--checkout-directory',$Source,'--root',$Root)
        foreach ($directory in $dllDirectories) { $arguments += @('--dll-dir',$directory) }
        foreach ($license in Get-ChildItem -LiteralPath $licenses -File) { $arguments += @('--license-file',($license.Name + '=' + $license.FullName)) }
        Invoke-Checked $uv $arguments | Out-Host
    } finally { Pop-Location }
    exit 0
} catch {
    Write-Host "Portable build failed: $($_.Exception.Message)" -ForegroundColor Red
    exit 1
} finally {
    if ($buildDrive) { Remove-PortableBuildDrive $buildDrive | Out-Null }
}
