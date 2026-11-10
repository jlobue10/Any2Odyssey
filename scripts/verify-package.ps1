# Open the ZIP that package.ps1 wrote and check it the way a user's first step would:
# the entry points exist and the installer's read-only preflight accepts the extracted folder.
# Changes nothing outside a temporary folder. Run after package.ps1.
[CmdletBinding()]
param([string]$Zip)
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
if(-not $Zip){
 $version=(Get-Content -LiteralPath (Join-Path $repo 'VERSION') -Raw).Trim()
 $Zip=Join-Path $repo ('dist\Any2Odyssey-'+$version+'-windows-x64.zip')
}
if(-not (Test-Path -LiteralPath $Zip -PathType Leaf)){throw "Package first: $Zip is missing."}
$out=Join-Path ([IO.Path]::GetTempPath()) ('Any2Odyssey-package-check-'+[Guid]::NewGuid().ToString('N'))
try{
 Expand-Archive -LiteralPath $Zip -DestinationPath $out
 # The files a user starts, which install.ps1's own list of package files does not cover.
 foreach($name in @('Install.cmd','Install-GUI.cmd','Install-Launcher.ps1','InstallSupport.ps1','install.ps1','README.md','CHANGELOG.md')){
  if(-not (Test-Path -LiteralPath (Join-Path $out $name) -PathType Leaf)){throw "The package has no $name."}
 }
 # -CheckOnly verifies every file the installer copies, VERSION and the executable's version, and
 # returns before any change; -ControlsOnly keeps Samsung's engine out of the check.
 & (Join-Path $out 'install.ps1') -CheckOnly -ControlsOnly
 Write-Host "Package verified: $Zip"
}finally{
 if(Test-Path -LiteralPath $out){Remove-Item -LiteralPath $out -Recurse -Force}
}
