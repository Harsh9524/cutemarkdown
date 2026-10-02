; cutemarkdown installer (NSIS 3, Unicode, Modern UI 2)
;
; Per-user install: no administrator rights, no UAC prompt. Everything is written under
; HKCU and %LOCALAPPDATA%. Pages: Welcome -> Install -> Finish.
;
; Build it with scripts/package.sh (the single place that calls makensis; used by
; scripts/build-windows.sh and by the GitHub workflows). Equivalent manual command:
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
!ifndef VERSION
  !error "Pass the version: makensis /DVERSION=1.2.3 ..."
!endif
!ifndef EXE_PATH
  !error "Pass the path of the built executable: makensis /DEXE_PATH=...\cutemarkdown.exe ..."
!endif
!ifndef OUTFILE
  !define OUTFILE "../dist/cutemarkdown-${VERSION}-setup-x64.exe"
!endif

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

; Delete HKCU\<key> if it has no values and no subkeys. (DeleteRegKey /ifempty only looks at subkeys.)
Function un.PruneEmptyKey
  Exch $0                              ; key
  Push $1
  Push $2
  Push $3
  ReadRegStr $1 HKCU "$0" ""           ; default value
  ClearErrors
  EnumRegValue $2 HKCU "$0" 0          ; first named value
  EnumRegKey $3 HKCU "$0" 0            ; first subkey
  ${If} $1 == ""
  ${AndIf} $2 == ""
  ${AndIf} $3 == ""
    DeleteRegKey HKCU "$0"
  ${EndIf}
  Pop $3
  Pop $2
  Pop $1
  Pop $0
FunctionEnd

;--------------------------------------------------------------------------------------------
; Install
;--------------------------------------------------------------------------------------------
Function .onInit
  ${IfNot} ${RunningX64}
  ${OrIfNot} ${AtLeastWin10}
    MessageBox MB_ICONSTOP|MB_OK "cutemarkdown requires 64-bit Windows 10 or later." /SD IDOK
    Abort
  ${EndIf}
FunctionEnd

Section "cutemarkdown" SecMain
  SectionIn RO
  SetShellVarContext current
  SetOutPath "$INSTDIR"

  ; In-place upgrade without closing anything: Windows lets us rename a running .exe even
  ; though it cannot be overwritten, so move it aside and write the new one next to it.
  Delete "$INSTDIR\${EXE}.old"
  ${If} ${FileExists} "$INSTDIR\${EXE}"
    ClearErrors
    Rename "$INSTDIR\${EXE}" "$INSTDIR\${EXE}.old"
  ${EndIf}
  File "/oname=${EXE}" "${EXE_PATH}"
  Delete "$INSTDIR\${EXE}.old"          ; best effort; stays until the old copy exits if it is running
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
  ; Never kill the app: if it is running, ask the user to close it. (Silent uninstalls skip
  ; the question and remove what they can.) ".old" is the previous copy left by an upgrade
  ; that happened while the app was open; it is the locked one until the app is restarted.
  ${Unless} ${Silent}
    retry:
    StrCpy $9 0
    !insertmacro CheckLocked "$INSTDIR\${EXE}"
    !insertmacro CheckLocked "$INSTDIR\${EXE}.old"
    ${If} $9 == 1
      MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "cutemarkdown is running.$\r$\nClose it, then click Retry to continue uninstalling." IDRETRY retry
      Abort
    ${EndIf}
  ${EndUnless}
FunctionEnd

Section "Uninstall"
  SetShellVarContext current

  ; Files (never RMDir /r: only our own files, and the folder only if it is then empty)
  Delete "$INSTDIR\${EXE}"
  Delete "$INSTDIR\${EXE}.old"
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
