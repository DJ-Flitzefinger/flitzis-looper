@echo off
setlocal EnableExtensions DisableDelayedExpansion
call "%~dp0scripts\start-app.bat" build-release %*
exit /b %errorlevel%
