@echo off
setlocal EnableExtensions DisableDelayedExpansion
call "%~dp0scripts\start-app.bat" release
exit /b %errorlevel%
