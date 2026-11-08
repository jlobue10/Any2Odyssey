# Isolated security regression checks. No real installation or elevation.
$ErrorActionPreference='Stop'
$installer=Split-Path -Parent $PSScriptRoot
$root=Join-Path ([IO.Path]::GetTempPath()) ('Any2Odyssey-helper-test-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root | Out-Null
$helper=Join-Path $root 'UninstallWorker.ps1'
$worker=Join-Path $root 'Any2OdysseyWorker.exe'
$sentinel=Join-Path $root 'ConversionWrapper.dll'
$userExe=Join-Path $root 'UserWritable.exe'
function Assert($condition,[string]$message){if(-not $condition){throw $message};Write-Host "PASS: $message"}
try{
 $source=Get-Content -LiteralPath (Join-Path $installer 'UninstallWorker.ps1') -Raw
 $source=$source.Replace('C:\Program Files\Odyssey 3D Hub\Conversion\Any2OdysseyWorker.exe',$worker)
 Set-Content -LiteralPath $helper -Value $source
 . $helper
 $context=@{ExpectedSid='S-1-5-21-1';ActualSid='S-1-5-21-1';IsAdmin=$true;ScriptPath=$helper;InstalledPath=$helper}
 Assert-Any2OdysseyUninstallContext @context
 foreach($field in @('ActualSid','IsAdmin','ScriptPath')){
  $bad=$context.Clone()
  switch($field){'ActualSid'{$bad[$field]='S-1-5-21-2'}'IsAdmin'{$bad[$field]=$false}'ScriptPath'{$bad[$field]=$userExe}}
  $failed=$false;try{Assert-Any2OdysseyUninstallContext @bad}catch{$failed=$true}
  Assert $failed "Reject unsafe helper context: $field"
 }
 Set-Content -LiteralPath $worker -Value 'owned worker'
 Set-Content -LiteralPath $sentinel -Value 'Samsung file'
 Set-Content -LiteralPath $userExe -Value 'must not run or be deleted by elevated helper'
 $script:running=$true
 function Get-Process {param($Name,$ErrorAction)if($script:running){return [pscustomobject]@{Id=1}}}
 $failed=$false;try{Remove-Any2OdysseyInstalledWorker -HelperPath $helper}catch{$failed=$true}
 Assert ($failed -and (Test-Path -LiteralPath $worker) -and (Test-Path -LiteralPath $helper)) 'Running app blocks deletion without killing processes'
 $script:running=$false
 Remove-Any2OdysseyInstalledWorker -HelperPath $helper
 Assert (-not (Test-Path -LiteralPath $worker) -and -not (Test-Path -LiteralPath $helper)) 'Helper removes only the owned worker and itself'
 Assert ((Test-Path -LiteralPath $sentinel) -and (Test-Path -LiteralPath $userExe)) 'Samsung and per-user files remain untouched'
 $uninstall=Get-Content -LiteralPath (Join-Path $installer 'Uninstall.ps1') -Raw
 Assert (-not $uninstall.Contains('Start-Process -FilePath $exe') -and -not $uninstall.Contains('$PSCommandPath')) 'Uninstaller never launches the per-user EXE or re-elevates itself'
 $errors=$null;$tokens=$null
 foreach($file in @('Uninstall.ps1','UninstallWorker.ps1','install.ps1')){
  $null=[Management.Automation.Language.Parser]::ParseFile((Join-Path $installer $file),[ref]$tokens,[ref]$errors)
  Assert ($errors.Count -eq 0) "PowerShell syntax: $file"
 }
}finally{Remove-Item -LiteralPath $root -Recurse -Force}
exit 0
