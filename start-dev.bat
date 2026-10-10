@echo off
setlocal EnableExtensions DisableDelayedExpansion
call "%~dp0scripts\start-app.bat" debug %*
exit /b %errorlevel%
