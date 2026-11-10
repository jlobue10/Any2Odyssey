# Full-installation uninstall flow in isolated folders and an isolated registry key. Windows only.
# Start-Process is stubbed to record how the protected helper is launched and to stand in for its
# removal of the worker and itself; Get-Process is stubbed so no real instance is consulted, and the
# quit event is routed to a name of this test's own, so an Any2Odyssey running on this PC is never
# told to quit.
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$installerDir=Join-Path $repo 'installer'
$root=Join-Path ([IO.Path]::GetTempPath()) ('Any2Odyssey-uninstall-test-'+[Guid]::NewGuid().ToString('N'))
$local=Join-Path $root 'local';$engine=Join-Path $root 'engine';$desktop=Join-Path $root 'desktop';$programs=Join-Path $root 'programs';$programFiles=Join-Path $root 'programfiles'
$registration='HKCU:\Software\Any2OdysseyInstallerTests\'+[Guid]::NewGuid().ToString('N')
$app=Join-Path $local 'Programs\Any2Odyssey';$data=Join-Path $local 'Any2Odyssey'
$exe=Join-Path $app 'Any2Odyssey.exe';$worker=Join-Path $engine 'Any2OdysseyWorker.exe';$helper=Join-Path $programFiles 'Any2Odyssey\UninstallWorker.ps1'
$ownerFile=Join-Path (Split-Path -Parent $helper) 'UninstallWorker.owner'
# Where the real helper leaves the reason for a failure, for the uninstaller to relay.
$resultFile=Join-Path (Split-Path -Parent $helper) 'UninstallWorker.result'
$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
# The uninstaller signals Local\Any2OdysseyQuit. The fixture routes it to this event, which the
# test owns, so the signal can be observed and never reaches a real instance.
$quitEventName='Local\Any2OdysseyTestQuit-'+[Guid]::NewGuid().ToString('N')
$quitEvent=New-Object Threading.EventWaitHandle -ArgumentList $false,([Threading.EventResetMode]::ManualReset),$quitEventName
$ownedNames=@('Any2Odyssey.exe','Any2Odyssey.ico','LICENSE','NOTICE','THIRD-PARTY-LICENSES.md','USER-GUIDE.html')
function Assert($condition,[string]$message){if(-not $condition){throw $message};Write-Host "PASS: $message"}
function New-Fixture {
 # $HelperOwner is the SID written beside the helper by the installer; '' leaves no marker (pre-0.8.5 helper).
 # $NoDesktop routes the Desktop lookup to the empty path Windows reports for a folder that does not exist.
 param([bool]$Elevated,[string]$Record='{"Version":"0.8.2","ControlsOnly":false}',[string]$HelperOwner=$sid,[bool]$NoDesktop=$false)
 foreach($folder in @($app,$data,$engine,$desktop,$programs,(Split-Path -Parent $helper))){New-Item -ItemType Directory -Path $folder -Force | Out-Null}
 foreach($name in $ownedNames){Set-Content -LiteralPath (Join-Path $app $name) -Value "installed $name"}
 Set-Content -LiteralPath (Join-Path $app 'version.json') -Value $Record
 Set-Content -LiteralPath (Join-Path $data 'settings.json') -Value '{"Depth":1.1,"Popout":0}'
 Set-Content -LiteralPath $worker -Value 'installed worker'
 Set-Content -LiteralPath (Join-Path $engine 'ConversionWrapper.dll') -Value 'Samsung file; never loaded'
 Copy-Item -LiteralPath (Join-Path $installerDir 'UninstallWorker.ps1') -Destination $helper
 if(Test-Path -LiteralPath $ownerFile){Remove-Item -LiteralPath $ownerFile}
 if(Test-Path -LiteralPath $resultFile){Remove-Item -LiteralPath $resultFile}
 if($HelperOwner){[IO.File]::WriteAllText($ownerFile,$HelperOwner,[Text.UTF8Encoding]::new($false))}
 $shell=New-Object -ComObject WScript.Shell
 foreach($folder in @($desktop,$programs)){$link=$shell.CreateShortcut((Join-Path $folder 'Any2Odyssey.lnk'));$link.TargetPath=$exe;$link.Save()}
 New-Item -Path $registration -Force | Out-Null
 New-ItemProperty -LiteralPath $registration -Name DisplayName -Value 'Any2Odyssey' -PropertyType String -Force | Out-Null
 $script=Get-Content -LiteralPath (Join-Path $installerDir 'Uninstall.ps1') -Raw
 $script=$script.Replace('$env:LOCALAPPDATA',("'"+$local+"'")).Replace('C:\Program Files\Odyssey 3D Hub\Conversion',$engine).Replace('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Any2Odyssey',$registration)
 $desktopRoute=if($NoDesktop){''}else{$desktop}
 $script=$script.Replace("[Environment]::GetFolderPath('Desktop')",("'"+$desktopRoute+"'")).Replace("[Environment]::GetFolderPath('Programs')",("'"+$programs+"'"))
 $script=$script.Replace("[Environment]::GetFolderPath('ProgramFiles')",("'"+$programFiles+"'"))
 $script=$script.Replace('Local\Any2OdysseyQuit',$quitEventName)
 $token=if($Elevated){'$alreadyElevated=$true'}else{'$alreadyElevated=$false'}
 $script=$script.Replace('$alreadyElevated=$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)',$token)
 # With -Pause the uninstaller waits for a key; the fixture counts those waits instead of blocking.
 $script=$script.Replace('[void][Console]::ReadKey($true)','Add-TestKeyWait')
 if($script.Contains('C:\Program Files\Odyssey 3D Hub') -or $script.Contains('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall') -or $script.Contains('$env:LOCALAPPDATA') -or $script.Contains('[Environment]::GetFolderPath') -or $script.Contains('IsInRole') -or $script.Contains('Any2OdysseyQuit') -or $script.Contains('ReadKey')){throw 'Fixture routing incomplete; refusing to run.'}
 Set-Content -LiteralPath (Join-Path $app 'Uninstall.ps1') -Value $script -Encoding UTF8
}
# The stubs find $launches, $helperExitCode, $helperReason, $worker and $helper through the caller
# chain (the uninstaller is invoked from this script), so no $script: or $global: state is needed.
$launches=New-Object 'System.Collections.Generic.List[object]'
$helperExitCode=0
# What a failing helper leaves in UninstallWorker.result; '' leaves no note.
$helperReason=''
# '' (helper runs), 'declined' (UAC prompt cancelled: Win32 error 1223) or 'other' (any other launch failure).
$launchFailure=''
$keyWaits=[pscustomobject]@{Count=0}
function Add-TestKeyWait {$keyWaits.Count++}
# Notes the uninstaller prints are recorded here: capturing the information stream with 6>&1
# behaves differently under Windows PowerShell 5.1, a function shadowing the cmdlet does not.
$notes=New-Object 'System.Collections.Generic.List[string]'
function Write-Host {param($Object,$ForegroundColor,$BackgroundColor,[switch]$NoNewline)$text=[string]::Join(' ',@($Object));$notes.Add($text);$Host.UI.WriteLine($text)}
# Get-Process reports what $processes.List holds, so no real instance is consulted. Start-Sleep is
# what the uninstaller's wait loop calls: the stub lets a simulated instance of this session exit.
$processes=[pscustomobject]@{List=@();Waits=0}
$thisSession=[Diagnostics.Process]::GetCurrentProcess().SessionId
function Get-Process {param($Name,$ErrorAction)return @($processes.List | Where-Object {@($Name) -contains $_.Name})}
function Start-Sleep {param($Milliseconds,$Seconds)$processes.Waits++;$processes.List=@($processes.List | Where-Object {$_.SessionId -ne $thisSession})}
function Start-Process {
 param($FilePath,$ArgumentList,$Verb,$WindowStyle,[switch]$Wait,[switch]$PassThru)
 $launches.Add([pscustomobject]@{FilePath=$FilePath;ArgumentList=$ArgumentList;Verb=$Verb;Wait=[bool]$Wait})
 # A declined UAC prompt surfaces from Start-Process -Verb RunAs as InvalidOperationException wrapping Win32Exception 1223.
 if($launchFailure -eq 'declined'){throw (New-Object InvalidOperationException -ArgumentList 'This command cannot be run due to the error: The operation was canceled by the user.',(New-Object ComponentModel.Win32Exception 1223))}
 if($launchFailure -eq 'other'){throw (New-Object InvalidOperationException -ArgumentList 'This command cannot be run due to the error: simulated launch failure.',(New-Object ComponentModel.Win32Exception 2))}
 if($helperExitCode -eq 0){
  # Stand in for UninstallWorker.ps1: it removes the fixed worker path, itself and its owner marker.
  Remove-Item -LiteralPath $worker;Remove-Item -LiteralPath $helper
  if(Test-Path -LiteralPath $ownerFile){Remove-Item -LiteralPath $ownerFile}
  if(Test-Path -LiteralPath $resultFile){Remove-Item -LiteralPath $resultFile}
 }elseif($helperReason){[IO.File]::WriteAllText($resultFile,$helperReason,[Text.UTF8Encoding]::new($false))}
 return [pscustomobject]@{ExitCode=$helperExitCode}
}
$passed=$false
try{
 New-Fixture -Elevated $false
 $settingsHash=(Get-FileHash -LiteralPath (Join-Path $data 'settings.json')).Hash
 & (Join-Path $app 'Uninstall.ps1')
 Assert ($quitEvent.WaitOne(0)) 'The uninstaller signals the quit event (routed here to the test''s own event)'
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
 $failed=$false;try{& (Join-Path $app 'Uninstall.ps1')}catch{$failed=$_.Exception.Message.Contains('Worker removal did not complete and its helper gave no reason (exit code 1)')}
 Assert $failed 'Helper failure stops the uninstall with its own message'
 Assert ((Test-Path -LiteralPath $exe) -and (Test-Path -LiteralPath $registration) -and (Test-Path -LiteralPath (Join-Path $desktop 'Any2Odyssey.lnk')) -and (Test-Path -LiteralPath $worker)) 'Helper failure leaves per-user files, shortcuts, registration and worker in place'

 # The helper's window closes with it; the reason it leaves beside itself reaches the user here.
 $launches.Clear()
 $helperReason='Any2Odyssey is still running on this PC (Windows session 3)'
 New-Fixture -Elevated $false
 $message='';try{& (Join-Path $app 'Uninstall.ps1')}catch{$message=$_.Exception.Message}
 Assert ($message -eq 'Worker removal did not complete: Any2Odyssey is still running on this PC (Windows session 3). No per-user installation files were removed.') 'The reason the helper leaves is relayed to the user'
 Assert ((Test-Path -LiteralPath $exe) -and (Test-Path -LiteralPath $registration) -and (Test-Path -LiteralPath $worker)) 'A relayed helper failure leaves everything in place'
 # A note from an earlier run must not be reported for a helper that failed without writing one.
 $launches.Clear()
 $helperReason=''
 New-Fixture -Elevated $false
 [IO.File]::WriteAllText($resultFile,'Reason of an earlier run',[Text.UTF8Encoding]::new($false))
 (Get-Item -LiteralPath $resultFile).LastWriteTimeUtc=[DateTime]::UtcNow.AddHours(-1)
 $message='';try{& (Join-Path $app 'Uninstall.ps1')}catch{$message=$_.Exception.Message}
 Assert ($message.Contains('gave no reason') -and -not $message.Contains('earlier run')) 'A note left by an earlier helper run is not reported as the reason'

 $launches.Clear()
 $helperExitCode=0;$launchFailure='declined'
 New-Fixture -Elevated $false
 $failed=$false;try{& (Join-Path $app 'Uninstall.ps1')}catch{$failed=$_.Exception.Message.Contains('Elevation was declined')}
 Assert $failed 'Declined UAC prompt stops the uninstall with a plain message'
 Assert ((Test-Path -LiteralPath $exe) -and (Test-Path -LiteralPath $registration) -and (Test-Path -LiteralPath $worker) -and (Test-Path -LiteralPath $helper)) 'Declined elevation leaves everything in place'

 $launches.Clear()
 $launchFailure='other'
 New-Fixture -Elevated $false
 $failed=$false;try{& (Join-Path $app 'Uninstall.ps1')}catch{$failed=$_.Exception.Message.Contains('simulated launch failure') -and -not $_.Exception.Message.Contains('Elevation was declined')}
 Assert $failed 'A launch failure other than a declined prompt keeps its own message'
 Assert ((Test-Path -LiteralPath $exe) -and (Test-Path -LiteralPath $registration) -and (Test-Path -LiteralPath $worker) -and (Test-Path -LiteralPath $helper)) 'A failed helper launch leaves everything in place'
 $launchFailure=''

 # Shared PC: another account's full installation owns the helper; a controls-only uninstall leaves it.
 $launches.Clear()
 New-Fixture -Elevated $false -Record '{"Version":"0.8.2","ControlsOnly":true}' -HelperOwner 'S-1-5-21-1-2-3-1001'
 $notes.Clear();& (Join-Path $app 'Uninstall.ps1')
 Assert ($launches.Count -eq 0) 'Controls-only uninstall never elevates for a helper owned by another account'
 Assert (-not (Test-Path -LiteralPath $exe) -and -not (Test-Path -LiteralPath $registration)) 'Controls removed while another account owns the helper'
 Assert ((Test-Path -LiteralPath $helper) -and (Test-Path -LiteralPath $worker) -and (Test-Path -LiteralPath $ownerFile)) 'Another account''s helper, worker and owner marker are left alone'
 Assert (($notes -join [char]10).Contains('belong to another account or an older release')) 'The left-alone helper is explained to the user'

 # A helper without an owner marker (installed before the marker existed) is left like any other.
 $launches.Clear()
 New-Fixture -Elevated $false -Record '{"Version":"0.8.2","ControlsOnly":true}' -HelperOwner ''
 & (Join-Path $app 'Uninstall.ps1')
 Assert ($launches.Count -eq 0) 'Controls-only uninstall never elevates for a helper without an owner marker'
 Assert ((Test-Path -LiteralPath $helper) -and (Test-Path -LiteralPath $worker) -and -not (Test-Path -LiteralPath $exe)) 'Unmarked helper and worker are left; the controls are removed'

 # A controls-only record never runs the helper, whatever the marker says: the installer refuses a
 # controls-only install while a helper exists, so this account's marker beside one cannot arise.
 $launches.Clear()
 New-Fixture -Elevated $false -Record '{"Version":"0.8.2","ControlsOnly":true}'
 $notes.Clear();& (Join-Path $app 'Uninstall.ps1')
 Assert ($launches.Count -eq 0 -and (Test-Path -LiteralPath $helper) -and (Test-Path -LiteralPath $worker)) 'Controls-only uninstall leaves a helper even when the marker names this account'
 Assert (($notes -join [char]10).Contains('belong to another account or an older release')) 'The left-alone helper is explained for a controls-only record'

 # Shared PC, full record: another account ran Install.cmd afterwards and owns the worker and helper now.
 $launches.Clear()
 New-Fixture -Elevated $false -HelperOwner 'S-1-5-21-1-2-3-1001'
 $notes.Clear();& (Join-Path $app 'Uninstall.ps1')
 Assert ($launches.Count -eq 0) 'Full uninstall never elevates for a worker and helper another account installed over it'
 Assert (-not (Test-Path -LiteralPath $exe) -and -not (Test-Path -LiteralPath $registration) -and -not (Test-Path -LiteralPath $app)) 'Per-user installation is removed while another account owns the worker'
 Assert ((Test-Path -LiteralPath $worker) -and (Test-Path -LiteralPath $helper) -and (Test-Path -LiteralPath $ownerFile)) 'Another account''s worker, helper and marker are left alone by a full uninstall'
 Assert (($notes -join [char]10).Contains('belong to another account or an older release')) 'The full uninstall explains what it left alone'

 # A truncated version.json must not block the uninstall; the worker and helper decide.
 $launches.Clear()
 New-Fixture -Elevated $false -Record '{"Version":"0.8.2",'
 $notes.Clear();& (Join-Path $app 'Uninstall.ps1')
 Assert (($notes -join [char]10).Contains('version.json) is unreadable')) 'Damaged installation record is reported'
 Assert ($launches.Count -eq 1 -and $launches[0].Verb -eq 'RunAs') 'Damaged record falls back to the files present and runs the helper'
 Assert (-not (Test-Path -LiteralPath $exe) -and -not (Test-Path -LiteralPath $worker) -and -not (Test-Path -LiteralPath $helper) -and -not (Test-Path -LiteralPath $registration)) 'Uninstall completes despite the damaged record'

 # Installed apps runs the uninstaller with -Pause in a console that closes with it. A notice about
 # something left in place must wait for a key; a run with nothing to report must not.
 $launches.Clear()
 New-Fixture -Elevated $false -Record '{"Version":"0.8.2","ControlsOnly":true}' -HelperOwner 'S-1-5-21-1-2-3-1001'
 $keyWaits.Count=0;$notes.Clear()
 & (Join-Path $app 'Uninstall.ps1') -Pause
 Assert ($keyWaits.Count -eq 1 -and ($notes -join [char]10).Contains('belong to another account or an older release') -and -not (Test-Path -LiteralPath $exe)) 'With -Pause a notice keeps the window open after a completed uninstall'
 New-Fixture -Elevated $false -Record '{"Version":"0.8.2","ControlsOnly":true}'
 Remove-Item -LiteralPath $worker;Remove-Item -LiteralPath $helper
 $keyWaits.Count=0
 & (Join-Path $app 'Uninstall.ps1') -Pause
 Assert ($keyWaits.Count -eq 0 -and -not (Test-Path -LiteralPath $exe)) 'With -Pause and nothing to report the window closes by itself'
 # A failure with -Pause is printed, waits for a key and ends with exit code 1 instead of throwing.
 $helperExitCode=1;$helperReason='Simulated helper failure'
 New-Fixture -Elevated $false
 $keyWaits.Count=0;$notes.Clear()
 & (Join-Path $app 'Uninstall.ps1') -Pause
 Assert ($LASTEXITCODE -eq 1 -and $keyWaits.Count -eq 1 -and ($notes -join [char]10).Contains('Uninstall did not complete: Worker removal did not complete: Simulated helper failure.') -and (Test-Path -LiteralPath $exe)) 'With -Pause a failure stays on screen until a key is pressed'
 $helperExitCode=0;$helperReason=''

 # Another signed-in account's instance runs its own files and never received this session's quit
 # signal: it does not block removing this account's controls. This session's instance is waited for.
 $launches.Clear()
 New-Fixture -Elevated $false -Record '{"Version":"0.8.2","ControlsOnly":true}'
 Remove-Item -LiteralPath $worker;Remove-Item -LiteralPath $helper
 $processes.Waits=0
 $processes.List=@([pscustomobject]@{Name='Any2Odyssey';SessionId=($thisSession+1)},[pscustomobject]@{Name='Any2Odyssey';SessionId=$thisSession})
 & (Join-Path $app 'Uninstall.ps1')
 Assert ($processes.Waits -eq 1 -and $processes.List.Count -eq 1) 'The uninstaller waits for the instance of its own session only'
 Assert (-not (Test-Path -LiteralPath $exe) -and -not (Test-Path -LiteralPath $registration)) 'An instance in another session does not block removing this account''s controls'
 $processes.List=@()

 # An account whose Desktop folder is gone (Windows reports an empty path) can still uninstall.
 $launches.Clear()
 New-Fixture -Elevated $false -Record '{"Version":"0.8.2","ControlsOnly":true}' -NoDesktop $true
 Remove-Item -LiteralPath $worker;Remove-Item -LiteralPath $helper
 & (Join-Path $app 'Uninstall.ps1')
 Assert (-not (Test-Path -LiteralPath $exe) -and -not (Test-Path -LiteralPath $registration) -and -not (Test-Path -LiteralPath (Join-Path $programs 'Any2Odyssey.lnk'))) 'Uninstall completes when Windows reports no Desktop folder'

 # A genuine controls-only installation has neither worker nor helper and needs no elevation.
 $launches.Clear()
 New-Fixture -Elevated $false -Record '{"Version":"0.8.2","ControlsOnly":true}'
 Remove-Item -LiteralPath $worker;Remove-Item -LiteralPath $helper
 & (Join-Path $app 'Uninstall.ps1')
 Assert ($launches.Count -eq 0) 'Controls-only uninstall never asks for elevation'
 Assert (-not (Test-Path -LiteralPath $exe) -and -not (Test-Path -LiteralPath $registration)) 'Controls-only uninstall removes the controls and registration'
 $passed=$true
}finally{
 $quitEvent.Dispose()
 if(Test-Path -LiteralPath $registration){Remove-Item -LiteralPath $registration -Recurse}
 $testKeys=Split-Path -Parent $registration
 if((Test-Path -LiteralPath $testKeys) -and -not (Get-ChildItem -LiteralPath $testKeys | Select-Object -First 1) -and -not (Get-Item -LiteralPath $testKeys).GetValueNames().Count){Remove-Item -LiteralPath $testKeys}
 if($passed){Remove-Item -LiteralPath $root -Recurse -Force}else{Write-Host "Fixtures retained for inspection: $root"}
}

# Assertion failures throw above; a clean run must not inherit a native command's exit code.
exit 0
