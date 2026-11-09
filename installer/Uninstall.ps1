param([switch]$Pause)
$ErrorActionPreference='Stop'
# Installed apps launches this script in a console that closes with it, so with -Pause (the
# registered UninstallString) a failure stays on screen until a key is pressed. Callers that
# capture the error (the README command, the tests) omit the switch and get the throw as before.
try{
$app=Join-Path $env:LOCALAPPDATA 'Programs\Any2Odyssey'
$controlsOnly=$false
$versionFile=Join-Path $app 'version.json'
if(Test-Path -LiteralPath $versionFile){
 try{$record=Get-Content -LiteralPath $versionFile -Raw | ConvertFrom-Json;$controlsOnly=$record.ControlsOnly -eq $true}
 catch{
  # A truncated or damaged record must not block the uninstall; the files decide instead.
  $controlsOnly=$false
  Write-Host 'The installation record (version.json) is unreadable; whether the conversion worker or its helper exists decides what is removed.'
 }
}
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
$principal=New-Object Security.Principal.WindowsPrincipal($identity)
# True when this process already holds an administrator token (UAC disabled, the
# built-in Administrator account or an elevated console). There is no higher token
# to request then, so the helper runs as this process instead of through UAC.
$alreadyElevated=$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if(-not [Environment]::Is64BitProcess){throw 'Use 64-bit PowerShell to uninstall Any2Odyssey.'}
$exe=Join-Path $app 'Any2Odyssey.exe'
$worker='C:\Program Files\Odyssey 3D Hub\Conversion\Any2OdysseyWorker.exe'
$helper=Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'Any2Odyssey\UninstallWorker.ps1'
# Signal the existing process directly; never execute a writable installed EXE.
try{
 $quit=[Threading.EventWaitHandle]::OpenExisting('Local\Any2OdysseyQuit')
 try{$null=$quit.Set()}finally{$quit.Dispose()}
}catch [Threading.WaitHandleCannotBeOpenedException]{
}catch [UnauthorizedAccessException]{
 # The instance was started elevated, so its event refuses this token. Fall through
 # to the wait below; its message then asks for a manual quit instead of a raw error.
}
$deadline=(Get-Date).AddSeconds(12)
while((Get-Process Any2Odyssey,Any2OdysseyWorker -ErrorAction SilentlyContinue) -and (Get-Date) -lt $deadline){Start-Sleep -Milliseconds 250}
if(Get-Process Any2Odyssey,Any2OdysseyWorker -ErrorAction SilentlyContinue){throw 'Exit Any2Odyssey from its tray, then run uninstall again.'}
# A full installation removes its worker through the helper. A controls-only record never
# elevates for a worker it did not install: a helper that is still present then belongs to
# another account's full installation or to an older release, and is removed by running
# Install.cmd and uninstalling again from that account (or by an administrator by hand).
# The installer writes the installing account's SID beside the helper (UninstallWorker.owner).
# On a shared PC a later Install.cmd from another account replaces the worker and that marker,
# so a full record whose marker names another account leaves both alone as well.
$ownerFile=Join-Path (Split-Path -Parent $helper) 'UninstallWorker.owner'
$helperPresent=Test-Path -LiteralPath $helper -PathType Leaf
$ownerSid=''
if($helperPresent -and (Test-Path -LiteralPath $ownerFile -PathType Leaf)){$ownerSid=([string](Get-Content -LiteralPath $ownerFile -Raw)).Trim()}
$helperIsForeign=$helperPresent -and $ownerSid -ne '' -and $ownerSid -ne $identity.User.Value
$needsHelper=(-not $controlsOnly) -and ((Test-Path -LiteralPath $worker) -or $helperPresent) -and -not $helperIsForeign
if($helperPresent -and ($controlsOnly -or $helperIsForeign)){Write-Host 'A full installation''s conversion worker and uninstall helper in Program Files belong to another account or an older release; they are left alone.'}
if($needsHelper){
 if(-not (Test-Path -LiteralPath $helper -PathType Leaf)){throw 'The protected uninstall helper is missing. Repair the full installation with Install.cmd, then retry uninstall.'}
 $powershell=Join-Path ([Environment]::SystemDirectory) 'WindowsPowerShell\v1.0\powershell.exe'
 $arguments='-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "'+$helper+'" -ExpectedUserSid "'+$identity.User.Value+'"'
 # The helper itself checks the approving account, its own Program Files path and the fixed worker path.
 $launch=@{FilePath=$powershell;ArgumentList=$arguments;Wait=$true;PassThru=$true}
 if(-not $alreadyElevated){$launch.Verb='RunAs'}
 try{$removal=Start-Process @launch}
 catch [InvalidOperationException]{
  # Declining the UAC prompt surfaces as this exception; nothing has been removed yet.
  throw 'Elevation was declined. The conversion worker needs administrator approval to be removed; no files were changed.'
 }
 if($removal.ExitCode -ne 0){throw 'Worker removal did not complete. No per-user installation files were removed. Retry using the same Windows account.'}
}
$shell=New-Object -ComObject WScript.Shell
foreach($folder in @([Environment]::GetFolderPath('Desktop'),[Environment]::GetFolderPath('Programs'))){
 $link=Join-Path $folder 'Any2Odyssey.lnk'
 if((Test-Path -LiteralPath $link) -and $shell.CreateShortcut($link).TargetPath -eq $exe){Remove-Item -LiteralPath $link}
}
# Exact file list only: leave Samsung files, personal settings and logs intact.
$ownedFiles=@($exe,(Join-Path $app 'Any2Odyssey.ico'),(Join-Path $app 'LICENSE'),(Join-Path $app 'NOTICE'),(Join-Path $app 'THIRD-PARTY-LICENSES.md'),(Join-Path $app 'USER-GUIDE.html'),(Join-Path $app 'Uninstall.ps1'))
foreach($file in $ownedFiles){
 if(Test-Path -LiteralPath $file){Remove-Item -LiteralPath $file}
}
$registration='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Any2Odyssey'
if(Test-Path -LiteralPath $versionFile){Remove-Item -LiteralPath $versionFile}
if(Test-Path -LiteralPath $registration){Remove-Item -LiteralPath $registration}
# The program folder held only the files above; it stays if anything else was placed in it.
if((Test-Path -LiteralPath $app -PathType Container) -and -not (Get-ChildItem -LiteralPath $app -Force | Select-Object -First 1)){Remove-Item -LiteralPath $app -ErrorAction SilentlyContinue}
Write-Host 'Any2Odyssey removed. Samsung software, your settings and logs were retained. No reboot is needed.'
}catch{
 if($Pause){
  Write-Host ('Uninstall did not complete: '+$_.Exception.Message) -ForegroundColor Red
  Write-Host 'Press any key to close this window.'
  try{[void][Console]::ReadKey($true)}catch{}
  exit 1
 }
 throw
}
