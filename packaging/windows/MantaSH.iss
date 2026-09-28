#define AppName "MantaSH"
#ifndef AppVersion
  #error AppVersion must be provided
#endif
#ifndef BuildArch
  #error BuildArch must be provided
#endif
#ifndef SourceDir
  #error SourceDir must be provided
#endif
#ifndef OutputDir
  #error OutputDir must be provided
#endif
#ifndef OutputBase
  #error OutputBase must be provided
#endif

[Setup]
AppId={{A7A17D44-0650-46A4-ACD0-1B2F1DB83026}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=MantaSH contributors
AppPublisherURL=https://github.com/realmx/mantash
DefaultDirName={autopf}\MantaSH
DefaultGroupName=MantaSH
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed={#BuildArch}
#if BuildArch != "x86compatible"
ArchitecturesInstallIn64BitMode={#BuildArch}
#endif
OutputDir={#OutputDir}
OutputBaseFilename={#OutputBase}
SetupIconFile={#SourceDir}\mantash.ico
UninstallDisplayIcon={app}\mantash.exe
Compression=lzma
SolidCompression=yes
WizardStyle=modern

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional icons:"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\MantaSH"; Filename: "{app}\mantash.exe"
Name: "{autodesktop}\MantaSH"; Filename: "{app}\mantash.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\mantash.exe"; Description: "Launch MantaSH"; Flags: nowait postinstall skipifsilent
