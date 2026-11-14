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
$result=[IO.File]::ReadAllText($report,[Text.Encoding]::UTF8) | ConvertFrom-Json
Assert ($LASTEXITCODE -eq 1 -and -not $result.Success -and $result.Message.Contains('Simulated missing engine')) 'Worker failure writes a precise error report'
Assert (([IO.File]::ReadAllBytes($report))[0] -eq [byte][char]'{') 'The error report is UTF-8 without a byte-order mark'
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
# Under PowerShell 7 this also exercises the launcher's own remedy: it clears the inherited
# PSModulePath, with which the Windows PowerShell it starts could not load its modules.
$output='' | & (Join-Path $root 'Install-GUI.cmd') -AllowDowngrade 2>&1 | Out-String
Assert ($LASTEXITCODE -eq 0 -and (Test-Path -LiteralPath $marker) -and (Get-Content -LiteralPath $marker -Raw).Contains('AllowDowngrade=True')) 'Install-GUI.cmd passes -AllowDowngrade on to the installer'
Remove-Item -LiteralPath $marker
# An exclamation mark in the folder name, in a console with delayed expansion switched on (cmd /V:ON,
# or the DelayedExpansion registry value): cmd then drops every ! from %~dp0, and the launcher looked
# for the package in a folder that does not exist.
$bang=Join-Path $root 'new!folder!';New-Item -ItemType Directory -Path $bang | Out-Null
foreach($name in @('Install-GUI.cmd','Install-Launcher.ps1','install.ps1')){Copy-Item -LiteralPath (Join-Path $root $name) -Destination $bang}
$output='' | & $env:ComSpec /d /v:on /c (Join-Path $bang 'Install-GUI.cmd') 2>&1 | Out-String
Assert ($LASTEXITCODE -eq 0 -and (Test-Path -LiteralPath (Join-Path $bang 'marker.txt'))) 'Install-GUI.cmd finds its package in a folder with ! in its name while delayed expansion is on'
foreach($name in @('Install.cmd','Install-GUI.cmd')){
 Assert (@(Get-Content -LiteralPath (Join-Path $installerDir $name) | Where-Object {$_ -match '^setlocal'}) -join '|' -eq 'setlocal DisableDelayedExpansion') "$name switches delayed expansion off before it uses its own path"
}
# Double-clicked inside the ZIP preview, Windows extracts only the .cmd file to a temporary folder.
# Nothing is beside it then, so no launcher runs and this is safe for Install.cmd as well.
$alone=Join-Path $root 'zip-preview';New-Item -ItemType Directory -Path $alone | Out-Null
foreach($name in @('Install.cmd','Install-GUI.cmd')){
 Copy-Item -LiteralPath (Join-Path $installerDir $name) -Destination $alone
 $output='' | & (Join-Path $alone $name) 2>&1 | Out-String
 Assert ($LASTEXITCODE -eq 1 -and $output.Contains('Extract the whole ZIP')) "$name run without the rest of the package says to extract the ZIP first"
}
# Install.cmd would ask for elevation here, so its command line is compared with the one just exercised.
$guiLine=@(Get-Content -LiteralPath (Join-Path $installerDir 'Install-GUI.cmd') | Where-Object {$_ -like '"%ANY2ODYSSEY_POWERSHELL%" *'})
$fullLine=@(Get-Content -LiteralPath (Join-Path $installerDir 'Install.cmd') | Where-Object {$_ -like '"%ANY2ODYSSEY_POWERSHELL%" *'})
Assert ($guiLine.Count -eq 1 -and $fullLine.Count -eq 1 -and $guiLine[0].EndsWith(' -ControlsOnly %*') -and $fullLine[0] -eq $guiLine[0].Replace(' -ControlsOnly %*',' %*')) 'Install.cmd passes its arguments on the same way'
$output=& $powershell -NoProfile -ExecutionPolicy Bypass -File $launcher -RunInstall -ResultFile $report -ExpectedUserSid 'S-1-0-0' 2>&1 | Out-String
$result=[IO.File]::ReadAllText($report,[Text.Encoding]::UTF8) | ConvertFrom-Json
Assert (-not $result.Success -and $result.Message.Contains('different Windows account') -and -not (Test-Path -LiteralPath (Join-Path $root 'marker.txt'))) 'Different-account elevation is refused before installation'
# The setup program (Any2Odyssey.iss) runs the launcher without a console, passes -ReportFile and
# shows what that file says: a status line, the closing message, then every line that was printed.
$markerInstaller=Get-Content -LiteralPath $installer -Raw
$setupReport=Join-Path $root 'setup-report.txt'
$fake=@'
param([switch]$ControlsOnly,[switch]$CheckOnly,[switch]$AllowDowngrade)
Write-Host ('Install Any2Odyssey 0.0.1 for C:\Users\Zo'+[char]0xEB)
Write-Host 'Note: Windows reports no Desktop folder for this account; no shortcut is placed there.'
if($env:ANY2ODYSSEY_TEST_FAILURE){throw $env:ANY2ODYSSEY_TEST_FAILURE}
if($CheckOnly){Write-Host 'Install preflight passed for controls only. No changes made.';return}
Write-Host 'Any2Odyssey 0.0.1 install completed.'
'@
Set-Content -LiteralPath $installer -Encoding UTF8 -Value $fake
try{
 $env:ANY2ODYSSEY_TEST_FAILURE=$null
 $output=& $powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $launcher -ControlsOnly -ReportFile $setupReport 2>&1 | Out-String
 $lines=[IO.File]::ReadAllLines($setupReport,[Text.Encoding]::UTF8)
 Assert ($LASTEXITCODE -eq 0 -and $lines[0] -ceq 'OK' -and $lines[1] -ceq 'Any2Odyssey installation completed. Open Any2Odyssey from the desktop or Start menu.') 'A completed installation is reported to the setup program as OK with its closing message'
 Assert (@($lines | Where-Object {$_ -ceq ('Install Any2Odyssey 0.0.1 for C:\Users\Zo'+[char]0xEB)}).Count -eq 2 -and @($lines | Where-Object {$_ -like 'Note: Windows reports no Desktop folder*'}).Count -eq 2 -and $lines -ccontains 'Any2Odyssey 0.0.1 install completed.') 'The report carries the lines of the preflight and of the installation, with characters outside ASCII intact'
 $bytes=[IO.File]::ReadAllBytes($setupReport)
 Assert ($bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF) 'The setup report is UTF-8 with a byte-order mark, which Setup needs to read it as UTF-8'
 Assert ($output.Contains('Any2Odyssey 0.0.1 install completed.')) 'With -ReportFile the lines are still printed'
 Remove-Item -LiteralPath $setupReport
 $output=& $powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $launcher -ControlsOnly 2>&1 | Out-String
 Assert ($LASTEXITCODE -eq 0 -and -not (Test-Path -LiteralPath $setupReport) -and $output.Contains('Any2Odyssey 0.0.1 install completed.')) 'Without -ReportFile no report is written and the console shows the same lines'
 $output=& $powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $launcher -ControlsOnly -ReportFile (Join-Path $root 'no-such-folder\report.txt') 2>&1 | Out-String
 Assert ($LASTEXITCODE -eq 0 -and $output.Contains('The setup report could not be written')) 'A report that cannot be written does not turn a completed installation into a failure'
 $env:ANY2ODYSSEY_TEST_FAILURE="Samsung engine changed.`nThis release needs a compatibility update."
 $output=& $powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $launcher -ControlsOnly -ReportFile $setupReport 2>&1 | Out-String
 $lines=[IO.File]::ReadAllLines($setupReport,[Text.Encoding]::UTF8)
 Assert ($LASTEXITCODE -eq 1 -and $lines[0] -ceq 'FAILED' -and $lines[1] -ceq 'Samsung engine changed. This release needs a compatibility update.' -and $output.Contains('Installation did not complete: Samsung engine changed.')) 'A refusal is reported as FAILED with its reason on one line'
 # The two refusals the setup program offers to override, with the text the real checks throw: the
 # status tells Setup to ask, and the advice about Install.cmd is left out of what it shows.
 . (Join-Path $installerDir 'InstallSupport.ps1')
 $refusals=@()
 try{$null=Get-Any2OdysseyInstallAction -PackageVersion ([version]'0.0.1.0') -InstalledVersion ([version]'9.9.9.0')}catch{$refusals+=$_.Exception.Message}
 try{Assert-Any2OdysseyWorkerNotNewer -PackageVersion ([version]'0.0.1.0') -WorkerVersion ([version]'9.9.9.0') -OwnerSid 'S-1-5-21-1' -CurrentSid 'S-1-5-21-2'}catch{$refusals+=$_.Exception.Message}
 Assert ($refusals.Count -eq 2) 'Both downgrade guards refuse an older package'
 foreach($refusal in $refusals){
  $env:ANY2ODYSSEY_TEST_FAILURE=$refusal
  $output=& $powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $launcher -ControlsOnly -ReportFile $setupReport 2>&1 | Out-String
  $lines=[IO.File]::ReadAllLines($setupReport,[Text.Encoding]::UTF8)
  Assert ($LASTEXITCODE -eq 1 -and $lines[0] -ceq 'DOWNGRADE' -and $lines[1].StartsWith($refusal.Substring(0,40)) -and $lines[1].EndsWith('Downgrade blocked; nothing was changed.') -and -not $lines[1].Contains('Install.cmd') -and $output.Contains('Install.cmd -AllowDowngrade')) "A blocked downgrade is reported as DOWNGRADE without the console advice: $($refusal.Substring(0,40))..."
 }
 $env:ANY2ODYSSEY_TEST_FAILURE=$null
 Remove-Item -LiteralPath $setupReport
 $output=& $powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $launcher -ControlsOnly -AllowDowngrade -ReportFile $setupReport 2>&1 | Out-String
 Assert ($LASTEXITCODE -eq 0 -and ([IO.File]::ReadAllLines($setupReport,[Text.Encoding]::UTF8))[0] -ceq 'OK') 'The setup program''s second run, with -AllowDowngrade, reports OK'
}finally{
 $env:ANY2ODYSSEY_TEST_FAILURE=$null
 [IO.File]::WriteAllText($installer,$markerInstaller,(New-Object Text.UTF8Encoding $true))
 if(Test-Path -LiteralPath $setupReport){Remove-Item -LiteralPath $setupReport}
}
# Simulate the elevated process's response without showing UAC or installing anything. The stub
# writes the result exactly as the elevated step does (UTF-8, no byte-order mark), with a character
# outside ASCII in it, as a profile path can have; what the launcher prints is recorded as UTF-8 in
# host.txt so that the console's code page plays no part in the comparison.
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
 [IO.File]::WriteAllText($resultPath,(@{Success=$false;Message=('Simulated elevated file-copy failure in C:\Users\Zo'+[char]0xEB)} | ConvertTo-Json),(New-Object Text.UTF8Encoding $false))
 return [pscustomobject]@{ExitCode=1}
}
function Write-Host {
 param($Object,$ForegroundColor,$BackgroundColor,[switch]$NoNewline)
 [IO.File]::AppendAllText((Join-Path $PSScriptRoot 'host.txt'),([string]$Object+[Environment]::NewLine),(New-Object Text.UTF8Encoding $false))
 Microsoft.PowerShell.Utility\Write-Host $Object
}
& (Join-Path $PSScriptRoot 'Install-Launcher.ps1')
exit $LASTEXITCODE
'@
$wrapper=Join-Path $root 'simulate-elevation.ps1';Set-Content -LiteralPath $wrapper -Value $stub -Encoding UTF8
$output=& $powershell -NoProfile -ExecutionPolicy Bypass -File $wrapper 2>&1 | Out-String
Assert ($LASTEXITCODE -eq 1 -and $output.Contains('Simulated elevated file-copy failure')) 'Elevated installation error is relayed to the original console'
Assert ([IO.File]::ReadAllText((Join-Path $root 'host.txt'),[Text.Encoding]::UTF8).Contains('Installation did not complete: Simulated elevated file-copy failure in C:\Users\Zo'+[char]0xEB)) 'A relayed message keeps characters outside ASCII'
# With -AllowDowngrade the elevated command line carries the switch, and a success is relayed too.
$allowed=@'
function Start-Process {
 param($FilePath,$ArgumentList,$Verb,$WindowStyle,[switch]$Wait,[switch]$PassThru)
 if($ArgumentList -notmatch '-ResultFile "([^"]+)"'){throw 'Missing result path'}
 $resultPath=$Matches[1]
 if($ArgumentList -notmatch ' -RunInstall .* -AllowDowngrade$'){throw 'The elevated command line lost -AllowDowngrade'}
 [IO.File]::WriteAllText($resultPath,(@{Success=$true;Message='Simulated elevated success'} | ConvertTo-Json),(New-Object Text.UTF8Encoding $false))
 return [pscustomobject]@{ExitCode=0}
}
& (Join-Path $PSScriptRoot 'Install-Launcher.ps1') -AllowDowngrade
exit $LASTEXITCODE
'@
$allowedWrapper=Join-Path $root 'simulate-allowed-downgrade.ps1';Set-Content -LiteralPath $allowedWrapper -Value $allowed -Encoding UTF8
$output=& $powershell -NoProfile -ExecutionPolicy Bypass -File $allowedWrapper 2>&1 | Out-String
Assert ($LASTEXITCODE -eq 0 -and $output.Contains('Simulated elevated success')) 'AllowDowngrade is carried into the elevated command line and its success is relayed'
# The setup program's report on the elevated path: the relayed failure and the relayed success.
$reported=$stub.Replace("& (Join-Path `$PSScriptRoot 'Install-Launcher.ps1')","& (Join-Path `$PSScriptRoot 'Install-Launcher.ps1') -ReportFile (Join-Path `$PSScriptRoot 'setup-report.txt')")
Assert ($reported -ne $stub) 'The elevation stub was given a report file'
$reportedWrapper=Join-Path $root 'simulate-elevation-report.ps1';Set-Content -LiteralPath $reportedWrapper -Value $reported -Encoding UTF8
$output=& $powershell -NoProfile -ExecutionPolicy Bypass -File $reportedWrapper 2>&1 | Out-String
$lines=[IO.File]::ReadAllLines($setupReport,[Text.Encoding]::UTF8)
Assert ($LASTEXITCODE -eq 1 -and $lines[0] -ceq 'FAILED' -and $lines[1] -ceq ('Simulated elevated file-copy failure in C:\Users\Zo'+[char]0xEB)) 'A failure of the elevated step reaches the setup program''s report'
$reportedAllowed=$allowed.Replace("& (Join-Path `$PSScriptRoot 'Install-Launcher.ps1') -AllowDowngrade","& (Join-Path `$PSScriptRoot 'Install-Launcher.ps1') -AllowDowngrade -ReportFile (Join-Path `$PSScriptRoot 'setup-report.txt')")
Assert ($reportedAllowed -ne $allowed) 'The success stub was given a report file'
$reportedAllowedWrapper=Join-Path $root 'simulate-allowed-report.ps1';Set-Content -LiteralPath $reportedAllowedWrapper -Value $reportedAllowed -Encoding UTF8
$output=& $powershell -NoProfile -ExecutionPolicy Bypass -File $reportedAllowedWrapper 2>&1 | Out-String
$lines=[IO.File]::ReadAllLines($setupReport,[Text.Encoding]::UTF8)
Assert ($LASTEXITCODE -eq 0 -and $lines[0] -ceq 'OK' -and $lines[1] -ceq 'Simulated elevated success') 'A success of the elevated step reaches the setup program''s report with its message'
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
