# Unit-level checks of InstallSupport.ps1 with isolated folders. Windows only; nothing is installed.
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot '..\InstallSupport.ps1')
function Assert($condition,[string]$message){if(-not $condition){throw $message};Write-Host "PASS: $message"}
$root=Join-Path ([IO.Path]::GetTempPath()) ('Any2Odyssey-installer-test-'+[Guid]::NewGuid().ToString('N'))
$app=Join-Path $root 'app';$data=Join-Path $root 'data';$engine=Join-Path $root 'engine'
$registration='HKCU:\Software\Any2OdysseyInstallerTests\'+[Guid]::NewGuid().ToString('N')
New-Item -ItemType Directory -Path $app,$data,$engine | Out-Null
$passed=$false
try{
$repo=Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$current=Join-Path $repo 'build\Any2Odyssey.exe'
if(-not (Test-Path -LiteralPath $current)){throw "Build the release first: $current is missing (copy target\release\Any2Odyssey.exe to build\)."}
Assert ((ConvertTo-Any2OdysseyFourPartVersion ([version]'0.8.2')) -eq [version]'0.8.2.0' -and (ConvertTo-Any2OdysseyFourPartVersion ([version]'0.8.2.0')) -eq [version]'0.8.2.0') 'Resource versions compare on four parts'
Assert ((Format-Any2OdysseyVersion ([version]'0.8.6.0')) -eq '0.8.6' -and (Format-Any2OdysseyVersion ([version]'0.8.6')) -eq '0.8.6' -and (Format-Any2OdysseyVersion ([version]'0.8')) -eq '0.8.0' -and (Format-Any2OdysseyVersion ([version]'0.8.6.2')) -eq '0.8.6.2') 'Versions print in the three-part form of VERSION unless a fourth part is set'
Assert ((Get-Any2OdysseyInstallAction ([version]'0.8.0') $null) -eq 'Install') 'Fresh installation recognized'
Assert ((Get-Any2OdysseyInstallAction ([version]'0.8.1') ([version]'0.8.0.0')) -eq 'Upgrade') 'Older installation is upgraded in place'
Assert ((Get-Any2OdysseyInstallAction ([version]'0.8.0') ([version]'0.8.0.0')) -eq 'Repair') 'Same version is a repair'
$blocked=$false;try{Get-Any2OdysseyInstallAction ([version]'0.8.0') ([version]'0.9.0')}catch{$blocked=$_.Exception.Message.Contains('0.9.0 is newer than this package (0.8.0)') -and $_.Exception.Message.Contains('.\Install.cmd -AllowDowngrade')};Assert $blocked 'Downgrade blocked by default, with the command that allows it'
Assert ((Get-Any2OdysseyInstallAction ([version]'0.8.0') ([version]'0.9.0') -AllowDowngrade) -eq 'Downgrade') 'Explicit downgrade is supported'
Assert ($null -eq (Get-Any2OdysseyInstalledVersion $app '' $data)) 'Empty installation has no version'
# The conversion worker is one file for the whole PC: an older package must not replace a newer one unless asked.
Assert-Any2OdysseyWorkerNotNewer -PackageVersion ([version]'0.8.6.0') -WorkerVersion $null
Assert-Any2OdysseyWorkerNotNewer -PackageVersion ([version]'0.8.6.0') -WorkerVersion ([version]'0.8.6.0')
Assert-Any2OdysseyWorkerNotNewer -PackageVersion ([version]'0.8.6.0') -WorkerVersion ([version]'0.8.5')
Assert-Any2OdysseyWorkerNotNewer -PackageVersion ([version]'0.8.6.0') -WorkerVersion ([version]'0.9.0.0') -AllowDowngrade
Assert $true 'An unreadable, equal or older worker, and an allowed downgrade, pass the worker check'
$message='';try{Assert-Any2OdysseyWorkerNotNewer -PackageVersion ([version]'0.8.6.0') -WorkerVersion ([version]'0.9.0.0') -OwnerSid 'S-1-5-21-1' -CurrentSid 'S-1-5-21-1'}catch{$message=$_.Exception.Message}
Assert ($message.Contains('version 0.9.0, newer than this package (0.8.6)') -and $message.Contains('.\Install.cmd -AllowDowngrade') -and -not $message.Contains('another Windows account')) 'A newer worker of the same account is refused with the command that allows it'
$message='';try{Assert-Any2OdysseyWorkerNotNewer -PackageVersion ([version]'0.8.6.0') -WorkerVersion ([version]'0.9.0.0') -OwnerSid 'S-1-5-21-2' -CurrentSid 'S-1-5-21-1'}catch{$message=$_.Exception.Message}
Assert ($message.Contains('newer than this package') -and $message.Contains('another Windows account (S-1-5-21-2)')) 'A newer worker installed by another account is refused and that account is named'
Set-Content -LiteralPath (Join-Path $root 'not-a-program.exe') -Value 'no version resource'
Assert ($null -eq (Get-Any2OdysseyFileVersion -Path (Join-Path $root 'not-a-program.exe'))) 'A file without a version resource has no version'
# A truncated record must count as absent rather than throw from ConvertFrom-Json.
Set-Content -LiteralPath (Join-Path $app 'version.json') -Value '{"Version":"0.8.2",' -NoNewline
Set-Content -LiteralPath (Join-Path $data 'install-result.json') -Value '' -NoNewline
Assert ($null -eq (Get-Any2OdysseyInstalledVersion $app '' $data)) 'Damaged version records are treated as absent'
# Uninstall.ps1 keeps install-result.json. Without an executable or a registration it is history,
# not an installation: a reinstall is an Install and an older release is not a blocked downgrade.
Set-Content -LiteralPath (Join-Path $data 'install-result.json') -Value '{"Version":"99.0.0"}'
Assert ($null -eq (Get-Any2OdysseyInstalledVersion $app '' $data)) 'A record left by an uninstall is not an installed version'
Assert ($null -eq (Get-Any2OdysseyInstalledVersion $app $registration $data)) 'The record uninstall leaves in the data folder is not an installation'
Assert ((Get-Any2OdysseyInstallAction ([version]'0.8.0') (Get-Any2OdysseyInstalledVersion $app $registration $data)) -eq 'Install') 'An older release installs after a newer one was uninstalled'
# version.json in the application folder is what the uninstaller removes: readable, it does count.
Set-Content -LiteralPath (Join-Path $app 'version.json') -Value '{"Version":"0.8.1"}'
Assert ((Get-Any2OdysseyInstalledVersion $app '' $data).Version -eq [version]'0.8.1') 'A readable version.json is the record of an installation without its executable'
Remove-Item -LiteralPath (Join-Path $app 'version.json')
# The registration's DisplayVersion is consulted before version.json; an unparsable one must count as
# absent too, and the kept install-result.json (still naming 99.0.0 here) is not consulted then either.
New-Item -Path $registration -Force | Out-Null
New-ItemProperty -LiteralPath $registration -Name DisplayVersion -Value 'not a version' -PropertyType String -Force | Out-Null
Assert ($null -eq (Get-Any2OdysseyInstalledVersion $app $registration $data)) 'An unparsable registry DisplayVersion is treated as absent, with or without a kept install-result.json'
Remove-Item -LiteralPath (Join-Path $data 'install-result.json')
Set-ItemProperty -LiteralPath $registration -Name DisplayVersion -Value '0.8.1'
Assert ((Get-Any2OdysseyInstalledVersion $app $registration $data).Version -eq [version]'0.8.1') 'A valid registry DisplayVersion is read when no executable is installed'
Remove-Item -LiteralPath $registration -Recurse
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
# A destination two folders deep that does not exist yet: a failed transaction must remove what it created.
$newFolder=Join-Path $root 'programfiles\Any2Odyssey'
$failingOperations=$operations+@([pscustomobject]@{Source=$current;Destination=Join-Path $newFolder 'UninstallWorker.ps1'})
# Backup folders are named Any2Odyssey-backup-<guid>; only a rollback that needs attention may leave one.
$backupNames={@(Get-ChildItem -LiteralPath ([IO.Path]::GetTempPath()) -Directory -Filter 'Any2Odyssey-backup-*' | Select-Object -ExpandProperty Name | Sort-Object) -join '|'}
$backupsBefore=& $backupNames
$rollback=[pscustomobject]@{Called=$false};$failed=$false;$failureMessage=''
try{Invoke-Any2OdysseyFileTransaction $failingOperations -AfterCopy {throw 'Simulated registration failure'} -RollbackState {$rollback.Called=$true}}catch{$failed=$true;$failureMessage=$_.Exception.Message}
Assert ($failed -and $rollback.Called) 'Failure invokes registration rollback'
Assert (-not (Test-Path -LiteralPath $newFolder) -and -not (Test-Path -LiteralPath (Split-Path -Parent $newFolder))) 'Failed install removes the folders it created'
Assert ((Test-Path -LiteralPath $app) -and (Test-Path -LiteralPath $engine)) 'Failed install keeps the folders that already existed'
Assert ((Get-FileHash -LiteralPath (Join-Path $app 'Any2Odyssey.exe')).Hash -eq $oldHash) 'Failed upgrade restores old executable byte-for-byte'
Assert (-not (Test-Path -LiteralPath (Join-Path $engine 'Any2OdysseyWorker.exe'))) 'Failed upgrade removes newly introduced worker'
Assert ($failureMessage.Contains('nothing was changed') -and $failureMessage.Contains('Simulated registration failure.') -and -not $failureMessage.Contains('Rollback needs attention')) 'A rolled-back transaction says that nothing was changed, and why'
Assert ((& $backupNames) -eq $backupsBefore) 'A clean rollback removes its backup copies'
Invoke-Any2OdysseyFileTransaction $operations
Assert ((& $backupNames) -eq $backupsBefore) 'A successful transaction removes its backup copies'
# A package file with the read-only attribute (kept by some unpackers, or set by read-only media):
# Copy-Item carries it over, and an installed file with it stopped the uninstaller and a rollback.
$readOnlySource=Join-Path $root 'read-only-package-file.txt';Set-Content -LiteralPath $readOnlySource -Value 'packaged'
(Get-Item -LiteralPath $readOnlySource).IsReadOnly=$true
$readOnlyTarget=Join-Path $root 'read-only-target\installed.txt'
$readOnlyOperations=@([pscustomobject]@{Source=$readOnlySource;Destination=$readOnlyTarget})
$readOnlyMessage='';try{Invoke-Any2OdysseyFileTransaction $readOnlyOperations -AfterCopy {throw 'Simulated registration failure'}}catch{$readOnlyMessage=$_.Exception.Message}
Assert ($readOnlyMessage.Contains('nothing was changed') -and -not $readOnlyMessage.Contains('Rollback needs attention') -and -not (Test-Path -LiteralPath (Split-Path -Parent $readOnlyTarget))) 'A rollback removes a file that was copied from a read-only package file'
Invoke-Any2OdysseyFileTransaction $readOnlyOperations
Assert ((Get-Content -LiteralPath $readOnlyTarget -Raw).Trim() -eq 'packaged' -and -not (Get-Item -LiteralPath $readOnlyTarget).IsReadOnly) 'A file installed from a read-only package file is not read-only'
Set-Content -LiteralPath (Join-Path $root 'newer-package-file.txt') -Value 'newer'
(Get-Item -LiteralPath $readOnlyTarget).IsReadOnly=$true
Invoke-Any2OdysseyFileTransaction @([pscustomobject]@{Source=Join-Path $root 'newer-package-file.txt';Destination=$readOnlyTarget})
Assert ((Get-Content -LiteralPath $readOnlyTarget -Raw).Trim() -eq 'newer') 'An installed file that was made read-only is still replaced'
Assert ((& $backupNames) -eq $backupsBefore) 'Read-only files leave no backup copies behind'
Assert ((Get-FileHash -LiteralPath (Join-Path $app 'Any2Odyssey.exe')).Hash -eq (Get-FileHash -LiteralPath $current).Hash) 'Successful upgrade installs the new executable'
Assert ((Get-FileHash -LiteralPath (Join-Path $engine 'Any2OdysseyWorker.exe')).Hash -eq (Get-FileHash -LiteralPath $current).Hash) 'App and worker match after upgrade'
Assert ((Get-FileHash -LiteralPath (Join-Path $data 'settings.json')).Hash -eq $settingsHash) 'Preferences preserved byte-for-byte'
Assert ((Get-FileHash -LiteralPath (Join-Path $data 'Samsung-OverlaySettings-before.json')).Hash -eq $backupHash) 'Samsung settings backup preserved'
Assert ((Get-FileHash -LiteralPath (Join-Path $engine 'ConversionWrapper.dll')).Hash -eq $samsungHash) 'Samsung engine untouched'
# A destination another process holds. The lock allows reading (the snapshot succeeds) but not
# writing, like a program that is still running. Start-Sleep is shadowed so that the retries cost
# no time and, when asked, release the lock the way a program that finishes quitting would.
$appExe=Join-Path $app 'Any2Odyssey.exe';$workerExe=Join-Path $engine 'Any2OdysseyWorker.exe'
$older=[byte[]]([IO.File]::ReadAllBytes($current)+[byte[]](0))
$wait=[pscustomobject]@{Sleeps=0;Release=$false;Lock=$null}
function Start-Sleep {param($Milliseconds,$Seconds)$wait.Sleeps++;if($wait.Release -and $wait.Lock){$wait.Lock.Dispose();$wait.Lock=$null}}
[IO.File]::WriteAllBytes($appExe,$older);[IO.File]::WriteAllBytes($workerExe,$older)
$wait.Lock=[IO.File]::Open($workerExe,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
$lockedMessage=''
try{Invoke-Any2OdysseyFileTransaction $operations}catch{$lockedMessage=$_.Exception.Message}finally{$wait.Lock.Dispose();$wait.Lock=$null}
Assert ($wait.Sleeps -eq 4) 'A copy onto a file in use is retried before the installer gives up'
Assert ($lockedMessage.Contains('nothing was changed') -and $lockedMessage.Contains('The file is in use') -and -not $lockedMessage.Contains('Rollback needs attention')) 'A file that stays in use fails the update without a false rollback report'
Assert ((Get-FileHash -LiteralPath $appExe).Hash -eq $oldHash -and (Get-FileHash -LiteralPath $workerExe).Hash -eq $oldHash) 'The file replaced before the locked one is restored; the locked one was never touched'
Assert ((& $backupNames) -eq $backupsBefore) 'A rollback that skipped an untouched file still removes its backup copies'
$wait.Sleeps=0;$wait.Release=$true
$wait.Lock=[IO.File]::Open($workerExe,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
try{Invoke-Any2OdysseyFileTransaction $operations}finally{if($wait.Lock){$wait.Lock.Dispose();$wait.Lock=$null}}
Assert ($wait.Sleeps -eq 1 -and (Get-FileHash -LiteralPath $workerExe).Hash -eq (Get-FileHash -LiteralPath $current).Hash -and (Get-FileHash -LiteralPath $appExe).Hash -eq (Get-FileHash -LiteralPath $current).Hash) 'A file released while the installer waits is replaced on the next attempt'
# A rollback that cannot put a file back is the one case that keeps the backups, and says where.
$wait.Sleeps=0;$wait.Release=$false
[IO.File]::WriteAllBytes($appExe,$older)
$attention=''
try{Invoke-Any2OdysseyFileTransaction $operations -AfterCopy {$wait.Lock=[IO.File]::Open($appExe,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read);throw 'Simulated registration failure'}}catch{$attention=$_.Exception.Message}finally{if($wait.Lock){$wait.Lock.Dispose();$wait.Lock=$null}}
Assert ($attention -match '^Update failed: Simulated registration failure\. Rollback needs attention: .+ Backups retained at (.+?)\s*$') 'A rollback that could not restore a file says so and names its backups'
$retainedBackup=$Matches[1]
Assert ((Split-Path -Leaf $retainedBackup) -like 'Any2Odyssey-backup-*' -and (Test-Path -LiteralPath $retainedBackup -PathType Container)) 'The retained backup folder exists under the temporary folder'
Assert (@(Get-ChildItem -LiteralPath $retainedBackup -File | Where-Object {(Get-FileHash -LiteralPath $_.FullName).Hash -eq $oldHash}).Count -ge 1) 'The retained backups hold the file that could not be restored'
Remove-Item -LiteralPath $retainedBackup -Recurse -Force
Remove-Item -LiteralPath function:Start-Sleep
# An installation interrupted while the executable was being copied: no version, no record.
$partial=Join-Path $root 'partial';New-Item -ItemType Directory -Path $partial | Out-Null
[IO.File]::WriteAllBytes((Join-Path $partial 'Any2Odyssey.exe'),[byte[]](77,90,0,0))
$message='';try{$null=Get-Any2OdysseyInstalledVersion $partial '' $data}catch{$message=$_.Exception.Message}
Assert ($message.Contains('Delete that file') -and $message.Contains((Join-Path $partial 'Any2Odyssey.exe'))) 'An incomplete installed executable without a record is reported with what to do about it'
$passed=$true
}finally{
 if(Test-Path -LiteralPath $registration){Remove-Item -LiteralPath $registration -Recurse}
 $testKeys=Split-Path -Parent $registration
 if((Test-Path -LiteralPath $testKeys) -and -not (Get-ChildItem -LiteralPath $testKeys | Select-Object -First 1) -and -not (Get-Item -LiteralPath $testKeys).GetValueNames().Count){Remove-Item -LiteralPath $testKeys}
 if($passed){Remove-Item -LiteralPath $root -Recurse -Force}else{Write-Host "Fixtures retained for inspection: $root"}
}

# Assertion failures throw above; a clean run must not inherit a native command's exit code.
exit 0
