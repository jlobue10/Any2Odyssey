# Isolated security regression checks. No real installation or elevation.
$ErrorActionPreference='Stop'
$installer=Split-Path -Parent $PSScriptRoot
$root=Join-Path ([IO.Path]::GetTempPath()) ('Any2Odyssey-helper-test-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root | Out-Null
$helper=Join-Path $root 'UninstallWorker.ps1'
$owner=Join-Path $root 'UninstallWorker.owner'
$result=Join-Path $root 'UninstallWorker.result'
$worker=Join-Path $root 'Any2OdysseyWorker.exe'
$sentinel=Join-Path $root 'ConversionWrapper.dll'
$userExe=Join-Path $root 'UserWritable.exe'
function Assert($condition,[string]$message){if(-not $condition){throw $message};Write-Host "PASS: $message"}
try{
 $source=Get-Content -LiteralPath (Join-Path $installer 'UninstallWorker.ps1') -Raw
 $source=$source.Replace('C:\Program Files\Odyssey 3D Hub\Conversion\Any2OdysseyWorker.exe',$worker)
 # The functions below delete what this path names. If the literal in UninstallWorker.ps1 ever
 # changes, the replacement above matches nothing and an elevated run would delete the real worker.
 if($source.Contains('C:\Program Files') -or -not $source.Contains($worker)){throw 'Fixture routing incomplete; refusing to run.'}
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
 Set-Content -LiteralPath $owner -Value 'S-1-5-21-1'
 Set-Content -LiteralPath $result -Value 'reason left by an earlier failed run'
 Set-Content -LiteralPath $sentinel -Value 'Samsung file'
 Set-Content -LiteralPath $userExe -Value 'must not run or be deleted by elevated helper'
 $script:running=$true
 function Get-Process {param($Name,$ErrorAction)if($script:running){return [pscustomobject]@{Id=1}}}
 $failed=$false;try{Remove-Any2OdysseyInstalledWorker -HelperPath $helper}catch{$failed=$true}
 Assert ($failed -and (Test-Path -LiteralPath $worker) -and (Test-Path -LiteralPath $helper) -and (Test-Path -LiteralPath $owner)) 'Running app blocks deletion without killing processes'
 $script:running=$false
 # Installers up to 0.9.2 copied a read-only attribute from the package onto the worker and the helper.
 foreach($file in @($worker,$helper,$owner,$result)){(Get-Item -LiteralPath $file).IsReadOnly=$true}
 Remove-Any2OdysseyInstalledWorker -HelperPath $helper
 Assert (-not (Test-Path -LiteralPath $worker) -and -not (Test-Path -LiteralPath $helper) -and -not (Test-Path -LiteralPath $owner) -and -not (Test-Path -LiteralPath $result)) 'Helper removes only the owned worker, itself, its owner marker and its result note, read-only or not'
 Assert ((Test-Path -LiteralPath $sentinel) -and (Test-Path -LiteralPath $userExe)) 'Samsung and per-user files remain untouched'
 $errors=$null;$tokens=$null;$uninstallAst=$null
 foreach($file in @('Uninstall.ps1','UninstallWorker.ps1','install.ps1','InstallSupport.ps1','Install-Launcher.ps1')){
  $ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $installer $file),[ref]$tokens,[ref]$errors)
  Assert ($errors.Count -eq 0) "PowerShell syntax: $file"
  if($file -eq 'Uninstall.ps1'){$uninstallAst=$ast}
 }
 # Only path tests and deletions may name the per-user executable. Any other command, or an
 # & / . invocation, that references it would run user-writable code from the uninstaller.
 $allowedCommands=@('Test-Path','Remove-Item','Join-Path')
 $exeCommands=@($uninstallAst.FindAll({param($node)$node -is [Management.Automation.Language.CommandAst]},$true) | Where-Object {
  $command=$_
  $references=@($command.CommandElements | Where-Object {$_.Extent.Text -match '\$exe\b|Any2Odyssey\.exe'})
  $name=$command.GetCommandName()
  $references.Count -gt 0 -and (-not $name -or $allowedCommands -notcontains $name)
 })
 Assert ($exeCommands.Count -eq 0) "Uninstaller never launches the per-user EXE (found: $(($exeCommands | ForEach-Object {$_.Extent.Text}) -join '; '))"
 # Method calls ([Diagnostics.Process]::Start($exe), $shell.Run($exe)) are not CommandAsts; check them too.
 $exeMethodCalls=@($uninstallAst.FindAll({param($node)$node -is [Management.Automation.Language.InvokeMemberExpressionAst]},$true) | Where-Object {
  $call=$_
  @($call.Arguments | Where-Object {$_.Extent.Text -match '\$exe\b|Any2Odyssey\.exe'}).Count -gt 0
 })
 Assert ($exeMethodCalls.Count -eq 0) "Uninstaller never passes the per-user EXE to a method call (found: $(($exeMethodCalls | ForEach-Object {$_.Extent.Text}) -join '; '))"
 $selfReferences=@($uninstallAst.FindAll({param($node)$node -is [Management.Automation.Language.VariableExpressionAst] -and $node.VariablePath.UserPath -eq 'PSCommandPath'},$true))
 Assert ($selfReferences.Count -eq 0) 'Uninstaller never re-elevates itself'
}finally{Remove-Item -LiteralPath $root -Recurse -Force}
exit 0
