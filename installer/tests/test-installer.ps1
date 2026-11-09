# Unit-level checks of InstallSupport.ps1 with isolated folders. Windows only; nothing is installed.
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot '..\InstallSupport.ps1')
function Assert($condition,[string]$message){if(-not $condition){throw $message};Write-Host "PASS: $message"}
$root=Join-Path ([IO.Path]::GetTempPath()) ('Any2Odyssey-installer-test-'+[Guid]::NewGuid().ToString('N'))
$app=Join-Path $root 'app';$data=Join-Path $root 'data';$engine=Join-Path $root 'engine'
New-Item -ItemType Directory -Path $app,$data,$engine | Out-Null
$passed=$false
try{
$repo=Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$current=Join-Path $repo 'build\Any2Odyssey.exe'
if(-not (Test-Path -LiteralPath $current)){throw "Build the release first: $current is missing (copy target\release\Any2Odyssey.exe to build\)."}
Assert ((ConvertTo-Any2OdysseyFourPartVersion ([version]'0.8.2')) -eq [version]'0.8.2.0' -and (ConvertTo-Any2OdysseyFourPartVersion ([version]'0.8.2.0')) -eq [version]'0.8.2.0') 'Resource versions compare on four parts'
Assert ((Get-Any2OdysseyInstallAction ([version]'0.8.0') $null) -eq 'Install') 'Fresh installation recognized'
Assert ((Get-Any2OdysseyInstallAction ([version]'0.8.1') ([version]'0.8.0.0')) -eq 'Upgrade') 'Older installation is upgraded in place'
Assert ((Get-Any2OdysseyInstallAction ([version]'0.8.0') ([version]'0.8.0.0')) -eq 'Repair') 'Same version is a repair'
$blocked=$false;try{Get-Any2OdysseyInstallAction ([version]'0.8.0') ([version]'0.9.0')}catch{$blocked=$true};Assert $blocked 'Downgrade blocked by default'
Assert ((Get-Any2OdysseyInstallAction ([version]'0.8.0') ([version]'0.9.0') -AllowDowngrade) -eq 'Downgrade') 'Explicit downgrade is supported'
Assert ($null -eq (Get-Any2OdysseyInstalledVersion $app '' $data)) 'Empty installation has no version'
Assert ((Get-Any2OdysseyPackageVersion -Binary $current) -eq [version]((Get-Content -LiteralPath (Join-Path $repo 'VERSION') -Raw).Trim()+'.0')) 'Packaged executable carries the VERSION file version in its resource'
Copy-Item -LiteralPath $current -Destination (Join-Path $app 'Any2Odyssey.exe')
Assert ((Get-Any2OdysseyInstalledVersion $app '' $data).Version -eq (Get-Any2OdysseyPackageVersion -Binary $current)) 'Installed executable version is read from its version resource'
Set-Content -LiteralPath (Join-Path $data 'settings.json') -Value '{"Depth":1.37,"Popout":-0.25,"Mode":"Window","MinimizeToTray":false}'
Set-Content -LiteralPath (Join-Path $data 'Samsung-OverlaySettings-before.json') -Value 'original Samsung settings'
Set-Content -LiteralPath (Join-Path $engine 'ConversionWrapper.dll') -Value 'Samsung-owned file'
$settingsHash=(Get-FileHash -LiteralPath (Join-Path $data 'settings.json')).Hash
$backupHash=(Get-FileHash -LiteralPath (Join-Path $data 'Samsung-OverlaySettings-before.json')).Hash
$samsungHash=(Get-FileHash -LiteralPath (Join-Path $engine 'ConversionWrapper.dll')).Hash
# Simulate an older installed build by appending a byte so the hashes differ.
[IO.File]::WriteAllBytes((Join-Path $app 'Any2Odyssey.exe'),([IO.File]::ReadAllBytes($current)+[byte[]](0)))
$oldHash=(Get-FileHash -LiteralPath (Join-Path $app 'Any2Odyssey.exe')).Hash
$operations=@([pscustomobject]@{Source=$current;Destination=Join-Path $app 'Any2Odyssey.exe'},[pscustomobject]@{Source=$current;Destination=Join-Path $engine 'Any2OdysseyWorker.exe'})
$rollback=[pscustomobject]@{Called=$false};$failed=$false
try{Invoke-Any2OdysseyFileTransaction $operations -AfterCopy {throw 'Simulated registration failure'} -RollbackState {$rollback.Called=$true}}catch{$failed=$true}
Assert ($failed -and $rollback.Called) 'Failure invokes registration rollback'
Assert ((Get-FileHash -LiteralPath (Join-Path $app 'Any2Odyssey.exe')).Hash -eq $oldHash) 'Failed upgrade restores old executable byte-for-byte'
Assert (-not (Test-Path -LiteralPath (Join-Path $engine 'Any2OdysseyWorker.exe'))) 'Failed upgrade removes newly introduced worker'
Invoke-Any2OdysseyFileTransaction $operations
Assert ((Get-FileHash -LiteralPath (Join-Path $app 'Any2Odyssey.exe')).Hash -eq (Get-FileHash -LiteralPath $current).Hash) 'Successful upgrade installs the new executable'
Assert ((Get-FileHash -LiteralPath (Join-Path $engine 'Any2OdysseyWorker.exe')).Hash -eq (Get-FileHash -LiteralPath $current).Hash) 'App and worker match after upgrade'
Assert ((Get-FileHash -LiteralPath (Join-Path $data 'settings.json')).Hash -eq $settingsHash) 'Preferences preserved byte-for-byte'
Assert ((Get-FileHash -LiteralPath (Join-Path $data 'Samsung-OverlaySettings-before.json')).Hash -eq $backupHash) 'Samsung settings backup preserved'
Assert ((Get-FileHash -LiteralPath (Join-Path $engine 'ConversionWrapper.dll')).Hash -eq $samsungHash) 'Samsung engine untouched'
$passed=$true
}finally{if($passed){Remove-Item -LiteralPath $root -Recurse -Force}else{Write-Host "Fixtures retained for inspection: $root"}}

# Assertion failures throw above; a clean run must not inherit a native command's exit code.
exit 0
