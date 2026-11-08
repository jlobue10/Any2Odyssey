$ErrorActionPreference='Stop'
$app=Join-Path $env:LOCALAPPDATA 'Programs\Any2Odyssey'
$controlsOnly=$false
$versionFile=Join-Path $app 'version.json'
if(Test-Path -LiteralPath $versionFile){$record=Get-Content -LiteralPath $versionFile -Raw | ConvertFrom-Json;$controlsOnly=$record.ControlsOnly -eq $true}
$identity=[Security.Principal.WindowsIdentity]::GetCurrent()
$principal=New-Object Security.Principal.WindowsPrincipal($identity)
if(-not $controlsOnly -and -not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)){
 $arguments='-NoProfile -ExecutionPolicy Bypass -File "'+$PSCommandPath+'"'
 $elevated=Start-Process -FilePath 'powershell.exe' -ArgumentList $arguments -Verb RunAs -WindowStyle Hidden -Wait -PassThru
 exit $elevated.ExitCode
}
$exe=Join-Path $app 'Any2Odyssey.exe'
$worker='C:\Program Files\Odyssey 3D Hub\Conversion\Any2OdysseyWorker.exe'
if(Test-Path -LiteralPath $exe){Start-Process -FilePath $exe -ArgumentList '--quit' -WindowStyle Hidden -Wait}
$deadline=(Get-Date).AddSeconds(12)
while((Get-Process Any2Odyssey,Any2OdysseyWorker -ErrorAction SilentlyContinue) -and (Get-Date) -lt $deadline){Start-Sleep -Milliseconds 250}
if(Get-Process Any2Odyssey,Any2OdysseyWorker -ErrorAction SilentlyContinue){throw 'Exit Any2Odyssey from its tray, then run uninstall again.'}
$shell=New-Object -ComObject WScript.Shell
foreach($folder in @([Environment]::GetFolderPath('Desktop'),[Environment]::GetFolderPath('Programs'))){
 $link=Join-Path $folder 'Any2Odyssey.lnk'
 if((Test-Path -LiteralPath $link) -and $shell.CreateShortcut($link).TargetPath -eq $exe){Remove-Item -LiteralPath $link}
}
# Exact file list only: leave Samsung files, personal settings and logs intact.
$ownedFiles=@($exe,(Join-Path $app 'Any2Odyssey.ico'),(Join-Path $app 'LICENSE'),(Join-Path $app 'NOTICE'),(Join-Path $app 'THIRD-PARTY-LICENSES.md'),(Join-Path $app 'USER-GUIDE.html'),(Join-Path $app 'Uninstall.ps1'))
if(-not $controlsOnly){$ownedFiles+=$worker}
foreach($file in $ownedFiles){
 if(Test-Path -LiteralPath $file){Remove-Item -LiteralPath $file}
}
$registration='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Any2Odyssey'
if(Test-Path -LiteralPath $versionFile){Remove-Item -LiteralPath $versionFile}
if(Test-Path -LiteralPath $registration){Remove-Item -LiteralPath $registration}
Write-Host 'Any2Odyssey removed. Samsung software, your settings and logs were retained. No reboot is needed.'
