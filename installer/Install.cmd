@echo off
rem Delayed expansion (cmd /V:ON, or switched on in the registry) would drop every ! from the
rem folder name in %~dp0, and the package would then seem not to be beside this file.
setlocal DisableDelayedExpansion
rem Double-clicked inside the ZIP preview, Windows copies only this file to a temporary folder.
if not exist "%~dp0Install-Launcher.ps1" goto :notextracted
echo Checking Any2Odyssey setup. A full installation asks for elevation: approve it with this same Windows account, which has to be an administrator account.
rem A 32-bit host (a 32-bit file manager or console) redirects System32 to SysWOW64, whose
rem PowerShell is 32-bit and refused by install.ps1; Sysnative reaches the 64-bit one from there.
set "ANY2ODYSSEY_POWERSHELL=%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"
if defined PROCESSOR_ARCHITEW6432 set "ANY2ODYSSEY_POWERSHELL=%SystemRoot%\Sysnative\WindowsPowerShell\v1.0\powershell.exe"
rem Started from a PowerShell 7 prompt, this console inherits its module path, and Windows PowerShell
rem then cannot load its own modules: Get-FileHash is not found. Cleared, it builds its default.
set "PSModulePath="
rem Windows PowerShell takes [ and ] in the path of a script for a pattern before it looks for a file
rem of that very name: where the pattern fits one other folder it runs the installer found there, and
rem where a [ is not closed it runs nothing and speaks of wildcard characters. So a folder with a [ in
rem its path is asked about first, with no script named on the command line: which file would be run?
set "ANY2ODYSSEY_PACKAGE=%~dp0"
if "%ANY2ODYSSEY_PACKAGE:[=%"=="%ANY2ODYSSEY_PACKAGE%" goto :launch
"%ANY2ODYSSEY_POWERSHELL%" -NoProfile -ExecutionPolicy Bypass -Command "foreach($name in 'Install-Launcher.ps1','install.ps1','InstallSupport.ps1','build\Any2Odyssey.exe'){$path=$env:ANY2ODYSSEY_PACKAGE+$name;if(Test-Path -LiteralPath $path){try{$found=(Get-Command -Name $path -CommandType ExternalScript,Application -ErrorAction Stop).Source;$same=[IO.Path]::GetFullPath($found) -eq [IO.Path]::GetFullPath($path)}catch{exit 1};if(-not $same){exit 1}}};exit 0"
if errorlevel 1 goto :brackets
:launch
rem Arguments are passed on, for example: Install.cmd -AllowDowngrade
"%ANY2ODYSSEY_POWERSHELL%" -NoProfile -ExecutionPolicy Bypass -File "%~dp0Install-Launcher.ps1" %*
set "ANY2ODYSSEY_SETUP_EXIT=%ERRORLEVEL%"
pause
exit /b %ANY2ODYSSEY_SETUP_EXIT%

:brackets
echo This folder has a [ in its path, and Windows PowerShell, which runs the installer, takes [ and ] in a path for a pattern:
echo it would not find the installer in this folder, or would run the one of another folder whose name the pattern fits.
echo Move or extract the whole package to a folder without [ and ] in its path, then run Install.cmd from there.
echo Nothing was installed.
pause
exit /b 1

:notextracted
echo The rest of the Any2Odyssey package is not beside this file.
echo Extract the whole ZIP to a normal folder first, then run Install.cmd from that folder.
echo It cannot be run from inside the ZIP.
pause
exit /b 1
