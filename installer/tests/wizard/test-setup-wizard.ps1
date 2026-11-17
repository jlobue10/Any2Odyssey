# The setup program's own decisions, with a stand-in package: Any2Odyssey.iss is compiled around the
# real Install-Launcher.ps1 and a stand-in install.ps1 that writes only into this test's temporary
# folder, and the result is run silently with /CONTROLSONLY. Nothing is installed, nothing asks for
# elevation (the stand-in refuses anything but a controls-only run, so the launcher cannot reach its
# elevation step) and no window opens. It is safe on a PC where Any2Odyssey is installed.
#
# It needs Inno Setup's compiler (ISCC.exe: -Iscc, the ISCC environment variable, or one of its usual
# folders), which is why it is not one of the test-*.ps1 files in the folder above. Without it the
# test says so and is skipped; with -RequireIscc (the workflows) that is a failure.
[CmdletBinding()]
param([string]$Iscc=$env:ISCC,[switch]$RequireIscc)
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))
$installerDir=Join-Path $repo 'installer'
if(-not $Iscc){
 $candidates=@()
 foreach($folder in @(${env:ProgramFiles(x86)},$env:ProgramFiles,$(if($env:LOCALAPPDATA){Join-Path $env:LOCALAPPDATA 'Programs'}))){
  if($folder){$candidates+=Join-Path $folder 'Inno Setup 6\ISCC.exe'}
 }
 $Iscc=$candidates | Where-Object {Test-Path -LiteralPath $_ -PathType Leaf} | Select-Object -First 1
 if(-not $Iscc){$onPath=Get-Command ISCC.exe -ErrorAction SilentlyContinue;if($onPath){$Iscc=$onPath.Source}}
}
if(-not $Iscc -or -not (Test-Path -LiteralPath $Iscc -PathType Leaf)){
 if($RequireIscc){throw 'Inno Setup''s compiler (ISCC.exe) was not found, and -RequireIscc was given.'}
 Write-Host 'SKIPPED: test-setup-wizard.ps1 needs Inno Setup''s compiler (ISCC.exe). Install Inno Setup 6, or pass -Iscc <path to ISCC.exe>; CI runs this test.'
 exit 0
}
function Assert($condition,[string]$message){if(-not $condition){throw $message};Write-Host "PASS: $message"}
$root=Join-Path ([IO.Path]::GetTempPath()) ('Any2Odyssey-wizard-test-'+[Guid]::NewGuid().ToString('N'))
$package=Join-Path $root 'package';$state=Join-Path $root 'state'
New-Item -ItemType Directory -Path (Join-Path $package 'assets'),$state | Out-Null
Copy-Item -LiteralPath (Join-Path $installerDir 'Install-Launcher.ps1') -Destination $package
Copy-Item -LiteralPath (Join-Path $repo 'assets\Any2Odyssey.ico') -Destination (Join-Path $package 'assets')
[IO.File]::WriteAllText((Join-Path $package 'VERSION'),"0.0.1`n")
# The stand-in installer. What it does is chosen by case.txt in the folder the environment variable
# names; each call is recorded in calls.txt there, and "installing" is one more file in that folder.
$standIn=@'
[CmdletBinding()]
param([switch]$CheckOnly,[switch]$AllowDowngrade,[switch]$ControlsOnly)
$ErrorActionPreference='Stop'
$state=$env:ANY2ODYSSEY_WIZARD_TEST_STATE
if(-not $state -or -not (Test-Path -LiteralPath $state -PathType Container)){throw 'Stand-in installer: no test folder. Nothing was done.'}
if(-not $ControlsOnly){throw 'Stand-in installer: only a controls-only run is expected. Nothing was done.'}
# Get-FileHash comes from a module, which Windows PowerShell cannot load with PowerShell 7's module path.
$hashed=[bool](Get-FileHash -LiteralPath $PSCommandPath)
[IO.File]::AppendAllText((Join-Path $state 'calls.txt'),"CheckOnly=$([bool]$CheckOnly) AllowDowngrade=$([bool]$AllowDowngrade) Hashed=$hashed Bits=$([IntPtr]::Size*8)`r`n")
$case=[IO.File]::ReadAllText((Join-Path $state 'case.txt')).Trim()
Write-Host ('Install Any2Odyssey 0.0.1 for C:\Users\Zo'+[char]0xEB)
Write-Host 'Note: Windows reports no Desktop folder for this account; no shortcut is placed there.'
if($case -eq 'refuse'){throw ('Samsung engine changed (C:\Users\Zo'+[char]0xEB+'). This release needs a compatibility update.')}
if($case -eq 'downgrade' -and -not $AllowDowngrade){throw [IO.File]::ReadAllText((Join-Path $state 'downgrade.txt'),[Text.Encoding]::UTF8)}
if($case -eq 'end-in-checks'){[Environment]::Exit(3)}
if($CheckOnly){Write-Host 'Install preflight passed for controls only. No changes made.';return}
if($case -eq 'end-in-installation'){[Environment]::Exit(3)}
[IO.File]::WriteAllText((Join-Path $state 'installed.txt'),'stand-in')
Write-Host 'Any2Odyssey 0.0.1 install completed.'
'@
[IO.File]::WriteAllText((Join-Path $package 'install.ps1'),$standIn,(New-Object Text.UTF8Encoding $true))
# The refusal of an older package, in the words the real guard uses.
. (Join-Path $installerDir 'InstallSupport.ps1')
$refusal=$null
try{$null=Get-Any2OdysseyInstallAction -PackageVersion ([version]'0.0.1.0') -InstalledVersion ([version]'9.9.9.0')}catch{$refusal=$_.Exception.Message}
[IO.File]::WriteAllText((Join-Path $state 'downgrade.txt'),[string]$refusal,(New-Object Text.UTF8Encoding $false))
$previous=@{State=$env:ANY2ODYSSEY_WIZARD_TEST_STATE;Skip=$env:ANY2ODYSSEY_SKIP_OS_CHECK}
$env:ANY2ODYSSEY_WIZARD_TEST_STATE=$state
# The wizard has the installer's Windows 11 24H2 gate as well; the test is about its logic, not the OS.
$env:ANY2ODYSSEY_SKIP_OS_CHECK='1'
function Invoke-Wizard {
 param([string]$Case,[string[]]$Switches=@())
 foreach($name in @('calls.txt','installed.txt')){$file=Join-Path $state $name;if(Test-Path -LiteralPath $file){Remove-Item -LiteralPath $file}}
 [IO.File]::WriteAllText((Join-Path $state 'case.txt'),$Case)
 $log=Join-Path $root ($Case+'-'+[Guid]::NewGuid().ToString('N')+'.log')
 # /CONTROLSONLY always: with it the launcher installs directly and never asks for elevation.
 $process=Start-Process -FilePath $setup -ArgumentList (@('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/CONTROLSONLY',('/LOG="'+$log+'"'))+$Switches) -PassThru
 # Windows PowerShell reads the exit code of a process it did not wait for only when the handle was taken early.
 $null=$process.Handle
 # Security software can hold a program it has not seen before for half a minute before it starts.
 if(-not $process.WaitForExit(300000)){
  & (Join-Path $env:WINDIR 'System32\taskkill.exe') /PID $process.Id /T /F | Out-Null
  throw "The setup program did not end within five minutes (case '$Case'). Is another Any2Odyssey setup open? Its log, if any: $log"
 }
 $text=if(Test-Path -LiteralPath $log){[IO.File]::ReadAllText($log)}else{''}
 $calls=@();$callFile=Join-Path $state 'calls.txt';if(Test-Path -LiteralPath $callFile){$calls=@(Get-Content -LiteralPath $callFile)}
 Write-Host "--- $Case ($($Switches -join ' ')): exit code $($process.ExitCode), $($calls.Count) installer call(s)"
 return [pscustomobject]@{ExitCode=$process.ExitCode;Log=$text;Calls=$calls;Installed=(Test-Path -LiteralPath (Join-Path $state 'installed.txt'))}
}
function Get-LeftoverPackage {
 return @(Get-ChildItem -LiteralPath ([IO.Path]::GetTempPath()) -Directory -Filter 'is-*.tmp' -ErrorAction SilentlyContinue | Where-Object {Test-Path -LiteralPath (Join-Path $_.FullName 'Any2Odyssey-package\install.ps1')} | Where-Object {(Get-Content -LiteralPath (Join-Path $_.FullName 'Any2Odyssey-package\install.ps1') -Raw).Contains('ANY2ODYSSEY_WIZARD_TEST_STATE')})
}
$passed=$false
try{
 $compiled=Join-Path $root 'compiled'
 $output=& $Iscc '/Q' ('/DPackageDir='+$package) '/DAppVersion=0.0.1' ('/O'+$compiled) (Join-Path $installerDir 'Any2Odyssey.iss') 2>&1 | Out-String
 if($LASTEXITCODE -ne 0){throw "ISCC.exe exited with code $LASTEXITCODE.`n$output"}
 $setup=Join-Path $compiled 'Any2Odyssey-0.0.1-windows-x64-setup.exe'
 Assert (Test-Path -LiteralPath $setup -PathType Leaf) 'Any2Odyssey.iss compiles around the stand-in package'

 $done=Invoke-Wizard -Case 'install'
 Assert ($done.ExitCode -eq 0 -and $done.Installed) 'A silent installation that completes exits with 0'
 Assert ($done.Calls.Count -eq 2 -and $done.Calls[0].StartsWith('CheckOnly=True AllowDowngrade=False') -and $done.Calls[1].StartsWith('CheckOnly=False AllowDowngrade=False')) 'The launcher ran the checks and then the installation, without -AllowDowngrade'
 # Under PowerShell 7 this test hands its module path down to Setup, which has to clear it.
 Assert (@($done.Calls | Where-Object {$_ -match 'Hashed=True Bits=64'}).Count -eq 2) 'The installer ran in 64-bit Windows PowerShell and could load its modules'
 Assert ($done.Log.Contains('Installation completed: Any2Odyssey installation completed.') -and $done.Log.Contains('Installer: Install Any2Odyssey 0.0.1 for C:\Users\Zo'+[char]0xEB)) 'The installer''s lines are in Setup''s log, with characters outside ASCII intact'
 # The checks and the installation both print a notice; the box that shows it afterwards (answered
 # by /SUPPRESSMSGBOXES here, and written to the log) must hold it once.
 $box=[regex]::Match($done.Log,'(?s)message box \(OK\):\r?\n(.*?)\r?\n\d{4}-\d\d-\d\d ')
 Assert ($box.Success -and ([regex]::Matches($box.Groups[1].Value,'Note: Windows reports no Desktop folder')).Count -eq 1) 'A notice printed by the checks and by the installation is shown once'
 Assert ((Get-LeftoverPackage).Count -eq 0) 'Setup removed its unpacked package from the temporary folder'

 $refused=Invoke-Wizard -Case 'refuse'
 Assert ($refused.ExitCode -eq 7 -and -not $refused.Installed -and $refused.Calls.Count -eq 1) 'A refusal of the installer''s checks exits with 7 and installs nothing'
 Assert ($refused.Log.Contains('Failed: Samsung engine changed (C:\Users\Zo'+[char]0xEB+'). This release needs a compatibility update.')) 'The installer''s reason is in Setup''s log, with characters outside ASCII intact'

 $blocked=Invoke-Wizard -Case 'downgrade'
 Assert ($blocked.ExitCode -eq 7 -and -not $blocked.Installed -and @($blocked.Calls | Where-Object {$_ -match 'AllowDowngrade=True'}).Count -eq 0) 'A silent installation of an older package is refused without /ALLOWDOWNGRADE'
 Assert ($blocked.Log.Contains('Downgrade blocked; nothing was changed. To install this older version on purpose, run Setup again with /ALLOWDOWNGRADE.') -and -not $blocked.Log.Contains('Install.cmd')) 'The refusal names the setup program''s switch, not the ZIP''s command'
 $allowed=Invoke-Wizard -Case 'downgrade' -Switches '/ALLOWDOWNGRADE'
 Assert ($allowed.ExitCode -eq 0 -and $allowed.Installed -and $allowed.Calls.Count -eq 2 -and @($allowed.Calls | Where-Object {$_ -match 'AllowDowngrade=True'}).Count -eq 2) '/ALLOWDOWNGRADE reaches the installer''s checks and the installation'

 # PowerShell ended in the middle: closed from Task Manager, a sign-out, security software.
 $early=Invoke-Wizard -Case 'end-in-checks'
 Assert ($early.ExitCode -eq 7 -and $early.Log.Contains('Windows PowerShell ended with code 3 before the installer could report. Nothing was installed.')) 'PowerShell ended during the read-only checks: exit code 7, and nothing was installed'
 $late=Invoke-Wizard -Case 'end-in-installation'
 Assert ($late.ExitCode -eq 7 -and $late.Calls.Count -eq 2) 'PowerShell ended during the installation: exit code 7'
 Assert ($late.Log.Contains('the installation may be incomplete') -and -not $late.Log.Contains('Nothing was installed')) 'Setup then says that the installation may be incomplete, not that nothing was installed'
 Assert ((Get-LeftoverPackage).Count -eq 0) 'Setup removed its unpacked package after the failed runs as well'
 $passed=$true
}finally{
 $env:ANY2ODYSSEY_WIZARD_TEST_STATE=$previous.State;$env:ANY2ODYSSEY_SKIP_OS_CHECK=$previous.Skip
 if($passed){Remove-Item -LiteralPath $root -Recurse -Force}else{Write-Host "Fixtures and logs retained for inspection: $root"}
}
exit 0
