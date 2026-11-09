# Launcher behaviour with a fake installer: visible preflight errors, result relay, same-account check. Windows only.
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
Set-Content -LiteralPath $installer -Encoding UTF8 -Value "param([switch]`$ControlsOnly,[switch]`$CheckOnly);if(-not `$CheckOnly){Set-Content -LiteralPath (Join-Path `$PSScriptRoot 'marker.txt') -Value 'test only'}"
$output=& $powershell -NoProfile -ExecutionPolicy Bypass -File $launcher -ControlsOnly 2>&1 | Out-String
Assert ($LASTEXITCODE -eq 0 -and (Test-Path -LiteralPath (Join-Path $root 'marker.txt'))) 'Controls-only launcher executes without elevation'
Remove-Item -LiteralPath (Join-Path $root 'marker.txt')
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
 @{Success=$false;Message='Simulated elevated file-copy failure'} | ConvertTo-Json | Set-Content -LiteralPath $Matches[1] -Encoding UTF8
 return [pscustomobject]@{ExitCode=1}
}
& (Join-Path $PSScriptRoot 'Install-Launcher.ps1')
exit $LASTEXITCODE
'@
$wrapper=Join-Path $root 'simulate-elevation.ps1';Set-Content -LiteralPath $wrapper -Value $stub -Encoding UTF8
$output=& $powershell -NoProfile -ExecutionPolicy Bypass -File $wrapper 2>&1 | Out-String
Assert ($LASTEXITCODE -eq 1 -and $output.Contains('Simulated elevated file-copy failure')) 'Elevated installation error is relayed to the original console'
# A declined UAC prompt surfaces from Start-Process -Verb RunAs as InvalidOperationException.
$declined=@'
function Start-Process {
 param($FilePath,$ArgumentList,$Verb,$WindowStyle,[switch]$Wait,[switch]$PassThru)
 throw (New-Object InvalidOperationException 'This command cannot be run due to the error: The operation was canceled by the user.')
}
& (Join-Path $PSScriptRoot 'Install-Launcher.ps1')
exit $LASTEXITCODE
'@
$declinedWrapper=Join-Path $root 'simulate-declined.ps1';Set-Content -LiteralPath $declinedWrapper -Value $declined -Encoding UTF8
$output=& $powershell -NoProfile -ExecutionPolicy Bypass -File $declinedWrapper 2>&1 | Out-String
Assert ($LASTEXITCODE -eq 1 -and $output.Contains('Elevation was declined. Nothing was installed.') -and -not (Test-Path -LiteralPath (Join-Path $root 'marker.txt'))) 'Declined elevation stops setup with a plain message'
$passed=$true
}finally{if($passed){Remove-Item -LiteralPath $root -Recurse -Force}else{Write-Host "Fixtures retained for inspection: $root"}}

# Assertion failures throw above; a clean run must not inherit a native command's exit code.
exit 0
