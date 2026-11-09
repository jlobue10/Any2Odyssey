# Controls-only install/repair/uninstall and promotion to a full install, in isolated folders. Windows only.
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$installerDir=Join-Path $repo 'installer'
$root=Join-Path ([IO.Path]::GetTempPath()) ('Any2Odyssey-controls-test-'+[Guid]::NewGuid().ToString('N'))
$package=Join-Path $root 'package';$local=Join-Path $root 'local';$engine=Join-Path $root 'engine';$desktop=Join-Path $root 'desktop';$programs=Join-Path $root 'programs'
$registration='HKCU:\Software\Any2OdysseyInstallerTests\'+[Guid]::NewGuid().ToString('N')
New-Item -ItemType Directory -Path $package,$desktop,$programs,(Join-Path $package 'build'),(Join-Path $package 'assets') | Out-Null
Copy-Item -LiteralPath (Join-Path $repo 'build\Any2Odyssey.exe') -Destination (Join-Path $package 'build')
foreach($file in @('VERSION','LICENSE','NOTICE','THIRD-PARTY-LICENSES.md','USER-GUIDE.html')){Copy-Item -LiteralPath (Join-Path $repo $file) -Destination $package}
foreach($support in @('InstallSupport.ps1','UninstallWorker.ps1')){Copy-Item -LiteralPath (Join-Path $installerDir $support) -Destination $package}
Copy-Item -LiteralPath (Join-Path $repo 'assets\Any2Odyssey.ico') -Destination (Join-Path $package 'assets')
foreach($file in @('install.ps1','Uninstall.ps1')){
 $script=Get-Content -LiteralPath (Join-Path $installerDir $file) -Raw
 $script=$script.Replace('$env:LOCALAPPDATA',("'"+$local+"'")).Replace('C:\Program Files\Odyssey 3D Hub\Conversion',$engine).Replace('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Any2Odyssey',$registration)
 $script=$script.Replace("[Environment]::GetFolderPath('Desktop')",("'"+$desktop+"'")).Replace("[Environment]::GetFolderPath('Programs')",("'"+$programs+"'"))
 $script=$script.Replace("[Environment]::GetFolderPath('ProgramFiles')",("'"+(Join-Path $root 'programfiles')+"'"))
 if($file -eq 'Uninstall.ps1'){$script=$script.Replace('$alreadyElevated=$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)','$alreadyElevated=$false')}
 if($file -eq 'install.ps1'){$script=$script.Replace('if(-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))','if($false)')}
 if($script.Contains('C:\Program Files\Odyssey 3D Hub') -or $script.Contains('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall') -or $script.Contains('$env:LOCALAPPDATA') -or $script.Contains('[Environment]::GetFolderPath') -or $script.Contains('IsInRole')){throw 'Fixture routing incomplete; refusing to run.'}
 Set-Content -LiteralPath (Join-Path $package $file) -Value $script -Encoding UTF8
}
function Assert($condition,[string]$message){if(-not $condition){throw $message};Write-Host "PASS: $message"}
function Get-Process {param($Name,$ErrorAction)return @()}
function Start-Process {param($FilePath,$ArgumentList,$WindowStyle,[switch]$Wait,$Verb,[switch]$PassThru)if($Verb){throw 'Controls-only uninstall must not elevate.'};return [pscustomobject]@{ExitCode=0}}
$installer=Join-Path $package 'install.ps1';$app=Join-Path $local 'Programs\Any2Odyssey';$data=Join-Path $local 'Any2Odyssey'
New-Item -ItemType Directory -Path $data -Force | Out-Null
Set-Content -LiteralPath (Join-Path $data 'settings.json') -Value '{"AutoConvert":false,"Depth":1.2}'
$settingsHash=(Get-FileHash -LiteralPath (Join-Path $data 'settings.json')).Hash
$passed=$false
try{
 & $installer -CheckOnly -ControlsOnly
 Assert (-not (Test-Path -LiteralPath $app) -and -not (Test-Path -LiteralPath $registration) -and -not (Test-Path -LiteralPath $engine)) 'Controls-only preflight makes no installation changes'
 $failed=$false;try{& $installer -CheckOnly}catch{$failed=$_.Exception.Message.Contains('Install-GUI.cmd')};Assert $failed 'Full preflight explains missing Hub and points to controls-only installer'
 & $installer -ControlsOnly
 $record=Get-Content -LiteralPath (Join-Path $app 'version.json') -Raw | ConvertFrom-Json
 Assert ($record.ControlsOnly -eq $true -and $null -eq $record.Worker) 'Controls-only mode records no conversion worker'
 Assert (-not (Test-Path -LiteralPath $engine)) 'Controls-only install does not create or modify the Samsung directory'
 Assert ((Get-ItemProperty -LiteralPath $registration).DisplayName -eq 'Any2Odyssey (controls only)') 'Installed apps identifies controls-only mode'
 & $installer -ControlsOnly
 Assert ((Get-Content -LiteralPath (Join-Path $app 'version.json') -Raw | ConvertFrom-Json).Action -eq 'Repair') 'Controls-only reinstall repairs in place'
 New-Item -ItemType Directory -Path $engine | Out-Null
 Set-Content -LiteralPath (Join-Path $engine 'Any2OdysseyWorker.exe') -Value 'Unrelated sentinel created after controls-only install'
 Set-Content -LiteralPath (Join-Path $engine 'ConversionWrapper.dll') -Value 'Simulated Samsung engine; never loaded'
 $engineHash=(Get-FileHash -LiteralPath (Join-Path $engine 'ConversionWrapper.dll')).Hash
 & (Join-Path $app 'Uninstall.ps1')
 Assert (-not (Test-Path -LiteralPath (Join-Path $app 'Any2Odyssey.exe')) -and -not (Test-Path -LiteralPath $registration)) 'Controls-only uninstall removes the app and registration without elevation'
 Assert (-not (Test-Path -LiteralPath $app)) 'Uninstall removes the emptied program folder'
 Assert ((Test-Path -LiteralPath (Join-Path $engine 'Any2OdysseyWorker.exe')) -and (Get-FileHash -LiteralPath (Join-Path $engine 'ConversionWrapper.dll')).Hash -eq $engineHash) 'Controls-only uninstall leaves all Samsung-directory files untouched'
 Assert ((Get-FileHash -LiteralPath (Join-Path $data 'settings.json')).Hash -eq $settingsHash) 'Uninstall preserves user preferences'
 Remove-Item -LiteralPath (Join-Path $engine 'Any2OdysseyWorker.exe')
 & $installer -ControlsOnly
 $failed=$false;try{& $installer -CheckOnly}catch{$failed=$_.Exception.Message.Contains('engine changed')};Assert $failed 'Full setup still rejects an incompatible engine'
 function Get-FileHash {param([string]$LiteralPath,$Algorithm)if($LiteralPath -eq (Join-Path $engine 'ConversionWrapper.dll')){return [pscustomobject]@{Hash='0116B33697CA0192FBE26DA96CB9E2592599C0CACFBB1129AE03E445CB3A840B'}};Microsoft.PowerShell.Utility\Get-FileHash -LiteralPath $LiteralPath -Algorithm SHA256}
 & $installer
 $record=Get-Content -LiteralPath (Join-Path $app 'version.json') -Raw | ConvertFrom-Json
 Assert ($record.ControlsOnly -eq $false -and (Test-Path -LiteralPath $record.Worker)) 'Full setup promotes a controls-only installation and adds its renderer'
 Assert ((Get-ItemProperty -LiteralPath $registration).DisplayName -eq 'Any2Odyssey') 'Full setup clears the controls-only registration label'
 Assert ((Get-FileHash -LiteralPath (Join-Path $data 'settings.json')).Hash -eq $settingsHash) 'Promotion preserves user preferences'
 $failed=$false;try{& $installer -ControlsOnly}catch{$failed=$_.Exception.Message.Contains('full installation')};Assert $failed 'Controls-only setup refuses to split an existing full installation'
 $passed=$true
}finally{
 if(Test-Path -LiteralPath $registration){Remove-Item -LiteralPath $registration -Recurse}
 $testKeys=Split-Path -Parent $registration
 if((Test-Path -LiteralPath $testKeys) -and -not (Get-ChildItem -LiteralPath $testKeys | Select-Object -First 1) -and -not (Get-Item -LiteralPath $testKeys).GetValueNames().Count){Remove-Item -LiteralPath $testKeys}
 if($passed){Remove-Item -LiteralPath $root -Recurse -Force}else{Write-Host "Fixtures retained for inspection: $root"}
}

# Assertion failures throw above; a clean run must not inherit a native command's exit code.
exit 0
