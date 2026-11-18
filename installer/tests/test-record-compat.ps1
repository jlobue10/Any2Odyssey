# The installation record (version.json) as every release wrote it, read the way this release's
# installer and uninstaller read it, and the record this release writes beside them. Nothing is
# installed and nothing outside a temporary folder is written.
#
# crates\core\tests\fixtures\releases\<tag>\windows-record\<pwsh|ps51>\<case>\version.json were made by
# running the two statements of each release's install.ps1 that build and write the record, under
# PowerShell 7 and under Windows PowerShell 5.1 (the README there says how). The same is done here
# with this release's install.ps1.
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot '..\InstallSupport.ps1')
function Assert($condition,[string]$message){if(-not $condition){throw $message};Write-Host "PASS: $message"}
$repo=Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$releases=Join-Path $repo 'crates\core\tests\fixtures\releases'
# The profile folder the records were made for: Latin, Cyrillic and CJK letters, an apostrophe and
# an ampersand (written as code points to keep this file ASCII).
$recordApp='C:\Users\Zo'+[char]0x00EB+' '+[char]0x0416+[char]0x0142+[char]0x4E2D+" O'Brien & Co\AppData\Local\Programs\Any2Odyssey"
$names=@('Installed','Version','PreviousVersion','Action','App','ControlsOnly','Worker','Hash')
# Uninstall.ps1 reads the record with this statement; the copy below must stay the one it runs.
$uninstallReads='$record=Get-Content -LiteralPath $versionFile -Raw | ConvertFrom-Json;$controlsOnly=$record.ControlsOnly -eq $true'
Assert ((Get-Content -LiteralPath (Join-Path $repo 'installer\Uninstall.ps1') -Raw).Contains($uninstallReads)) 'The uninstaller reads the record the way this test does'
$tags=@(Get-ChildItem -LiteralPath $releases -Directory | Where-Object {$_.Name -match '^v\d+\.\d+\.\d+$'} | Sort-Object {[version]$_.Name.Substring(1)})
Assert ($tags.Count -ge 14 -and $tags[0].Name -eq 'v0.8.0') "Records of $($tags.Count) releases, from 0.8.0 on, are in the corpus"
$read=0
foreach($tag in $tags){
 $version=$tag.Name.Substring(1)
 foreach($shell in @('pwsh','ps51')){
  foreach($case in @(@{Name='install-full';ControlsOnly=$false;Action='Install'},@{Name='upgrade-controls-only';ControlsOnly=$true;Action='Upgrade'})){
   $versionFile=Join-Path $tag.FullName (Join-Path 'windows-record' (Join-Path $shell (Join-Path $case.Name 'version.json')))
   $what="$($tag.Name) $shell $($case.Name)"
   if(-not (Test-Path -LiteralPath $versionFile -PathType Leaf)){throw "$what`: the record is missing from the corpus"}
   # The installer's fallback when the installed program has no readable version.
   $recorded=Get-Any2OdysseyRecordedVersion -Path $versionFile
   if("$recorded" -ne $version){throw "$what`: the installer reads version '$recorded'"}
   # The uninstaller: a controls-only record must never be taken for a full one, nor the reverse.
   $record=$null;$controlsOnly=$null
   Invoke-Expression $uninstallReads
   if($controlsOnly -ne $case.ControlsOnly){throw "$what`: the uninstaller reads ControlsOnly as '$controlsOnly'"}
   if($record.Action -ne $case.Action){throw "$what`: Action is '$($record.Action)'"}
   foreach($name in $names){if($record.PSObject.Properties.Name -notcontains $name){throw "$what`: the record has no $name"}}
   # A record without a byte-order mark (0.8.0 to 0.9.0 under PowerShell 7) is read in the ANSI code
   # page by Windows PowerShell: the folder's letters come back wrong there, which nothing here uses.
   $marked=([IO.File]::ReadAllBytes($versionFile)[0] -eq 0xEF)
   if(($marked -or $PSVersionTable.PSVersion.Major -ge 6) -and $record.App -cne $recordApp){throw "$what`: the application folder is read as '$($record.App)'"}
   $read++
  }
 }
}
Assert ($read -ge 56) "$read records of earlier releases are read as written by the installer and the uninstaller"

# This release's record: the same two statements of install.ps1, with stand-in values.
$lines=Get-Content -LiteralPath (Join-Path $repo 'installer\install.ps1')
$build=@($lines | Where-Object {$_ -match '^\$record='})
$write=@($lines | Where-Object {$_ -match "version\.json'\)" -and $_ -match 'WriteAllText'})
Assert ($build.Count -eq 1 -and $write.Count -eq 1) 'install.ps1 builds the record in one statement and writes it in one'
$root=Join-Path ([IO.Path]::GetTempPath()) ('Any2Odyssey-record-test-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root | Out-Null
$passed=$false
try{
 $stage=$root
 $binary=Join-Path $root 'stand-in.bin';[IO.File]::WriteAllBytes($binary,[byte[]](1,2,3))
 $versionText=(Get-Content -LiteralPath (Join-Path $repo 'VERSION') -Raw).Trim()
 $app=$recordApp;$engine='C:\Program Files\Odyssey 3D Hub\Conversion';$workerName='Any2OdysseyWorker.exe'
 # The installed version as the three sources give it: a file version (four parts), a record or a
 # registered DisplayVersion (three), and a file version with a real fourth part.
 foreach($previous in @(@{Installed=[version]'0.9.4.0';Recorded='0.9.4'},@{Installed=[version]'0.9.4';Recorded='0.9.4'},@{Installed=[version]'0.9.4.2';Recorded='0.9.4.2'},@{Installed=$null;Recorded=$null})){
  $installed=if($previous.Installed){[pscustomobject]@{Version=$previous.Installed}}else{$null}
  $action=if($installed){'Upgrade'}else{'Install'}
  $ControlsOnly=[switch]$false
  Invoke-Expression $build[0]
  Invoke-Expression $write[0]
  $versionFile=Join-Path $root 'version.json'
  $bytes=[IO.File]::ReadAllBytes($versionFile)
  if($bytes[0] -ne 0xEF -or $bytes[1] -ne 0xBB -or $bytes[2] -ne 0xBF){throw 'The record is written without a byte-order mark'}
  $written=Get-Content -LiteralPath $versionFile -Raw | ConvertFrom-Json
  if($written.PreviousVersion -cne $previous.Recorded){throw "An installed version $($previous.Installed) is recorded as replaced version '$($written.PreviousVersion)', not '$($previous.Recorded)'"}
  if((@($written.PSObject.Properties.Name) -join ',') -cne ($names -join ',')){throw "The record's values are now $(@($written.PSObject.Properties.Name) -join ','): earlier releases wrote and read $($names -join ',')"}
  if($written.Version -cne $versionText -or $written.App -cne $recordApp -or $written.ControlsOnly -ne $false -or $written.Worker -cne (Join-Path $engine $workerName)){throw 'The record does not hold what the installation was given'}
  if("$(Get-Any2OdysseyRecordedVersion -Path $versionFile)" -ne $versionText){throw 'The installer does not read back the version it recorded'}
 }
 Assert $true 'This release writes the eight values every release wrote, marked as UTF-8, with the replaced version as the releases are named'
 $passed=$true
}finally{
 if($passed){Remove-Item -LiteralPath $root -Recurse -Force}else{Write-Host "Fixtures retained for inspection: $root"}
}

# Assertion failures throw above; a clean run must not inherit a native command's exit code.
exit 0
