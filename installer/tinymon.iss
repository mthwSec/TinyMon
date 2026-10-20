; The TinyMon setup, for Inno Setup 7: a per-user install (no admin rights)
; into %LOCALAPPDATA%\Programs\TinyMon with a Start menu shortcut, an optional
; desktop shortcut and an optional sign-in entry. The release workflow builds
; it from the released exe:
;
;   ISCC /DSourceExe=<path of tinymon.exe> /O<output folder> installer\tinymon.iss
;
; Without /DSourceExe it takes target\release\tinymon.exe. The version comes
; from the exe's version information, which build.rs takes from Cargo.toml.
; Setup never installs or overwrites tinymon.cfg, so an upgrade keeps the
; settings; the uninstaller asks whether to delete them.

#ifndef SourceExe
  #define SourceExe AddBackslash(SourcePath) + "..\target\release\tinymon.exe"
#endif
#if !FileExists(SourceExe)
  #error tinymon.exe not found: build the release exe first, or pass /DSourceExe
#endif
#define AppVersion GetFileProductVersionString(SourceExe)
#if AppVersion == ""
  #error tinymon.exe has no version information
#endif
; Setup finds an earlier install, to upgrade it, by this id: never change it.
#define AppGuid "FBA59099-AB4D-4303-8FBA-8B259E24F38C"

[Setup]
AppId={{{#AppGuid}}
AppName=TinyMon
AppVersion={#AppVersion}
AppPublisher=mthwSec
AppPublisherURL=https://github.com/mthwSec/TinyMon
AppSupportURL=https://github.com/mthwSec/TinyMon/issues
AppUpdatesURL=https://github.com/mthwSec/TinyMon/releases
AppCopyright=Copyright (C) 2026 mthwSec
VersionInfoDescription=TinyMon Setup
; Numbers only, which a pre-release version such as 1.2.0-rc.1 is not.
VersionInfoVersion={#GetVersionNumbersString(SourceExe)}
; Per user: no UAC prompt, and {autopf} is %LOCALAPPDATA%\Programs.
PrivilegesRequired=lowest
DefaultDirName={autopf}\TinyMon
; The folder may already hold a copy of tinymon.exe put there by hand.
DirExistsWarning=no
DisableProgramGroupPage=yes
DisableReadyPage=yes
; tinymon.exe is 64-bit and needs Windows 10 version 1703. Setup itself stays
; 32-bit, which keeps it 0.8 MB smaller, and installs in 64-bit mode.
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.15063
WizardStyle=modern dynamic
SetupIconFile=..\icon.ico
UninstallDisplayIcon={app}\tinymon.exe
UninstallDisplayName=TinyMon
OutputDir=..\target\installer
OutputBaseFilename=TinyMon-{#AppVersion}-setup
Compression=lzma2/max
SolidCompression=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
StartAtSignIn=Start TinyMon when I sign in
ViewGuide=View the user guide
DeleteSettingsQuestion=Also delete your TinyMon settings?%n%nChoose No to keep them, so that installing TinyMon again brings back your metrics, the widget's place, the theme and the hotkeys.

[Tasks]
Name: "startup"; Description: "{cm:StartAtSignIn}"; Check: StartupTaskOffered
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "README.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE-MIT"; DestDir: "{app}"; DestName: "LICENSE-MIT.txt"; Flags: ignoreversion
Source: "..\LICENSE-APACHE"; DestDir: "{app}"; DestName: "LICENSE-APACHE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\TinyMon"; Filename: "{app}\tinymon.exe"; Comment: "Tiny always-on-top resource monitor"
Name: "{autodesktop}\TinyMon"; Filename: "{app}\tinymon.exe"; Comment: "Tiny always-on-top resource monitor"; Tasks: desktopicon

[Run]
; The last page offers to start TinyMon and to open the guide. A silent
; install starts TinyMon again only if it closed it.
Filename: "{code:LaunchFile}"; Parameters: "{code:LaunchParams}"; Description: "{cm:LaunchProgram,TinyMon}"; Flags: nowait postinstall skipifsilent
Filename: "{app}\README.txt"; Description: "{cm:ViewGuide}"; Flags: shellexec nowait postinstall skipifsilent unchecked
Filename: "{code:LaunchFile}"; Parameters: "{code:LaunchParams}"; Flags: nowait; Check: RestartAfterSilentInstall

[UninstallDelete]
; Left behind only when TinyMon stopped in the middle of saving its settings
; or of testing whether its folder is writable (src/config.rs).
Type: files; Name: "{app}\tinymon.cfg.tmp"
Type: files; Name: "{app}\.tinymon_write_test"

[Code]
const
  WM_COMMAND = $0111;
  { The menu's Exit (ID_EXIT in src/main.rs): TinyMon saves its settings and ends. }
  ID_EXIT = 3;
  { Held by a running TinyMon until its process ends (src/main.rs). }
  InstanceMutex = 'Local\TinyMon.Instance';
  PROCESS_QUERY_LIMITED_INFORMATION = $1000;
  UninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{{#AppGuid}}_is1';
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';
  ApprovedKey = 'Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run';

var
  WasInstalled: Boolean;
  ClosedTinyMon: Boolean;
  DeleteSettings: Boolean;

function WinGetWindowThreadProcessId(Wnd: HWND; var ProcessId: DWORD): DWORD;
  external 'GetWindowThreadProcessId@user32.dll stdcall';
function WinOpenProcess(Access: DWORD; Inherit: Integer; ProcessId: DWORD): THandle;
  external 'OpenProcess@kernel32.dll stdcall';
function WinQueryFullProcessImageName(Process: THandle; Flags: DWORD; ExeName: String; var Size: DWORD): Integer;
  external 'QueryFullProcessImageNameW@kernel32.dll stdcall';
function WinCloseHandle(Handle: THandle): Integer;
  external 'CloseHandle@kernel32.dll stdcall';

{ Whether Path names this install's tinymon.exe. }
function IsOurExe(const Path: String): Boolean;
begin
  Result := CompareText(Path, ExpandConstant('{app}\tinymon.exe')) = 0;
end;

{ The path of the exe whose process owns window Wnd, or '' if Windows does not
  tell. }
function ExeOfWindow(Wnd: HWND): String;
var
  Pid, Size: DWORD;
  Process: THandle;
  Name: String;
begin
  Result := '';
  Pid := 0;
  WinGetWindowThreadProcessId(Wnd, Pid);
  Process := WinOpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, Pid);
  if Process = 0 then
    Exit;
  Size := 1024;
  SetLength(Name, Size);
  if WinQueryFullProcessImageName(Process, 0, Name, Size) <> 0 then
    Result := Copy(Name, 1, Size);
  WinCloseHandle(Process);
end;

{ If this install's tinymon.exe runs, ask it to exit the way its menu's Exit
  does, and wait up to ten seconds for its process to end. True when it ran
  and has ended. A TinyMon run from another folder is left alone. So is one
  that runs as administrator, which takes no messages from Setup; if it is
  this install's, Windows then reports tinymon.exe in use. }
function CloseTinyMon: Boolean;
var
  Wnd: HWND;
  Exe: String;
  I: Integer;
begin
  Result := False;
  if not CheckForMutexes(InstanceMutex) then
    Exit;
  Wnd := FindWindowByClassName('TinyMonWindow');
  if Wnd = 0 then
    Exit;
  Exe := ExeOfWindow(Wnd);
  if not IsOurExe(Exe) then begin
    Log('TinyMon runs from another folder, left running: ' + Exe);
    Exit;
  end;
  Log('Closing TinyMon: ' + Exe);
  if not PostMessage(Wnd, WM_COMMAND, ID_EXIT, 0) then
    Exit;
  for I := 1 to 100 do begin
    Sleep(100);
    if not CheckForMutexes(InstanceMutex) then begin
      { The mutex goes as the process ends; give Windows a moment to let go of the exe file too. }
      Sleep(300);
      Result := True;
      Exit;
    end;
  end;
end;

{ What starts TinyMon at the end. A Setup that runs as administrator would
  hand its rights to TinyMon; Explorer starts it with the user's normal rights
  instead. }
function LaunchFile(Param: String): String;
begin
  if IsAdmin then
    Result := ExpandConstant('{win}\explorer.exe')
  else
    Result := ExpandConstant('{app}\tinymon.exe');
end;

function LaunchParams(Param: String): String;
begin
  if IsAdmin then
    Result := '"' + ExpandConstant('{app}\tinymon.exe') + '"'
  else
    Result := '';
end;

function InitializeSetup: Boolean;
begin
  { Read before this install writes its own uninstall entry. }
  WasInstalled := RegKeyExists(HKCU, UninstallKey);
  Result := True;
end;

{ An upgrade leaves the sign-in entry as it is, since the widget's menu (Start
  with Windows) may have turned it on or off after the first install. }
function StartupTaskOffered: Boolean;
begin
  Result := not WasInstalled;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  ClosedTinyMon := CloseTinyMon;
  Result := '';
end;

function RestartAfterSilentInstall: Boolean;
begin
  Result := WizardSilent and ClosedTinyMon;
end;

{ Whether the Run value starts this install's tinymon.exe: the path inside a
  leading pair of double quotes, or else the whole value, as launches in
  src/startup.rs reads it. }
function RunValueIsOurs: Boolean;
var
  Value: String;
  Quote: Integer;
begin
  Result := False;
  if not RegQueryStringValue(HKCU, RunKey, 'TinyMon', Value) then
    Exit;
  Value := Trim(Value);
  if Copy(Value, 1, 1) = '"' then begin
    Delete(Value, 1, 1);
    Quote := Pos('"', Value);
    if Quote > 0 then
      Value := Copy(Value, 1, Quote - 1);
  end;
  Result := IsOurExe(Value);
end;

{ On: the Run value as the widget's menu writes it (src/startup.rs), the
  quoted path, with Task Manager's disabled flag for it cleared. Off: only an
  entry that starts this install's exe goes; one that starts a copy elsewhere
  belongs to that copy. }
procedure SetStartAtSignIn(Enable: Boolean);
begin
  if Enable then begin
    RegWriteStringValue(HKCU, RunKey, 'TinyMon', '"' + ExpandConstant('{app}\tinymon.exe') + '"');
    RegDeleteValue(HKCU, ApprovedKey, 'TinyMon');
  end else if RunValueIsOurs then begin
    RegDeleteValue(HKCU, RunKey, 'TinyMon');
    RegDeleteValue(HKCU, ApprovedKey, 'TinyMon');
  end else
    Log('No sign-in entry of this install to remove');
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and StartupTaskOffered then
    SetStartAtSignIn(WizardIsTaskSelected('startup'));
end;

{ Whether the command line holds Switch, in any case. }
function HasSwitch(const Switch: String): Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), Switch) = 0 then begin
      Result := True;
      Exit;
    end;
end;

{ TinyMon's files in Dir (src/config.rs), then Dir itself if that left it empty. }
procedure DeleteSettingsIn(const Dir: String);
begin
  DeleteFile(Dir + '\tinymon.cfg');
  DeleteFile(Dir + '\tinymon.cfg.tmp');
  DeleteFile(Dir + '\.tinymon_write_test');
  RemoveDir(Dir);
end;

{ The settings go when the command line has /DELETESETTINGS or the user says
  Yes; a silent uninstall asks nothing and keeps them otherwise. The sign-in
  entry of this install always goes. }
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  case CurUninstallStep of
    usUninstall:
      begin
        CloseTinyMon;
        if HasSwitch('/DELETESETTINGS') then
          DeleteSettings := True
        else if UninstallSilent then
          DeleteSettings := False
        else
          DeleteSettings := MsgBox(CustomMessage('DeleteSettingsQuestion'),
            mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES;
      end;
    usPostUninstall:
      begin
        SetStartAtSignIn(False);
        if DeleteSettings then begin
          DeleteSettingsIn(ExpandConstant('{app}'));
          DeleteSettingsIn(ExpandConstant('{userappdata}\TinyMon'));
        end;
      end;
  end;
end;
