#define MyAppVersion GetEnv("SPAN_VERSION")
#if MyAppVersion == ""
  #undef MyAppVersion
  #define MyAppVersion "0.0.0-dev"
#endif

[Setup]
AppId={{7F513DC3-27FB-4FA8-A0EB-B71053782142}
AppName=Span
AppVersion={#MyAppVersion}
AppPublisher=Span
DefaultDirName={autopf}\Span
DefaultGroupName=Span
DisableProgramGroupPage=yes
OutputDir=..\..\dist
OutputBaseFilename=span-windows-x64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SetupIconFile=..\..\crates\span\windows\Span.ico
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\span-gui.exe
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "附加选项："; Flags: unchecked

[Files]
Source: "..\..\target\x86_64-pc-windows-msvc\release\span.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\target\x86_64-pc-windows-msvc\release\span-gui.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Span"; Filename: "{app}\span-gui.exe"
Name: "{autodesktop}\Span"; Filename: "{app}\span-gui.exe"; Tasks: desktopicon

[Run]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM span.exe"; Flags: runhidden waituntilterminated
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""Span Discovery (UDP-In)"""; Flags: runhidden waituntilterminated
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""Span Discovery UI (UDP-In)"""; Flags: runhidden waituntilterminated
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""Span Clipboard (TCP-In)"""; Flags: runhidden waituntilterminated
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""Span Discovery (UDP-In)"" dir=in action=allow program=""{app}\span.exe"" enable=yes profile=any protocol=UDP localport=46792 remoteip=localsubnet"; Flags: runhidden waituntilterminated
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""Span Discovery UI (UDP-In)"" dir=in action=allow program=""{app}\span-gui.exe"" enable=yes profile=any protocol=UDP remoteip=localsubnet"; Flags: runhidden waituntilterminated
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""Span Clipboard (TCP-In)"" dir=in action=allow program=""{app}\span.exe"" enable=yes profile=any protocol=TCP localport=46793 remoteip=localsubnet"; Flags: runhidden waituntilterminated
Filename: "{app}\span.exe"; Parameters: "install"; StatusMsg: "正在启用后台同步…"; Flags: runasoriginaluser runhidden waituntilterminated
Filename: "{app}\span-gui.exe"; Description: "启动 Span"; Flags: runasoriginaluser nowait postinstall skipifsilent

[UninstallRun]
Filename: "{app}\span.exe"; Parameters: "uninstall"; Flags: runhidden waituntilterminated skipifdoesntexist
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM span.exe"; Flags: runhidden waituntilterminated
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""Span Discovery (UDP-In)"""; Flags: runhidden waituntilterminated
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""Span Discovery UI (UDP-In)"""; Flags: runhidden waituntilterminated
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""Span Clipboard (TCP-In)"""; Flags: runhidden waituntilterminated
