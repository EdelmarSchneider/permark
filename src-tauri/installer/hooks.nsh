; Hooks for the Windows (NSIS) installer, wired in by `bundle.windows.nsis.installerHooks` in tauri.conf.json.
;
; The installer asks which language to use (English, Portuguese (Brazil), Spanish, German). After the files are in
; place, that language is written down as the code the app uses for its own interface (`en`, `pt-BR`, `es`, `de`) in
; HKCU\Software\TrailMark\Language. The app reads it only until a language is picked in Settings, so an update, or a
; reinstall in another language, never replaces a choice made in the app.

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
    ReadRegStr $R9 HKCU "Software\TrailMark" "Language"
    ${If} $UpdateMode <> 1
    ${OrIf} $R9 == ""
      WriteRegStr HKCU "Software\TrailMark" "Language" "$R8"
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Uninstalling with "delete the application data" ticked takes the recorded language with it; an update's
  ; uninstall step keeps it.
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    DeleteRegKey HKCU "Software\TrailMark"
  ${EndIf}
!macroend
