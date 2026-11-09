# Full-installation uninstall flow in isolated folders and an isolated registry key. Windows only.
# Start-Process is stubbed to record how the protected helper is launched and to stand in for its
# removal of the worker and itself; Get-Process is stubbed so no real instance is consulted.
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$installerDir=Join-Path $repo 'installer'
$root=Join-Path ([IO.Path]::GetTempPath()) ('Any2Odyssey-uninstall-test-'+[Guid]::NewGuid().ToString('N'))
$local=Join-Path $root 'local';$engine=Join-Path $root 'engine';$desktop=Join-Path $root 'desktop';$programs=Join-Path $root 'programs';$programFiles=Join-Path $root 'programfiles'
$registration='HKCU:\Software\Any2OdysseyInstallerTests\'+[Guid]::NewGuid().ToString('N')
$app=Join-Path $local 'Programs\Any2Odyssey';$data=Join-Path $local 'Any2Odyssey'
$exe=Join-Path $app 'Any2Odyssey.exe';$worker=Join-Path $engine 'Any2OdysseyWorker.exe';$helper=Join-Path $programFiles 'Any2Odyssey\UninstallWorker.ps1'
$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$ownedNames=@('Any2Odyssey.exe','Any2Odyssey.ico','LICENSE','NOTICE','THIRD-PARTY-LICENSES.md','USER-GUIDE.html')
function Assert($condition,[string]$message){if(-not $condition){throw $message};Write-Host "PASS: $message"}
function New-Fixture {
 param([bool]$Elevated)
 foreach($folder in @($app,$data,$engine,$desktop,$programs,(Split-Path -Parent $helper))){New-Item -ItemType Directory -Path $folder -Force | Out-Null}
 foreach($name in $ownedNames){Set-Content -LiteralPath (Join-Path $app $name) -Value "installed $name"}
 Set-Content -LiteralPath (Join-Path $app 'version.json') -Value '{"Version":"0.8.2","ControlsOnly":false}'
 Set-Content -LiteralPath (Join-Path $data 'settings.json') -Value '{"Depth":1.1,"Popout":0}'
 Set-Content -LiteralPath $worker -Value 'installed worker'
 Set-Content -LiteralPath (Join-Path $engine 'ConversionWrapper.dll') -Value 'Samsung file; never loaded'
 Copy-Item -LiteralPath (Join-Path $installerDir 'UninstallWorker.ps1') -Destination $helper
 $shell=New-Object -ComObject WScript.Shell
 foreach($folder in @($desktop,$programs)){$link=$shell.CreateShortcut((Join-Path $folder 'Any2Odyssey.lnk'));$link.TargetPath=$exe;$link.Save()}
 New-Item -Path $registration -Force | Out-Null
 New-ItemProperty -LiteralPath $registration -Name DisplayName -Value 'Any2Odyssey' -PropertyType String -Force | Out-Null
 $script=Get-Content -LiteralPath (Join-Path $installerDir 'Uninstall.ps1') -Raw
 $script=$script.Replace('$env:LOCALAPPDATA',("'"+$local+"'")).Replace('C:\Program Files\Odyssey 3D Hub\Conversion',$engine).Replace('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Any2Odyssey',$registration)
 $script=$script.Replace("[Environment]::GetFolderPath('Desktop')",("'"+$desktop+"'")).Replace("[Environment]::GetFolderPath('Programs')",("'"+$programs+"'"))
 $script=$script.Replace("[Environment]::GetFolderPath('ProgramFiles')",("'"+$programFiles+"'"))
 $token=if($Elevated){'$alreadyElevated=$true'}else{'$alreadyElevated=$false'}
 $script=$script.Replace('$alreadyElevated=$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)',$token)
 if($script.Contains('C:\Program Files\Odyssey 3D Hub') -or $script.Contains('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall') -or $script.Contains('$env:LOCALAPPDATA') -or $script.Contains('[Environment]::GetFolderPath') -or $script.Contains('IsInRole')){throw 'Fixture routing incomplete; refusing to run.'}
 Set-Content -LiteralPath (Join-Path $app 'Uninstall.ps1') -Value $script -Encoding UTF8
}
# The stubs find $launches, $helperExitCode, $worker and $helper through the caller chain
# (the uninstaller is invoked from this script), so no $script: or $global: state is needed.
$launches=New-Object 'System.Collections.Generic.List[object]'
$helperExitCode=0
function Get-Process {param($Name,$ErrorAction)return @()}
function Start-Process {
 param($FilePath,$ArgumentList,$Verb,$WindowStyle,[switch]$Wait,[switch]$PassThru)
 $launches.Add([pscustomobject]@{FilePath=$FilePath;ArgumentList=$ArgumentList;Verb=$Verb;Wait=[bool]$Wait})
 if($helperExitCode -eq 0){
  # Stand in for UninstallWorker.ps1: it removes the fixed worker path and itself.
  Remove-Item -LiteralPath $worker;Remove-Item -LiteralPath $helper
 }
 return [pscustomobject]@{ExitCode=$helperExitCode}
}
$passed=$false
try{
 New-Fixture -Elevated $false
 $settingsHash=(Get-FileHash -LiteralPath (Join-Path $data 'settings.json')).Hash
 & (Join-Path $app 'Uninstall.ps1')
 Assert ($launches.Count -eq 1 -and $launches[0].Verb -eq 'RunAs' -and $launches[0].Wait) 'Full uninstall requests elevation exactly once and waits for it'
 Assert ($launches[0].FilePath -like '*\WindowsPowerShell\v1.0\powershell.exe') 'Helper runs under the system Windows PowerShell'
 Assert ($launches[0].ArgumentList -like ('*-NonInteractive*-File "'+$helper+'" -ExpectedUserSid "'+$sid+'"*')) 'Helper receives its installed path and the invoking account SID'
 Assert (-not (Test-Path -LiteralPath $worker) -and -not (Test-Path -LiteralPath $helper)) 'Worker and helper are removed through the helper'
 foreach($name in $ownedNames+@('Uninstall.ps1','version.json')){Assert (-not (Test-Path -LiteralPath (Join-Path $app $name))) "Per-user file removed: $name"}
 Assert (-not (Test-Path -LiteralPath $app)) 'Emptied program folder is removed'
 Assert (-not (Test-Path -LiteralPath (Join-Path $desktop 'Any2Odyssey.lnk')) -and -not (Test-Path -LiteralPath (Join-Path $programs 'Any2Odyssey.lnk'))) 'Shortcuts pointing at the installation are removed'
 Assert (-not (Test-Path -LiteralPath $registration)) 'Installed-apps registration is removed'
 Assert ((Test-Path -LiteralPath (Join-Path $engine 'ConversionWrapper.dll')) -and (Get-FileHash -LiteralPath (Join-Path $data 'settings.json')).Hash -eq $settingsHash) 'Samsung engine and user preferences are untouched'

 $launches.Clear()
 New-Fixture -Elevated $true
 & (Join-Path $app 'Uninstall.ps1')
 Assert ($launches.Count -eq 1 -and -not $launches[0].Verb) 'Already-elevated uninstall runs the helper directly without UAC'
 Assert ($launches[0].ArgumentList -like ('*-File "'+$helper+'" -ExpectedUserSid "'+$sid+'"*')) 'Direct helper launch still names the invoking account'
 Assert (-not (Test-Path -LiteralPath $exe) -and -not (Test-Path -LiteralPath $registration) -and -not (Test-Path -LiteralPath $worker)) 'Already-elevated uninstall removes the installation'

 $launches.Clear()
 $helperExitCode=1
 New-Fixture -Elevated $false
 $failed=$false;try{& (Join-Path $app 'Uninstall.ps1')}catch{$failed=$_.Exception.Message.Contains('Worker removal did not complete')}
 Assert $failed 'Helper failure stops the uninstall with its own message'
 Assert ((Test-Path -LiteralPath $exe) -and (Test-Path -LiteralPath $registration) -and (Test-Path -LiteralPath (Join-Path $desktop 'Any2Odyssey.lnk')) -and (Test-Path -LiteralPath $worker)) 'Helper failure leaves per-user files, shortcuts, registration and worker in place'
 $passed=$true
}finally{
 if(Test-Path -LiteralPath $registration){Remove-Item -LiteralPath $registration -Recurse}
 $testKeys=Split-Path -Parent $registration
 if((Test-Path -LiteralPath $testKeys) -and -not (Get-ChildItem -LiteralPath $testKeys | Select-Object -First 1) -and -not (Get-Item -LiteralPath $testKeys).GetValueNames().Count){Remove-Item -LiteralPath $testKeys}
 if($passed){Remove-Item -LiteralPath $root -Recurse -Force}else{Write-Host "Fixtures retained for inspection: $root"}
}

# Assertion failures throw above; a clean run must not inherit a native command's exit code.
exit 0
