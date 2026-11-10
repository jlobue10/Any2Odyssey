@echo off
setlocal
echo Installing Any2Odyssey desktop controls only. No Odyssey Hub or administrator access is required. 3D conversion will be unavailable.
rem A 32-bit host (a 32-bit file manager or console) redirects System32 to SysWOW64, whose
rem PowerShell is 32-bit and refused by install.ps1; Sysnative reaches the 64-bit one from there.
set "ANY2ODYSSEY_POWERSHELL=%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"
if defined PROCESSOR_ARCHITEW6432 set "ANY2ODYSSEY_POWERSHELL=%SystemRoot%\Sysnative\WindowsPowerShell\v1.0\powershell.exe"
rem Arguments are passed on, for example: Install-GUI.cmd -AllowDowngrade
"%ANY2ODYSSEY_POWERSHELL%" -NoProfile -ExecutionPolicy Bypass -File "%~dp0Install-Launcher.ps1" -ControlsOnly %*
set "ANY2ODYSSEY_SETUP_EXIT=%ERRORLEVEL%"
pause
exit /b %ANY2ODYSSEY_SETUP_EXIT%
