# Launcher behaviour with a fake installer: visible preflight errors, result relay, same-account check,
# and the arguments Install.cmd / Install-GUI.cmd pass on. Windows only.
$ErrorActionPreference='Stop'
$installerDir=Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) 'installer'
$root=Join-Path ([IO.Path]::GetTempPath()) ('Any2Odyssey-launcher-test-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root | Out-Null
Copy-Item -LiteralPath (Join-Path $installerDir 'Install-Launcher.ps1') -Destination $root
$launcher=Join-Path $root 'Install-Launcher.ps1';$installer=Join-Path $root 'install.ps1';$report=Join-Path $root 'result.json'
$powershell=Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'
function Assert($condition,[string]$message){if(-not $condition){throw $message};Write-Host "PASS: $message"}
$passed=$false
try{
Set-Content -LiteralPath $installer -Encoding UTF8 -Value "param([switch]`$ControlsOnly,[switch]`$CheckOnly);throw 'Simulated missing engine; run Install-GUI.cmd.'"
$output=& $powershell -NoProfile -ExecutionPolicy Bypass -File $launcher 2>&1 | Out-String
Assert ($LASTEXITCODE -eq 1 -and $output.Contains('Simulated missing engine; run Install-GUI.cmd.')) 'Preflight failure remains visible in the original launcher console'
$output=& $powershell -NoProfile -ExecutionPolicy Bypass -File $launcher -RunInstall -ResultFile $report 2>&1 | Out-String
$result=Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
Assert ($LASTEXITCODE -eq 1 -and -not $result.Success -and $result.Message.Contains('Simulated missing engine')) 'Worker failure writes a precise error report'
# The fake installer records whether -AllowDowngrade reached it.
$marker=Join-Path $root 'marker.txt'
Set-Content -LiteralPath $installer -Encoding UTF8 -Value "param([switch]`$ControlsOnly,[switch]`$CheckOnly,[switch]`$AllowDowngrade);if(-not `$CheckOnly){Set-Content -LiteralPath (Join-Path `$PSScriptRoot 'marker.txt') -Value ('test only; AllowDowngrade='+[bool]`$AllowDowngrade)}"
$output=& $powershell -NoProfile -ExecutionPolicy Bypass -File $launcher -ControlsOnly 2>&1 | Out-String
Assert ($LASTEXITCODE -eq 0 -and (Test-Path -LiteralPath $marker)) 'Controls-only launcher executes without elevation'
Assert ((Get-Content -LiteralPath $marker -Raw).Contains('AllowDowngrade=False')) 'The launcher does not allow a downgrade unless asked'
Remove-Item -LiteralPath $marker
$output=& $powershell -NoProfile -ExecutionPolicy Bypass -File $launcher -ControlsOnly -AllowDowngrade 2>&1 | Out-String
Assert ($LASTEXITCODE -eq 0 -and (Get-Content -LiteralPath $marker -Raw).Contains('AllowDowngrade=True')) 'AllowDowngrade reaches the installer on the direct path'
Remove-Item -LiteralPath $marker
$output=& $powershell -NoProfile -ExecutionPolicy Bypass -File $launcher -RunInstall -ResultFile $report -AllowDowngrade 2>&1 | Out-String
Assert ($LASTEXITCODE -eq 0 -and (Get-Content -LiteralPath $marker -Raw).Contains('AllowDowngrade=True')) 'AllowDowngrade reaches the installer in the elevated step'
Remove-Item -LiteralPath $marker
# Install-GUI.cmd passes its arguments on (.\Install-GUI.cmd -AllowDowngrade). A controls-only setup never
# elevates, so running the real launcher file is safe on any PC; the piped empty line answers its pause.
Copy-Item -LiteralPath (Join-Path $installerDir 'Install-GUI.cmd') -Destination $root
$output='' | & (Join-Path $root 'Install-GUI.cmd') -AllowDowngrade 2>&1 | Out-String
Assert ($LASTEXITCODE -eq 0 -and (Test-Path -LiteralPath $marker) -and (Get-Content -LiteralPath $marker -Raw).Contains('AllowDowngrade=True')) 'Install-GUI.cmd passes -AllowDowngrade on to the installer'
Remove-Item -LiteralPath $marker
# Install.cmd would ask for elevation here, so its command line is compared with the one just exercised.
$guiLine=@(Get-Content -LiteralPath (Join-Path $installerDir 'Install-GUI.cmd') | Where-Object {$_ -like '"%ANY2ODYSSEY_POWERSHELL%" *'})
$fullLine=@(Get-Content -LiteralPath (Join-Path $installerDir 'Install.cmd') | Where-Object {$_ -like '"%ANY2ODYSSEY_POWERSHELL%" *'})
Assert ($guiLine.Count -eq 1 -and $fullLine.Count -eq 1 -and $guiLine[0].EndsWith(' -ControlsOnly %*') -and $fullLine[0] -eq $guiLine[0].Replace(' -ControlsOnly %*',' %*')) 'Install.cmd passes its arguments on the same way'
$output=& $powershell -NoProfile -ExecutionPolicy Bypass -File $launcher -RunInstall -ResultFile $report -ExpectedUserSid 'S-1-0-0' 2>&1 | Out-String
$result=Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
Assert (-not $result.Success -and $result.Message.Contains('different Windows account') -and -not (Test-Path -LiteralPath (Join-Path $root 'marker.txt'))) 'Different-account elevation is refused before installation'
# Simulate the elevated process's response without showing UAC or installing anything.
$source=Get-Content -LiteralPath $launcher -Raw
$source=$source.Replace('if($ControlsOnly -or $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator))','if($false)')
Set-Content -LiteralPath $launcher -Value $source -Encoding UTF8
$stub=@'
function Start-Process {
 param($FilePath,$ArgumentList,$Verb,$WindowStyle,[switch]$Wait,[switch]$PassThru)
 if($Verb -ne 'RunAs' -or $WindowStyle -ne 'Hidden'){throw 'Unexpected launch options'}
 if($ArgumentList -notmatch '-ResultFile "([^"]+)"'){throw 'Missing result path'}
 $resultPath=$Matches[1]
 if($ArgumentList -match 'AllowDowngrade'){throw 'The elevated command line allows a downgrade nobody asked for'}
 @{Success=$false;Message='Simulated elevated file-copy failure'} | ConvertTo-Json | Set-Content -LiteralPath $resultPath -Encoding UTF8
 return [pscustomobject]@{ExitCode=1}
}
& (Join-Path $PSScriptRoot 'Install-Launcher.ps1')
exit $LASTEXITCODE
'@
$wrapper=Join-Path $root 'simulate-elevation.ps1';Set-Content -LiteralPath $wrapper -Value $stub -Encoding UTF8
$output=& $powershell -NoProfile -ExecutionPolicy Bypass -File $wrapper 2>&1 | Out-String
Assert ($LASTEXITCODE -eq 1 -and $output.Contains('Simulated elevated file-copy failure')) 'Elevated installation error is relayed to the original console'
# With -AllowDowngrade the elevated command line carries the switch, and a success is relayed too.
$allowed=@'
function Start-Process {
 param($FilePath,$ArgumentList,$Verb,$WindowStyle,[switch]$Wait,[switch]$PassThru)
 if($ArgumentList -notmatch '-ResultFile "([^"]+)"'){throw 'Missing result path'}
 $resultPath=$Matches[1]
 if($ArgumentList -notmatch ' -RunInstall .* -AllowDowngrade$'){throw 'The elevated command line lost -AllowDowngrade'}
 @{Success=$true;Message='Simulated elevated success'} | ConvertTo-Json | Set-Content -LiteralPath $resultPath -Encoding UTF8
 return [pscustomobject]@{ExitCode=0}
}
& (Join-Path $PSScriptRoot 'Install-Launcher.ps1') -AllowDowngrade
exit $LASTEXITCODE
'@
$allowedWrapper=Join-Path $root 'simulate-allowed-downgrade.ps1';Set-Content -LiteralPath $allowedWrapper -Value $allowed -Encoding UTF8
$output=& $powershell -NoProfile -ExecutionPolicy Bypass -File $allowedWrapper 2>&1 | Out-String
Assert ($LASTEXITCODE -eq 0 -and $output.Contains('Simulated elevated success')) 'AllowDowngrade is carried into the elevated command line and its success is relayed'
# A declined UAC prompt surfaces from Start-Process -Verb RunAs as InvalidOperationException wrapping
# Win32Exception 1223 (ERROR_CANCELLED); the stub throws that exact shape.
$declined=@'
function Start-Process {
 param($FilePath,$ArgumentList,$Verb,$WindowStyle,[switch]$Wait,[switch]$PassThru)
 throw (New-Object InvalidOperationException -ArgumentList 'This command cannot be run due to the error: The operation was canceled by the user.',(New-Object ComponentModel.Win32Exception 1223))
}
& (Join-Path $PSScriptRoot 'Install-Launcher.ps1')
exit $LASTEXITCODE
'@
$declinedWrapper=Join-Path $root 'simulate-declined.ps1';Set-Content -LiteralPath $declinedWrapper -Value $declined -Encoding UTF8
$output=& $powershell -NoProfile -ExecutionPolicy Bypass -File $declinedWrapper 2>&1 | Out-String
Assert ($LASTEXITCODE -eq 1 -and $output.Contains('Elevation was declined. Nothing was installed.') -and -not (Test-Path -LiteralPath (Join-Path $root 'marker.txt'))) 'Declined elevation stops setup with a plain message'
# Any other launch failure (here Win32 error 2, file not found) must keep its own message.
$otherFailure=@'
function Start-Process {
 param($FilePath,$ArgumentList,$Verb,$WindowStyle,[switch]$Wait,[switch]$PassThru)
 throw (New-Object InvalidOperationException -ArgumentList 'This command cannot be run due to the error: simulated launch failure.',(New-Object ComponentModel.Win32Exception 2))
}
& (Join-Path $PSScriptRoot 'Install-Launcher.ps1')
exit $LASTEXITCODE
'@
$otherWrapper=Join-Path $root 'simulate-launch-failure.ps1';Set-Content -LiteralPath $otherWrapper -Value $otherFailure -Encoding UTF8
$output=& $powershell -NoProfile -ExecutionPolicy Bypass -File $otherWrapper 2>&1 | Out-String
Assert ($LASTEXITCODE -eq 1 -and $output.Contains('Installation did not complete: This command cannot be run due to the error: simulated launch failure.') -and -not $output.Contains('Elevation was declined')) 'A launch failure other than a declined prompt is not reported as declined'
$passed=$true
}finally{if($passed){Remove-Item -LiteralPath $root -Recurse -Force}else{Write-Host "Fixtures retained for inspection: $root"}}

# Assertion failures throw above; a clean run must not inherit a native command's exit code.
exit 0
