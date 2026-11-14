; Any2Odyssey setup program for Windows (Inno Setup 6.3 or newer).
;
; This is a window in front of the PowerShell installer, not a second installer. It carries the
; release package (the content of Any2Odyssey-<version>-windows-x64.zip), unpacks it to Setup's
; temporary folder and runs Install-Launcher.ps1 from there, exactly as Install.cmd and
; Install-GUI.cmd do from an extracted ZIP. Every check, the elevation for the conversion worker,
; the file transaction with its rollback, the Installed apps entry and the uninstaller therefore
; stay in one place (install.ps1, InstallSupport.ps1, Uninstall.ps1), and a PC can be installed,
; updated and repaired with either the ZIP or this program in any order.
;
; Consequences, on purpose:
; - PrivilegesRequired=lowest: Setup itself never elevates. Any2Odyssey is installed per user, and
;   the launcher asks for elevation only for the worker, after its read-only checks have passed.
; - CreateAppDir=no, Uninstallable=no, no [Files] entry that is installed: Setup places nothing
;   itself and registers no uninstaller of its own. Installed apps lists the entry install.ps1
;   writes, whose uninstaller is Uninstall.ps1.
;
; Build it with scripts\build-setup.ps1, which unpacks the packaged ZIP and passes its folder and
; version here, so that this program holds the very files of the published ZIP:
;   ISCC /DPackageDir=<extracted ZIP> /DAppVersion=<x.y.z> /O<output folder> installer\Any2Odyssey.iss
; Setup's own command-line switches work (/SILENT, /VERYSILENT, /SUPPRESSMSGBOXES, /LOG=file), plus:
;   /CONTROLSONLY     the desktop controls only (what Install-GUI.cmd installs)
;   /ALLOWDOWNGRADE   install over a newer installed version (Install.cmd -AllowDowngrade)
; A silent installation installs the full converter unless /CONTROLSONLY is given. Exit code 0 means
; installed; when the installer refuses or fails, Setup stops at its "Preparing to Install" stage,
; shows the installer's reason (in the log with /LOG) and exits with a code other than 0.

#ifndef PackageDir
  #error PackageDir is not defined. Build with scripts\build-setup.ps1, or pass /DPackageDir=<extracted release ZIP> /DAppVersion=<x.y.z>.
#endif
#ifndef AppVersion
  #error AppVersion is not defined. Build with scripts\build-setup.ps1, or pass /DAppVersion=<x.y.z> (the content of VERSION).
#endif
#if !FileExists(AddBackslash(PackageDir) + "Install-Launcher.ps1")
  #error PackageDir does not hold an extracted release package (Install-Launcher.ps1 is missing).
#endif

#define AppName "Any2Odyssey"
#define AppExe "Any2Odyssey.exe"
#define AppUrl "https://github.com/jlobue10/Any2Odyssey"
; Folder under {tmp} that receives the package.
#define PackageFolder "Any2Odyssey-package"

[Setup]
; No uninstall entry is written under this identifier (Uninstallable=no); it only names this setup.
AppId=Any2OdysseySetupProgram
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Jonathan LoBue
AppPublisherURL={#AppUrl}
AppSupportURL={#AppUrl}/issues
AppUpdatesURL={#AppUrl}/releases
VersionInfoVersion={#AppVersion}.0
VersionInfoProductName={#AppName}
VersionInfoProductTextVersion={#AppVersion}
VersionInfoDescription={#AppName} Setup
VersionInfoCopyright=GPL-3.0-only
OutputBaseFilename={#AppName}-{#AppVersion}-windows-x64-setup
SetupIconFile={#PackageDir}\assets\Any2Odyssey.ico
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
PrivilegesRequired=lowest
CreateAppDir=no
Uninstallable=no
; The executable is x64. x64compatible also admits Windows on Arm with x64 emulation, as the ZIP does.
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; The PowerShell installer asks a running Any2Odyssey of this session to quit and waits for it.
CloseApplications=no
RestartApplications=no
; Two of these at once would run two installations side by side.
SetupMutex=Any2OdysseySetupProgram
DisableWelcomePage=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
; The whole release package. dontcopy: nothing is installed by Setup; PrepareToInstall unpacks
; these to {tmp} (ExtractTemporaryFiles), where Setup removes them again when it exits.
Source: "{#PackageDir}\*"; DestDir: "{tmp}\{#PackageFolder}"; Flags: dontcopy recursesubdirs createallsubdirs

[Run]
; Not ticked by default: on a fresh installation Any2Odyssey starts converting as soon as it opens.
Filename: "{localappdata}\Programs\Any2Odyssey\{#AppExe}"; WorkingDir: "{localappdata}\Programs\Any2Odyssey"; Description: "Open {#AppName} now"; Flags: nowait postinstall skipifsilent unchecked

[Code]
const
  // install.ps1 uses this fixed path too; here it only chooses the default installation type.
  EngineDll = 'C:\Program Files\Odyssey 3D Hub\Conversion\ConversionWrapper.dll';
  // install.ps1 enforces this; checked here as well so that an older Windows hears it on the first page.
  MinimumBuild = 26100;

var
  TypePage: TInputOptionWizardPage;
  PackageReady: Boolean;
  DowngradeAllowed: Boolean;
  InstallNotes: String;

function SetEnvironmentVariable(lpName, lpValue: String): Boolean;
  external 'SetEnvironmentVariableW@kernel32.dll stdcall';

function HasSwitch(const Name: String): Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), '/' + Name) = 0 then
    begin
      Result := True;
      Exit;
    end;
end;

function ControlsOnly: Boolean;
begin
  if WizardSilent then
    Result := HasSwitch('CONTROLSONLY')
  else
    Result := TypePage.SelectedValueIndex = 1;
end;

function InitializeSetup: Boolean;
var
  Version: TWindowsVersion;
begin
  Result := True;
  DowngradeAllowed := HasSwitch('ALLOWDOWNGRADE');
  GetWindowsVersionEx(Version);
  // ANY2ODYSSEY_SKIP_OS_CHECK=1 is the test switch install.ps1 has for older CI images.
  if (Version.Build < MinimumBuild) and (GetEnv('ANY2ODYSSEY_SKIP_OS_CHECK') <> '1') then
  begin
    SuppressibleMsgBox('Any2Odyssey needs Windows 11 version 24H2 (build ' + IntToStr(MinimumBuild) +
      ') or newer. This PC runs build ' + IntToStr(Version.Build) + '. Nothing was installed.',
      mbCriticalError, MB_OK, IDOK);
    Result := False;
    Exit;
  end;
  // "Run as administrator", UAC switched off, or the built-in Administrator: no elevation prompt
  // will show which account this is, and everything except the worker is installed per user.
  if IsAdmin then
  begin
    Log('Setup runs with administrator rights; installing for the Windows account ' + GetUserNameString + '.');
    if SuppressibleMsgBox('Setup is running with administrator rights.' + #13#10#13#10 +
      'Any2Odyssey, its shortcuts and its settings are installed for one Windows account only: ' +
      GetUserNameString + '. If it is meant for another account, choose No and start Setup signed in ' +
      'as that account, without "Run as administrator".' + #13#10#13#10 +
      'Install for ' + GetUserNameString + '?', mbConfirmation, MB_YESNO, IDYES) <> IDYES then
      Result := False;
  end;
end;

procedure InitializeWizard;
var
  Note: String;
begin
  Note := 'Any2Odyssey is installed for your Windows account. The full converter also places one ' +
    'worker program beside Samsung''s conversion engine, which an administrator has to approve: ' +
    'approve that prompt with the account you are signed in to.';
  if not FileExists(EngineDll) then
    Note := Note + #13#10#13#10 + 'Samsung Odyssey 3D Hub was not found on this PC, so the controls-only ' +
      'installation is selected.';
  TypePage := CreateInputOptionPage(wpWelcome, 'Installation Type', 'What should Setup install?',
    Note, True, False);
  TypePage.Add('Full converter: the controls and the 3D conversion worker (needs Samsung Odyssey 3D Hub)');
  TypePage.Add('Desktop controls only: tray, settings and Help; no 3D conversion, no administrator approval');
  if HasSwitch('CONTROLSONLY') or not FileExists(EngineDll) then
    TypePage.SelectedValueIndex := 1
  else
    TypePage.SelectedValueIndex := 0;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if (CurPageID = TypePage.ID) and (TypePage.SelectedValueIndex = 0) and not FileExists(EngineDll) then
  begin
    MsgBox('Samsung Odyssey 3D Hub was not found on this PC (' + EngineDll + ').' + #13#10#13#10 +
      'The full converter drives Samsung''s conversion engine and cannot be installed without it. ' +
      'Install the supported Odyssey 3D Hub first and run Setup again, or choose Desktop controls only.',
      mbError, MB_OK);
    Result := False;
  end;
end;

function UpdateReadyMemo(Space, NewLine, MemoUserInfoInfo, MemoDirInfo, MemoTypeInfo, MemoComponentsInfo,
  MemoGroupInfo, MemoTasksInfo: String): String;
begin
  Result := 'Installation type:' + NewLine + Space;
  if ControlsOnly then
    Result := Result + 'Desktop controls only (no 3D conversion)'
  else
    Result := Result + 'Full converter (controls and conversion worker; asks for administrator approval)';
  Result := Result + NewLine + NewLine + 'Installed for:' + NewLine + Space + 'Windows account ' +
    GetUserNameString + NewLine + Space + ExpandConstant('{localappdata}\Programs\Any2Odyssey');
end;

// One run of Install-Launcher.ps1. Status is the first line of its report (OK, FAILED or
// DOWNGRADE), Reason the second; the installer's console lines follow and go to Setup's log.
function RunLauncher(const Package: String; var Status, Reason, Notes: String): Boolean;
var
  PowerShell, Params, ReportFile, Line: String;
  Lines: TArrayOfString;
  Code, I: Integer;
begin
  Result := False;
  Status := '';
  Reason := '';
  Notes := '';
  ReportFile := ExpandConstant('{tmp}\Any2Odyssey-setup-report.txt');
  DeleteFile(ReportFile);
  // The 64-bit Windows PowerShell also when Setup is a 32-bit program; install.ps1 refuses a 32-bit one.
  PowerShell := ExpandConstant('{sysnative}\WindowsPowerShell\v1.0\powershell.exe');
  Params := '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + Package +
    '\Install-Launcher.ps1" -ReportFile "' + ReportFile + '"';
  if ControlsOnly then
    Params := Params + ' -ControlsOnly';
  if DowngradeAllowed then
    Params := Params + ' -AllowDowngrade';
  // Started from a PowerShell 7 prompt, Setup inherits its module path, with which Windows
  // PowerShell cannot load its own modules (Install.cmd clears it for the same reason).
  SetEnvironmentVariable('PSModulePath', '');
  Log('Running: ' + PowerShell + ' ' + Params);
  if not Exec(PowerShell, Params, Package, SW_HIDE, ewWaitUntilTerminated, Code) then
  begin
    Reason := 'Windows PowerShell could not be started (' + SysErrorMessage(Code) + '): ' + PowerShell +
      '. Nothing was installed.';
    Exit;
  end;
  Log('Install-Launcher.ps1 exited with code ' + IntToStr(Code) + '.');
  if LoadStringsFromFile(ReportFile, Lines) and (GetArrayLength(Lines) >= 2) then
  begin
    Status := Trim(Lines[0]);
    Reason := Trim(Lines[1]);
    for I := 2 to GetArrayLength(Lines) - 1 do
    begin
      Line := Trim(Lines[I]);
      Log('Installer: ' + Line);
      if Pos('Note:', Line) = 1 then
      begin
        if Notes <> '' then
          Notes := Notes + #13#10#13#10;
        Notes := Notes + Line;
      end;
    end;
  end;
  if (Status = 'OK') and (Code = 0) then
    Result := True
  else if Reason = '' then
    // Windows PowerShell ended before the launcher could write its report.
    Reason := 'Windows PowerShell ended with code ' + IntToStr(Code) + ' before the installer could report. ' +
      'Nothing was installed. A script execution policy set by Group Policy, Constrained Language Mode ' +
      '(AppLocker, App Control for Business) or security software stops the installer on this PC; ' +
      'the project''s installer notes (installer/README.md) describe these cases.';
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Package, Status, Reason, Notes: String;
  Again: Boolean;
begin
  Result := '';
  Package := ExpandConstant('{tmp}\{#PackageFolder}');
  if not PackageReady then
  begin
    try
      ExtractTemporaryFiles('{tmp}\{#PackageFolder}\*');
      PackageReady := True;
    except
      Result := 'Setup could not unpack its files: ' + GetExceptionMessage + ' Nothing was installed.';
      Exit;
    end;
  end;
  repeat
    Again := False;
    // This page is on screen while the installer works; Setup replaces the text if it has to stop here.
    WizardForm.PreparingLabel.Caption := 'Installing Any2Odyssey. If Windows asks for administrator ' +
      'approval, approve it with the account you are signed in to.';
    WizardForm.AdjustLabelHeight(WizardForm.PreparingLabel);
    WizardForm.PreparingLabel.Visible := True;
    WizardForm.PreparingLabel.Update;
    if RunLauncher(Package, Status, Reason, Notes) then
    begin
      Log('Installation completed: ' + Reason);
      InstallNotes := Notes;
    end
    else if (Status = 'DOWNGRADE') and not DowngradeAllowed then
    begin
      Log('Refused: ' + Reason);
      if WizardSilent then
        Result := Reason + ' To install this older version on purpose, run Setup again with /ALLOWDOWNGRADE.'
      else if MsgBox(Reason + #13#10#13#10 + 'Install this older version anyway? Settings and logs are kept.',
        mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
      begin
        DowngradeAllowed := True;
        Again := True;
      end
      else
        Result := Reason;
    end
    else
    begin
      Log('Failed: ' + Reason);
      Result := Reason;
    end;
  until not Again;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  // What the console of Install.cmd would have shown beside the result: who else's worker was
  // replaced, a shortcut folder Windows does not have.
  if (CurStep = ssPostInstall) and (InstallNotes <> '') then
    SuppressibleMsgBox(InstallNotes, mbInformation, MB_OK, IDOK);
end;
