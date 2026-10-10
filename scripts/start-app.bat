@echo off
setlocal EnableExtensions DisableDelayedExpansion
set "APP_PROFILE="
set "APP_PROFILE_KEY="
set "APP_BUILD_ARGS="
set "APP_PUSHED="
set "APP_LAUNCH_ONLY="
set "APP_CHECK_ONLY="
set "APP_BUILD_ONLY="
set "APP_NO_PAUSE="
set "APP_KEEP_OPEN="

if /i "%~2"=="--check" set "APP_NO_PAUSE=1"
if /i "%~2"=="--build-only" set "APP_NO_PAUSE=1"
if /i "%~1"=="debug" (
    set "APP_PROFILE=Debug"
    set "APP_PROFILE_KEY=debug"
)
if /i "%~1"=="release" (
    set "APP_PROFILE=Release"
    set "APP_PROFILE_KEY=release"
    set "APP_BUILD_ARGS=--release"
)
if /i "%~1"=="build-release" (
    set "APP_PROFILE=Release"
    set "APP_PROFILE_KEY=release"
    set "APP_BUILD_ARGS=--release"
    set "APP_BUILD_ONLY=1"
    set "APP_KEEP_OPEN=1"
    set "APP_NO_PAUSE="
)
if /i "%~1"=="launch" (
    set "APP_PROFILE=Release"
    set "APP_PROFILE_KEY=release"
    set "APP_LAUNCH_ONLY=1"
)
if not defined APP_PROFILE goto invalid_profile
if not "%~3"=="" goto invalid_options
if defined APP_KEEP_OPEN (
    if not [%2]==[] goto invalid_options
    goto selected
)
if "%~2"=="" goto selected
if defined APP_LAUNCH_ONLY (
    if /i not "%~2"=="--check" goto invalid_options
    set "APP_CHECK_ONLY=1"
) else (
    if /i not "%~2"=="--build-only" goto invalid_options
    set "APP_BUILD_ONLY=1"
)

:selected
set "APP_STEP=opening the repository directory"
pushd "%~dp0.."
set "APP_EXIT_CODE=%errorlevel%"
if not "%APP_EXIT_CODE%"=="0" goto failed
set "APP_PUSHED=1"

if defined APP_LAUNCH_ONLY goto verify_profile

where uv >nul 2>nul
if not "%errorlevel%"=="0" goto missing_uv

echo Preparing the %APP_PROFILE% build of Flitzi's Looper.
echo Close other Looper windows before rebuilding or switching profiles.
echo.

set "APP_STEP=dependency setup"
call uv sync --locked
set "APP_EXIT_CODE=%errorlevel%"
if not "%APP_EXIT_CODE%"=="0" goto failed

set "APP_STEP=%APP_PROFILE% native build"
call uv run --no-sync maturin develop --locked %APP_BUILD_ARGS%
set "APP_EXIT_CODE=%errorlevel%"
if not "%APP_EXIT_CODE%"=="0" goto failed

:verify_profile
if not exist ".venv\Scripts\python.exe" goto missing_environment
set "APP_STEP=checking the installed %APP_PROFILE% native build"
call ".venv\Scripts\python.exe" -c "import sys; import flitzis_looper_audio as native; profile = getattr(native, 'native_build_profile', lambda: 'unknown')(); print('Native build profile: ' + profile); sys.exit(0 if profile == sys.argv[1] else 3)" "%APP_PROFILE_KEY%"
set "APP_EXIT_CODE=%errorlevel%"
if not "%APP_EXIT_CODE%"=="0" goto profile_failed
if defined APP_CHECK_ONLY (
    echo Release startup check passed. The app was not started.
    goto succeeded
)
if defined APP_BUILD_ONLY (
    echo %APP_PROFILE% build completed. The app was not started.
    goto succeeded
)

echo.
echo Starting Flitzi's Looper with the %APP_PROFILE% build.
set "APP_STEP=application startup or execution"
call ".venv\Scripts\python.exe" -m flitzis_looper
set "APP_EXIT_CODE=%errorlevel%"
if not "%APP_EXIT_CODE%"=="0" goto failed

:succeeded
if defined APP_KEEP_OPEN pause
popd
exit /b 0

:invalid_profile
set "APP_STEP=profile selection; use start.bat, start-dev.bat, start-release.bat or build-release.bat"
set "APP_EXIT_CODE=2"
goto failed

:invalid_options
echo Usage: start.bat [--check]
echo        start-dev.bat [--build-only]
echo        start-release.bat [--build-only]
echo        build-release.bat
set "APP_STEP=argument validation"
set "APP_EXIT_CODE=2"
goto failed

:missing_environment
set "APP_STEP=locating the existing project Python environment"
set "APP_EXIT_CODE=3"
goto profile_failed

:profile_failed
echo ERROR: A usable %APP_PROFILE% native build is required.
echo A missing profile getter also requires a current build.
if /i "%APP_PROFILE_KEY%"=="release" (
    echo Run start-release.bat --build-only once, then use start.bat.
) else (
    echo Run start-dev.bat --build-only to rebuild the Debug extension.
)
goto failed

:missing_uv
echo ERROR: uv was not found on PATH. Install uv before using these launchers.
set "APP_STEP=locating uv"
set "APP_EXIT_CODE=127"

:failed
echo.
echo ERROR: %APP_STEP% failed (exit code %APP_EXIT_CODE%).
if not defined APP_NO_PAUSE pause
if defined APP_PUSHED popd
exit /b %APP_EXIT_CODE%
