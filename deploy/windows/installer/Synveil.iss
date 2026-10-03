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

[Code]
var
  { P026 consumes this reviewed state boundary; P023 performs no startup mutation. }
  StartupRequested: Boolean;
  StartupChoiceExplicit: Boolean;
  FreshInstall: Boolean;
  DesktopIconRequested: Boolean;
  LaunchRequested: Boolean;
  StartupCheck: TNewCheckBox;
  DesktopIconCheck: TNewCheckBox;
  LaunchCheck: TNewCheckBox;
  NoticeButton: TNewButton;

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

function InitializeSetup(): Boolean;
var
  StartupValue: String;
begin
  { Silent defaults fail closed. Interactive fresh-install defaults opt in. }
  FreshInstall := not DirExists(ExpandConstant('{app}'));
  StartupValue := ExpandConstant('{param:STARTUP|__MISSING__}');
  StartupChoiceExplicit := StartupValue <> '__MISSING__';
  if WizardSilent and (not StartupChoiceExplicit) then
    RaiseException('Silent installation requires /STARTUP=0 or /STARTUP=1');
  StartupRequested := ParseBooleanOption('STARTUP', (not WizardSilent) and FreshInstall);
  DesktopIconRequested := ParseBooleanOption('DESKTOPICON', (not WizardSilent) and
    (FreshInstall or FileExists(ExpandConstant('{userdesktop}\Synveil.lnk'))));
  LaunchRequested := ParseBooleanOption('LAUNCH', not WizardSilent);
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
  WizardForm.WelcomeLabel1.Caption := 'Install Synveil';
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
