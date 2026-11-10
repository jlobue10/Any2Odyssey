[CmdletBinding()]
param([switch]$ControlsOnly,[switch]$AllowDowngrade,[switch]$RunInstall,[string]$ResultFile,[string]$ExpectedUserSid)
$ErrorActionPreference='Stop'
$installer=Join-Path $PSScriptRoot 'install.ps1'
$options=@{ControlsOnly=[bool]$ControlsOnly}
# Passed on only when asked for: to the preflight, to a direct installation and to the elevated one.
if($AllowDowngrade){$options.AllowDowngrade=$true}
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()

if($RunInstall){
 # Relay the elevated result to the original console instead of losing its error.
 $result=@{Success=$false;Message='Installer did not complete.'}
 try{
  # From a standard account the elevation prompt asks for an administrator's password, and the
  # elevated step then runs as that administrator: the application, shortcuts and settings would
  # land in the administrator's profile, not in the profile of the person who started setup.
  if($ExpectedUserSid -and $identity.User.Value -ne $ExpectedUserSid){throw "Elevation used a different Windows account ($($identity.Name)) than the one that started setup, so nothing was installed. The full converter is installed for the account that approves elevation, which therefore has to be an administrator account itself. Sign in to the administrator account that will use Any2Odyssey and run Install.cmd there, or run Install-GUI.cmd from this account for the desktop controls only (no 3D conversion)."}
  & $installer @options
  $result.Success=$true;$result.Message='Any2Odyssey installation completed. Open Any2Odyssey from the desktop or Start menu.'
 }catch{$result.Message=$_.Exception.Message}
 if($ResultFile){[IO.File]::WriteAllText($ResultFile,($result | ConvertTo-Json),[Text.UTF8Encoding]::new($false))}
 if($result.Success){exit 0}else{Write-Host $result.Message;exit 1}
}

$report=$null
try{
 # Package/engine/version failures stay visible here, before a UAC prompt.
 & $installer @options -CheckOnly
 $principal=New-Object Security.Principal.WindowsPrincipal($identity)
 if($ControlsOnly -or $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){
  # Already elevated ("Run as administrator", UAC switched off, the built-in Administrator): no
  # prompt will show which account this is, so say whose installation this becomes.
  if(-not $ControlsOnly){Write-Host "This window already has administrator rights. Any2Odyssey, its shortcuts and its settings are installed for the Windows account $($identity.Name) only ($env:LOCALAPPDATA\Programs\Any2Odyssey). If it was meant for another account, uninstall it afterwards and run Install.cmd signed in as that account, without 'Run as administrator'."}
  & $installer @options
 }else{
  $report=Join-Path ([IO.Path]::GetTempPath()) ('Any2Odyssey-setup-'+[Guid]::NewGuid().ToString('N')+'.json')
  $arguments='-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "'+$PSCommandPath+'" -RunInstall -ResultFile "'+$report+'" -ExpectedUserSid "'+$identity.User.Value+'"'+$(if($AllowDowngrade){' -AllowDowngrade'}else{''})
  try{$process=Start-Process -FilePath (Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe') -ArgumentList $arguments -Verb RunAs -WindowStyle Hidden -Wait -PassThru}
  catch [InvalidOperationException]{
   # A declined UAC prompt surfaces as InvalidOperationException wrapping Win32Exception 1223
   # (ERROR_CANCELLED); the elevated step never started. Any other launch failure keeps its own message.
   $inner=$_.Exception.InnerException
   if($inner -is [ComponentModel.Win32Exception] -and $inner.NativeErrorCode -eq 1223){throw 'Elevation was declined. Nothing was installed.'}
   throw
  }
  if(-not (Test-Path -LiteralPath $report)){throw "Installer exited with code $($process.ExitCode) without a result. Retry setup and check whether security software interrupted PowerShell, or extract the package to a local folder: an elevated process cannot see mapped network drives."}
  # Written as UTF-8 without a byte-order mark by the elevated step; Windows PowerShell's Get-Content
  # would read such a file in the ANSI code page and garble a profile path with other characters.
  $result=[IO.File]::ReadAllText($report,[Text.Encoding]::UTF8) | ConvertFrom-Json
  if(-not $result.Success -or $process.ExitCode -ne 0){throw $result.Message}
  Write-Host $result.Message
 }
 exit 0
}catch{
 Write-Host ''
 Write-Host ('Installation did not complete: '+$_.Exception.Message)
 exit 1
}finally{
 if($report -and (Test-Path -LiteralPath $report)){Remove-Item -LiteralPath $report}
}
