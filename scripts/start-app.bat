@echo off
setlocal EnableExtensions DisableDelayedExpansion
set "APP_PROFILE="
set "APP_BUILD_ARGS="
set "APP_PUSHED="

if /i "%~1"=="debug" set "APP_PROFILE=Debug"
if /i "%~1"=="release" (
    set "APP_PROFILE=Release"
    set "APP_BUILD_ARGS=--release"
)
if not defined APP_PROFILE goto invalid_profile

set "APP_STEP=opening the repository directory"
pushd "%~dp0.."
set "APP_EXIT_CODE=%errorlevel%"
if not "%APP_EXIT_CODE%"=="0" goto failed
set "APP_PUSHED=1"

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

echo.
echo Starting Flitzi's Looper with the %APP_PROFILE% build.
set "APP_STEP=application startup or execution"
call uv run --no-sync python -m flitzis_looper
set "APP_EXIT_CODE=%errorlevel%"
if not "%APP_EXIT_CODE%"=="0" goto failed

popd
exit /b 0

:invalid_profile
set "APP_STEP=profile selection; use start-dev.bat or start-release.bat"
set "APP_EXIT_CODE=2"
goto failed

:missing_uv
echo ERROR: uv was not found on PATH. Install uv before using these launchers.
set "APP_STEP=locating uv"
set "APP_EXIT_CODE=127"

:failed
echo.
echo ERROR: %APP_STEP% failed (exit code %APP_EXIT_CODE%).
pause
if defined APP_PUSHED popd
exit /b %APP_EXIT_CODE%
