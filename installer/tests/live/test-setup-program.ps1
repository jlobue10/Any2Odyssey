# The built setup program, run for real. It CHANGES THE PC IT RUNS ON: it installs the desktop
# controls for the account that runs it, repairs them and uninstalls them again. That is why it is
# not one of the test-*.ps1 files in the folder above (those are safe on any PC, and CI and
# developers run all of them), and why it refuses to start without -AllowRealInstall, on a PC where
# Any2Odyssey is installed, and on a PC with Samsung's Odyssey 3D Hub. CI runs it on a disposable
# runner, after scripts\package.ps1 and scripts\build-setup.ps1.
[CmdletBinding()]
param([switch]$AllowRealInstall,[string]$Setup)
$ErrorActionPreference='Stop'
if(-not $AllowRealInstall){throw 'This test installs and uninstalls Any2Odyssey for the current Windows account. Run it only on a disposable PC or CI runner, with -AllowRealInstall.'}
$repo=Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))
$version=(Get-Content -LiteralPath (Join-Path $repo 'VERSION') -Raw).Trim()
if(-not $Setup){$Setup=Join-Path $repo ('dist\Any2Odyssey-'+$version+'-windows-x64-setup.exe')}
$zip=Join-Path $repo ('dist\Any2Odyssey-'+$version+'-windows-x64.zip')
foreach($file in @($Setup,$zip)){if(-not (Test-Path -LiteralPath $file -PathType Leaf)){throw "Package and build the setup program first: $file is missing."}}
$app=Join-Path $env:LOCALAPPDATA 'Programs\Any2Odyssey'
$registration='HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Any2Odyssey'
$engineDll='C:\Program Files\Odyssey 3D Hub\Conversion\ConversionWrapper.dll'
$helperFolder=Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'Any2Odyssey'
if((Test-Path -LiteralPath $app) -or (Test-Path -LiteralPath $registration) -or (Test-Path -LiteralPath $helperFolder)){throw 'Any2Odyssey is installed on this PC. This test is for a PC without it.'}
if(Test-Path -LiteralPath $engineDll){throw 'This PC has Odyssey 3D Hub. This test expects a PC without it: the full installation it attempts must be refused.'}
if(Get-Process Any2Odyssey,Any2OdysseyWorker -ErrorAction SilentlyContinue){throw 'Any2Odyssey is running on this PC.'}
function Assert($condition,[string]$message){if(-not $condition){throw $message};Write-Host "PASS: $message"}
$work=Join-Path ([IO.Path]::GetTempPath()) ('Any2Odyssey-setup-test-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
function Invoke-Setup {
 param([string]$Name,[string[]]$Switches=@())
 $log=Join-Path $work ($Name+'.log')
 # /LOG takes the installer's reason for a refusal, which a silent run shows nowhere else.
 $process=Start-Process -FilePath $Setup -ArgumentList (@('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART',('/LOG="'+$log+'"'))+$Switches) -Wait -PassThru
 $text=if(Test-Path -LiteralPath $log){[IO.File]::ReadAllText($log)}else{''}
 Write-Host "--- $Name ($($Switches -join ' ')): exit code $($process.ExitCode)"
 foreach($line in ($text -split "`r?`n" | Where-Object {$_ -match 'Installer: |Failed: |Refused: |Installation completed: |administrator rights'})){Write-Host ('    '+($line -replace '^\S+ \S+\s+',''))}
 return [pscustomobject]@{ExitCode=$process.ExitCode;Log=$text}
}
function Get-LeftoverPackage {
 # Setup unpacks the package to is-XXXXX.tmp\Any2Odyssey-package under %TEMP% and removes it when it exits.
 return @(Get-ChildItem -LiteralPath ([IO.Path]::GetTempPath()) -Directory -Filter 'is-*.tmp' -ErrorAction SilentlyContinue | Where-Object {Test-Path -LiteralPath (Join-Path $_.FullName 'Any2Odyssey-package')})
}
$passed=$false
try{
 # A full installation without Samsung's engine: refused by install.ps1's read-only checks.
 $full=Invoke-Setup -Name 'full-refused'
 Assert ($full.ExitCode -ne 0) 'A silent full installation on a PC without Odyssey 3D Hub exits with a failure code'
 Assert ($full.Log.Contains('Samsung Odyssey 3D Hub is not installed')) 'The installer''s reason for the refusal is in Setup''s log'
 Assert (-not (Test-Path -LiteralPath $app) -and -not (Test-Path -LiteralPath $registration) -and -not (Test-Path -LiteralPath $helperFolder)) 'The refused installation changed nothing'

 $controls=Invoke-Setup -Name 'controls' -Switches '/CONTROLSONLY'
 Assert ($controls.ExitCode -eq 0) 'A silent controls-only installation exits with 0'
 $exe=Join-Path $app 'Any2Odyssey.exe'
 Assert (Test-Path -LiteralPath $exe -PathType Leaf) 'Any2Odyssey.exe is in the application folder'
 $info=[Diagnostics.FileVersionInfo]::GetVersionInfo($exe)
 Assert ($info.FileVersion -eq ($version+'.0')) "The installed executable is version $version"
 # The setup program must carry the files of the ZIP that is published beside it.
 $unpacked=Join-Path $work 'zip';Expand-Archive -LiteralPath $zip -DestinationPath $unpacked
 Assert ((Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash -eq (Get-FileHash -LiteralPath (Join-Path $unpacked 'build\Any2Odyssey.exe') -Algorithm SHA256).Hash) 'The installed executable is the one in the ZIP package'
 foreach($name in @('Uninstall.ps1','USER-GUIDE.html','LICENSE','NOTICE','THIRD-PARTY-LICENSES.md')){
  Assert ((Get-FileHash -LiteralPath (Join-Path $app $name) -Algorithm SHA256).Hash -eq (Get-FileHash -LiteralPath (Join-Path $unpacked $name) -Algorithm SHA256).Hash) "The installed $name is the one in the ZIP package"
 }
 $record=[IO.File]::ReadAllText((Join-Path $app 'version.json'),[Text.Encoding]::UTF8) | ConvertFrom-Json
 Assert ($record.Version -eq $version -and $record.ControlsOnly -eq $true -and $record.Action -eq 'Install' -and $null -eq $record.Worker) 'version.json records a controls-only installation of this version'
 $key=Get-ItemProperty -LiteralPath $registration
 Assert ($key.DisplayName -eq 'Any2Odyssey (controls only)' -and $key.DisplayVersion -eq $version -and $key.UninstallString.Contains((Join-Path $app 'Uninstall.ps1'))) 'Installed apps lists the entry of the PowerShell installer, with its uninstaller'
 Assert (-not (Test-Path -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Any2OdysseySetupProgram_is1') -and -not (Test-Path -LiteralPath 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Any2OdysseySetupProgram_is1')) 'Setup registered no uninstaller of its own'
 $startMenu=[Environment]::GetFolderPath('Programs')
 if($startMenu){Assert (Test-Path -LiteralPath (Join-Path $startMenu 'Any2Odyssey.lnk') -PathType Leaf) 'The Start menu shortcut exists'}
 Assert (-not (Test-Path -LiteralPath $helperFolder) -and -not (Test-Path -LiteralPath 'C:\Program Files\Odyssey 3D Hub')) 'A controls-only installation placed nothing in Program Files'
 Assert ((Get-LeftoverPackage).Count -eq 0) 'Setup removed its unpacked package from the temporary folder'

 $repair=Invoke-Setup -Name 'repair' -Switches '/CONTROLSONLY'
 Assert ($repair.ExitCode -eq 0 -and $repair.Log.Contains('Repair Any2Odyssey')) 'Running the setup program again repairs the same version'
 Assert (([IO.File]::ReadAllText((Join-Path $app 'version.json'),[Text.Encoding]::UTF8) | ConvertFrom-Json).Action -eq 'Repair') 'version.json records the repair'

 # Uninstall as Installed apps does, without the -Pause that would wait for a key.
 $powershell=Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'
 $modulePath=$env:PSModulePath;$env:PSModulePath=$null
 try{$output=& $powershell -NoProfile -ExecutionPolicy Bypass -File (Join-Path $app 'Uninstall.ps1') 2>&1 | Out-String}finally{$env:PSModulePath=$modulePath}
 Write-Host $output
 Assert ($LASTEXITCODE -eq 0) 'The installed uninstaller exits with 0'
 Assert (-not (Test-Path -LiteralPath $app) -and -not (Test-Path -LiteralPath $registration)) 'The uninstaller removed the application folder and the Installed apps entry'
 if($startMenu){Assert (-not (Test-Path -LiteralPath (Join-Path $startMenu 'Any2Odyssey.lnk'))) 'The uninstaller removed the Start menu shortcut'}
 $passed=$true
}finally{
 if($passed){Remove-Item -LiteralPath $work -Recurse -Force}else{Write-Host "Logs retained for inspection: $work"}
}
exit 0
