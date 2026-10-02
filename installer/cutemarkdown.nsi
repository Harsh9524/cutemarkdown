; cutemarkdown installer (NSIS 3, Unicode, Modern UI 2)
;
; Per-user install: no administrator rights, no UAC prompt. Everything is written under
; HKCU and %LOCALAPPDATA%. Pages: Welcome -> Install -> Finish.
;
; Build it with scripts/package.sh (the single place that calls makensis; used by
; scripts/build-windows.sh and by the GitHub workflows). package.sh writes the build parameters
; (VERSION, EXE_PATH, OUTFILE) to installer/build-defs.nsh, which this script includes. They can
; also be given by hand instead:
;   makensis -DVERSION=1.0.0 -DEXE_PATH=<abs path>/cutemarkdown.exe -DOUTFILE=<abs path>/setup.exe installer/cutemarkdown.nsi
; Relative paths inside this script are relative to its own directory (makensis changes into it),
; so EXE_PATH and OUTFILE must be ABSOLUTE.
;
; Silent install / uninstall:
;   cutemarkdown-<ver>-setup-x64.exe /S            (optionally /D=C:\some\dir, must be the last argument)
;   "%LOCALAPPDATA%\Programs\cutemarkdown\uninstall.exe" /S

Unicode true
SetCompressor /SOLID lzma
RequestExecutionLevel user
ManifestSupportedOS Win10
ManifestLongPathAware true

;--------------------------------------------------------------------------------------------
; Build-time parameters
;--------------------------------------------------------------------------------------------
; Written by scripts/package.sh (absent when the parameters are given with -D instead). Passing them
; through a file keeps paths and the version out of makensis' command line, where Git Bash/MSYS
; argument rewriting and stray "\r" characters from Windows tools can corrupt them.
!include /NONFATAL "build-defs.nsh"
!ifndef VERSION
  !error "Pass the version: makensis /DVERSION=1.2.3 ..."
!endif
!ifndef EXE_PATH
  !error "Pass the path of the built executable: makensis /DEXE_PATH=...\cutemarkdown.exe ..."
!endif
!ifndef OUTFILE
  !define OUTFILE "../dist/cutemarkdown-${VERSION}-setup-x64.exe"
!endif

; makensis on Windows only treats "\" as a path separator in the File command (NSIS util.cpp,
; get_dir_name: "BUGBUG: Windows should support \ and /"). Given C:/a/b/app.exe it looks for a file
; with that whole name in the current directory and fails with 'File: "C:/a/b/app.exe" -> no files
; found.' even though the file exists. So always hand File a backslash path. (makensis on Linux
; converts "\" back to "/".)
!searchreplace EXE_PATH_NSIS "${EXE_PATH}" "/" "\"

; "1.2.3-rc.1" -> numeric quad "1.2.3.0" for the version resource.
!searchparse "${VERSION}-" "" VER_CORE "-"
!searchparse "${VER_CORE}." "" VER_MAJOR "." VER_MINOR "." VER_PATCH "."
!define VERSION_QUAD "${VER_MAJOR}.${VER_MINOR}.${VER_PATCH}.0"

;--------------------------------------------------------------------------------------------
; Identity
;--------------------------------------------------------------------------------------------
!define APPNAME       "cutemarkdown"
!define EXE           "cutemarkdown.exe"
!define PUBLISHER     "cutemarkdown"
!define PROGID        "cutemarkdown.Markdown"
!define APP_URL       "https://github.com/harsh9524/cutemarkdown"
!define APP_KEY       "Software\cutemarkdown"
!define CAP_KEY       "Software\cutemarkdown\Capabilities"
!define UNINST_KEY    "Software\Microsoft\Windows\CurrentVersion\Uninstall\cutemarkdown"

Name "${APPNAME}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${APPNAME}"
InstallDirRegKey HKCU "${APP_KEY}" "InstallDir"   ; upgrades go to the existing location
BrandingText "${APPNAME} ${VERSION}"
ShowInstDetails nevershow
ShowUninstDetails nevershow

VIProductVersion "${VERSION_QUAD}"
VIFileVersion    "${VERSION_QUAD}"
VIAddVersionKey "ProductName"     "${APPNAME}"
VIAddVersionKey "FileDescription" "${APPNAME} setup"
VIAddVersionKey "CompanyName"     "${PUBLISHER} contributors"
VIAddVersionKey "LegalCopyright"  "(c) cutemarkdown contributors. MIT License."
VIAddVersionKey "FileVersion"     "${VERSION}"
VIAddVersionKey "ProductVersion"  "${VERSION}"

;--------------------------------------------------------------------------------------------
; Modern UI
;--------------------------------------------------------------------------------------------
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "WinVer.nsh"
!include "x64.nsh"

!define MUI_ICON   "../assets/brand/app.ico"
!define MUI_UNICON "../assets/brand/app.ico"
!define MUI_WELCOMEFINISHPAGE_BITMAP "welcome.bmp"
!define MUI_HEADERIMAGE
!define MUI_HEADERIMAGE_RIGHT
!define MUI_HEADERIMAGE_BITMAP   "header.bmp"
!define MUI_HEADERIMAGE_UNBITMAP "header.bmp"
!define MUI_ABORTWARNING

; Welcome
!define MUI_WELCOMEPAGE_TITLE "Welcome to cutemarkdown"
!define MUI_WELCOMEPAGE_TEXT "cutemarkdown ${VERSION} is a fast, tiny, native Markdown reader.$\r$\n$\r$\nSetup installs it for your account only. No administrator rights are needed, and nothing is changed outside your user profile.$\r$\n$\r$\nClick Install to continue."
!insertmacro MUI_PAGE_WELCOME

; Install
!insertmacro MUI_PAGE_INSTFILES

; Finish: [x] Launch cutemarkdown   [ ] Make cutemarkdown my default Markdown app
!define MUI_FINISHPAGE_TITLE "cutemarkdown is ready"
!define MUI_FINISHPAGE_TEXT "cutemarkdown ${VERSION} has been installed. Double-click any Markdown file to read it, or right-click it and choose Open with."
!define MUI_FINISHPAGE_RUN
!define MUI_FINISHPAGE_RUN_TEXT "Launch cutemarkdown"
!define MUI_FINISHPAGE_RUN_FUNCTION LaunchApp
!define MUI_FINISHPAGE_SHOWREADME ""
!define MUI_FINISHPAGE_SHOWREADME_TEXT "Make cutemarkdown my default Markdown app"
!define MUI_FINISHPAGE_SHOWREADME_FUNCTION OpenDefaultApps
!define MUI_FINISHPAGE_SHOWREADME_NOTCHECKED   ; opt-in: opens Windows Settings
!insertmacro MUI_PAGE_FINISH

; Uninstall
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

;--------------------------------------------------------------------------------------------
; File extensions we register for. One list, used by install and uninstall.
;--------------------------------------------------------------------------------------------
!macro ForEachExt MACRO
  !insertmacro ${MACRO} ".md"
  !insertmacro ${MACRO} ".markdown"
  !insertmacro ${MACRO} ".mdown"
  !insertmacro ${MACRO} ".mkd"
  !insertmacro ${MACRO} ".mkdn"
  !insertmacro ${MACRO} ".mdwn"
  !insertmacro ${MACRO} ".mdtext"
!macroend

; Install: offer cutemarkdown for the extension. If nothing at all handles the extension
; (no default in HKCU\Software\Classes nor HKLM\Software\Classes), also make it the default
; there. A handler that already exists is never replaced, and UserChoice is never touched.
!macro RegisterExt EXT
  WriteRegNone HKCU "Software\Classes\${EXT}\OpenWithProgids" "${PROGID}"
  WriteRegStr  HKCU "Software\Classes\Applications\${EXE}\SupportedTypes" "${EXT}" ""
  WriteRegStr  HKCU "${CAP_KEY}\FileAssociations" "${EXT}" "${PROGID}"
  ReadRegStr $0 HKCU "Software\Classes\${EXT}" ""
  ReadRegStr $1 HKLM "Software\Classes\${EXT}" ""
  ${If} $0 == ""
  ${AndIf} $1 == ""
    WriteRegStr HKCU "Software\Classes\${EXT}" "" "${PROGID}"
    WriteRegStr HKCU "Software\Classes\${EXT}" "Content Type" "text/markdown"
    WriteRegStr HKCU "Software\Classes\${EXT}" "PerceivedType" "text"
  ${EndIf}
!macroend

; Uninstall: remove only what we added; leave anything another app or the user put there.
!macro UnregisterExt EXT
  DeleteRegValue HKCU "Software\Classes\${EXT}\OpenWithProgids" "${PROGID}"
  ReadRegStr $0 HKCU "Software\Classes\${EXT}" ""
  ${If} $0 == "${PROGID}"
    DeleteRegValue HKCU "Software\Classes\${EXT}" ""
    DeleteRegValue HKCU "Software\Classes\${EXT}" "Content Type"
    DeleteRegValue HKCU "Software\Classes\${EXT}" "PerceivedType"
  ${EndIf}
  Push "Software\Classes\${EXT}\OpenWithProgids"
  Call un.PruneEmptyKey
  Push "Software\Classes\${EXT}"
  Call un.PruneEmptyKey
!macroend

;--------------------------------------------------------------------------------------------
; Helpers
;--------------------------------------------------------------------------------------------
!macro NotifyAssocChanged
  ; SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, NULL, NULL): refresh icons and associations
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend

Function LaunchApp
  Exec '"$INSTDIR\${EXE}"'
FunctionEnd

Function OpenDefaultApps
  ; Windows 11 can open straight to our entry; Windows 10 shows the Default apps page.
  ${If} ${AtLeastBuild} 22000
    ExecShell "open" "ms-settings:defaultapps?registeredAppUser=${APPNAME}"
  ${Else}
    ExecShell "open" "ms-settings:defaultapps"
  ${EndIf}
FunctionEnd

; Delete HKCU\<key> if it has no values (the default value included) and no subkeys.
; (DeleteRegKey /ifempty only looks at subkeys.)
Function un.PruneEmptyKey
  Exch $0                              ; key
  Push $1
  Push $2
  ClearErrors
  EnumRegValue $1 HKCU "$0" 0          ; error flag set = no values at all. A present default value enumerates with an EMPTY name, so the returned name cannot be used for this test
  ${If} ${Errors}
    EnumRegKey $2 HKCU "$0" 0          ; "" = no subkeys. (EnumRegKey does NOT set the error flag at the end of the list, so test the name here, not ${Errors}.)
    ${If} $2 == ""
      DeleteRegKey HKCU "$0"
    ${EndIf}
  ${EndIf}
  Pop $2
  Pop $1
  Pop $0
FunctionEnd

;--------------------------------------------------------------------------------------------
; Install
;--------------------------------------------------------------------------------------------
Function .onInit
  ; The stub is 32-bit x86, so ${RunningX64} (IsWow64) is also true on ARM64, including
  ; Windows 10 on ARM, which cannot run x64 code. Allow native x64, or ARM64 with Windows 11
  ; (build 22000+), the first ARM release that emulates x64.
  StrCpy $0 0
  ${If} ${AtLeastWin10}
    ${If} ${IsNativeAMD64}
      StrCpy $0 1
    ${ElseIf} ${IsNativeARM64}
    ${AndIf} ${AtLeastBuild} 22000
      StrCpy $0 1
    ${EndIf}
  ${EndIf}
  ${If} $0 = 0
    MessageBox MB_ICONSTOP|MB_OK "cutemarkdown requires 64-bit (x64) Windows 10 or later, or Windows 11 on ARM." /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
FunctionEnd

Section "cutemarkdown" SecMain
  SectionIn RO
  SetShellVarContext current
  SetOutPath "$INSTDIR"

  ; In-place upgrade without closing anything: Windows allows renaming a running .exe but not
  ; overwriting or deleting it. Move it aside under a free name (.old, .old1, ...), because an
  ; earlier .old may still be running too.
  Delete "$INSTDIR\${EXE}.old*"                 ; best effort: earlier copies that have exited
  StrCpy $R0 ""                                 ; suffix actually used ("" = nothing moved)
  ${If} ${FileExists} "$INSTDIR\${EXE}"
    StrCpy $R1 0
    ${Do}
      ${If} $R1 == 0
        StrCpy $R2 ""
      ${Else}
        StrCpy $R2 $R1
      ${EndIf}
      ClearErrors
      Rename "$INSTDIR\${EXE}" "$INSTDIR\${EXE}.old$R2"
      ${IfNot} ${Errors}
        StrCpy $R0 ".old$R2"
        ${Break}
      ${EndIf}
      IntOp $R1 $R1 + 1
    ${LoopUntil} $R1 >= 100
  ${EndIf}
  ClearErrors
  ${If} ${FileExists} "$INSTDIR\${EXE}"
    SetErrors                                   ; could not be moved aside: do not try to overwrite it
  ${Else}
    File "/oname=${EXE}" "${EXE_PATH_NSIS}"
  ${EndIf}
  ${If} ${Errors}
    ${If} $R0 != ""
      Rename "$INSTDIR\${EXE}$R0" "$INSTDIR\${EXE}"   ; put the previous version back
    ${EndIf}
    ; Nothing else has been changed yet (no uninstaller, shortcut or registry writes), so the
    ; registry still matches the binary on disk.
    MessageBox MB_ICONSTOP|MB_OK "Could not replace $INSTDIR\${EXE}.$\r$\nClose cutemarkdown and run setup again." /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
  Delete "$INSTDIR\${EXE}.old*"                 ; best effort; a copy that is still running stays until the next install or uninstall
  WriteUninstaller "$INSTDIR\uninstall.exe"

  ; Start Menu shortcut
  CreateShortcut "$SMPROGRAMS\${APPNAME}.lnk" "$INSTDIR\${EXE}" "" "$INSTDIR\${EXE}" 0 SW_SHOWNORMAL "" "cutemarkdown - Markdown reader"

  ; Install location and version (read back on upgrade)
  WriteRegStr HKCU "${APP_KEY}" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "${APP_KEY}" "Version" "${VERSION}"

  ; ProgID: how Windows opens and displays a Markdown document
  WriteRegStr HKCU "Software\Classes\${PROGID}" "" "Markdown document"
  WriteRegStr HKCU "Software\Classes\${PROGID}" "FriendlyTypeName" "Markdown document"
  WriteRegStr HKCU "Software\Classes\${PROGID}\DefaultIcon" "" '"$INSTDIR\${EXE}",0'
  WriteRegStr HKCU "Software\Classes\${PROGID}\shell" "" "open"
  WriteRegStr HKCU "Software\Classes\${PROGID}\shell\open\command" "" '"$INSTDIR\${EXE}" "%1"'

  ; "Open with" entry for the exe itself
  WriteRegStr HKCU "Software\Classes\Applications\${EXE}" "FriendlyAppName" "${APPNAME}"
  WriteRegStr HKCU "Software\Classes\Applications\${EXE}\DefaultIcon" "" '"$INSTDIR\${EXE}",0'
  WriteRegStr HKCU "Software\Classes\Applications\${EXE}\shell\open\command" "" '"$INSTDIR\${EXE}" "%1"'

  ; Windows Settings > Default apps
  WriteRegStr HKCU "${CAP_KEY}" "ApplicationName" "${APPNAME}"
  WriteRegStr HKCU "${CAP_KEY}" "ApplicationDescription" "A fast, minimal, native Markdown reader."
  WriteRegStr HKCU "${CAP_KEY}" "ApplicationIcon" '"$INSTDIR\${EXE}",0'
  WriteRegStr HKCU "Software\RegisteredApplications" "${APPNAME}" "${CAP_KEY}"

  ; File extensions
  !insertmacro ForEachExt RegisterExt

  ; Add/Remove Programs
  SectionGetSize ${SecMain} $0          ; KB of installed files
  WriteRegStr   HKCU "${UNINST_KEY}" "DisplayName" "${APPNAME}"
  WriteRegStr   HKCU "${UNINST_KEY}" "DisplayIcon" '"$INSTDIR\${EXE}",0'
  WriteRegStr   HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr   HKCU "${UNINST_KEY}" "Publisher" "${PUBLISHER}"
  WriteRegStr   HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr   HKCU "${UNINST_KEY}" "URLInfoAbout" "${APP_URL}"
  WriteRegStr   HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegStr   HKCU "${UNINST_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegDWORD HKCU "${UNINST_KEY}" "EstimatedSize" $0
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1

  !insertmacro NotifyAssocChanged
SectionEnd

;--------------------------------------------------------------------------------------------
; Uninstall
;--------------------------------------------------------------------------------------------
; $9 = 1 if PATH exists but cannot be opened for writing (a running .exe is locked by Windows).
; Uses $0.
!macro CheckLocked PATH
  ${If} ${FileExists} "${PATH}"
    ClearErrors
    FileOpen $0 "${PATH}" a
    ${If} ${Errors}
      StrCpy $9 1
    ${Else}
      FileClose $0
    ${EndIf}
  ${EndIf}
!macroend

Function un.onInit
  ; Never kill the app. If it (or a ".old*" copy left by an upgrade done while it was open) is
  ; running, ask the user to close it. A silent uninstall cannot ask, so it changes nothing and
  ; exits with code 2; run it again once the app is closed.
  retry:
  StrCpy $9 0
  !insertmacro CheckLocked "$INSTDIR\${EXE}"
  FindFirst $R3 $R4 "$INSTDIR\${EXE}.old*"
  ${DoWhile} $R4 != ""
    !insertmacro CheckLocked "$INSTDIR\$R4"
    FindNext $R3 $R4
  ${Loop}
  FindClose $R3
  ${If} $9 == 1
    MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "cutemarkdown is running.$\r$\nClose it, then click Retry to continue uninstalling." /SD IDCANCEL IDRETRY retry
    SetErrorLevel 2
    Abort
  ${EndIf}
FunctionEnd

Section "Uninstall"
  SetShellVarContext current

  ; Files (never RMDir /r: only our own files, and the folder only if it is then empty)
  ; The program goes first. If it cannot be deleted (it was started after the check in
  ; un.onInit), stop before removing uninstall.exe or any registry entry, so the uninstall can be
  ; run again.
  ClearErrors
  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\${EXE}.old*"
  ${If} ${Errors}
    MessageBox MB_ICONSTOP|MB_OK "cutemarkdown is running. Close it, then run the uninstaller again." /SD IDOK
    SetErrorLevel 2
    Abort
  ${EndIf}
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"

  ; Start Menu
  Delete "$SMPROGRAMS\${APPNAME}.lnk"

  ; File extensions first (uses the ProgID name, not the keys below)
  !insertmacro ForEachExt UnregisterExt

  ; Registry
  DeleteRegKey   HKCU "Software\Classes\${PROGID}"
  DeleteRegKey   HKCU "Software\Classes\Applications\${EXE}"
  DeleteRegValue HKCU "Software\RegisteredApplications" "${APPNAME}"
  DeleteRegKey   HKCU "${APP_KEY}"
  DeleteRegKey   HKCU "${UNINST_KEY}"

  ; Settings the app itself created are the user's data: keep them unless asked
  ; (and always keep them on a silent uninstall).
  ${Unless} ${Silent}
    ${If} ${FileExists} "$APPDATA\${APPNAME}\*.*"
      MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 "Also delete your cutemarkdown settings and history?$\r$\n$\r$\n$APPDATA\${APPNAME}" IDNO keep_settings
      RMDir /r "$APPDATA\${APPNAME}"
      keep_settings:
    ${EndIf}
  ${EndUnless}

  !insertmacro NotifyAssocChanged
SectionEnd
