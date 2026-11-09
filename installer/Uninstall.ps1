$ErrorActionPreference='Stop'
$app=Join-Path $env:LOCALAPPDATA 'Programs\Any2Odyssey'
$controlsOnly=$false
$versionFile=Join-Path $app 'version.json'
if(Test-Path -LiteralPath $versionFile){$record=Get-Content -LiteralPath $versionFile -Raw | ConvertFrom-Json;$controlsOnly=$record.ControlsOnly -eq $true}
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
if(-not $controlsOnly -and ((Test-Path -LiteralPath $worker) -or (Test-Path -LiteralPath $helper))){
 if(-not (Test-Path -LiteralPath $helper -PathType Leaf)){throw 'The protected uninstall helper is missing. Repair the full installation with Install.cmd, then retry uninstall.'}
 $powershell=Join-Path ([Environment]::SystemDirectory) 'WindowsPowerShell\v1.0\powershell.exe'
 $arguments='-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "'+$helper+'" -ExpectedUserSid "'+$identity.User.Value+'"'
 # The helper itself checks the approving account, its own Program Files path and the fixed worker path.
 $launch=@{FilePath=$powershell;ArgumentList=$arguments;Wait=$true;PassThru=$true}
 if(-not $alreadyElevated){$launch.Verb='RunAs'}
 $removal=Start-Process @launch
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
