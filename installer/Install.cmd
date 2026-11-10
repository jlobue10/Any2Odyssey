@echo off
setlocal
echo Checking Any2Odyssey setup. For full installation, approve elevation using the same Windows account.
rem A 32-bit host (a 32-bit file manager or console) redirects System32 to SysWOW64, whose
rem PowerShell is 32-bit and refused by install.ps1; Sysnative reaches the 64-bit one from there.
set "ANY2ODYSSEY_POWERSHELL=%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"
if defined PROCESSOR_ARCHITEW6432 set "ANY2ODYSSEY_POWERSHELL=%SystemRoot%\Sysnative\WindowsPowerShell\v1.0\powershell.exe"
rem Opened from inside the ZIP, Explorer extracts only this file to a temporary folder.
if not exist "%~dp0Install-Launcher.ps1" (
 echo This file was started from inside the ZIP. Extract the whole ZIP to a folder first, then run it from there.
 pause
 exit /b 1
)
rem Started from a PowerShell 7 prompt, this console inherits its module path, and Windows PowerShell
rem then cannot load its own modules: Get-FileHash is not found. Cleared, it builds its default.
set "PSModulePath="
"%ANY2ODYSSEY_POWERSHELL%" -NoProfile -ExecutionPolicy Bypass -File "%~dp0Install-Launcher.ps1"
set "ANY2ODYSSEY_SETUP_EXIT=%ERRORLEVEL%"
pause
exit /b %ANY2ODYSSEY_SETUP_EXIT%
