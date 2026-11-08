# Stage a release ZIP: executable, assets, documents, installer scripts and VERSION.
# Run after `cargo build --release` on Windows x64. Writes dist\Any2Odyssey-<version>-windows-x64.zip and a .sha256 file.
[CmdletBinding()]
param([string]$Binary=(Join-Path $PSScriptRoot '..\target\release\Any2Odyssey.exe'))
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
$version=(Get-Content -LiteralPath (Join-Path $repo 'VERSION') -Raw).Trim()
if($version -notmatch '^\d+\.\d+\.\d+$'){throw 'VERSION must contain major.minor.patch.'}
if(-not (Test-Path -LiteralPath $Binary -PathType Leaf)){throw "Build first: $Binary is missing."}
$info=[Diagnostics.FileVersionInfo]::GetVersionInfo($Binary)
$fileVersion=if($info.FileVersion){$info.FileVersion}else{$info.ProductVersion}
if(-not ($fileVersion -match '^\d+(\.\d+){1,3}')){throw "Executable has no readable version resource ('$fileVersion'). Rebuild before packaging."}
$exeVersion=[version]$Matches[0];$wantVersion=[version]$version
$normalize={param($v)[version]::new($v.Major,$v.Minor,[Math]::Max(0,$v.Build),[Math]::Max(0,$v.Revision))}
if((& $normalize $exeVersion) -ne (& $normalize $wantVersion)){throw "Executable version '$fileVersion' does not match VERSION '$version'. Rebuild before packaging."}
$dist=Join-Path $repo 'dist'
New-Item -ItemType Directory -Path $dist -Force | Out-Null
# Fresh staging directory: never pick up logs, diagnostics or Samsung assets.
$stage=Join-Path $dist ('stage-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path (Join-Path $stage 'build'),(Join-Path $stage 'assets'),(Join-Path $stage 'docs') -Force | Out-Null
Copy-Item -LiteralPath $Binary -Destination (Join-Path $stage 'build\Any2Odyssey.exe')
foreach($name in @('Any2Odyssey.ico','Any2Odyssey.png','Any2Odyssey-wordmark.png')){Copy-Item -LiteralPath (Join-Path $repo ('assets\'+$name)) -Destination (Join-Path $stage 'assets')}
foreach($name in @('README.md','CHANGELOG.md','LICENSE','NOTICE','THIRD-PARTY-LICENSES.md','VERSION')){Copy-Item -LiteralPath (Join-Path $repo $name) -Destination $stage}
$guide=Join-Path $repo 'USER-GUIDE.html'
if(Test-Path -LiteralPath $guide){Copy-Item -LiteralPath $guide -Destination $stage}else{throw 'USER-GUIDE.html is missing; export the offline guide before packaging.'}
Get-ChildItem -LiteralPath (Join-Path $repo 'docs') -Filter '*.md' -File | ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $stage 'docs') }
foreach($name in @('install.ps1','InstallSupport.ps1','Install-Launcher.ps1','Uninstall.ps1','UninstallWorker.ps1','Install.cmd','Install-GUI.cmd')){Copy-Item -LiteralPath (Join-Path $repo ('installer\'+$name)) -Destination $stage}
$zip=Join-Path $dist ('Any2Odyssey-'+$version+'-windows-x64.zip')
if(Test-Path -LiteralPath $zip){Remove-Item -LiteralPath $zip}
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
$hash=(Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText(($zip+'.sha256'),$hash+'  '+[IO.Path]::GetFileName($zip)+[Environment]::NewLine,[Text.UTF8Encoding]::new($false))
Remove-Item -LiteralPath $stage -Recurse
Write-Host $zip
Write-Host ('SHA256 '+$hash)
