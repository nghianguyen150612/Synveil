#ifndef GeneratedDir
  #error GeneratedDir must identify the generated installer inputs
#endif
#include GeneratedDir + "\\version.iss"

[Setup]
AppId={{7DDE2E8A-376A-4FC8-96FF-7DB529F0945D}
AppName=Synveil
AppVersion={#SynveilVersion}
AppVerName=Synveil {#SynveilVersion}
AppPublisher=Synveil
VersionInfoVersion={#SynveilWindowsVersion}
VersionInfoProductVersion={#SynveilWindowsVersion}
DefaultDirName={localappdata}\Programs\Synveil
DefaultGroupName=Synveil
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=none
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
OutputDir={#SynveilOutputDir}
OutputBaseFilename=SynveilSetup
LicenseFile={#SynveilPayloadDir}\LICENSE
DisableWelcomePage=no
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableReadyPage=yes
Compression=lzma2/max
SolidCompression=yes
SetupLogging=yes
Uninstallable=yes
UninstallDisplayName=Synveil
UninstallDisplayIcon={app}\synveil-desktop.exe
UsePreviousAppDir=yes
RestartIfNeededByRun=no
CloseApplications=yes

[Files]
#include GeneratedDir + "\\files.iss"
Source: "{#SynveilPayloadDir}\NOTICE"; Flags: dontcopy

[Icons]
Name: "{userprograms}\Synveil"; Filename: "{app}\synveil-desktop.exe"; WorkingDir: "{app}"
Name: "{userdesktop}\Synveil"; Filename: "{app}\synveil-desktop.exe"; WorkingDir: "{app}"; Check: ShouldCreateDesktopIcon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Uninstall\{7DDE2E8A-376A-4FC8-96FF-7DB529F0945D}_is1"; ValueType: string; ValueName: "SynveilManifestSha256"; ValueData: "{#SynveilManifestSha256}"; Flags: uninsdeletevalue

[Code]
var
  { P026 consumes this reviewed state boundary; P023 performs no startup mutation. }
  StartupRequested: Boolean;
  StartupChoiceExplicit: Boolean;
  FreshInstall: Boolean;
  RepairMode: Boolean;
  UpgradeMode: Boolean;
  DesktopIconRequested: Boolean;
  LaunchRequested: Boolean;
  StartupCheck: TNewCheckBox;
  DesktopIconCheck: TNewCheckBox;
  LaunchCheck: TNewCheckBox;
  NoticeButton: TNewButton;
  PreviousManifest: TStringList;

const
  UninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{7DDE2E8A-376A-4FC8-96FF-7DB529F0945D}_is1';
  FileAttributeReparsePoint = $400;

function ParseBooleanOption(const Name: String; const DefaultValue: Boolean): Boolean;
var
  Value: String;
begin
  Value := ExpandConstant('{param:' + Name + '|__MISSING__}');
  if Value = '__MISSING__' then begin
    Result := DefaultValue;
    exit;
  end;
  if Value = '0' then begin
    Result := False;
    exit;
  end;
  if Value = '1' then begin
    Result := True;
    exit;
  end;
  RaiseException('/' + Name + ' accepts only 0 or 1');
end;

function GetFileAttributes(lpFileName: String): LongWord;
  external 'GetFileAttributesW@kernel32.dll stdcall';

function ParseVersionPart(const Value: String): Integer;
var
  I: Integer;
begin
  if Value = '' then RaiseException('Installed Synveil version is malformed.');
  if (Length(Value) > 1) and (Value[1] = '0') then
    RaiseException('Installed Synveil version is malformed.');
  for I := 1 to Length(Value) do
    if (Value[I] < '0') or (Value[I] > '9') then
      RaiseException('Installed Synveil version is malformed.');
  Result := StrToInt(Value);
end;

procedure ParseStrictVersion(const Value: String; var Major, Minor, Patch: Integer);
var
  FirstDot, SecondDot: Integer;
begin
  FirstDot := Pos('.', Value);
  if FirstDot = 0 then RaiseException('Installed Synveil version is malformed.');
  SecondDot := Pos('.', Copy(Value, FirstDot + 1, MaxInt));
  if SecondDot = 0 then RaiseException('Installed Synveil version is malformed.');
  SecondDot := SecondDot + FirstDot;
  if Pos('.', Copy(Value, SecondDot + 1, MaxInt)) <> 0 then
    RaiseException('Installed Synveil version is malformed.');
  Major := ParseVersionPart(Copy(Value, 1, FirstDot - 1));
  Minor := ParseVersionPart(Copy(Value, FirstDot + 1, SecondDot - FirstDot - 1));
  Patch := ParseVersionPart(Copy(Value, SecondDot + 1, MaxInt));
end;

function CompareStrictVersion(const Left, Right: String): Integer;
var
  LM, Lmnr, LP, RM, Rmnr, RP: Integer;
begin
  ParseStrictVersion(Left, LM, Lmnr, LP);
  ParseStrictVersion(Right, RM, Rmnr, RP);
  if LM <> RM then Result := LM - RM
  else if Lmnr <> Rmnr then Result := Lmnr - Rmnr
  else Result := LP - RP;
end;

function IsSafeRelativeManifestPath(const Value: String): Boolean;
var
  Normalized: String;
begin
  Normalized := Value;
  StringChangeEx(Normalized, '\', '/', True);
  Result := (Value <> '') and (Value[1] <> '\') and (Value[1] <> '/') and
    (Pos(':', Value) = 0) and (Pos('../', Normalized) = 0) and
    (Pos('/..', Normalized) = 0);
end;

function ManifestEntryPath(const Line: String): String;
var
  FirstSpace, SecondSpace: Integer;
begin
  Result := '';
  FirstSpace := Pos(' ', Line);
  if FirstSpace <> 65 then exit;
  SecondSpace := Pos(' ', Copy(Line, FirstSpace + 1, MaxInt));
  if SecondSpace = 0 then exit;
  SecondSpace := SecondSpace + FirstSpace;
  Result := Copy(Line, SecondSpace + 1, MaxInt);
  if not IsSafeRelativeManifestPath(Result) then Result := '';
end;

procedure LoadTrustedPreviousManifest(const Path, ExpectedVersion, ExpectedHash: String);
var
  Lines: TStringList;
  I, Marker: Integer;
  Relative: String;
begin
  if not FileExists(Path) then
    RaiseException('The installed package ownership manifest is missing.');
  if (ExpectedHash = '') or
     (CompareText(GetSHA256OfFile(Path), ExpectedHash) <> 0) then
    RaiseException('The installed package ownership manifest identity is not trusted.');
  Lines := TStringList.Create;
  try
    Lines.LoadFromFile(Path);
    if (Lines.Count < 6) or (Lines[0] <> 'Synveil Windows portable package manifest') or
       (Lines[1] <> 'format=1') or (Lines[2] <> 'version=' + ExpectedVersion) or
       (Lines[3] <> 'platform=windows-x86_64') then
      RaiseException('The installed package ownership manifest is not trusted.');
    Marker := Lines.IndexOf('files=sha256 size path');
    if Marker < 0 then RaiseException('The installed package ownership manifest is incomplete.');
    for I := Marker + 1 to Lines.Count - 1 do begin
      Relative := ManifestEntryPath(Lines[I]);
      if Relative = '' then RaiseException('The installed package ownership manifest is malformed.');
      PreviousManifest.Add(Relative);
    end;
  finally
    Lines.Free;
  end;
end;

function CurrentManifestOwns(const Relative: String): Boolean;
var
  Lines: TStringList;
  I, Marker: Integer;
begin
  Result := False;
  if CompareText(GetSHA256OfFile(ExpandConstant('{app}\SYNVEIL-MANIFEST.txt')),
       '{#SynveilManifestSha256}') <> 0 then
    RaiseException('The target package manifest identity is not trusted.');
  Lines := TStringList.Create;
  try
    Lines.LoadFromFile(ExpandConstant('{app}\SYNVEIL-MANIFEST.txt'));
    Marker := Lines.IndexOf('files=sha256 size path');
    if Marker < 0 then RaiseException('The target package manifest is incomplete.');
    for I := Marker + 1 to Lines.Count - 1 do
      if CompareText(ManifestEntryPath(Lines[I]), Relative) = 0 then begin
        Result := True;
        exit;
      end;
  finally
    Lines.Free;
  end;
end;

procedure RemoveProvenObsoleteFiles;
var
  I: Integer;
  Relative, Candidate, Root: String;
  Attributes: LongWord;
begin
  Root := AddBackslash(ExpandConstant('{app}'));
  for I := 0 to PreviousManifest.Count - 1 do begin
    Relative := PreviousManifest[I];
    if not CurrentManifestOwns(Relative) then begin
      Candidate := ExpandFileName(Root + Relative);
      if Pos(Uppercase(Root), Uppercase(Candidate)) <> 1 then
        RaiseException('Obsolete package path escapes the installation root.');
      if FileExists(Candidate) then begin
        Attributes := GetFileAttributes(Candidate);
        if (Attributes = $FFFFFFFF) or ((Attributes and FileAttributeReparsePoint) <> 0) then
          RaiseException('Obsolete package object has ambiguous reparse-point identity.');
        if not DeleteFile(Candidate) then
          RaiseException('An obsolete Synveil package file could not be removed.');
      end;
    end;
  end;
end;

function InitializeSetup(): Boolean;
var
  StartupValue, RepairValue, InstalledVersion, InstalledLocation, InstalledManifestHash: String;
  VersionComparison: Integer;
begin
  { Registry identity, not a writable directory, is the installed-product authority. }
  FreshInstall := not RegQueryStringValue(HKCU, UninstallKey, 'DisplayVersion', InstalledVersion);
  RepairValue := ExpandConstant('{param:REPAIR|__MISSING__}');
  if (RepairValue <> '__MISSING__') and (RepairValue <> '1') then
    RaiseException('/REPAIR accepts only 1');
  RepairMode := RepairValue = '1';
  UpgradeMode := False;
  if RepairMode and FreshInstall then
    RaiseException('Synveil is not installed for this Windows account; Repair cannot continue.');
  if not FreshInstall then begin
    if (not RegQueryStringValue(HKCU, UninstallKey, 'InstallLocation', InstalledLocation)) or
       (not RegQueryStringValue(HKCU, UninstallKey, 'SynveilManifestSha256', InstalledManifestHash)) or
       (CompareText(RemoveBackslashUnlessRoot(InstalledLocation), RemoveBackslashUnlessRoot(ExpandConstant('{app}'))) <> 0) then
      RaiseException('The installed Synveil identity is incomplete or conflicts with this Setup.');
    VersionComparison := CompareStrictVersion(InstalledVersion, '{#SynveilVersion}');
    if VersionComparison > 0 then
      RaiseException('A newer Synveil version is already installed. Downgrade is not supported.');
    if RepairMode and (VersionComparison <> 0) then
      RaiseException('Repair requires the same Synveil version.');
    if (VersionComparison = 0) and WizardSilent and (not RepairMode) then
      RaiseException('Silent same-version Setup requires /REPAIR=1.');
    RepairMode := VersionComparison = 0;
    UpgradeMode := VersionComparison < 0;
  end;
  StartupValue := ExpandConstant('{param:STARTUP|__MISSING__}');
  StartupChoiceExplicit := StartupValue <> '__MISSING__';
  if WizardSilent and FreshInstall and (not StartupChoiceExplicit) then
    RaiseException('Silent installation requires /STARTUP=0 or /STARTUP=1');
  StartupRequested := ParseBooleanOption('STARTUP', (not WizardSilent) and FreshInstall);
  DesktopIconRequested := ParseBooleanOption('DESKTOPICON', (not WizardSilent) and
    (FreshInstall or FileExists(ExpandConstant('{userdesktop}\Synveil.lnk'))));
  LaunchRequested := ParseBooleanOption('LAUNCH', not WizardSilent);
  PreviousManifest := TStringList.Create;
  if not FreshInstall then
    LoadTrustedPreviousManifest(ExpandConstant('{app}\SYNVEIL-MANIFEST.txt'), InstalledVersion,
      InstalledManifestHash);
  Result := True;
end;

procedure OpenNotice(Sender: TObject);
var
  ErrorCode: Integer;
begin
  ExtractTemporaryFile('NOTICE');
  if not ShellExec('open', ExpandConstant('{tmp}\NOTICE'), '', '', SW_SHOWNORMAL, ewNoWait, ErrorCode) then
    MsgBox('Third-party notices could not be opened.', mbError, MB_OK);
end;

procedure InitializeWizard();
begin
  if RepairMode then WizardForm.WelcomeLabel1.Caption := 'Repair Synveil'
  else if UpgradeMode then WizardForm.WelcomeLabel1.Caption := 'Update Synveil'
  else WizardForm.WelcomeLabel1.Caption := 'Install Synveil';
  WizardForm.WelcomeLabel2.Caption := 'Synveil will be installed for your Windows account.';
  WizardForm.LicenseLabel1.Caption := 'Review the MIT License, then choose how Synveil should work on this Windows account.';
  WizardForm.LicenseMemo.Height := ScaleY(100);
  WizardForm.LicenseAcceptedRadio.Top := ScaleY(151);
  WizardForm.LicenseNotAcceptedRadio.Top := ScaleY(174);

  StartupCheck := TNewCheckBox.Create(WizardForm);
  StartupCheck.Name := 'StartupRequestedCheckBox';
  StartupCheck.Parent := WizardForm.LicensePage;
  StartupCheck.SetBounds(ScaleX(0), ScaleY(202), WizardForm.LicensePage.ClientWidth, ScaleY(22));
  StartupCheck.Caption := 'Start Synveil when I sign in';
  StartupCheck.Checked := StartupRequested;
  StartupCheck.TabOrder := 3;

  DesktopIconCheck := TNewCheckBox.Create(WizardForm);
  DesktopIconCheck.Name := 'DesktopIconRequestedCheckBox';
  DesktopIconCheck.Parent := WizardForm.LicensePage;
  DesktopIconCheck.SetBounds(ScaleX(0), ScaleY(226), WizardForm.LicensePage.ClientWidth, ScaleY(22));
  DesktopIconCheck.Caption := 'Create a desktop shortcut';
  DesktopIconCheck.Checked := DesktopIconRequested;
  DesktopIconCheck.TabOrder := 4;

  LaunchCheck := TNewCheckBox.Create(WizardForm);
  LaunchCheck.Name := 'LaunchRequestedCheckBox';
  LaunchCheck.Parent := WizardForm.LicensePage;
  LaunchCheck.SetBounds(ScaleX(0), ScaleY(250), ScaleX(260), ScaleY(22));
  LaunchCheck.Caption := 'Open Synveil after installation';
  LaunchCheck.Checked := LaunchRequested;
  LaunchCheck.TabOrder := 5;

  NoticeButton := TNewButton.Create(WizardForm);
  NoticeButton.Name := 'ThirdPartyNoticeButton';
  NoticeButton.Parent := WizardForm.LicensePage;
  NoticeButton.SetBounds(WizardForm.LicensePage.ClientWidth - ScaleX(155), ScaleY(248), ScaleX(155), ScaleY(25));
  NoticeButton.Caption := 'Third-party &notices';
  NoticeButton.OnClick := @OpenNotice;
  NoticeButton.TabOrder := 6;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  if CurPageID = wpLicense then begin
    StartupRequested := StartupCheck.Checked;
    DesktopIconRequested := DesktopIconCheck.Checked;
    LaunchRequested := LaunchCheck.Checked;
  end;
  Result := True;
end;

procedure CurInstallProgressChanged(CurProgress, MaxProgress: Integer);
begin
  WizardForm.StatusLabel.Caption := 'Installing Synveil...';
end;

function ShouldCreateDesktopIcon(): Boolean;
begin
  Result := DesktopIconRequested;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  ResultCode: Integer;
  StartupState: String;
begin
  if (CurStep = ssPostInstall) and (not FreshInstall) then
    RemoveProvenObsoleteFiles;
  if (CurStep = ssPostInstall) and (FreshInstall or StartupChoiceExplicit) then begin
    if StartupRequested then
      StartupState := 'enabled'
    else
      StartupState := 'disabled';
    if (not Exec(ExpandConstant('{app}\synveil-client.exe'),
      '--startup-preference ' + StartupState, ExpandConstant('{app}'),
      SW_HIDE, ewWaitUntilTerminated, ResultCode)) or (ResultCode <> 0) then
      RaiseException('Synveil could not save the sign-in startup preference. Try again from Settings.');
  end;
  if (CurStep = ssPostInstall) and LaunchRequested and (not WizardSilent) then begin
    if not Exec(ExpandConstant('{app}\synveil-desktop.exe'), '', ExpandConstant('{app}'),
      SW_SHOWNORMAL, ewNoWait, ResultCode) then
      MsgBox('Synveil was installed, but could not be opened. You can open it from the Start menu.', mbError, MB_OK);
  end;
end;

procedure DeinitializeSetup();
begin
  PreviousManifest.Free;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  ResultCode: Integer;
begin
  { Runtime owner removes only its authoritative profile task; preference remains durable. }
  if CurUninstallStep = usUninstall then begin
    if FileExists(ExpandConstant('{app}\synveil-client.exe')) and
       ((not Exec(ExpandConstant('{app}\synveil-client.exe'),
         '--cleanup-startup-integration', ExpandConstant('{app}'), SW_HIDE,
         ewWaitUntilTerminated, ResultCode)) or (ResultCode <> 0)) then
      RaiseException('Synveil could not remove its sign-in task; uninstall stopped before binary removal.');
  end;
end;
