[CmdletBinding()]
param([switch]$ControlsOnly,[switch]$RunInstall,[string]$ResultFile,[string]$ExpectedUserSid)
$ErrorActionPreference='Stop'
$installer=Join-Path $PSScriptRoot 'install.ps1'
$options=@{ControlsOnly=[bool]$ControlsOnly}
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()

if($RunInstall){
 # Relay the elevated result to the original console instead of losing its error.
 $result=@{Success=$false;Message='Installer did not complete.'}
 try{
  if($ExpectedUserSid -and $identity.User.Value -ne $ExpectedUserSid){throw 'Elevation used a different Windows account. Cancel and run setup using the account that will use Any2Odyssey.'}
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
  & $installer @options
 }else{
  $report=Join-Path ([IO.Path]::GetTempPath()) ('Any2Odyssey-setup-'+[Guid]::NewGuid().ToString('N')+'.json')
  $arguments='-NoProfile -ExecutionPolicy Bypass -File "'+$PSCommandPath+'" -RunInstall -ResultFile "'+$report+'" -ExpectedUserSid "'+$identity.User.Value+'"'
  $process=Start-Process -FilePath (Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe') -ArgumentList $arguments -Verb RunAs -WindowStyle Hidden -Wait -PassThru
  if(-not (Test-Path -LiteralPath $report)){throw "Installer exited with code $($process.ExitCode) without a result. Retry setup and check whether security software interrupted PowerShell."}
  $result=Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
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
