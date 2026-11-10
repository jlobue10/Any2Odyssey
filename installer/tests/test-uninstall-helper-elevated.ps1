# The worker-removal helper's real main body, run the way the uninstaller runs it: as an administrator,
# in its own Windows PowerShell 5.1 process started with -File. Windows only.
# The other tests dot-source the helper's functions or replace the helper with a stub, so this is the
# only place where its account check, its own-path check, its result note, its folder cleanup and the
# uninstaller's real Start-Process launch are executed.
# Everything the helper touches is routed into a temporary folder first: its Program Files location,
# the worker path and the process names it refuses to run beside. It needs an elevated console
# (CI runners are administrators); without one the test says so and is skipped.
$ErrorActionPreference='Stop'
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
if(-not (New-Object Security.Principal.WindowsPrincipal($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){
 Write-Host 'SKIPPED: test-uninstall-helper-elevated.ps1 needs an elevated console, because the helper refuses to run without administrator rights. Run it from an administrator PowerShell; CI runs it elevated.'
 exit 0
}
$installerDir=Split-Path -Parent $PSScriptRoot
# The helper compares its own path with its installed path. %TEMP% can be an 8.3 short path
# (C:\Users\RUNNER~1\...), which the child process might report in its long form; the known-folder
# API returns the long form, so both sides agree.
$root=Join-Path (Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Temp') ('Any2Odyssey-helper-run-test-'+[Guid]::NewGuid().ToString('N'))
$programFiles=Join-Path $root 'programfiles';$helperFolder=Join-Path $programFiles 'Any2Odyssey'
$helper=Join-Path $helperFolder 'UninstallWorker.ps1';$owner=Join-Path $helperFolder 'UninstallWorker.owner';$result=Join-Path $helperFolder 'UninstallWorker.result'
$engine=Join-Path $root 'engine';$worker=Join-Path $engine 'Any2OdysseyWorker.exe';$sentinel=Join-Path $engine 'ConversionWrapper.dll'
$local=Join-Path $root 'local';$app=Join-Path $local 'Programs\Any2Odyssey';$exe=Join-Path $app 'Any2Odyssey.exe';$uninstaller=Join-Path $app 'Uninstall.ps1'
$desktop=Join-Path $root 'desktop';$programs=Join-Path $root 'programs'
$registration='HKCU:\Software\Any2OdysseyInstallerTests\'+[Guid]::NewGuid().ToString('N')
$powershell=Join-Path ([Environment]::SystemDirectory) 'WindowsPowerShell\v1.0\powershell.exe'
$sid=$identity.User.Value
function Assert($condition,[string]$message){if(-not $condition){throw $message};Write-Host "PASS: $message"}

$helperSource=Get-Content -LiteralPath (Join-Path $installerDir 'UninstallWorker.ps1') -Raw
$helperSource=$helperSource.Replace('C:\Program Files\Odyssey 3D Hub\Conversion\Any2OdysseyWorker.exe',$worker)
$helperSource=$helperSource.Replace("[Environment]::GetFolderPath('ProgramFiles')",("'"+$programFiles+"'"))
# An Any2Odyssey that is really running on this PC must neither block this test nor be judged by it.
$helperSource=$helperSource.Replace('Get-Process Any2Odyssey,Any2OdysseyWorker','Get-Process Any2OdysseyHelperTestNoSuchProcess')
if($helperSource.Contains('C:\Program Files') -or $helperSource.Contains('[Environment]::GetFolderPath') -or $helperSource.Contains('Get-Process Any2Odyssey,') -or -not $helperSource.Contains($worker) -or -not $helperSource.Contains($programFiles) -or -not $helperSource.Contains('Any2OdysseyHelperTestNoSuchProcess')){throw 'Fixture routing incomplete; refusing to run.'}

$uninstallSource=Get-Content -LiteralPath (Join-Path $installerDir 'Uninstall.ps1') -Raw
$uninstallSource=$uninstallSource.Replace('$env:LOCALAPPDATA',("'"+$local+"'")).Replace('C:\Program Files\Odyssey 3D Hub\Conversion',$engine).Replace('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Any2Odyssey',$registration)
$uninstallSource=$uninstallSource.Replace("[Environment]::GetFolderPath('Desktop')",("'"+$desktop+"'")).Replace("[Environment]::GetFolderPath('Programs')",("'"+$programs+"'"))
$uninstallSource=$uninstallSource.Replace("[Environment]::GetFolderPath('ProgramFiles')",("'"+$programFiles+"'"))
$uninstallSource=$uninstallSource.Replace('Local\Any2OdysseyQuit',('Local\Any2OdysseyTestQuit-'+[Guid]::NewGuid().ToString('N')))
# This console is elevated, so the uninstaller starts the helper directly, as it does with UAC off.
$uninstallSource=$uninstallSource.Replace('$alreadyElevated=$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)','$alreadyElevated=$true')
if($uninstallSource.Contains('C:\Program Files\Odyssey 3D Hub') -or $uninstallSource.Contains('HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall') -or $uninstallSource.Contains('$env:LOCALAPPDATA') -or $uninstallSource.Contains('[Environment]::GetFolderPath') -or $uninstallSource.Contains('IsInRole') -or $uninstallSource.Contains('Any2OdysseyQuit')){throw 'Fixture routing incomplete; refusing to run.'}

function New-Fixture {
 foreach($folder in @($helperFolder,$engine,$app,$desktop,$programs)){New-Item -ItemType Directory -Path $folder -Force | Out-Null}
 Set-Content -LiteralPath $helper -Value $helperSource -Encoding UTF8
 [IO.File]::WriteAllText($owner,$sid,[Text.UTF8Encoding]::new($false))
 if(Test-Path -LiteralPath $result){Remove-Item -LiteralPath $result}
 Set-Content -LiteralPath $worker -Value 'installed worker'
 Set-Content -LiteralPath $sentinel -Value 'Samsung file; never loaded'
 Set-Content -LiteralPath $exe -Value 'installed executable; never run'
 Set-Content -LiteralPath (Join-Path $app 'version.json') -Value '{"Version":"0.8.2","ControlsOnly":false}'
 Set-Content -LiteralPath $uninstaller -Value $uninstallSource -Encoding UTF8
 New-Item -Path $registration -Force | Out-Null
 New-ItemProperty -LiteralPath $registration -Name DisplayName -Value 'Any2Odyssey' -PropertyType String -Force | Out-Null
}
function Invoke-Helper {
 # The command line Uninstall.ps1 builds. No -Verb RunAs: this console already is elevated.
 param([string]$Path,[string]$ExpectedSid)
 $process=Start-Process -FilePath $powershell -ArgumentList ('-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "'+$Path+'" -ExpectedUserSid "'+$ExpectedSid+'"') -WindowStyle Hidden -Wait -PassThru
 return $process.ExitCode
}
# The uninstaller runs inside this process; its process discovery is stubbed as in the other tests.
function Get-Process {param($Name,$ErrorAction)return @()}
# Under PowerShell 7 a child Windows PowerShell must not inherit PowerShell 7's module path; in
# production the uninstaller itself runs under Windows PowerShell, where the question does not arise.
$savedModulePath=$env:PSModulePath
if($PSVersionTable.PSEdition -eq 'Core'){$env:PSModulePath=[Environment]::GetEnvironmentVariable('PSModulePath','Machine')}
$lock=$null
$passed=$false
try{
 New-Fixture
 $code=Invoke-Helper -Path $helper -ExpectedSid 'S-1-5-21-1-2-3-1001'
 Assert ($code -eq 1 -and (Test-Path -LiteralPath $worker) -and (Test-Path -LiteralPath $helper) -and (Test-Path -LiteralPath $owner)) 'The helper removes nothing when another account started the uninstall'
 Assert ((Test-Path -LiteralPath $result) -and ([IO.File]::ReadAllText($result,[Text.Encoding]::UTF8)).Contains('different Windows account')) 'The refused helper leaves its reason beside itself'
 Assert (([IO.File]::ReadAllBytes($result))[0] -ne 0xEF) 'The result note is written without a byte-order mark'

 # A copy of the helper anywhere else (a user-writable folder, say) must not remove the worker.
 $strayFolder=Join-Path $root 'elsewhere';New-Item -ItemType Directory -Path $strayFolder | Out-Null
 $stray=Join-Path $strayFolder 'UninstallWorker.ps1';Copy-Item -LiteralPath $helper -Destination $stray
 Remove-Item -LiteralPath $result
 $code=Invoke-Helper -Path $stray -ExpectedSid $sid
 Assert ($code -eq 1 -and (Test-Path -LiteralPath $worker) -and (Test-Path -LiteralPath $helper) -and (Test-Path -LiteralPath $stray)) 'A copy of the helper outside its installed folder removes nothing'
 Assert ((Test-Path -LiteralPath $result) -and ([IO.File]::ReadAllText($result,[Text.Encoding]::UTF8)).Contains('Run only the installed')) 'The stray copy''s refusal is written to the installed folder, not beside the copy'
 Assert (-not (Test-Path -LiteralPath (Join-Path $strayFolder 'UninstallWorker.result'))) 'Nothing is written beside a stray copy'

 # The real uninstaller, its real Start-Process launch and the real helper. First with the worker
 # held open, so the helper fails for a reason only it knows; the uninstaller must relay that reason.
 New-Fixture
 $lock=[IO.File]::Open($worker,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::None)
 $message=''
 try{& $uninstaller}catch{$message=$_.Exception.Message}finally{$lock.Dispose();$lock=$null}
 Assert ($message.StartsWith('Worker removal did not complete: ') -and $message.Contains('Any2OdysseyWorker.exe') -and $message.EndsWith('No per-user installation files were removed.')) 'The uninstaller relays the reason the real helper failed with'
 Assert ((Test-Path -LiteralPath $worker) -and (Test-Path -LiteralPath $helper) -and (Test-Path -LiteralPath $owner) -and (Test-Path -LiteralPath $exe) -and (Test-Path -LiteralPath $registration)) 'A failed helper run leaves the worker, the helper and the per-user installation in place'

 # Then for real: worker, helper, marker, note and the emptied Any2Odyssey folder go; nothing else does.
 & $uninstaller
 Assert (-not (Test-Path -LiteralPath $worker) -and -not (Test-Path -LiteralPath $helper) -and -not (Test-Path -LiteralPath $owner) -and -not (Test-Path -LiteralPath $result)) 'The real helper removes the worker, itself, its owner marker and its result note'
 Assert (-not (Test-Path -LiteralPath $helperFolder) -and (Test-Path -LiteralPath $programFiles)) 'The helper removes its emptied Any2Odyssey folder and nothing above it'
 Assert ((Test-Path -LiteralPath $sentinel) -and (Test-Path -LiteralPath $engine)) 'Samsung''s file beside the worker is untouched'
 Assert (-not (Test-Path -LiteralPath $exe) -and -not (Test-Path -LiteralPath $app) -and -not (Test-Path -LiteralPath $registration)) 'The uninstaller then removes the per-user installation'
 $passed=$true
}finally{
 if($lock){$lock.Dispose()}
 $env:PSModulePath=$savedModulePath
 if(Test-Path -LiteralPath $registration){Remove-Item -LiteralPath $registration -Recurse}
 $testKeys=Split-Path -Parent $registration
 if((Test-Path -LiteralPath $testKeys) -and -not (Get-ChildItem -LiteralPath $testKeys | Select-Object -First 1) -and -not (Get-Item -LiteralPath $testKeys).GetValueNames().Count){Remove-Item -LiteralPath $testKeys}
 if($passed){Remove-Item -LiteralPath $root -Recurse -Force}else{Write-Host "Fixtures retained for inspection: $root"}
}

# Assertion failures throw above; a clean run must not inherit a native command's exit code.
exit 0
