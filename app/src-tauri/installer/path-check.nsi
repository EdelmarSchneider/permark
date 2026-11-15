; A check of the installer's PATH option (hooks.nsh), without installing PerMark and without touching the real PATH: the
; option's steps run on a throw-away key, HKCU\Software\PerMark-path-check, which stands in for HKCU\Environment and
; HKCU\Software\PerMark and is deleted at the end.
; Compile (NSIS is the one Tauri downloads, %LOCALAPPDATA%\tauri\NSIS):
;   makensis.exe /DOUT=<where>\path-check.exe path-check.nsi
; Run <where>\path-check.exe: it shows nothing, writes path-check.log beside itself and exits with the number of failed
; cases (0 = every case passed). Re-run it when the PATH steps of hooks.nsh change.

Unicode true
RequestExecutionLevel user
SilentInstall silent
!include LogicLib.nsh
!include WinMessages.nsh
!include nsDialogs.nsh
!include StrFunc.nsh
${StrCase}
${StrLoc}

!ifndef OUT
  !define OUT "path-check.exe"
!endif
Name "PerMark (PATH check)"
OutFile "${OUT}"

!define CHECK_KEY "Software\PerMark-path-check"
; what hooks.nsh expects from the template
!define MAINBINARYNAME "permark"
!define PERMARK_APP_KEY "${CHECK_KEY}\App"
!define PERMARK_CLASSES_KEY "${CHECK_KEY}\Classes"
!define PERMARK_REGAPPS_KEY "${CHECK_KEY}\RegisteredApplications"
!define PERMARK_ENV_KEY "${CHECK_KEY}\Environment"
!include "hooks.nsh"
Var UpdateMode
Var PassiveMode

Var Failed
Var Log
Var Got
Var GotType

; ---- Setting the stand-in PATH ----------------------------------------------------------------------------------------

!macro RESET
  DeleteRegKey HKCU "${CHECK_KEY}"
  StrCpy $PermarkPath ""
  StrCpy $UpdateMode 0
!macroend

!macro SET_PATH TYPE VALUE
  ${If} ${TYPE} = 1
    WriteRegStr HKCU "${PERMARK_ENV_KEY}" "Path" "${VALUE}"
  ${Else}
    WriteRegExpandStr HKCU "${PERMARK_ENV_KEY}" "Path" "${VALUE}"
  ${EndIf}
!macroend

; VAR = "C:\xxx...", LENGTH characters in all
!macro FILLER VAR LENGTH
  Push $4
  StrCpy ${VAR} "C:\"
  ${Do}
    StrLen $4 ${VAR}
    ${If} $4 >= ${LENGTH}
      ${ExitDo}
    ${EndIf}
    StrCpy ${VAR} "${VAR}x"
  ${Loop}
  Pop $4
!macroend

; A PATH longer than any NSIS string: CHARS characters (U+4141, from bytes 0x41), written straight through the API.
!macro SET_LONG_PATH CHARS
  Push $0
  Push $1
  Push $2
  IntOp $1 ${CHARS} + 1
  IntOp $1 $1 * 2
  System::Alloc $1
  Pop $2
  IntOp $0 $1 - 2
  System::Call 'kernel32::RtlFillMemory(p r2, i r0, i 0x41)'
  System::Call 'advapi32::RegCreateKeyExW(p 0x80000001, w "${PERMARK_ENV_KEY}", i 0, p 0, i 0, i 0x2, p 0, *p .r0, p 0)'
  System::Call 'advapi32::RegSetValueExW(p r0, w "Path", i 0, i 2, p r2, i r1)'
  System::Call 'advapi32::RegCloseKey(p r0)'
  System::Free $2
  Pop $2
  Pop $1
  Pop $0
!macroend

; ---- Reading it back --------------------------------------------------------------------------------------------------

; $GotType = the value's type (0 = no value), $Got = its size in bytes
!macro QUERY_PATH
  Push $0
  Push $1
  Push $2
  Push $3
  StrCpy $GotType 0
  StrCpy $Got 0
  System::Call 'advapi32::RegOpenKeyExW(p 0x80000001, w "${PERMARK_ENV_KEY}", i 0, i 0x1, *p .r1) i .r0'
  ${If} $0 = 0
    System::Call 'advapi32::RegQueryValueExW(p r1, w "Path", p 0, *i .r2, p 0, *i .r3) i .r0'
    System::Call 'advapi32::RegCloseKey(p r1)'
    ${If} $0 = 0
      StrCpy $GotType $2
      StrCpy $Got $3
    ${EndIf}
  ${EndIf}
  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend

!macro REPORT OK NAME
  ${If} ${OK} == 1
    FileWrite $Log "pass  ${NAME}$\r$\n"
  ${Else}
    IntOp $Failed $Failed + 1
    FileWrite $Log "FAIL  ${NAME}: type $GotType, value [$Got]$\r$\n"
  ${EndIf}
!macroend

; the stand-in PATH is exactly VALUE (case and all) with type TYPE
!macro EXPECT_PATH TYPE VALUE NAME
  !insertmacro QUERY_PATH
  ClearErrors
  ReadRegStr $Got HKCU "${PERMARK_ENV_KEY}" "Path"
  StrCpy $0 0
  ${If} $GotType = ${TYPE}
  ${AndIfNot} ${Errors}
  ${AndIf} $Got S== "${VALUE}"
    StrCpy $0 1
  ${EndIf}
  !insertmacro REPORT $0 "${NAME}"
!macroend

!macro EXPECT_NO_PATH NAME
  !insertmacro QUERY_PATH
  StrCpy $0 0
  ${If} $GotType = 0
    StrCpy $0 1
  ${EndIf}
  !insertmacro REPORT $0 "${NAME}"
!macroend

; the long PATH kept its type and every byte of its size
!macro EXPECT_LONG_PATH CHARS NAME
  !insertmacro QUERY_PATH
  IntOp $1 ${CHARS} + 1
  IntOp $1 $1 * 2
  StrCpy $0 0
  ${If} $GotType = 2
  ${AndIf} $Got = $1
    StrCpy $0 1
  ${EndIf}
  !insertmacro REPORT $0 "${NAME}"
!macroend

; the recorded entry (what an uninstall removes) is VALUE; "" = none
!macro EXPECT_RECORD VALUE NAME
  ReadRegStr $Got HKCU "${PERMARK_APP_KEY}" "${PERMARK_PATH_ENTRY}"
  StrCpy $GotType "record"
  StrCpy $0 0
  ${If} $Got S== "${VALUE}"
    StrCpy $0 1
  ${EndIf}
  !insertmacro REPORT $0 "${NAME}"
!macroend

; ---- The cases --------------------------------------------------------------------------------------------------------

Section
  StrCpy $Failed 0
  StrCpy $PassiveMode 0
  StrCpy $INSTDIR "$LOCALAPPDATA\PerMark-path-check"
  FileOpen $Log "$EXEDIR\path-check.log" w
  FileWrite $Log "folder: $INSTDIR$\r$\n"

  ; adding
  !insertmacro RESET
  !insertmacro PERMARK_PATH_ADD
  !insertmacro EXPECT_PATH 2 "$INSTDIR" "no PATH yet: one entry, REG_EXPAND_SZ"
  !insertmacro EXPECT_RECORD "$INSTDIR" "no PATH yet: entry recorded"

  !insertmacro RESET
  !insertmacro SET_PATH 2 "C:\Tools;%USERPROFILE%\bin"
  !insertmacro PERMARK_PATH_ADD
  !insertmacro EXPECT_PATH 2 "C:\Tools;%USERPROFILE%\bin;$INSTDIR" "appended, variables kept unexpanded"

  !insertmacro RESET
  !insertmacro SET_PATH 1 "C:\Tools"
  !insertmacro PERMARK_PATH_ADD
  !insertmacro EXPECT_PATH 1 "C:\Tools;$INSTDIR" "REG_SZ stays REG_SZ"

  !insertmacro RESET
  !insertmacro SET_PATH 2 "C:\Tools;"
  !insertmacro PERMARK_PATH_ADD
  !insertmacro EXPECT_PATH 2 "C:\Tools;$INSTDIR" "a closing ; is not doubled"

  !insertmacro RESET
  ${StrCase} $1 "$INSTDIR" "U"
  !insertmacro SET_PATH 2 "C:\Tools;$1\;D:\x"
  !insertmacro PERMARK_PATH_ADD
  !insertmacro EXPECT_PATH 2 "C:\Tools;$1\;D:\x" "present in other case with a closing \: unchanged"
  !insertmacro EXPECT_RECORD "" "present already: nothing recorded"

  !insertmacro RESET
  !insertmacro SET_PATH 2 "$\"%LOCALAPPDATA%\PerMark-path-check$\";C:\Tools"
  !insertmacro PERMARK_PATH_ADD
  !insertmacro EXPECT_PATH 2 "$\"%LOCALAPPDATA%\PerMark-path-check$\";C:\Tools" "present through a variable, quoted: unchanged"

  !insertmacro RESET
  !insertmacro SET_PATH 2 "C:\Tools;$INSTDIR"
  !insertmacro PERMARK_PATH_ADD
  !insertmacro PERMARK_PATH_ADD
  !insertmacro EXPECT_PATH 2 "C:\Tools;$INSTDIR" "never twice"

  ; the edge: a new value of NSIS_MAX_STRLEN - 2 characters (the most ReadRegStr reads back, so the entry can be removed
  ; later) is written; one more and the PATH is left exactly as it is
  StrLen $3 "$INSTDIR"
  IntOp $3 ${NSIS_MAX_STRLEN} - $3
  IntOp $3 $3 - 3
  !insertmacro FILLER $2 $3
  !insertmacro RESET
  !insertmacro SET_PATH 2 "$2"
  !insertmacro PERMARK_PATH_ADD
  !insertmacro EXPECT_PATH 2 "$2;$INSTDIR" "the longest value that is read back: added"
  !insertmacro PERMARK_PATH_REMOVE
  !insertmacro EXPECT_PATH 2 "$2" "the longest value: removed again"

  IntOp $3 $3 + 1
  !insertmacro FILLER $2 $3
  !insertmacro RESET
  !insertmacro SET_PATH 2 "$2"
  !insertmacro PERMARK_PATH_ADD
  !insertmacro EXPECT_PATH 2 "$2" "one character more: unchanged"
  !insertmacro EXPECT_RECORD "" "one character more: nothing recorded"

  !insertmacro RESET
  !insertmacro SET_LONG_PATH 5000
  !insertmacro PERMARK_PATH_ADD
  !insertmacro EXPECT_LONG_PATH 5000 "longer than an NSIS string: unchanged"
  !insertmacro EXPECT_RECORD "" "longer than an NSIS string: nothing recorded"

  !insertmacro RESET
  WriteRegDWORD HKCU "${PERMARK_ENV_KEY}" "Path" 7
  !insertmacro PERMARK_PATH_ADD
  !insertmacro QUERY_PATH
  StrCpy $0 0
  ${If} $GotType = 4
    StrCpy $0 1
  ${EndIf}
  !insertmacro REPORT $0 "not text: unchanged"

  ; removing
  !insertmacro RESET
  !insertmacro SET_PATH 2 "C:\Tools;D:\x"
  !insertmacro PERMARK_PATH_ADD
  !insertmacro SET_PATH 2 "C:\Tools;$INSTDIR;D:\x;;E:\y;"
  !insertmacro PERMARK_PATH_REMOVE
  !insertmacro EXPECT_PATH 2 "C:\Tools;D:\x;;E:\y;" "only the recorded entry goes, the rest byte for byte"
  !insertmacro EXPECT_RECORD "" "removed: record cleared"

  !insertmacro RESET
  !insertmacro SET_PATH 1 "C:\Tools"
  !insertmacro PERMARK_PATH_ADD
  !insertmacro PERMARK_PATH_REMOVE
  !insertmacro EXPECT_PATH 1 "C:\Tools" "add then remove: as before, REG_SZ"

  !insertmacro RESET
  !insertmacro PERMARK_PATH_ADD
  !insertmacro PERMARK_PATH_REMOVE
  !insertmacro EXPECT_NO_PATH "a PATH the installer created goes with the entry"

  !insertmacro RESET
  !insertmacro SET_PATH 2 "C:\Tools;$INSTDIR"
  !insertmacro PERMARK_PATH_REMOVE
  !insertmacro EXPECT_PATH 2 "C:\Tools;$INSTDIR" "nothing recorded: the person's own entry stays"

  !insertmacro RESET
  !insertmacro PERMARK_PATH_ADD
  !insertmacro SET_LONG_PATH 5000
  !insertmacro PERMARK_PATH_REMOVE
  !insertmacro EXPECT_LONG_PATH 5000 "too long to remove from: unchanged"
  !insertmacro EXPECT_RECORD "$INSTDIR" "too long to remove from: record kept"

  ; the choice
  !insertmacro RESET
  !insertmacro SET_PATH 2 "C:\Tools"
  !insertmacro PERMARK_PATH_APPLY
  !insertmacro EXPECT_PATH 2 "C:\Tools" "not asked, nothing recorded: off"

  !insertmacro RESET
  !insertmacro SET_PATH 2 "C:\Tools"
  WriteRegDWORD HKCU "${PERMARK_APP_KEY}" "${PERMARK_PATH_CHOICE}" 1
  !insertmacro PERMARK_PATH_APPLY
  !insertmacro EXPECT_PATH 2 "C:\Tools;$INSTDIR" "not asked, chosen before: added"

  !insertmacro RESET
  !insertmacro SET_PATH 2 "C:\Tools"
  WriteRegDWORD HKCU "${PERMARK_APP_KEY}" "${PERMARK_PATH_CHOICE}" 1
  StrCpy $UpdateMode 1
  !insertmacro PERMARK_PATH_APPLY
  !insertmacro EXPECT_PATH 2 "C:\Tools" "update: PATH left as it is"

  !insertmacro RESET
  !insertmacro SET_PATH 2 "C:\Tools"
  StrCpy $PermarkPath "1"
  !insertmacro PERMARK_PATH_APPLY
  !insertmacro EXPECT_PATH 2 "C:\Tools;$INSTDIR" "ticked: added"
  StrCpy $PermarkPath "0"
  !insertmacro PERMARK_PATH_APPLY
  !insertmacro EXPECT_PATH 2 "C:\Tools" "unticked: removed"
  ReadRegDWORD $Got HKCU "${PERMARK_APP_KEY}" "${PERMARK_PATH_CHOICE}"
  StrCpy $GotType "choice"
  StrCpy $0 0
  ${If} $Got == "0"
    StrCpy $0 1
  ${EndIf}
  !insertmacro REPORT $0 "unticked: choice recorded"

  DeleteRegKey HKCU "${CHECK_KEY}"
  FileWrite $Log "failed: $Failed$\r$\n"
  FileClose $Log
  SetErrorLevel $Failed
SectionEnd
