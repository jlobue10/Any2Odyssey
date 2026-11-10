[CmdletBinding()]
param([switch]$CheckOnly,[switch]$AllowDowngrade,[switch]$ControlsOnly)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'InstallSupport.ps1')
# Any2Odyssey identity: separate from any installed Any2Odd (different folders, worker, registry key).
$app=Join-Path $env:LOCALAPPDATA 'Programs\Any2Odyssey'
$data=Join-Path $env:LOCALAPPDATA 'Any2Odyssey'
$engine='C:\Program Files\Odyssey 3D Hub\Conversion'
$registration='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Any2Odyssey'
$binary=Join-Path $PSScriptRoot 'build\Any2Odyssey.exe'
$workerName='Any2OdysseyWorker.exe'
$helper=Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'Any2Odyssey\UninstallWorker.ps1'
$ownerFile=Join-Path (Split-Path -Parent $helper) 'UninstallWorker.owner'
$documents=@('USER-GUIDE.html','Uninstall.ps1','LICENSE','NOTICE','THIRD-PARTY-LICENSES.md')
if(-not [Environment]::Is64BitProcess){throw 'Use 64-bit PowerShell to install Any2Odyssey.'}
# ANY2ODYSSEY_SKIP_OS_CHECK=1 lets the isolated tests run on older CI images; it skips only this version gate.
if($env:ANY2ODYSSEY_SKIP_OS_CHECK -ne '1' -and [Environment]::OSVersion.Version.Build -lt 26100){throw 'Use 64-bit PowerShell on Windows 11 24H2 or newer.'}
foreach($relative in @('VERSION','build\Any2Odyssey.exe','assets\Any2Odyssey.ico','UninstallWorker.ps1')+$documents){
 if(-not (Test-Path -LiteralPath (Join-Path $PSScriptRoot $relative) -PathType Leaf)){throw "Missing package file: $relative. Build or extract the complete release first."}
}
$versionText=(Get-Content -LiteralPath (Join-Path $PSScriptRoot 'VERSION') -Raw).Trim()
if($versionText -notmatch '^\d+\.\d+\.\d+$'){throw 'Invalid package VERSION.'}
$version=[version]($versionText+'.0')
if((ConvertTo-Any2OdysseyFourPartVersion (Get-Any2OdysseyPackageVersion -Binary $binary)) -ne $version){throw 'Package VERSION and executable version disagree. Extract a complete release or rebuild it.'}
$installed=Get-Any2OdysseyInstalledVersion -AppPath $app -Registration $registration -DataPath $data
$installedVersion=if($installed){$installed.Version}else{$null}
$action=Get-Any2OdysseyInstallAction -PackageVersion $version -InstalledVersion $installedVersion -AllowDowngrade:$AllowDowngrade
Write-Host ("$action Any2Odyssey " + $(if($installed){(Format-Any2OdysseyVersion $installed.Version)+' -> '}else{''}) + $versionText)
$engineDll=Join-Path $engine 'ConversionWrapper.dll'
if($ControlsOnly){
 # Keep full-install app and renderer versions synchronized.
 if(Test-Path -LiteralPath (Join-Path $engine $workerName)){throw 'A conversion worker is already installed. Use Install.cmd to update the full installation together.'}
 if(Test-Path -LiteralPath $helper -PathType Leaf){throw 'The worker-removal helper of a full installation is still installed. Use Install.cmd to update the full installation together, or uninstall it first. If that full installation is already gone, remove C:\Program Files\Any2Odyssey as an administrator, then retry.'}
 Write-Host 'Controls-only mode: GUI, tray and Help; 3D conversion is unavailable. Samsung files will not be installed or changed.'
}else{
 if(-not (Test-Path -LiteralPath $engineDll)){throw 'Samsung Odyssey 3D Hub is not installed. For full 3D conversion, install the supported Hub first. To install just the desktop controls on this PC, run Install-GUI.cmd instead.'}
 if((Get-FileHash -LiteralPath $engineDll).Hash -ne '0116B33697CA0192FBE26DA96CB9E2592599C0CACFBB1129AE03E445CB3A840B'){throw 'Samsung engine changed. This release needs a compatibility update.'}
 # One worker path per PC. When another account placed the current worker and helper, say so here in
 # the preflight pass, which runs in the visible console; the elevated installer's own output is hidden.
 $previousOwner=''
 if(Test-Path -LiteralPath $ownerFile -PathType Leaf){$previousOwner=([string](Get-Content -LiteralPath $ownerFile -Raw)).Trim()}
 $currentSid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value
 # The worker serves every account on the PC: an older package must not replace a newer worker that
 # another installation relies on, unless -AllowDowngrade says so.
 $existingWorker=Join-Path $engine $workerName
 if(Test-Path -LiteralPath $existingWorker -PathType Leaf){Assert-Any2OdysseyWorkerNotNewer -PackageVersion $version -WorkerVersion (Get-Any2OdysseyFileVersion -Path $existingWorker) -AllowDowngrade:$AllowDowngrade -OwnerSid $previousOwner -CurrentSid $currentSid}
 if($previousOwner -and $previousOwner -ne $currentSid){Write-Host "Note: the conversion worker and uninstall helper in Program Files were installed by another Windows account ($previousOwner). This installation replaces them and becomes their owner; an uninstall from that account will leave them alone."}
}
# Windows reports a known folder that does not exist (a redirected Desktop that was removed, for
# example) as an empty path. Its shortcut is skipped instead of failing the installation.
$shortcutFolders=@()
foreach($place in @(@{Name='Desktop';Path=[Environment]::GetFolderPath('Desktop')},@{Name='Start menu';Path=[Environment]::GetFolderPath('Programs')})){
 if($place.Path){$shortcutFolders+=$place.Path}else{Write-Host "Note: Windows reports no $($place.Name) folder for this account; no shortcut is placed there."}
}
if($CheckOnly){Write-Host "$action preflight passed$(if($ControlsOnly){' for controls only'}else{': package, version and Samsung engine are compatible'}). No changes made.";return}
$principal=New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if(-not $ControlsOnly){
 if(-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){throw 'Run Install.cmd or administrator PowerShell using the same Windows account as the existing installation.'}
 if(Get-Process Odyssey3DPlayer -ErrorAction SilentlyContinue){throw 'Stop conversion in Samsung player before installing.'}
}
$running=@(Get-Process Any2Odyssey,Any2OdysseyWorker -ErrorAction SilentlyContinue)
if($running.Count){
 Write-Host 'Closing Any2Odyssey and waiting for its renderer to stop cleanly...'
 $signal=Start-Process -FilePath $binary -ArgumentList '--quit' -WindowStyle Hidden -Wait -PassThru
 if($signal.ExitCode){throw 'Could not request a clean shutdown. Quit Any2Odyssey from its tray and retry.'}
 $deadline=(Get-Date).AddSeconds(15)
 do{$remaining=@(Get-Process Any2Odyssey,Any2OdysseyWorker -ErrorAction SilentlyContinue);if(-not $remaining.Count){break};Start-Sleep -Milliseconds 250}while((Get-Date) -lt $deadline)
 if($remaining.Count){throw 'Any2Odyssey is still running. Quit it from its tray using its normal Windows account, then retry. No installed files were changed.'}
}
$stage=Join-Path ([IO.Path]::GetTempPath()) ('Any2Odyssey-install-'+[Guid]::NewGuid().ToString('N'))
try{
New-Item -ItemType Directory -Path $stage | Out-Null
$operations=@()
foreach($name in $documents){$operations+=[pscustomobject]@{Source=Join-Path $PSScriptRoot $name;Destination=Join-Path $app $name}}
$operations+=[pscustomobject]@{Source=$binary;Destination=Join-Path $app 'Any2Odyssey.exe'}
if(-not $ControlsOnly){
 $operations+=[pscustomobject]@{Source=$binary;Destination=Join-Path $engine $workerName}
 $operations+=[pscustomobject]@{Source=Join-Path $PSScriptRoot 'UninstallWorker.ps1';Destination=$helper}
 # Ownership marker: on a shared PC, Uninstall.ps1 runs the helper for a controls-only record only
 # when this account placed it, so another user's full installation is never removed by mistake.
 $ownerSource=Join-Path $stage 'UninstallWorker.owner'
 [IO.File]::WriteAllText($ownerSource,[Security.Principal.WindowsIdentity]::GetCurrent().User.Value,[Text.UTF8Encoding]::new($false))
 $operations+=[pscustomobject]@{Source=$ownerSource;Destination=$ownerFile}
}
$operations+=[pscustomobject]@{Source=Join-Path $PSScriptRoot 'assets\Any2Odyssey.ico';Destination=Join-Path $app 'Any2Odyssey.ico'}
$record=[pscustomobject]@{Installed=(Get-Date).ToString('o');Version=$versionText;PreviousVersion=$(if($installed){$installed.Version.ToString()}else{$null});Action=$action;App=$app;ControlsOnly=[bool]$ControlsOnly;Worker=$(if(-not $ControlsOnly){Join-Path $engine $workerName}else{$null});Hash=(Get-FileHash -LiteralPath $binary).Hash}
$record | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $stage 'version.json') -Encoding UTF8
$operations+=[pscustomobject]@{Source=Join-Path $stage 'version.json';Destination=Join-Path $app 'version.json'}
$operations+=[pscustomobject]@{Source=Join-Path $stage 'version.json';Destination=Join-Path $data 'install-result.json'}
$shell=New-Object -ComObject WScript.Shell
$shortcutSource=Join-Path $stage 'Any2Odyssey.lnk';$link=$shell.CreateShortcut($shortcutSource)
$link.TargetPath=Join-Path $app 'Any2Odyssey.exe';$link.WorkingDirectory=$app;$link.IconLocation=(Join-Path $app 'Any2Odyssey.ico')+',0';$link.Description='Any2Odyssey - continuous Odyssey 3D; right-click the tray icon for controls.';$link.Save()
foreach($folder in $shortcutFolders){$operations+=[pscustomobject]@{Source=$shortcutSource;Destination=Join-Path $folder 'Any2Odyssey.lnk'}}
$registrationExisted=Test-Path -LiteralPath $registration;$oldValues=@{}
if($registrationExisted){$key=Get-Item -LiteralPath $registration;foreach($name in $key.GetValueNames()){$oldValues[$name]=@{Value=$key.GetValue($name);Kind=$key.GetValueKind($name)}}}
# Installed apps resolves a bare powershell.exe through the caller's PATH; name the system one.
$uninstall='"'+(Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe')+'" -NoProfile -ExecutionPolicy Bypass -File "'+(Join-Path $app 'Uninstall.ps1')+'" -Pause'
$properties=@{DisplayName='Any2Odyssey';DisplayVersion=$versionText;Publisher='Jonathan LoBue';InstallLocation=$app;DisplayIcon=(Join-Path $app 'Any2Odyssey.ico')+',0';UninstallString=$uninstall;URLInfoAbout='https://github.com/jlobue10/Any2Odyssey'}
if($ControlsOnly){$properties.DisplayName='Any2Odyssey (controls only)'}
Invoke-Any2OdysseyFileTransaction -Operations $operations -AfterCopy {
 # -Force on an existing key clears its values; create it only when absent.
 if(-not (Test-Path -LiteralPath $registration)){New-Item -Path $registration -Force | Out-Null}
 foreach($entry in $properties.GetEnumerator()){New-ItemProperty -LiteralPath $registration -Name $entry.Key -Value $entry.Value -PropertyType String -Force | Out-Null}
 foreach($name in @('NoModify','NoRepair')){New-ItemProperty -LiteralPath $registration -Name $name -Value 1 -PropertyType DWord -Force | Out-Null}
} -RollbackState {
 if(-not $registrationExisted){if(Test-Path -LiteralPath $registration){Remove-Item -LiteralPath $registration}}
 else{foreach($name in @($properties.Keys)+@('NoModify','NoRepair')){if($oldValues.ContainsKey($name)){New-ItemProperty -LiteralPath $registration -Name $name -Value $oldValues[$name].Value -PropertyType $oldValues[$name].Kind -Force | Out-Null}else{Remove-ItemProperty -LiteralPath $registration -Name $name -ErrorAction SilentlyContinue}}}
}
}finally{
 # The staging folder holds only this run's copies; remove it whether or not the transaction succeeded.
 if(Test-Path -LiteralPath $stage){foreach($file in Get-ChildItem -LiteralPath $stage -File){Remove-Item -LiteralPath $file.FullName -ErrorAction SilentlyContinue};Remove-Item -LiteralPath $stage -ErrorAction SilentlyContinue}
}
Write-Host "Any2Odyssey $versionText $($action.ToLowerInvariant()) completed. Your settings and logs were kept. Open Any2Odyssey as your normal user. No reboot is needed."
if($ControlsOnly){Write-Host 'Desktop controls are installed; 3D remains unavailable. After installing the supported Samsung Hub, run Install.cmd to add the conversion worker.'}
