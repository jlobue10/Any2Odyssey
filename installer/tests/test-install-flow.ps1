# Full-install flow against isolated folders and an isolated registry key. Windows only.
# Only Samsung compatibility, elevation, running-process discovery and the quit request to a
# running instance are simulated; copy/registration/shortcuts run for real inside the temporary folders.
param([string]$PreviousPackage)
$ErrorActionPreference='Stop'
# install.ps1 refuses Windows builds before 24H2; the fixture is about files, not the OS version.
$previousSkip=$env:ANY2ODYSSEY_SKIP_OS_CHECK;$env:ANY2ODYSSEY_SKIP_OS_CHECK='1'
$repo=Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$installerDir=Join-Path $repo 'installer'
# The folder name carries Cyrillic, Polish and CJK letters (written as code points to keep this file
# ASCII): a profile folder outside the ANSI code page broke shortcut creation through WScript.Shell.
$root=Join-Path ([IO.Path]::GetTempPath()) ('Any2Odyssey-flow-test-'+[char]0x0416+[char]0x0142+[char]0x4E2D+'-'+[Guid]::NewGuid().ToString('N'))
$package=Join-Path $root 'package';$local=Join-Path $root 'local';$engine=Join-Path $root 'engine';$desktop=Join-Path $root 'desktop';$programs=Join-Path $root 'programs'
$registration='HKCU:\Software\Any2OdysseyInstallerTests\'+[Guid]::NewGuid().ToString('N')
New-Item -ItemType Directory -Path $package,$local,$engine,$desktop,$programs,(Join-Path $package 'build'),(Join-Path $package 'assets') | Out-Null
Copy-Item -LiteralPath (Join-Path $repo 'build\Any2Odyssey.exe') -Destination (Join-Path $package 'build')
foreach($file in @('VERSION','LICENSE','NOTICE','THIRD-PARTY-LICENSES.md','USER-GUIDE.html')){Copy-Item -LiteralPath (Join-Path $repo $file) -Destination $package}
foreach($file in @('InstallSupport.ps1','Uninstall.ps1','UninstallWorker.ps1')){Copy-Item -LiteralPath (Join-Path $installerDir $file) -Destination $package}
Copy-Item -LiteralPath (Join-Path $repo 'assets\Any2Odyssey.ico') -Destination (Join-Path $package 'assets')
$script=Get-Content -LiteralPath (Join-Path $installerDir 'install.ps1') -Raw
$script=$script.Replace('$env:LOCALAPPDATA',("'"+$local+"'"))
$script=$script.Replace('C:\Program Files\Odyssey 3D Hub\Conversion',$engine)
$script=$script.Replace('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Any2Odyssey',$registration)
$script=$script.Replace("[Environment]::GetFolderPath('Desktop')",("'"+$desktop+"'")).Replace("[Environment]::GetFolderPath('Programs')",("'"+$programs+"'"))
$script=$script.Replace("[Environment]::GetFolderPath('ProgramFiles')",("'"+(Join-Path $root 'programfiles')+"'"))
$script=$script.Replace('if(-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))','if($false)')
if($script.Contains('C:\Program Files\Odyssey 3D Hub') -or $script.Contains('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall') -or $script.Contains('$env:LOCALAPPDATA') -or $script.Contains('[Environment]::GetFolderPath') -or $script.Contains('IsInRole') -or $script.Contains('Any2OdysseyQuit')){throw 'Fixture routing incomplete; refusing to run.'}
$testInstall=Join-Path $package 'install.ps1';Set-Content -LiteralPath $testInstall -Value $script -Encoding UTF8
# The same installer for an account whose Desktop folder is gone: Windows reports that as an empty path.
$noDesktopScript=$script.Replace(("{Name='Desktop';Path='"+$desktop+"'}"),"{Name='Desktop';Path=''}")
if($noDesktopScript -ceq $script){throw 'Fixture routing incomplete; refusing to run.'}
$noDesktopInstall=Join-Path $package 'install-no-desktop.ps1';Set-Content -LiteralPath $noDesktopInstall -Value $noDesktopScript -Encoding UTF8
# Running processes are simulated: Get-Process reports what $processes.List holds, so no real
# instance is consulted. install.ps1 asks an instance to quit by starting the packaged executable
# with --quit; the Start-Process stub records that request and lets the simulated instance exit.
$processes=[pscustomobject]@{List=@()}
$thisSession=[Diagnostics.Process]::GetCurrentProcess().SessionId
function Get-Process {param($Name,$ErrorAction) return @($processes.List | Where-Object {@($Name) -contains $_.Name})}
$quitRequests=New-Object 'System.Collections.Generic.List[object]'
function Start-Process {
 param($FilePath,$ArgumentList,$WindowStyle,$Verb,[switch]$Wait,[switch]$PassThru)
 $quitRequests.Add([pscustomobject]@{FilePath=$FilePath;ArgumentList=[string]$ArgumentList})
 $processes.List=@()
 return [pscustomobject]@{ExitCode=0}
}
function Get-FileHash {
 param([string]$LiteralPath,$Algorithm)
 if($LiteralPath -eq (Join-Path $engine 'ConversionWrapper.dll')){return [pscustomobject]@{Hash='0116B33697CA0192FBE26DA96CB9E2592599C0CACFBB1129AE03E445CB3A840B'}}
 Microsoft.PowerShell.Utility\Get-FileHash -LiteralPath $LiteralPath -Algorithm SHA256
}
function Assert($condition,[string]$message){if(-not $condition){throw $message};Write-Host "PASS: $message"}
# Notes the installer prints are recorded here (a function shadowing Write-Host works under 5.1 and 7 alike).
$notes=New-Object 'System.Collections.Generic.List[string]'
function Write-Host {param($Object,$ForegroundColor,$BackgroundColor,[switch]$NoNewline)$text=[string]::Join(' ',@($Object));$notes.Add($text);$Host.UI.WriteLine($text)}
$app=Join-Path $local 'Programs\Any2Odyssey';$data=Join-Path $local 'Any2Odyssey'
New-Item -ItemType Directory -Path $app,$data | Out-Null
Set-Content -LiteralPath (Join-Path $engine 'ConversionWrapper.dll') -Value 'Simulated Samsung engine; never loaded'
Set-Content -LiteralPath (Join-Path $data 'settings.json') -Value '{"Mode":"Active","Depth":1.4,"MinimizeToTray":false}'
$settingsHash=(Get-FileHash -LiteralPath (Join-Path $data 'settings.json')).Hash
$expectedAction='Install'
if($PreviousPackage){
 # An extracted earlier Any2Odyssey release: its exe becomes the installed app and worker.
 Copy-Item -LiteralPath (Join-Path $PreviousPackage 'build\Any2Odyssey.exe') -Destination $app
 Copy-Item -LiteralPath (Join-Path $PreviousPackage 'build\Any2Odyssey.exe') -Destination (Join-Path $engine 'Any2OdysseyWorker.exe')
 $expectedAction='Upgrade'
}
# Staging folders are named Any2Odyssey-install-<guid>; a run must leave the set in %TEMP% unchanged.
$stagingNames={@(Get-ChildItem -LiteralPath ([IO.Path]::GetTempPath()) -Directory -Filter 'Any2Odyssey-install-*' | Select-Object -ExpandProperty Name | Sort-Object) -join '|'}
$stagingBefore=& $stagingNames
$passed=$false
try{
 & $testInstall -CheckOnly
 Assert (-not (Test-Path -LiteralPath $registration) -and -not (Test-Path -LiteralPath (Join-Path $app 'version.json'))) 'CheckOnly leaves installation and registration untouched'
 & $testInstall
 Assert ((Get-ItemProperty -LiteralPath $registration).DisplayVersion -eq (Get-Content -LiteralPath (Join-Path $repo 'VERSION') -Raw).Trim()) 'Full install registers the packaged version'
 Assert ((Get-ItemProperty -LiteralPath $registration).DisplayName -eq 'Any2Odyssey') 'Installed apps entry is named Any2Odyssey'
 Assert ((Get-Content -LiteralPath (Join-Path $data 'install-result.json') -Raw | ConvertFrom-Json).Action -eq $expectedAction) "Install record reports $expectedAction"
 Assert ((Get-FileHash -LiteralPath (Join-Path $data 'settings.json')).Hash -eq $settingsHash) 'Full installer preserves existing user preferences'
 Assert ((Get-FileHash -LiteralPath (Join-Path $engine 'Any2OdysseyWorker.exe')).Hash -eq (Get-FileHash -LiteralPath (Join-Path $repo 'build\Any2Odyssey.exe')).Hash) 'Worker beside the engine is the packaged executable'
 Assert (Test-Path -LiteralPath (Join-Path $root 'programfiles\Any2Odyssey\UninstallWorker.ps1')) 'Full install places the privileged helper outside per-user app data'
 $ownerFile=Join-Path $root 'programfiles\Any2Odyssey\UninstallWorker.owner'
 Assert ((Test-Path -LiteralPath $ownerFile) -and (Get-Content -LiteralPath $ownerFile -Raw).Trim() -eq [Security.Principal.WindowsIdentity]::GetCurrent().User.Value) 'Full install marks the helper with the installing account SID'
 Assert (([IO.File]::ReadAllBytes($ownerFile))[0] -eq [byte][char]'S') 'Owner marker is plain ASCII without a byte-order mark'
 Assert ((Test-Path -LiteralPath (Join-Path $app 'NOTICE')) -and (Test-Path -LiteralPath (Join-Path $app 'THIRD-PARTY-LICENSES.md'))) 'Credit and license files are installed'
 . (Join-Path $installerDir 'InstallSupport.ps1')
 Assert ((Get-Any2OdysseyShortcutTarget -Path (Join-Path $desktop 'Any2Odyssey.lnk')) -eq (Join-Path $app 'Any2Odyssey.exe')) 'Desktop shortcut targets the installed executable'
 Assert ((Get-Any2OdysseyShortcutTarget -Path (Join-Path $programs 'Any2Odyssey.lnk')) -eq (Join-Path $app 'Any2Odyssey.exe')) 'Start menu shortcut targets the installed executable'
 New-ItemProperty -LiteralPath $registration -Name EstimatedSize -Value 4096 -PropertyType DWord -Force | Out-Null
 # Shared PC: another account ran Install.cmd in between; the repair must say so and take the marker over.
 [IO.File]::WriteAllText($ownerFile,'S-1-5-21-1-2-3-1001',[Text.UTF8Encoding]::new($false))
 $notes.Clear()
 & $testInstall
 Assert ((Get-Content -LiteralPath (Join-Path $data 'install-result.json') -Raw | ConvertFrom-Json).Action -eq 'Repair') 'Repeated installer repairs the same release'
 Assert (($notes -join [char]10).Contains('installed by another Windows account (S-1-5-21-1-2-3-1001)')) 'Installing over another account''s worker prints a notice'
 Assert ((Get-Content -LiteralPath $ownerFile -Raw).Trim() -eq [Security.Principal.WindowsIdentity]::GetCurrent().User.Value) 'Installing over another account''s installation takes over the owner marker'
 Assert ((Get-ItemProperty -LiteralPath $registration).EstimatedSize -eq 4096) 'Repair keeps registry values the installer does not own'
 Assert ((Get-ItemProperty -LiteralPath $registration).UninstallString -like '*Uninstall.ps1" -Pause') 'Installed-apps uninstall keeps its console open on failure'
 Assert ((Get-ItemProperty -LiteralPath $registration).UninstallString -like '"*\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile *') 'Installed-apps uninstall names the system PowerShell by full path'
 # An instance in another signed-in session uses the worker a full installation replaces and cannot
 # be asked to quit from here: refused in the preflight, before any elevation, naming the session.
 $processes.List=@([pscustomobject]@{Name='Any2OdysseyWorker';SessionId=($thisSession+1)})
 $refusal='';try{& $testInstall -CheckOnly}catch{$refusal=$_.Exception.Message}
 Assert ($refusal.Contains('another signed-in Windows session (session '+($thisSession+1)+')') -and $quitRequests.Count -eq 0) 'An instance in another session stops a full installation in the preflight and is not asked to quit'
 $processes.List=@([pscustomobject]@{Name='Odyssey3DPlayer';SessionId=$thisSession})
 $refusal='';try{& $testInstall -CheckOnly}catch{$refusal=$_.Exception.Message}
 Assert ($refusal.Contains('Samsung player')) 'Samsung''s running player is reported in the preflight'
 # An instance in this session is asked to quit, and the installation continues once it has gone.
 $processes.List=@([pscustomobject]@{Name='Any2Odyssey';SessionId=$thisSession})
 & $testInstall
 Assert ($quitRequests.Count -eq 1 -and $quitRequests[0].ArgumentList -eq '--quit' -and $quitRequests[0].FilePath -eq (Join-Path $package 'build\Any2Odyssey.exe')) 'A running instance in this session is asked to quit through the packaged executable'
 Assert ($processes.List.Count -eq 0 -and (Get-Content -LiteralPath (Join-Path $data 'install-result.json') -Raw | ConvertFrom-Json).Action -eq 'Repair') 'The installation continues after the instance has quit'
 # No Desktop folder: the installation completes and places the Start menu shortcut only.
 Remove-Item -LiteralPath (Join-Path $desktop 'Any2Odyssey.lnk')
 $notes.Clear()
 & $noDesktopInstall
 Assert (-not (Test-Path -LiteralPath (Join-Path $desktop 'Any2Odyssey.lnk')) -and (Test-Path -LiteralPath (Join-Path $programs 'Any2Odyssey.lnk'))) 'An account without a Desktop folder installs with the Start menu shortcut only'
 Assert (($notes -join [char]10).Contains('no Desktop folder')) 'The skipped Desktop shortcut is explained'
 # Shared PC: another account has installed a newer release since. Its worker is the one worker of
 # this PC, so this older package must refuse to replace it, and say whose it is, in the preflight.
 $workerPath=Join-Path $engine 'Any2OdysseyWorker.exe'
 $packageResource=(Get-Content -LiteralPath (Join-Path $repo 'VERSION') -Raw).Trim()+'.0'
 $newerResource='9'+$packageResource.Substring(1)
 # The version resource holds its strings as UTF-16, and ISO-8859-1 maps bytes to characters one to
 # one: the stand-in below is the packaged executable with only those version strings changed.
 $latin1=[Text.Encoding]::GetEncoding(28591)
 $image=$latin1.GetString([IO.File]::ReadAllBytes((Join-Path $repo 'build\Any2Odyssey.exe')))
 $image=$image.Replace($latin1.GetString([Text.Encoding]::Unicode.GetBytes($packageResource)),$latin1.GetString([Text.Encoding]::Unicode.GetBytes($newerResource)))
 [IO.File]::WriteAllBytes($workerPath,$latin1.GetBytes($image))
 Assert ([Diagnostics.FileVersionInfo]::GetVersionInfo($workerPath).FileVersion -eq $newerResource) 'Fixture: the stand-in worker reads as a newer release'
 [IO.File]::WriteAllText($ownerFile,'S-1-5-21-1-2-3-1001',[Text.UTF8Encoding]::new($false))
 $newerHash=(Get-FileHash -LiteralPath $workerPath).Hash
 $refusal='';try{& $testInstall -CheckOnly}catch{$refusal=$_.Exception.Message}
 Assert ($refusal.Contains('newer than this package') -and $refusal.Contains('another Windows account (S-1-5-21-1-2-3-1001)') -and $refusal.Contains('-AllowDowngrade')) 'The preflight refuses to replace a newer worker and names the account that installed it'
 $refusal='';try{& $testInstall}catch{$refusal=$_.Exception.Message}
 Assert ($refusal.Contains('newer than this package') -and (Get-FileHash -LiteralPath $workerPath).Hash -eq $newerHash) 'The installation refuses too and leaves the newer worker in place'
 & $testInstall -AllowDowngrade
 Assert ((Get-FileHash -LiteralPath $workerPath).Hash -eq (Get-FileHash -LiteralPath (Join-Path $repo 'build\Any2Odyssey.exe')).Hash) 'AllowDowngrade replaces the newer worker with the packaged one'
 Assert ((Get-Content -LiteralPath $ownerFile -Raw).Trim() -eq [Security.Principal.WindowsIdentity]::GetCurrent().User.Value) 'The allowed replacement takes over the owner marker'
 Assert ((& $stagingNames) -eq $stagingBefore) 'Installer removes its staging folder'
 $passed=$true
}finally{
 if(Test-Path -LiteralPath $registration){Remove-Item -LiteralPath $registration -Recurse}
 $testKeys=Split-Path -Parent $registration
 if((Test-Path -LiteralPath $testKeys) -and -not (Get-ChildItem -LiteralPath $testKeys | Select-Object -First 1) -and -not (Get-Item -LiteralPath $testKeys).GetValueNames().Count){Remove-Item -LiteralPath $testKeys}
 if($passed){Remove-Item -LiteralPath $root -Recurse -Force}else{Write-Host "Fixtures retained for inspection: $root"}
 $env:ANY2ODYSSEY_SKIP_OS_CHECK=$previousSkip
}

# Assertion failures throw above; a clean run must not inherit a native command's exit code.
exit 0
