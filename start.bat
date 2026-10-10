@echo off
setlocal EnableExtensions DisableDelayedExpansion
call "%~dp0scripts\start-app.bat" launch %*
exit /b %errorlevel%
