[CmdletBinding()]
param([string]$ExpectedUserSid)
$ErrorActionPreference='Stop'

# This helper is installed under Program Files. It has no per-user imports,
# executable launches, registry cleanup or caller-selected deletion paths.
function Assert-Any2OdysseyUninstallContext {
 param([string]$ExpectedSid,[string]$ActualSid,[bool]$IsAdmin,[string]$ScriptPath,[string]$InstalledPath)
 if(-not $ExpectedSid -or $ExpectedSid -ne $ActualSid){throw 'Elevation used a different Windows account. Uninstall using the same account as the installation.'}
 if(-not $IsAdmin){throw 'Worker removal requires administrator access.'}
 if(-not [string]::Equals([IO.Path]::GetFullPath($ScriptPath),[IO.Path]::GetFullPath($InstalledPath),[StringComparison]::OrdinalIgnoreCase)){throw 'Run only the installed worker-removal helper from Program Files.'}
}

function Remove-Any2OdysseyInstalledWorker {
 param([string]$HelperPath)
 $worker='C:\Program Files\Odyssey 3D Hub\Conversion\Any2OdysseyWorker.exe'
 if(Get-Process Any2Odyssey,Any2OdysseyWorker -ErrorAction SilentlyContinue){throw 'Exit Any2Odyssey from its tray before removing the conversion worker.'}
 if(Test-Path -LiteralPath $worker){Remove-Item -LiteralPath $worker}
 Remove-Item -LiteralPath $HelperPath
 $folder=Split-Path -Parent $HelperPath
 if((Split-Path -Leaf $folder) -eq 'Any2Odyssey' -and -not (Get-ChildItem -LiteralPath $folder -Force | Select-Object -First 1)){Remove-Item -LiteralPath $folder}
}

# Dot sourcing is used only by isolated tests to load the pure checks.
if($MyInvocation.InvocationName -ne '.'){
 $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
 $principal=New-Object Security.Principal.WindowsPrincipal($identity)
 $installed=Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'Any2Odyssey\UninstallWorker.ps1'
 Assert-Any2OdysseyUninstallContext -ExpectedSid $ExpectedUserSid -ActualSid $identity.User.Value -IsAdmin $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator) -ScriptPath $PSCommandPath -InstalledPath $installed
 Remove-Any2OdysseyInstalledWorker -HelperPath $PSCommandPath
}
