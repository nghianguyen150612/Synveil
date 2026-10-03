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

[Icons]
Name: "{userprograms}\Synveil"; Filename: "{app}\synveil-desktop.exe"; WorkingDir: "{app}"

[Run]
Filename: "{app}\synveil-desktop.exe"; Description: "Launch Synveil"; Flags: nowait postinstall skipifsilent; Check: LaunchRequested

[Code]
var
  LaunchOpt: Boolean;

function ParseBooleanOption(const Name: String; const DefaultValue: Boolean): Boolean;
var
  Value: String;
begin
  Value := ExpandConstant('{param:' + Name + '|}');
  if Value = '' then begin
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
begin
  { STARTUP is reserved for P026 and is intentionally non-mutating. }
  if ParseBooleanOption('STARTUP', False) then
    RaiseException('/STARTUP=1 is not supported by this installer skeleton');
  if ParseBooleanOption('DESKTOPICON', False) then
    RaiseException('/DESKTOPICON=1 is deferred to P023');
  LaunchOpt := ParseBooleanOption('LAUNCH', False);
  Result := True;
end;

function LaunchRequested(): Boolean;
begin
  Result := LaunchOpt;
end;
