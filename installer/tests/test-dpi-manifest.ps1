# Inspect the built PE without executing it or loading its imports.
[CmdletBinding()]
param([string]$Binary=(Join-Path $PSScriptRoot '..\..\target\release\Any2Odyssey.exe'))
$ErrorActionPreference='Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class Any2OdysseyManifestReader {
 [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
 public static extern IntPtr LoadLibraryExW(string path, IntPtr file, uint flags);
 [DllImport("kernel32.dll", SetLastError=true)]
 public static extern IntPtr FindResourceW(IntPtr module, IntPtr name, IntPtr type);
 [DllImport("kernel32.dll", SetLastError=true)]
 public static extern uint SizeofResource(IntPtr module, IntPtr resource);
 [DllImport("kernel32.dll", SetLastError=true)]
 public static extern IntPtr LoadResource(IntPtr module, IntPtr resource);
 [DllImport("kernel32.dll")]
 public static extern IntPtr LockResource(IntPtr resource);
 [DllImport("kernel32.dll")]
 public static extern bool FreeLibrary(IntPtr module);
}
'@
$path=(Resolve-Path -LiteralPath $Binary).Path
# LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE
$module=[Any2OdysseyManifestReader]::LoadLibraryExW($path,[IntPtr]::Zero,0x22)
if($module -eq [IntPtr]::Zero){throw 'Could not map executable resources.'}
try {
 $resource=[Any2OdysseyManifestReader]::FindResourceW($module,[IntPtr]1,[IntPtr]24)
 if($resource -eq [IntPtr]::Zero){throw 'Executable has no process manifest (RT_MANIFEST, ID 1).'}
 $size=[Any2OdysseyManifestReader]::SizeofResource($module,$resource)
 if(-not $size){throw 'Executable manifest is empty.'}
 $data=[Any2OdysseyManifestReader]::LoadResource($module,$resource)
 $pointer=[Any2OdysseyManifestReader]::LockResource($data)
 if($pointer -eq [IntPtr]::Zero){throw 'Could not read executable manifest.'}
 $bytes=New-Object byte[] $size
 [Runtime.InteropServices.Marshal]::Copy($pointer,$bytes,0,$bytes.Length)
 [xml]$manifest=[Text.Encoding]::UTF8.GetString($bytes).TrimEnd([char]0)
 $namespaces=New-Object Xml.XmlNamespaceManager($manifest.NameTable)
 $namespaces.AddNamespace('asm','urn:schemas-microsoft-com:asm.v3')
 $namespaces.AddNamespace('dpi','http://schemas.microsoft.com/SMI/2016/WindowsSettings')
 $awareness=$manifest.SelectSingleNode('//asm:application/asm:windowsSettings/dpi:dpiAwareness',$namespaces)
 if(-not $awareness -or $awareness.InnerText.Trim() -ne 'PerMonitorV2'){throw 'Worker requires a PerMonitorV2 DPI manifest.'}
 Write-Host 'Built executable has the PerMonitorV2 DPI manifest.'
} finally {
 $null=[Any2OdysseyManifestReader]::FreeLibrary($module)
}
