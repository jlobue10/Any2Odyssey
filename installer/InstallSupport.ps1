# Shared, independently testable installer operations. No work is done when dot-sourced.
function Get-Any2OdysseyRecordedVersion {
 param([string]$Path)
 # version.json: a damaged record (truncated JSON, empty file) counts as
 # absent so the next source decides, instead of blocking every install and uninstall.
 if(-not (Test-Path -LiteralPath $Path -PathType Leaf)){return $null}
 try{$record=Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json;if($record.Version){return [version]$record.Version}}catch{}
 return $null
}
function Get-Any2OdysseyFileVersion {
 param([string]$Path)
 # A native executable: read its VERSIONINFO resource, not a .NET assembly name. $null when the
 # file carries no readable version (a truncated copy, or a file that is not this program).
 $info=[Diagnostics.FileVersionInfo]::GetVersionInfo($Path)
 $text=if($info.FileVersion){$info.FileVersion}elseif($info.ProductVersion){$info.ProductVersion}else{$null}
 if($text -and $text -match '^\d+(\.\d+){1,3}'){return [version]$Matches[0]}
 return $null
}
function Get-Any2OdysseyInstalledVersion {
 param([string]$AppPath,[string]$Registration,[string]$DataPath)
 $path=Join-Path $AppPath 'Any2Odyssey.exe'
 if(Test-Path -LiteralPath $path -PathType Leaf){
  $fromResource=Get-Any2OdysseyFileVersion -Path $path
  if($fromResource){return [pscustomobject]@{Version=$fromResource;Source=$path}}
  $record=Join-Path $AppPath 'version.json'
  $recorded=Get-Any2OdysseyRecordedVersion -Path $record
  if($recorded){return [pscustomobject]@{Version=$recorded;Source=$record}}
  # An installation interrupted while this file was being copied leaves it truncated, before any
  # record exists; without a version the downgrade guard cannot judge it, so say what to do.
  throw "The installed program file has no readable version and there is no installation record beside it: $path. An interrupted installation or a damaged file leaves it like this. Delete that file, then run the installer again."
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
 # version.json in the application folder is removed by the uninstaller and does count.
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
 $fromResource=Get-Any2OdysseyFileVersion -Path $Binary
 if(-not $fromResource){throw "The packaged executable has no readable version resource: $Binary"}
 return $fromResource
}
function ConvertTo-Any2OdysseyFourPartVersion {
 param([version]$Version)
 # A version resource may omit build or revision ([version] reports -1); compare on four parts.
 return [version]::new($Version.Major,$Version.Minor,[Math]::Max(0,$Version.Build),[Math]::Max(0,$Version.Revision))
}
function Format-Any2OdysseyVersion {
 param([version]$Version)
 # 0.8.6.0 reads as 0.8.6, the form VERSION and the release names use; a real fourth part is kept.
 $four=ConvertTo-Any2OdysseyFourPartVersion $Version
 if($four.Revision -eq 0){return $four.ToString(3)}
 return $four.ToString()
}
function Get-Any2OdysseyInstallAction {
 param([version]$PackageVersion,$InstalledVersion,[switch]$AllowDowngrade)
 if($null -eq $InstalledVersion){return 'Install'}
 $package=ConvertTo-Any2OdysseyFourPartVersion $PackageVersion
 $installed=ConvertTo-Any2OdysseyFourPartVersion $InstalledVersion
 if($package -gt $installed){return 'Upgrade'}
 if($package -eq $installed){return 'Repair'}
 if(-not $AllowDowngrade){throw "Installed version $(Format-Any2OdysseyVersion $installed) is newer than this package ($(Format-Any2OdysseyVersion $package)). Downgrade blocked; nothing was changed. To install this older version on purpose, open a terminal in the extracted folder and run .\Install.cmd -AllowDowngrade (or .\Install-GUI.cmd -AllowDowngrade for the controls only)."}
 return 'Downgrade'
}
function Assert-Any2OdysseyWorkerNotNewer {
 param([version]$PackageVersion,$WorkerVersion,[switch]$AllowDowngrade,[string]$OwnerSid,[string]$CurrentSid)
 # The conversion worker beside Samsung's engine is one file for the whole PC, and every account's
 # Any2Odyssey must drive a worker of its own version. The per-user downgrade guard cannot see it:
 # for a second account an older package is a plain Install. A worker without a readable version
 # is not judged and is replaced as before.
 if($null -eq $WorkerVersion -or $AllowDowngrade){return}
 $package=ConvertTo-Any2OdysseyFourPartVersion $PackageVersion
 $worker=ConvertTo-Any2OdysseyFourPartVersion $WorkerVersion
 if($worker -le $package){return}
 $whose=if($OwnerSid -and $OwnerSid -ne $CurrentSid){" It was installed by another Windows account ($OwnerSid); replacing it would leave that account's newer Any2Odyssey with an older worker."}else{''}
 throw "The conversion worker on this PC is version $(Format-Any2OdysseyVersion $worker), newer than this package ($(Format-Any2OdysseyVersion $package)).$whose Downgrade blocked; nothing was changed. Install the newer release instead, or to replace the worker with this older one on purpose open a terminal in the extracted folder and run .\Install.cmd -AllowDowngrade."
}
function Test-Any2OdysseyFileInUse {
 param($Exception)
 # Win32 sharing violation (32) or lock violation (33): another process holds the file.
 if($Exception -isnot [IO.IOException]){return $false}
 $code=$Exception.HResult -band 0xFFFF
 return ($code -eq 32 -or $code -eq 33)
}
function Copy-Any2OdysseyFile {
 param([string]$Source,[string]$Destination)
 # The program that was asked to quit a moment ago, or a scanner reading a new file, can hold a
 # file for an instant. A few short retries cover that; any other failure is reported at once.
 for($attempt=1;$true;$attempt++){
  try{
   Copy-Item -LiteralPath $Source -Destination $Destination -Force -ErrorAction Stop
   # Copy-Item carries the source's read-only attribute over (a package unpacked by a tool that
   # keeps it, or copied from read-only media). An installed file must not keep it: a rollback
   # could not remove the file again, and the uninstallers up to 0.9.2 stopped at it.
   $copied=Get-Item -LiteralPath $Destination -Force -ErrorAction SilentlyContinue
   if($copied -is [IO.FileInfo] -and $copied.IsReadOnly){$copied.IsReadOnly=$false}
   return
  }
  catch{
   if($attempt -ge 5 -or -not (Test-Any2OdysseyFileInUse $_.Exception)){throw}
   Start-Sleep -Milliseconds 400
  }
 }
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
   if($existed){Copy-Any2OdysseyFile -Source $operation.Destination -Destination $saved}
   $snapshots+= [pscustomobject]@{Source=$operation.Source;Destination=$operation.Destination;Existed=$existed;Backup=$saved}
  }
  foreach($snapshot in $snapshots){
   $folder=Split-Path -Parent $snapshot.Destination;$missing=@()
   while($folder -and -not (Test-Path -LiteralPath $folder)){$missing=@($folder)+$missing;$folder=Split-Path -Parent $folder}
   foreach($new in $missing){if(-not $createdFolders.Contains($new)){$createdFolders.Add($new)}}
   New-Item -ItemType Directory -Path (Split-Path -Parent $snapshot.Destination) -Force -ErrorAction Stop | Out-Null
   try{Copy-Any2OdysseyFile -Source $snapshot.Source -Destination $snapshot.Destination}
   catch{
    # A destination that could not even be opened (in use, or access denied) was not changed:
    # restoring it would fail the same way and report a rollback problem for an untouched file.
    # After any other failure it may be partly written, so the rollback puts it back.
    if(-not (Test-Any2OdysseyFileInUse $_.Exception) -and $_.Exception -isnot [UnauthorizedAccessException]){$written.Add($snapshot)}
    throw
   }
   $written.Add($snapshot)
  }
  & $AfterCopy
 } catch {
  $failure=$_;$rollbackErrors=@()
  for($i=$written.Count-1;$i -ge 0;$i--){$snapshot=$written[$i];try{if($snapshot.Existed){Copy-Any2OdysseyFile -Source $snapshot.Backup -Destination $snapshot.Destination}else{if(Test-Path -LiteralPath $snapshot.Destination -PathType Leaf){Remove-Item -LiteralPath $snapshot.Destination -ErrorAction Stop}}}catch{$rollbackErrors+=$_.Exception.Message}}
  try{& $RollbackState}catch{$rollbackErrors+=$_.Exception.Message}
  for($i=$createdFolders.Count-1;$i -ge 0;$i--){$folder=$createdFolders[$i];try{if((Test-Path -LiteralPath $folder -PathType Container) -and -not (Get-ChildItem -LiteralPath $folder -Force | Select-Object -First 1)){Remove-Item -LiteralPath $folder -ErrorAction Stop}}catch{$rollbackErrors+=$_.Exception.Message}}
  $reason=([string]$failure.Exception.Message).Trim()
  if($reason -notmatch '[.!?]$'){$reason+='.'}
  if(Test-Any2OdysseyFileInUse $failure.Exception){$reason+=' The file is in use: quit Any2Odyssey from its tray menu (Quit completely), close any program that has the file open, then retry.'}
  if($rollbackErrors.Count){throw "Update failed: $reason Rollback needs attention: $(($rollbackErrors | ForEach-Object {([string]$_).TrimEnd('.')}) -join '; '). Backups retained at $backup"}
  # Everything is back as it was, so the copies have served their purpose.
  try{Remove-Item -LiteralPath $backup -Recurse -Force -ErrorAction Stop}catch{}
  throw "Update failed; nothing was changed: previous files and registration were restored. $reason"
 }
 # The installation is complete. A copy that cannot be deleted right now (a scanner reading it in
 # %TEMP%) must not turn that into a reported failure.
 try{Remove-Item -LiteralPath $backup -Recurse -Force -ErrorAction Stop}catch{}
}
