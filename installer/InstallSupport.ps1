# Shared, independently testable installer operations. No work is done when dot-sourced.
function Get-Any2OdysseyRecordedVersion {
 param([string]$Path)
 # version.json: a damaged record (truncated JSON, empty file) counts as
 # absent so the next source decides, instead of blocking every install and uninstall.
 if(-not (Test-Path -LiteralPath $Path -PathType Leaf)){return $null}
 try{$record=Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json;if($record.Version){return [version]$record.Version}}catch{}
 return $null
}
function Get-Any2OdysseyInstalledVersion {
 param([string]$AppPath,[string]$Registration,[string]$DataPath)
 $path=Join-Path $AppPath 'Any2Odyssey.exe'
 if(Test-Path -LiteralPath $path -PathType Leaf){
  # A native executable: read its VERSIONINFO resource, not a .NET assembly name.
  $info=[Diagnostics.FileVersionInfo]::GetVersionInfo($path)
  $text=if($info.FileVersion){$info.FileVersion}elseif($info.ProductVersion){$info.ProductVersion}else{$null}
  if($text -and $text -match '^\d+(\.\d+){1,3}'){return [pscustomobject]@{Version=[version]$Matches[0];Source=$path}}
  $record=Join-Path $AppPath 'version.json'
  $recorded=Get-Any2OdysseyRecordedVersion -Path $record
  if($recorded){return [pscustomobject]@{Version=$recorded;Source=$record}}
  throw "Cannot read the installed executable version at $path. Repair or inspect that installation before continuing."
 }
 if($Registration -and (Test-Path -LiteralPath $Registration)){
  # A DisplayVersion edited by hand or by a registry cleaner counts as absent, like a damaged record.
  $value=(Get-ItemProperty -LiteralPath $Registration -Name DisplayVersion -ErrorAction SilentlyContinue).DisplayVersion
  $registered=$null
  if($value){try{$registered=[version]$value}catch{}}
  if($registered){return [pscustomobject]@{Version=$registered;Source=$Registration}}
 }
 # install-result.json in the data folder is not consulted: uninstall keeps it on purpose, so it
 # describes the last installation, not a present one. Read as installed, it blocked going back to
 # an older release after an uninstall and recorded a fresh install as a repair or upgrade.
 $path=Join-Path $AppPath 'version.json'
 $recorded=Get-Any2OdysseyRecordedVersion -Path $path
 if($recorded){return [pscustomobject]@{Version=$recorded;Source=$path}}
 return $null
}
function Get-Any2OdysseySessionProcess {
 # Any2Odyssey processes of this Windows session. Get-Process lists every session, but the quit
 # signal and the per-user files concern only this one: on a shared PC another account's
 # disconnected session must not block, or be asked to quit by, this account's setup.
 param([string[]]$Name=@('Any2Odyssey','Any2OdysseyWorker'))
 $session=[Diagnostics.Process]::GetCurrentProcess().SessionId
 return @(Get-Process $Name -ErrorAction SilentlyContinue | Where-Object {$_.SessionId -eq $session})
}
function Get-Any2OdysseyShortcutTarget {
 param([string]$Path)
 $item=(New-Object -ComObject Shell.Application).Namespace((Split-Path -Parent $Path)).ParseName((Split-Path -Leaf $Path))
 if(-not $item -or -not $item.IsLink){return $null}
 return $item.GetLink.Path
}
function New-Any2OdysseyShortcut {
 # Shell.Application's link object takes Unicode strings. WScript.Shell converts paths through the
 # ANSI code page: with a profile folder outside it (Cyrillic, CJK, some Latin letters) it throws
 # or saves a shortcut to a different path. The link object edits an existing file, so start empty.
 param([string]$Path,[string]$Target,[string]$WorkingDirectory,[string]$Icon,[string]$Description)
 [IO.File]::WriteAllBytes($Path,[byte[]]@())
 $item=(New-Object -ComObject Shell.Application).Namespace((Split-Path -Parent $Path)).ParseName((Split-Path -Leaf $Path))
 if(-not $item){throw "Could not open the new shortcut file: $Path"}
 $link=$item.GetLink
 $link.Path=$Target;$link.WorkingDirectory=$WorkingDirectory;$link.Description=$Description
 [void]$link.SetIconLocation($Icon,0)
 $link.Save()
 if((Get-Any2OdysseyShortcutTarget -Path $Path) -ne $Target){throw "The shortcut was not saved with its target ($Target)."}
}
function Get-Any2OdysseyPackageVersion {
 param([string]$Binary)
 $info=[Diagnostics.FileVersionInfo]::GetVersionInfo($Binary)
 $text=if($info.FileVersion){$info.FileVersion}else{$info.ProductVersion}
 if(-not $text -or $text -notmatch '^\d+(\.\d+){1,3}'){throw "The packaged executable has no readable version resource: $Binary"}
 return [version]$Matches[0]
}
function ConvertTo-Any2OdysseyFourPartVersion {
 param([version]$Version)
 # A version resource may omit build or revision ([version] reports -1); compare on four parts.
 return [version]::new($Version.Major,$Version.Minor,[Math]::Max(0,$Version.Build),[Math]::Max(0,$Version.Revision))
}
function Get-Any2OdysseyInstallAction {
 param([version]$PackageVersion,$InstalledVersion,[switch]$AllowDowngrade)
 if($null -eq $InstalledVersion){return 'Install'}
 $package=ConvertTo-Any2OdysseyFourPartVersion $PackageVersion
 $installed=ConvertTo-Any2OdysseyFourPartVersion $InstalledVersion
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
 # Folders this transaction creates, outermost first; rollback removes the ones left empty.
 $createdFolders=New-Object 'System.Collections.Generic.List[string]'
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
   $folder=Split-Path -Parent $snapshot.Destination;$missing=@()
   while($folder -and -not (Test-Path -LiteralPath $folder)){$missing=@($folder)+$missing;$folder=Split-Path -Parent $folder}
   foreach($new in $missing){if(-not $createdFolders.Contains($new)){$createdFolders.Add($new)}}
   New-Item -ItemType Directory -Path (Split-Path -Parent $snapshot.Destination) -Force -ErrorAction Stop | Out-Null
   $written.Add($snapshot)
   Copy-Item -LiteralPath $snapshot.Source -Destination $snapshot.Destination -Force -ErrorAction Stop
  }
  & $AfterCopy
 } catch {
  $failure=$_;$rollbackErrors=@()
  for($i=$written.Count-1;$i -ge 0;$i--){$snapshot=$written[$i];try{if($snapshot.Existed){Copy-Item -LiteralPath $snapshot.Backup -Destination $snapshot.Destination -Force -ErrorAction Stop}else{if(Test-Path -LiteralPath $snapshot.Destination -PathType Leaf){Remove-Item -LiteralPath $snapshot.Destination -ErrorAction Stop}}}catch{$rollbackErrors+=$_.Exception.Message}}
  try{& $RollbackState}catch{$rollbackErrors+=$_.Exception.Message}
  for($i=$createdFolders.Count-1;$i -ge 0;$i--){$folder=$createdFolders[$i];try{if((Test-Path -LiteralPath $folder -PathType Container) -and -not (Get-ChildItem -LiteralPath $folder -Force | Select-Object -First 1)){Remove-Item -LiteralPath $folder -ErrorAction Stop}}catch{$rollbackErrors+=$_.Exception.Message}}
  if($rollbackErrors.Count){throw "Update failed: $($failure.Exception.Message). Rollback needs attention: $($rollbackErrors -join '; '). Backups retained at $backup"}
  throw "Update failed; previous files and registration restored. $($failure.Exception.Message) Backups: $backup"
 }
 foreach($snapshot in $snapshots){if(Test-Path -LiteralPath $snapshot.Backup){Remove-Item -LiteralPath $snapshot.Backup}}
 Remove-Item -LiteralPath $backup
}
