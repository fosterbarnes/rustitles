#ifndef AppVersion
#define AppVersion "0.0.0"
#endif
#define AppName "Rustitles"
#define AppDisplayName "Rustitles (x64)"
#define AppPublisher "fosterbarnes"
#define AppURL "https://github.com/fosterbarnes/rustitles"
#define ExeName "rustitles.exe"

[Setup]
AppId={{E4B7C8A1-2D3F-4A90-8B1C-71F0A2D9E401}
AppName={#AppName}
UninstallDisplayName={#AppDisplayName}
AppVersion={#AppVersion}
DisableDirPage=auto
AppPublisher={#AppPublisher}
AppPublisherURL={#AppURL}
AppSupportURL={#AppURL}
AppUpdatesURL={#AppURL}
DefaultDirName={localappdata}\{#AppName}
UninstallDisplayIcon={app}\{#ExeName}
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
DisableProgramGroupPage=yes
LicenseFile=..\LICENSE
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=commandline
OutputDir=Output
OutputBaseFilename=rustitles-x64-installer
SetupIconFile=..\crate\resources\rustitles_icon.ico
WizardImageFile=..\.res\ico\installer-wizard-large.png
WizardSmallImageFile=..\.res\ico\installer-wizard-small.png
SolidCompression=yes
WizardStyle=classic dark
CloseApplications=yes
Uninstallable=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Messages]
SetupWindowTitle=Rustitles v{#AppVersion} installer

[CustomMessages]
CreateStartMenuIcon=Create Start Menu shortcut

[Tasks]
Name: "desktopicon"; Description: "Create Desktop shortcut"; GroupDescription: "{cm:AdditionalIcons}"
Name: "startmenuicon"; Description: "{cm:CreateStartMenuIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\publish\build\x64\*"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#ExeName}"; Tasks: startmenuicon
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#ExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#ExeName}"; Description: "{cm:LaunchProgram,{#StringChange(AppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[Code]
const
  PBM_SETBKCOLOR  = $0408;
  PBM_SETBARCOLOR = $0409;
  ProgressBarFillColor = $A0637C;
  ProgressBarBgColor = $2D2D2D;

procedure ApplyGaugeColors;
begin
  if (WizardForm <> nil) and (WizardForm.ProgressGauge.Handle <> 0) then
  begin
    SendMessage(WizardForm.ProgressGauge.Handle, PBM_SETBKCOLOR, 0, ProgressBarBgColor);
    SendMessage(WizardForm.ProgressGauge.Handle, PBM_SETBARCOLOR, 0, ProgressBarFillColor);
  end;
end;

procedure InitializeWizard();
begin
  WizardForm.LicenseAcceptedRadio.Checked := True;
end;

procedure CurPageChanged(CurPageID: Integer);
begin
  if (CurPageID = wpInstalling) or (CurPageID = wpReady) then
    ApplyGaugeColors;
end;
