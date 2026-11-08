@echo off
setlocal
echo Installing Any2Odyssey desktop controls only. No Odyssey Hub or administrator access is required. 3D conversion will be unavailable.
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -ExecutionPolicy Bypass -File "%~dp0Install-Launcher.ps1" -ControlsOnly
set "ANY2ODYSSEY_SETUP_EXIT=%ERRORLEVEL%"
pause
exit /b %ANY2ODYSSEY_SETUP_EXIT%
