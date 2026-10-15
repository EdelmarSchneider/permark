; Hooks for the Windows (NSIS) installer, wired in by `bundle.windows.nsis.installerHooks` in tauri.conf.json.
;
; This file does two things. Both write only under HKEY_CURRENT_USER, so no administrator rights are needed.
;
; 1. THE LANGUAGE. The installer asks which language to use (English, Portuguese (Brazil), Spanish, German). After the
;    files are in place, that language is written down as the code the app uses for its own interface (`en`, `pt-BR`,
;    `es`, `de`) in HKCU\Software\TrailMark\Language. The app reads it only until a language is picked in Settings, so
;    an update, or a reinstall in another language, never replaces a choice made in the app.
;
; 2. "OPEN WITH TRAILMARK" in the file manager. Two options, both offered on the first page of a fresh install (checked
;    by default) and both also available in the app's Settings:
;      - the context menu: "Open with TrailMark" on folders, on the background of a folder and on .md/.markdown files;
;      - the file association: TrailMark is listed in the "Open with" menu of .md/.markdown files. The default app
;        is never set here: Windows protects it, and the person picks it in Windows Settings.
;    The choices are remembered as HKCU\Software\TrailMark\OpenWithMenu and \FileAssociation (1 = on, 0 = off). An update
;    is not asked anything: it keeps what the person chose (it refreshes the entries that are on so they point at the
;    new files, never adds what was left off, never removes what was kept), and an update of an install that predates
;    this feature changes nothing. Uninstalling removes every entry. Turning an entry off, or uninstalling, also deletes
;    the keys above it that are left empty (see "The empty keys that removing the entries leaves").
;    The layout is the one src-tauri/src/shell_integration/entries.rs writes from the app; a Rust test keeps them equal.
;
; The tests compile this file in a small installer of their own and point TRAILMARK_APP_KEY / TRAILMARK_CLASSES_KEY at a
; throwaway registry key, which is why those two can be defined before this file is included.
;
; The two options sit on the welcome page, which is the first page the template shows: the MUI_PAGE_CUSTOMFUNCTION_*
; defines at the end of this file apply to the next page the template inserts, and this file is included before any.
; If a Tauri update ever changes the first page, check that the options still show up.

!ifndef TRAILMARK_APP_KEY
  !define TRAILMARK_APP_KEY "Software\TrailMark"
!endif
!ifndef TRAILMARK_CLASSES_KEY
  !define TRAILMARK_CLASSES_KEY "Software\Classes"
!endif

!define OPENWITH_PROGID "TrailMark.Markdown"
!define OPENWITH_MENU_CHOICE "OpenWithMenu"
!define OPENWITH_ASSOC_CHOICE "FileAssociation"

; The words, in the four languages of the installer (the numbers are the Windows language ids).
LangString OpenWithMenuLabel 1033 "Open with TrailMark"
LangString OpenWithMenuLabel 1046 "Abrir com o TrailMark"
LangString OpenWithMenuLabel 3082 "Abrir con TrailMark"
LangString OpenWithMenuLabel 1031 "Mit TrailMark öffnen"
LangString OpenWithTypeName 1033 "Markdown document"
LangString OpenWithTypeName 1046 "Documento Markdown"
LangString OpenWithTypeName 3082 "Documento de Markdown"
LangString OpenWithTypeName 1031 "Markdown-Dokument"
LangString OpenWithMenuOption 1033 "Add “Open with TrailMark” to the right-click menu"
LangString OpenWithMenuOption 1046 "Adicionar “Abrir com o TrailMark” ao menu de clique direito"
LangString OpenWithMenuOption 3082 "Agregar “Abrir con TrailMark” al menú de clic derecho"
LangString OpenWithMenuOption 1031 "„Mit TrailMark öffnen“ zum Kontextmenü hinzufügen"
LangString OpenWithAssocOption 1033 "Associate .md files with TrailMark"
LangString OpenWithAssocOption 1046 "Associar arquivos .md ao TrailMark"
LangString OpenWithAssocOption 3082 "Asociar los archivos .md con TrailMark"
LangString OpenWithAssocOption 1031 "Dateien vom Typ .md mit TrailMark verknüpfen"

; "1" / "0" for what the person ticked on the welcome page; "" when that page did not show (silent and passive installs).
Var OpenWithMenu
Var OpenWithAssoc
Var OpenWithMenuBox
Var OpenWithAssocBox
; The text of the menu entry and of the file type, and the program they start, while the entries are written.
Var OpenWithLabel
Var OpenWithTypeName
Var OpenWithExe
Var OpenWithChanged

; ---- The entries ---------------------------------------------------------------------------------------------------
; Each group is written (WRITE) or deleted (DELETE) by one macro, so the install and the uninstall name the same keys.

!macro OPENWITH_MENU_WRITE KEY ARGUMENT
  WriteRegStr HKCU "${TRAILMARK_CLASSES_KEY}\${KEY}" "" "$OpenWithLabel"
  WriteRegStr HKCU "${TRAILMARK_CLASSES_KEY}\${KEY}" "Icon" "$\"$OpenWithExe$\",0"
  WriteRegStr HKCU "${TRAILMARK_CLASSES_KEY}\${KEY}\command" "" "$\"$OpenWithExe$\" $\"${ARGUMENT}$\""
!macroend

!macro OPENWITH_MENU_DELETE KEY ARGUMENT
  DeleteRegKey HKCU "${TRAILMARK_CLASSES_KEY}\${KEY}"
!macroend

; %1 is the folder or file that was clicked; %V is the folder whose background was clicked.
!macro OPENWITH_MENU OP
  !insertmacro OPENWITH_MENU_${OP} "Directory\shell\TrailMark" "%1"
  !insertmacro OPENWITH_MENU_${OP} "Directory\Background\shell\TrailMark" "%V"
  !insertmacro OPENWITH_MENU_${OP} "SystemFileAssociations\.md\shell\TrailMark" "%1"
  !insertmacro OPENWITH_MENU_${OP} "SystemFileAssociations\.markdown\shell\TrailMark" "%1"
!macroend

!macro OPENWITH_PROGID_WRITE
  WriteRegStr HKCU "${TRAILMARK_CLASSES_KEY}\${OPENWITH_PROGID}" "" "$OpenWithTypeName"
  WriteRegStr HKCU "${TRAILMARK_CLASSES_KEY}\${OPENWITH_PROGID}\DefaultIcon" "" "$\"$OpenWithExe$\",0"
  WriteRegStr HKCU "${TRAILMARK_CLASSES_KEY}\${OPENWITH_PROGID}\shell\open" "FriendlyAppName" "TrailMark"
  WriteRegStr HKCU "${TRAILMARK_CLASSES_KEY}\${OPENWITH_PROGID}\shell\open\command" "" "$\"$OpenWithExe$\" $\"%1$\""
!macroend

!macro OPENWITH_PROGID_DELETE
  DeleteRegKey HKCU "${TRAILMARK_CLASSES_KEY}\${OPENWITH_PROGID}"
!macroend

; The "Open with" list of an extension is the names of the values under its OpenWithProgids key. Only our own value is
; ever deleted: the key holds the other programs' too.
!macro OPENWITH_LIST_WRITE KEY
  WriteRegStr HKCU "${TRAILMARK_CLASSES_KEY}\${KEY}" "${OPENWITH_PROGID}" ""
!macroend

!macro OPENWITH_LIST_DELETE KEY
  DeleteRegValue HKCU "${TRAILMARK_CLASSES_KEY}\${KEY}" "${OPENWITH_PROGID}"
!macroend

!macro OPENWITH_LIST OP
  !insertmacro OPENWITH_LIST_${OP} ".md\OpenWithProgids"
  !insertmacro OPENWITH_LIST_${OP} ".markdown\OpenWithProgids"
!macroend

; ---- The empty keys that removing the entries leaves --------------------------------------------------------------------
; Writing an entry creates the keys above it when they are not there; deleting it leaves them behind, empty. Each of these
; is deleted when nothing at all is in it: no subkey, no value, not even a default value. DeleteRegKey /ifempty alone is
; not enough: it only looks at subkeys, so it would take the values of the key with it (the program that opens .md files is
; the default value of the .md key). The one case missed is a default value that is not text, on an otherwise empty key.
; The first-level keys of Windows itself (Directory, SystemFileAssociations) are never deleted. The list is
; entries::tidy_parents in the app, and a Rust test keeps the two equal.

!macro OPENWITH_PRUNE KEY
  Push $R0
  Push $R1
  ClearErrors
  EnumRegKey $R0 HKCU "${TRAILMARK_CLASSES_KEY}\${KEY}" 0
  ${IfNot} ${Errors}
  ${AndIf} $R0 == ""
    ; no subkey. The default value is next, then the named values.
    ClearErrors
    ReadRegStr $R1 HKCU "${TRAILMARK_CLASSES_KEY}\${KEY}" ""
    ${If} ${Errors}
      EnumRegValue $R1 HKCU "${TRAILMARK_CLASSES_KEY}\${KEY}" 0
      ${If} $R1 == ""
        DeleteRegKey /ifempty HKCU "${TRAILMARK_CLASSES_KEY}\${KEY}"
      ${EndIf}
    ${EndIf}
  ${EndIf}
  ClearErrors
  Pop $R1
  Pop $R0
!macroend

; Deepest first, so a key is looked at after the keys under it have gone.
!macro OPENWITH_MENU_TIDY
  !insertmacro OPENWITH_PRUNE "Directory\shell"
  !insertmacro OPENWITH_PRUNE "Directory\Background\shell"
  !insertmacro OPENWITH_PRUNE "Directory\Background"
  !insertmacro OPENWITH_PRUNE "SystemFileAssociations\.md\shell"
  !insertmacro OPENWITH_PRUNE "SystemFileAssociations\.md"
  !insertmacro OPENWITH_PRUNE "SystemFileAssociations\.markdown\shell"
  !insertmacro OPENWITH_PRUNE "SystemFileAssociations\.markdown"
!macroend

; The extension's own shell key is not written here, but one left empty would keep the extension's key from going.
!macro OPENWITH_LIST_TIDY
  !insertmacro OPENWITH_PRUNE ".md\OpenWithProgids"
  !insertmacro OPENWITH_PRUNE ".md\shell"
  !insertmacro OPENWITH_PRUNE ".md"
  !insertmacro OPENWITH_PRUNE ".markdown\OpenWithProgids"
  !insertmacro OPENWITH_PRUNE ".markdown\shell"
  !insertmacro OPENWITH_PRUNE ".markdown"
!macroend

; ---- What to do -----------------------------------------------------------------------------------------------------

; Sets VAR ("1" on, "0" off, "" leave alone) from what the person chose earlier, when the welcome page did not ask. A
; fresh install that cannot ask (silent, passive) takes the defaults (on); an update with nothing recorded changes nothing.
!macro OPENWITH_RESOLVE VAR CHOICE
  ${If} ${VAR} == ""
    ClearErrors
    ReadRegDWORD $R0 HKCU "${TRAILMARK_APP_KEY}" "${CHOICE}"
    ${If} ${Errors}
      ${If} $UpdateMode = 1
        StrCpy ${VAR} ""
      ${Else}
        StrCpy ${VAR} "1"
      ${EndIf}
    ${Else}
      StrCpy ${VAR} "$R0"
    ${EndIf}
  ${EndIf}
!macroend

; Sets VAR to the text for an entry: an update keeps the text already there (the app rewords it when the language of
; the interface changes, and an update must not undo that), anything else uses the installer's language.
!macro OPENWITH_TEXT VAR KEY INSTALLER_TEXT
  StrCpy ${VAR} ""
  ${If} $UpdateMode = 1
    ReadRegStr ${VAR} HKCU "${TRAILMARK_CLASSES_KEY}\${KEY}" ""
  ${EndIf}
  ${If} ${VAR} == ""
    StrCpy ${VAR} "${INSTALLER_TEXT}"
  ${EndIf}
!macroend

; Applies the two options after the files are in place. Whatever is on is (re)written, so it points at the files just
; installed; whatever is off is removed; the choice is recorded for the next update.
!macro OPENWITH_APPLY
  StrCpy $OpenWithExe "$INSTDIR\${MAINBINARYNAME}.exe"
  StrCpy $OpenWithChanged 0

  !insertmacro OPENWITH_RESOLVE $OpenWithMenu "${OPENWITH_MENU_CHOICE}"
  ${If} $OpenWithMenu == "1"
    !insertmacro OPENWITH_TEXT $OpenWithLabel "Directory\shell\TrailMark" "$(OpenWithMenuLabel)"
    !insertmacro OPENWITH_MENU WRITE
    WriteRegDWORD HKCU "${TRAILMARK_APP_KEY}" "${OPENWITH_MENU_CHOICE}" 1
    StrCpy $OpenWithChanged 1
  ${ElseIf} $OpenWithMenu == "0"
    !insertmacro OPENWITH_MENU DELETE
    !insertmacro OPENWITH_MENU_TIDY
    WriteRegDWORD HKCU "${TRAILMARK_APP_KEY}" "${OPENWITH_MENU_CHOICE}" 0
    StrCpy $OpenWithChanged 1
  ${EndIf}

  !insertmacro OPENWITH_RESOLVE $OpenWithAssoc "${OPENWITH_ASSOC_CHOICE}"
  ${If} $OpenWithAssoc == "1"
    !insertmacro OPENWITH_TEXT $OpenWithTypeName "${OPENWITH_PROGID}" "$(OpenWithTypeName)"
    !insertmacro OPENWITH_PROGID_WRITE
    !insertmacro OPENWITH_LIST WRITE
    WriteRegDWORD HKCU "${TRAILMARK_APP_KEY}" "${OPENWITH_ASSOC_CHOICE}" 1
    StrCpy $OpenWithChanged 1
  ${ElseIf} $OpenWithAssoc == "0"
    !insertmacro OPENWITH_LIST DELETE
    !insertmacro OPENWITH_PROGID_DELETE
    !insertmacro OPENWITH_LIST_TIDY
    WriteRegDWORD HKCU "${TRAILMARK_APP_KEY}" "${OPENWITH_ASSOC_CHOICE}" 0
    StrCpy $OpenWithChanged 1
  ${EndIf}

  ; Explorer redraws its menus and icons.
  ${If} $OpenWithChanged = 1
    !insertmacro UPDATEFILEASSOC
  ${EndIf}
!macroend

; ---- The two options on the welcome page ----------------------------------------------------------------------------

; Adds the two check boxes under the welcome text. They start as the person last chose, and on for a first install.
Function OpenWithOptionsShow
  Push $0

  ClearErrors
  ReadRegDWORD $0 HKCU "${TRAILMARK_APP_KEY}" "${OPENWITH_MENU_CHOICE}"
  ${If} ${Errors}
    StrCpy $0 1
  ${EndIf}
  ${NSD_CreateCheckbox} 120u 146u 195u 18u "$(OpenWithMenuOption)"
  Pop $OpenWithMenuBox
  SetCtlColors $OpenWithMenuBox "000000" "FFFFFF"
  ; The welcome text is a tall label that would cover the box: raise the box above it.
  System::Call 'user32::SetWindowPos(p $OpenWithMenuBox, p 0, i 0, i 0, i 0, i 0, i 3)'
  ${If} $0 <> 0
    ${NSD_Check} $OpenWithMenuBox
  ${EndIf}

  ClearErrors
  ReadRegDWORD $0 HKCU "${TRAILMARK_APP_KEY}" "${OPENWITH_ASSOC_CHOICE}"
  ${If} ${Errors}
    StrCpy $0 1
  ${EndIf}
  ${NSD_CreateCheckbox} 120u 166u 195u 18u "$(OpenWithAssocOption)"
  Pop $OpenWithAssocBox
  SetCtlColors $OpenWithAssocBox "000000" "FFFFFF"
  System::Call 'user32::SetWindowPos(p $OpenWithAssocBox, p 0, i 0, i 0, i 0, i 0, i 3)'
  ${If} $0 <> 0
    ${NSD_Check} $OpenWithAssocBox
  ${EndIf}

  Pop $0
FunctionEnd

Function OpenWithOptionsLeave
  Push $0

  ${NSD_GetState} $OpenWithMenuBox $0
  ${If} $0 = ${BST_CHECKED}
    StrCpy $OpenWithMenu "1"
  ${Else}
    StrCpy $OpenWithMenu "0"
  ${EndIf}

  ${NSD_GetState} $OpenWithAssocBox $0
  ${If} $0 = ${BST_CHECKED}
    StrCpy $OpenWithAssoc "1"
  ${Else}
    StrCpy $OpenWithAssoc "0"
  ${EndIf}

  Pop $0
FunctionEnd

!define MUI_PAGE_CUSTOMFUNCTION_SHOW OpenWithOptionsShow
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE OpenWithOptionsLeave

; ---- The hooks the template calls -----------------------------------------------------------------------------------

!macro NSIS_HOOK_POSTINSTALL
  ; $LANGUAGE is the Windows language id (LCID) of the language the installer ran in.
  StrCpy $R8 ""
  ${If} $LANGUAGE = 1046
    StrCpy $R8 "pt-BR"
  ${ElseIf} $LANGUAGE = 3082
  ${OrIf} $LANGUAGE = 1034
    StrCpy $R8 "es"
  ${ElseIf} $LANGUAGE = 1031
    StrCpy $R8 "de"
  ${ElseIf} $LANGUAGE = 1033
    StrCpy $R8 "en"
  ${EndIf}

  ${If} $R8 != ""
    ; An update (run with /UPDATE) keeps what an earlier install recorded, whatever language its own run guessed.
    ; A fresh install, or a reinstall run by hand, records the language it ran in.
    ReadRegStr $R9 HKCU "${TRAILMARK_APP_KEY}" "Language"
    ${If} $UpdateMode <> 1
    ${OrIf} $R9 == ""
      WriteRegStr HKCU "${TRAILMARK_APP_KEY}" "Language" "$R8"
    ${EndIf}
  ${EndIf}

  !insertmacro OPENWITH_APPLY
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Uninstalling takes every "Open with TrailMark" entry with it. An update's uninstall step leaves them: the install that
  ; follows rewrites them according to the person's choices. What the person chose stays recorded (as the language does)
  ; unless they also ask for the application data to go.
  ${If} $UpdateMode <> 1
    !insertmacro OPENWITH_MENU DELETE
    !insertmacro OPENWITH_MENU_TIDY
    !insertmacro OPENWITH_LIST DELETE
    !insertmacro OPENWITH_PROGID_DELETE
    !insertmacro OPENWITH_LIST_TIDY
    !insertmacro UPDATEFILEASSOC
  ${EndIf}

  ; Uninstalling with "delete the application data" ticked takes the recorded language and choices with it; an update's
  ; uninstall step keeps them.
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    DeleteRegKey HKCU "${TRAILMARK_APP_KEY}"
  ${EndIf}
!macroend
