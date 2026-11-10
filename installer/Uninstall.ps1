param([switch]$Pause)
$ErrorActionPreference='Stop'
# Installed apps launches this script in a console that closes with it, so with -Pause (the
# registered UninstallString) a failure, or a notice about something left in place, stays on
# screen until a key is pressed. Callers that capture the error (the README command, the tests)
# omit the switch and get the throw as before.
$notices=0
try{
$app=Join-Path $env:LOCALAPPDATA 'Programs\Any2Odyssey'
$controlsOnly=$false
$versionFile=Join-Path $app 'version.json'
if(Test-Path -LiteralPath $versionFile){
 try{$record=Get-Content -LiteralPath $versionFile -Raw | ConvertFrom-Json;$controlsOnly=$record.ControlsOnly -eq $true}
 catch{
  # A truncated or damaged record must not block the uninstall; the files decide instead.
  $controlsOnly=$false;$notices++
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
# Only this session's instance received that signal (the event is per session), and only it runs
# this account's files. An instance in another signed-in session does not block removing them; the
# elevated helper checks every session before it removes the worker, and says which one holds it.
$session=[Diagnostics.Process]::GetCurrentProcess().SessionId
$running={@(Get-Process Any2Odyssey,Any2OdysseyWorker -ErrorAction SilentlyContinue | Where-Object {$_.SessionId -eq $session})}
$deadline=(Get-Date).AddSeconds(12)
while(@(& $running).Count -and (Get-Date) -lt $deadline){Start-Sleep -Milliseconds 250}
if(@(& $running).Count){throw 'Exit Any2Odyssey from its tray, then run uninstall again.'}
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
if($helperPresent -and ($controlsOnly -or $helperIsForeign)){$notices++;Write-Host 'A full installation''s conversion worker and uninstall helper in Program Files belong to another account or an older release; they are left alone.'}
if($needsHelper){
 if(-not (Test-Path -LiteralPath $helper -PathType Leaf)){throw 'The protected uninstall helper is missing. Repair the full installation with Install.cmd, then retry uninstall.'}
 $powershell=Join-Path ([Environment]::SystemDirectory) 'WindowsPowerShell\v1.0\powershell.exe'
 $arguments='-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "'+$helper+'" -ExpectedUserSid "'+$identity.User.Value+'"'
 # The helper itself checks the approving account, its own Program Files path and the fixed worker path.
 $launch=@{FilePath=$powershell;ArgumentList=$arguments;Wait=$true;PassThru=$true}
 if(-not $alreadyElevated){$launch.Verb='RunAs'}
 # The helper's window closes with it. When it fails it leaves the reason beside itself in Program
 # Files, where only administrators can write; a note older than this launch is from an earlier run.
 $resultFile=Join-Path (Split-Path -Parent $helper) 'UninstallWorker.result'
 $launchedUtc=[DateTime]::UtcNow
 try{$removal=Start-Process @launch}
 catch [InvalidOperationException]{
  # A declined UAC prompt surfaces as InvalidOperationException wrapping Win32Exception 1223
  # (ERROR_CANCELLED); nothing has been removed yet. Any other launch failure keeps its own message.
  $inner=$_.Exception.InnerException
  if($inner -is [ComponentModel.Win32Exception] -and $inner.NativeErrorCode -eq 1223){throw 'Elevation was declined. The conversion worker needs administrator approval to be removed; no files were changed.'}
  throw
 }
 if($removal.ExitCode -ne 0){
  $reason=''
  try{
   if((Test-Path -LiteralPath $resultFile -PathType Leaf) -and (Get-Item -LiteralPath $resultFile).LastWriteTimeUtc -ge $launchedUtc.AddSeconds(-2)){$reason=([string][IO.File]::ReadAllText($resultFile,[Text.Encoding]::UTF8)).Trim()}
  }catch{}
  if($reason -and $reason -notmatch '[.!?]$'){$reason+='.'}
  if($reason){throw "Worker removal did not complete: $reason No per-user installation files were removed."}
  throw "Worker removal did not complete and its helper gave no reason (exit code $($removal.ExitCode)). No per-user installation files were removed. Retry; if it fails again, check whether security software or a policy stops Windows PowerShell from running $helper"
 }
}
# Shell.Application reads the target as Unicode; WScript.Shell goes through the ANSI code page and
# fails or misreads it when the profile folder has characters outside that page.
$shellApp=New-Object -ComObject Shell.Application
foreach($folder in @([Environment]::GetFolderPath('Desktop'),[Environment]::GetFolderPath('Programs'))){
 # Windows reports a known folder that does not exist as an empty path; there is no shortcut to look for.
 if(-not $folder){continue}
 $link=Join-Path $folder 'Any2Odyssey.lnk'
 if(-not (Test-Path -LiteralPath $link -PathType Leaf)){continue}
 $item=$shellApp.Namespace($folder).ParseName('Any2Odyssey.lnk')
 if($item -and $item.IsLink -and $item.GetLink.Path -eq $exe){Remove-Item -LiteralPath $link}
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
if($Pause -and $notices){
 Write-Host 'Read the notice above, then press any key to close this window.'
 try{[void][Console]::ReadKey($true)}catch{}
}
}catch{
 if($Pause){
  Write-Host ('Uninstall did not complete: '+$_.Exception.Message) -ForegroundColor Red
  Write-Host 'Press any key to close this window.'
  try{[void][Console]::ReadKey($true)}catch{}
  exit 1
 }
 throw
}
