; A look at the installer's welcome page (hooks.nsh) in the six languages, without installing PerMark.
; Compile (NSIS is the one Tauri downloads, %LOCALAPPDATA%\tauri\NSIS):
;   makensis.exe /DOUT=<where>\welcome-preview.exe welcome-preview.nsi
; Run it, pick a language, look at the page, close the window. It only shows the page: it writes nothing and installs
; nothing (the page reads and writes only the throw-away key below). Re-check the page here when a text of hooks.nsh
; changes or a language is added; the real installer then needs one look too (docs: distribution.md, Default app).

Unicode true
RequestExecutionLevel user
!include MUI2.nsh
!include LogicLib.nsh
!include StrFunc.nsh
${StrLoc}

!ifndef OUT
  !define OUT "welcome-preview.exe"
!endif
Name "PerMark (welcome page preview)"
OutFile "${OUT}"
InstallDir "$TEMP\PerMark-welcome-preview"

; what hooks.nsh expects from the template
!define MAINBINARYNAME "permark"
!define PERMARK_APP_KEY "Software\PerMark-welcome-preview"
!define PERMARK_CLASSES_KEY "Software\PerMark-welcome-preview\Classes"
!define PERMARK_REGAPPS_KEY "Software\PerMark-welcome-preview\RegisteredApplications"
!include "hooks.nsh"
Var UpdateMode
Var DeleteAppDataCheckboxState
!macro UPDATEFILEASSOC
!macroend

!define MUI_LANGDLL_ALLLANGUAGES
!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "PortugueseBR"
!insertmacro MUI_LANGUAGE "SpanishInternational"
!insertmacro MUI_LANGUAGE "German"
!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "Italian"

Function .onInit
  !insertmacro MUI_LANGDLL_DISPLAY
FunctionEnd

Section
  DetailPrint "Preview only: nothing was installed."
SectionEnd
