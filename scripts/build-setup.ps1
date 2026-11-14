# Build the Windows setup program from the ZIP that package.ps1 wrote, so that it carries the very
# files of the published ZIP. Run after package.ps1 (and verify-package.ps1). Needs Inno Setup 6.3 or
# newer: pass -Iscc, or set the ISCC environment variable to ISCC.exe, or have it installed in one of
# its usual folders. Writes dist\Any2Odyssey-<version>-windows-x64-setup.exe and a .sha256 file.
[CmdletBinding()]
param([string]$Zip,[string]$Iscc=$env:ISCC)
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
$version=(Get-Content -LiteralPath (Join-Path $repo 'VERSION') -Raw).Trim()
if($version -notmatch '^\d+\.\d+\.\d+$'){throw 'VERSION must contain major.minor.patch.'}
$dist=Join-Path $repo 'dist'
if(-not $Zip){$Zip=Join-Path $dist ('Any2Odyssey-'+$version+'-windows-x64.zip')}
if(-not (Test-Path -LiteralPath $Zip -PathType Leaf)){throw "Package first: $Zip is missing."}
if(-not $Iscc){
 $candidates=@()
 foreach($root in @(${env:ProgramFiles(x86)},$env:ProgramFiles,$(if($env:LOCALAPPDATA){Join-Path $env:LOCALAPPDATA 'Programs'}))){
  if($root){$candidates+=Join-Path $root 'Inno Setup 6\ISCC.exe'}
 }
 $Iscc=$candidates | Where-Object {Test-Path -LiteralPath $_ -PathType Leaf} | Select-Object -First 1
 if(-not $Iscc){$onPath=Get-Command ISCC.exe -ErrorAction SilentlyContinue;if($onPath){$Iscc=$onPath.Source}}
}
if(-not $Iscc -or -not (Test-Path -LiteralPath $Iscc -PathType Leaf)){throw 'Inno Setup''s compiler (ISCC.exe) was not found. Install Inno Setup 6 (https://jrsoftware.org/isdl.php), or pass -Iscc <path to ISCC.exe>.'}
$out=Join-Path ([IO.Path]::GetTempPath()) ('Any2Odyssey-setup-build-'+[Guid]::NewGuid().ToString('N'))
try{
 $package=Join-Path $out 'package';$compiled=Join-Path $out 'compiled'
 New-Item -ItemType Directory -Path $compiled -Force | Out-Null
 Expand-Archive -LiteralPath $Zip -DestinationPath $package
 $packaged=(Get-Content -LiteralPath (Join-Path $package 'VERSION') -Raw).Trim()
 if($packaged -ne $version){throw "The ZIP holds version '$packaged', VERSION says '$version'. Package again before building the setup program."}
 # /Q: errors and warnings only. The script takes the package folder and the version from here, so it
 # has no version of its own to keep in step.
 & $Iscc '/Q' ('/DPackageDir='+$package) ('/DAppVersion='+$version) ('/O'+$compiled) (Join-Path $repo 'installer\Any2Odyssey.iss')
 if($LASTEXITCODE -ne 0){throw "ISCC.exe exited with code $LASTEXITCODE."}
 $name='Any2Odyssey-'+$version+'-windows-x64-setup.exe'
 $built=Join-Path $compiled $name
 if(-not (Test-Path -LiteralPath $built -PathType Leaf)){throw "ISCC.exe did not write $name."}
 # Installed apps and a file's Properties page show this; it must name the release it installs.
 $info=[Diagnostics.FileVersionInfo]::GetVersionInfo($built)
 # Inno Setup pads the two strings with spaces.
 $fileVersion=([string]$info.FileVersion).Trim();$productVersion=([string]$info.ProductVersion).Trim()
 if($fileVersion -ne ($version+'.0') -or $productVersion -ne $version){throw "The setup program's version resource ('$fileVersion', '$productVersion') does not match VERSION '$version'."}
 New-Item -ItemType Directory -Path $dist -Force | Out-Null
 $setup=Join-Path $dist $name
 Copy-Item -LiteralPath $built -Destination $setup -Force
 $hash=(Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash.ToLowerInvariant()
 # LF, as for the ZIP: a CRLF line makes `sha256sum -c` report an improperly formatted line.
 [IO.File]::WriteAllText(($setup+'.sha256'),$hash+'  '+$name+"`n",[Text.UTF8Encoding]::new($false))
 Write-Host $setup
 Write-Host ('SHA256 '+$hash)
}finally{
 if(Test-Path -LiteralPath $out){Remove-Item -LiteralPath $out -Recurse -Force}
}
