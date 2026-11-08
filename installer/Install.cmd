@echo off
setlocal
echo Checking Any2Odyssey setup. For full installation, approve elevation using the same Windows account.
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File "%~dp0Install-Launcher.ps1"
set "ANY2ODYSSEY_SETUP_EXIT=%ERRORLEVEL%"
pause
exit /b %ANY2ODYSSEY_SETUP_EXIT%
