# Full-install flow against isolated folders and an isolated registry key. Windows only.
# Only Samsung compatibility, elevation and running-process discovery are simulated;
# copy/registration/shortcuts run for real inside the temporary folders.
param([string]$PreviousPackage)
$ErrorActionPreference='Stop'
# install.ps1 refuses Windows builds before 24H2; the fixture is about files, not the OS version.
$previousSkip=$env:ANY2ODYSSEY_SKIP_OS_CHECK;$env:ANY2ODYSSEY_SKIP_OS_CHECK='1'
$repo=Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$installerDir=Join-Path $repo 'installer'
$root=Join-Path ([IO.Path]::GetTempPath()) ('Any2Odyssey-flow-test-'+[Guid]::NewGuid().ToString('N'))
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
if($script.Contains('C:\Program Files\Odyssey 3D Hub') -or $script.Contains('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall') -or $script.Contains('$env:LOCALAPPDATA') -or $script.Contains('[Environment]::GetFolderPath')){throw 'Fixture routing incomplete; refusing to run.'}
$testInstall=Join-Path $package 'install.ps1';Set-Content -LiteralPath $testInstall -Value $script -Encoding UTF8
function Get-Process {param($Name,$ErrorAction) return @()}
function Get-FileHash {
 param([string]$LiteralPath,$Algorithm)
 if($LiteralPath -eq (Join-Path $engine 'ConversionWrapper.dll')){return [pscustomobject]@{Hash='0116B33697CA0192FBE26DA96CB9E2592599C0CACFBB1129AE03E445CB3A840B'}}
 Microsoft.PowerShell.Utility\Get-FileHash -LiteralPath $LiteralPath -Algorithm SHA256
}
function Assert($condition,[string]$message){if(-not $condition){throw $message};Write-Host "PASS: $message"}
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
 $shell=New-Object -ComObject WScript.Shell
 Assert ($shell.CreateShortcut((Join-Path $desktop 'Any2Odyssey.lnk')).TargetPath -eq (Join-Path $app 'Any2Odyssey.exe')) 'Desktop shortcut targets the installed executable'
 Assert ($shell.CreateShortcut((Join-Path $programs 'Any2Odyssey.lnk')).TargetPath -eq (Join-Path $app 'Any2Odyssey.exe')) 'Start menu shortcut targets the installed executable'
 New-ItemProperty -LiteralPath $registration -Name EstimatedSize -Value 4096 -PropertyType DWord -Force | Out-Null
 & $testInstall
 Assert ((Get-Content -LiteralPath (Join-Path $data 'install-result.json') -Raw | ConvertFrom-Json).Action -eq 'Repair') 'Repeated installer repairs the same release'
 Assert ((Get-ItemProperty -LiteralPath $registration).EstimatedSize -eq 4096) 'Repair keeps registry values the installer does not own'
 Assert ((Get-ItemProperty -LiteralPath $registration).UninstallString -like '*Uninstall.ps1" -Pause') 'Installed-apps uninstall keeps its console open on failure'
 Assert ((Get-ItemProperty -LiteralPath $registration).UninstallString -like '"*\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile *') 'Installed-apps uninstall names the system PowerShell by full path'
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
