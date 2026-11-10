[CmdletBinding()]
param([string]$ExpectedUserSid)
$ErrorActionPreference='Stop'

# This helper is installed under Program Files. It has no per-user imports,
# executable launches, registry cleanup or caller-selected deletion paths.
function Assert-Any2OdysseyUninstallContext {
 param([string]$ExpectedSid,[string]$ActualSid,[bool]$IsAdmin,[string]$ScriptPath,[string]$InstalledPath)
 if(-not $ExpectedSid -or $ExpectedSid -ne $ActualSid){throw 'Elevation used a different Windows account than the one that started the uninstall. Approve the prompt with the account that installed Any2Odyssey; it has to be an administrator account.'}
 if(-not $IsAdmin){throw 'Worker removal requires administrator access.'}
 if(-not [string]::Equals([IO.Path]::GetFullPath($ScriptPath),[IO.Path]::GetFullPath($InstalledPath),[StringComparison]::OrdinalIgnoreCase)){throw 'Run only the installed worker-removal helper from Program Files.'}
}

function Remove-Any2OdysseyInstalledWorker {
 param([string]$HelperPath)
 $worker='C:\Program Files\Odyssey 3D Hub\Conversion\Any2OdysseyWorker.exe'
 # Elevated, this sees every signed-in session, and the worker is one file for all of them.
 $running=@(Get-Process Any2Odyssey,Any2OdysseyWorker -ErrorAction SilentlyContinue)
 if($running.Count){throw "Any2Odyssey is still running on this PC (Windows session $((@($running | ForEach-Object {$_.SessionId} | Sort-Object -Unique)) -join ', ')). The conversion worker cannot be removed while any account uses it: quit Any2Odyssey from its tray menu in every signed-in account, then uninstall again."}
 if(Test-Path -LiteralPath $worker){Remove-Item -LiteralPath $worker}
 Remove-Item -LiteralPath $HelperPath
 $folder=Split-Path -Parent $HelperPath
 # Beside the helper live the installer's ownership marker (the installing account's SID) and,
 # after a run that failed, the note that told the uninstaller why.
 foreach($name in @('UninstallWorker.owner','UninstallWorker.result')){
  $leftover=Join-Path $folder $name
  if(Test-Path -LiteralPath $leftover -PathType Leaf){Remove-Item -LiteralPath $leftover}
 }
 if((Split-Path -Leaf $folder) -eq 'Any2Odyssey' -and -not (Get-ChildItem -LiteralPath $folder -Force | Select-Object -First 1)){Remove-Item -LiteralPath $folder}
}

# Dot sourcing is used only by isolated tests to load the pure checks.
if($MyInvocation.InvocationName -ne '.'){
 $installed=Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'Any2Odyssey\UninstallWorker.ps1'
 # This window closes with the script, so the reason for a failure is left beside the installed
 # helper for the unelevated uninstaller to show. The path is fixed and under Program Files, where
 # only administrators can write; nothing is read from it here, and no path chosen by the caller
 # is ever written.
 $resultFile=Join-Path (Split-Path -Parent $installed) 'UninstallWorker.result'
 try{
  $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
  $principal=New-Object Security.Principal.WindowsPrincipal($identity)
  Assert-Any2OdysseyUninstallContext -ExpectedSid $ExpectedUserSid -ActualSid $identity.User.Value -IsAdmin $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator) -ScriptPath $PSCommandPath -InstalledPath $installed
  Remove-Any2OdysseyInstalledWorker -HelperPath $PSCommandPath
 }catch{
  $reason=$_.Exception.Message
  try{[IO.File]::WriteAllText($resultFile,$reason,[Text.UTF8Encoding]::new($false))}catch{}
  Write-Host $reason
  exit 1
 }
}
