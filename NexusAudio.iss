[Setup]
AppName=Nexus Audio
AppVersion=2.4.1
DefaultDirName={autopf}\Nexus Audio
DefaultGroupName=Nexus Audio
UninstallDisplayIcon={app}\nexus-audio.exe
Compression=lzma2
SolidCompression=yes
OutputDir=Output
OutputBaseFilename=nexus_audio_2.4.1_setup
PrivilegesRequired=lowest

[Files]
Source: "target\release\nexus-audio.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Nexus Audio"; Filename: "{app}\nexus-audio.exe"
Name: "{autodesktop}\Nexus Audio"; Filename: "{app}\nexus-audio.exe"

[Run]
Filename: "{app}\nexus-audio.exe"; Description: "Launch Nexus Audio"; Flags: postinstall nowait skipifsilent
