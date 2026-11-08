# Shared, independently testable installer operations. No work is done when dot-sourced.
function Get-Any2OdysseyInstalledVersion {
 param([string]$AppPath,[string]$Registration,[string]$DataPath)
 $path=Join-Path $AppPath 'Any2Odyssey.exe'
 if(Test-Path -LiteralPath $path -PathType Leaf){
  # A native executable: read its VERSIONINFO resource, not a .NET assembly name.
  $info=[Diagnostics.FileVersionInfo]::GetVersionInfo($path)
  $text=if($info.FileVersion){$info.FileVersion}elseif($info.ProductVersion){$info.ProductVersion}else{$null}
  if($text -and $text -match '^\d+(\.\d+){1,3}'){return [pscustomobject]@{Version=[version]$Matches[0];Source=$path}}
  $record=Join-Path $AppPath 'version.json'
  if(Test-Path -LiteralPath $record){$json=Get-Content -LiteralPath $record -Raw | ConvertFrom-Json;if($json.Version){return [pscustomobject]@{Version=[version]$json.Version;Source=$record}}}
  throw "Cannot read the installed executable version at $path. Repair or inspect that installation before continuing."
 }
 if($Registration -and (Test-Path -LiteralPath $Registration)){
  $value=(Get-ItemProperty -LiteralPath $Registration -Name DisplayVersion -ErrorAction SilentlyContinue).DisplayVersion
  if($value){return [pscustomobject]@{Version=[version]$value;Source=$Registration}}
 }
 foreach($path in @((Join-Path $AppPath 'version.json'),(Join-Path $DataPath 'install-result.json'))){
  if(Test-Path -LiteralPath $path){$record=Get-Content -LiteralPath $path -Raw | ConvertFrom-Json;if($record.Version){return [pscustomobject]@{Version=[version]$record.Version;Source=$path}}}
 }
 return $null
}
function Get-Any2OdysseyPackageVersion {
 param([string]$Binary)
 $info=[Diagnostics.FileVersionInfo]::GetVersionInfo($Binary)
 $text=if($info.FileVersion){$info.FileVersion}else{$info.ProductVersion}
 if(-not $text -or $text -notmatch '^\d+(\.\d+){1,3}'){throw "The packaged executable has no readable version resource: $Binary"}
 return [version]$Matches[0]
}
function Get-Any2OdysseyInstallAction {
 param([version]$PackageVersion,$InstalledVersion,[switch]$AllowDowngrade)
 if($null -eq $InstalledVersion){return 'Install'}
 $package=[version]::new($PackageVersion.Major,$PackageVersion.Minor,[Math]::Max(0,$PackageVersion.Build),[Math]::Max(0,$PackageVersion.Revision))
 $installed=[version]::new($InstalledVersion.Major,$InstalledVersion.Minor,[Math]::Max(0,$InstalledVersion.Build),[Math]::Max(0,$InstalledVersion.Revision))
 if($package -gt $installed){return 'Upgrade'}
 if($package -eq $installed){return 'Repair'}
 if(-not $AllowDowngrade){throw "Installed version $installed is newer than package $package. Downgrade blocked. Use -AllowDowngrade only if you intend to install this older version."}
 return 'Downgrade'
}
function Invoke-Any2OdysseyFileTransaction {
 param([object[]]$Operations,[scriptblock]$AfterCopy={},[scriptblock]$RollbackState={})
 $backup=Join-Path ([IO.Path]::GetTempPath()) ('Any2Odyssey-backup-'+[Guid]::NewGuid().ToString('N'))
 New-Item -ItemType Directory -Path $backup | Out-Null
 $snapshots=@();$written=New-Object 'System.Collections.Generic.List[object]'
 try {
  # Snapshot all destinations before replacing the first file.
  $index=0
  foreach($operation in $Operations){
   if(-not (Test-Path -LiteralPath $operation.Source -PathType Leaf)){throw "Missing staged file: $($operation.Source)"}
   $existed=Test-Path -LiteralPath $operation.Destination -PathType Leaf
   $saved=Join-Path $backup ([string]$index++)
   if($existed){Copy-Item -LiteralPath $operation.Destination -Destination $saved -ErrorAction Stop}
   $snapshots+= [pscustomobject]@{Source=$operation.Source;Destination=$operation.Destination;Existed=$existed;Backup=$saved}
  }
  foreach($snapshot in $snapshots){
   New-Item -ItemType Directory -Path (Split-Path -Parent $snapshot.Destination) -Force -ErrorAction Stop | Out-Null
   $written.Add($snapshot)
   Copy-Item -LiteralPath $snapshot.Source -Destination $snapshot.Destination -Force -ErrorAction Stop
  }
  & $AfterCopy
 } catch {
  $failure=$_;$rollbackErrors=@()
  for($i=$written.Count-1;$i -ge 0;$i--){$snapshot=$written[$i];try{if($snapshot.Existed){Copy-Item -LiteralPath $snapshot.Backup -Destination $snapshot.Destination -Force -ErrorAction Stop}else{if(Test-Path -LiteralPath $snapshot.Destination -PathType Leaf){Remove-Item -LiteralPath $snapshot.Destination -ErrorAction Stop}}}catch{$rollbackErrors+=$_.Exception.Message}}
  try{& $RollbackState}catch{$rollbackErrors+=$_.Exception.Message}
  if($rollbackErrors.Count){throw "Update failed: $($failure.Exception.Message). Rollback needs attention: $($rollbackErrors -join '; '). Backups retained at $backup"}
  throw "Update failed; previous files and registration restored. $($failure.Exception.Message) Backups: $backup"
 }
 foreach($snapshot in $snapshots){if(Test-Path -LiteralPath $snapshot.Backup){Remove-Item -LiteralPath $snapshot.Backup}}
 Remove-Item -LiteralPath $backup
}
