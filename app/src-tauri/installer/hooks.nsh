; Hooks of the Windows (NSIS) installer, wired in by `bundle.windows.nsis.installerHooks` in tauri.conf.json.
; Everything here writes under HKEY_CURRENT_USER: no administrator rights.
;
; 1. LANGUAGE. The installer's language is recorded as the app's language code (en, pt-BR, es, de, zh-CN, it) in
;    HKCU\Software\PerMark\Language. The app reads it once, at its first start, when no language was picked in it.
;
; 2. "OPEN WITH". Every file type PerMark reads is registered so it is offered under "Open with" (a ProgID per group of
;    types, the extension's OpenWithProgids list, and the app's capabilities for the Default Apps page). The default app
;    of a type is NEVER set here: Windows protects it. The welcome page asks "Open these files with PerMark" (Markdown
;    checked, the other types unchecked; the welcome text says Windows confirms); with a type checked, the installer opens Windows' Default Apps page for
;    PerMark at the end, where the person confirms. An update asks nothing and opens nothing.
;
; 3. FILE MANAGER ENTRIES. "Open with PerMark" (folders, the background of a folder, Markdown files) and "New Markdown
;    file" (folders, the background of a folder), each a check box on the welcome page, also switchable in the app's
;    settings. The choice is remembered as HKCU\Software\PerMark\OpenWithMenu and \NewMarkdownMenu (1 on, 0 off): an
;    update keeps it (rewrites what is on so it points at the new files, never adds what is off).
;
; Uninstalling removes everything above (and the keys it leaves empty); with "Delete the application data" it also removes
; HKCU\Software\PerMark and PerMark's folders in the profile. An update's uninstall step removes nothing.
; The layout is the one the app writes (crates/plugins/shell-integration, crates/core/src/desktop); a Rust test keeps them
; equal. A test installer may define the three roots below before including this file.

!ifndef PERMARK_APP_KEY
  !define PERMARK_APP_KEY "Software\PerMark"
!endif
!ifndef PERMARK_CLASSES_KEY
  !define PERMARK_CLASSES_KEY "Software\Classes"
!endif
!ifndef PERMARK_REGAPPS_KEY
  !define PERMARK_REGAPPS_KEY "Software\RegisteredApplications"
!endif

!define PERMARK_MENU_CHOICE "OpenWithMenu"
!define PERMARK_NEW_CHOICE "NewMarkdownMenu"
!define PERMARK_TYPES_CHOICE "DefaultTypes"
!define PERMARK_DEFAULT_APPS "ms-settings:defaultapps?registeredAppUser=PerMark"

; ---- Words (Windows language ids: 1033 en, 1046 pt-BR, 3082 es, 1031 de, 2052 zh-CN, 1040 it) ----------------------

LangString PermarkWelcome 1033 "Setup will install PerMark for you. No administrator rights are needed.$\r$\n$\r$\nFor the file types you check below, Windows asks you to confirm in its Settings after the installation."
LangString PermarkWelcome 1046 "O instalador vai instalar o PerMark para você. Não é preciso ser administrador.$\r$\n$\r$\nPara os tipos de arquivo marcados abaixo, o Windows pede sua confirmação nas Configurações depois da instalação."
LangString PermarkWelcome 3082 "El instalador instalará PerMark para usted. No se necesitan permisos de administrador.$\r$\n$\r$\nPara los tipos de archivo que marque abajo, Windows le pedirá que lo confirme en Configuración después de la instalación."
LangString PermarkWelcome 1031 "Das Setup installiert PerMark für Sie. Administratorrechte sind nicht nötig.$\r$\n$\r$\nFür die unten angehakten Dateitypen bittet Windows Sie nach der Installation um Bestätigung in den Einstellungen."
LangString PermarkWelcome 2052 "安装程序将为您安装 PerMark，无需管理员权限。$\r$\n$\r$\n对于下面勾选的文件类型，安装后 Windows 会请您在“设置”中确认。"
LangString PermarkWelcome 1040 "Il programma di installazione installerà PerMark per te. Non servono i permessi di amministratore.$\r$\n$\r$\nPer i tipi di file selezionati qui sotto, dopo l'installazione Windows ti chiederà di confermare nelle Impostazioni."

LangString PermarkTypesTitle 1033 "Open these files with PerMark:"
LangString PermarkTypesTitle 1046 "Abrir estes arquivos com o PerMark:"
LangString PermarkTypesTitle 3082 "Abrir estos archivos con PerMark:"
LangString PermarkTypesTitle 1031 "Diese Dateien mit PerMark öffnen:"
LangString PermarkTypesTitle 2052 "用 PerMark 打开这些文件："
LangString PermarkTypesTitle 1040 "Apri questi file con PerMark:"

LangString PermarkText 1033 "Text"
LangString PermarkText 1046 "Texto"
LangString PermarkText 3082 "Texto"
LangString PermarkText 1031 "Text"
LangString PermarkText 2052 "文本"
LangString PermarkText 1040 "Testo"

LangString PermarkProperties 1033 "Properties"
LangString PermarkProperties 1046 "Properties"
LangString PermarkProperties 3082 "Properties"
LangString PermarkProperties 1031 "Properties"
LangString PermarkProperties 2052 "Properties"
LangString PermarkProperties 1040 "Properties"

LangString OpenWithMenuLabel 1033 "Open with PerMark"
LangString OpenWithMenuLabel 1046 "Abrir com o PerMark"
LangString OpenWithMenuLabel 3082 "Abrir con PerMark"
LangString OpenWithMenuLabel 1031 "Mit PerMark öffnen"
LangString OpenWithMenuLabel 2052 "用 PerMark 打开"
LangString OpenWithMenuLabel 1040 "Apri con PerMark"

LangString NewMarkdownLabel 1033 "New Markdown file with PerMark"
LangString NewMarkdownLabel 1046 "Novo arquivo Markdown com o PerMark"
LangString NewMarkdownLabel 3082 "Nuevo archivo Markdown con PerMark"
LangString NewMarkdownLabel 1031 "Neue Markdown-Datei mit PerMark"
LangString NewMarkdownLabel 2052 "用 PerMark 新建 Markdown 文件"
LangString NewMarkdownLabel 1040 "Nuovo file Markdown con PerMark"

LangString OpenWithMenuOption 1033 "Add “Open with PerMark” to the right-click menu"
LangString OpenWithMenuOption 1046 "Adicionar “Abrir com o PerMark” ao menu de clique direito"
LangString OpenWithMenuOption 3082 "Agregar “Abrir con PerMark” al menú de clic derecho"
LangString OpenWithMenuOption 1031 "„Mit PerMark öffnen“ zum Kontextmenü hinzufügen"
LangString OpenWithMenuOption 2052 "在右键菜单中添加“用 PerMark 打开”"
LangString OpenWithMenuOption 1040 "Aggiungi “Apri con PerMark” al menu contestuale"

LangString NewMarkdownOption 1033 "Add “New Markdown file” to the right-click menu of folders"
LangString NewMarkdownOption 1046 "Adicionar “Novo arquivo Markdown” ao menu de clique direito das pastas"
LangString NewMarkdownOption 3082 "Agregar “Nuevo archivo Markdown” al menú de clic derecho de las carpetas"
LangString NewMarkdownOption 1031 "„Neue Markdown-Datei“ zum Kontextmenü von Ordnern hinzufügen"
LangString NewMarkdownOption 2052 "在文件夹的右键菜单中添加“新建 Markdown 文件”"
LangString NewMarkdownOption 1040 "Aggiungi “Nuovo file Markdown” al menu contestuale delle cartelle"

LangString PermarkAbout 1033 "Read, write and organize Markdown and the files around it."
LangString PermarkAbout 1046 "Leia, escreva e organize Markdown e os arquivos ao redor dele."
LangString PermarkAbout 3082 "Lea, escriba y organice Markdown y los archivos que lo rodean."
LangString PermarkAbout 1031 "Markdown und die Dateien drumherum lesen, schreiben und ordnen."
LangString PermarkAbout 2052 "阅读、编写和整理 Markdown 及其周边文件。"
LangString PermarkAbout 1040 "Leggi, scrivi e organizza Markdown e i file che lo circondano."

LangString TypeMarkdown 1033 "Markdown document"
LangString TypeMarkdown 1046 "Documento Markdown"
LangString TypeMarkdown 3082 "Documento de Markdown"
LangString TypeMarkdown 1031 "Markdown-Dokument"
LangString TypeMarkdown 2052 "Markdown 文档"
LangString TypeMarkdown 1040 "Documento Markdown"
LangString TypeText 1033 "Text document"
LangString TypeText 1046 "Documento de texto"
LangString TypeText 3082 "Documento de texto"
LangString TypeText 1031 "Textdokument"
LangString TypeText 2052 "文本文档"
LangString TypeText 1040 "Documento di testo"
LangString TypeJson 1033 "JSON file"
LangString TypeJson 1046 "Arquivo JSON"
LangString TypeJson 3082 "Archivo JSON"
LangString TypeJson 1031 "JSON-Datei"
LangString TypeJson 2052 "JSON 文件"
LangString TypeJson 1040 "File JSON"
LangString TypeXml 1033 "XML file"
LangString TypeXml 1046 "Arquivo XML"
LangString TypeXml 3082 "Archivo XML"
LangString TypeXml 1031 "XML-Datei"
LangString TypeXml 2052 "XML 文件"
LangString TypeXml 1040 "File XML"
LangString TypeYaml 1033 "YAML file"
LangString TypeYaml 1046 "Arquivo YAML"
LangString TypeYaml 3082 "Archivo YAML"
LangString TypeYaml 1031 "YAML-Datei"
LangString TypeYaml 2052 "YAML 文件"
LangString TypeYaml 1040 "File YAML"
LangString TypeToml 1033 "TOML file"
LangString TypeToml 1046 "Arquivo TOML"
LangString TypeToml 3082 "Archivo TOML"
LangString TypeToml 1031 "TOML-Datei"
LangString TypeToml 2052 "TOML 文件"
LangString TypeToml 1040 "File TOML"
LangString TypeIni 1033 "Settings file"
LangString TypeIni 1046 "Arquivo de configuração"
LangString TypeIni 3082 "Archivo de configuración"
LangString TypeIni 1031 "Einstellungsdatei"
LangString TypeIni 2052 "设置文件"
LangString TypeIni 1040 "File di impostazioni"
LangString TypeProperties 1033 "Properties file"
LangString TypeProperties 1046 "Arquivo de propriedades"
LangString TypeProperties 3082 "Archivo de propiedades"
LangString TypeProperties 1031 "Properties-Datei"
LangString TypeProperties 2052 "Properties 文件"
LangString TypeProperties 1040 "File di proprietà"

; ---- State ----------------------------------------------------------------------------------------------------------

; "1" | "0" ticked on the welcome page; "" when the page did not show (silent, passive, update).
Var PermarkMenu
Var PermarkNew
; "markdown;json;" ticked on the welcome page; "" when the page did not show.
Var PermarkTypes
Var PermarkExe
Var PermarkLabel
Var PermarkNewLabel
Var PermarkMenuBox
Var PermarkNewBox
Var PermarkBoxMarkdown
Var PermarkBoxText
Var PermarkBoxJson
Var PermarkBoxXml
Var PermarkBoxYaml
Var PermarkBoxToml
Var PermarkBoxIni
Var PermarkBoxProperties

; ---- Open with: ProgIDs, the extensions' lists, the app's capabilities ----------------------------------------------

!macro PERMARK_PROGID_WRITE PROGID TYPENAME
  WriteRegStr HKCU "${PERMARK_CLASSES_KEY}\${PROGID}" "" "${TYPENAME}"
  WriteRegStr HKCU "${PERMARK_CLASSES_KEY}\${PROGID}\DefaultIcon" "" "$\"$PermarkExe$\",0"
  WriteRegStr HKCU "${PERMARK_CLASSES_KEY}\${PROGID}\shell\open" "FriendlyAppName" "PerMark"
  WriteRegStr HKCU "${PERMARK_CLASSES_KEY}\${PROGID}\shell\open\command" "" "$\"$PermarkExe$\" $\"%1$\""
!macroend

!macro PERMARK_PROGID_DELETE PROGID TYPENAME
  DeleteRegKey HKCU "${PERMARK_CLASSES_KEY}\${PROGID}"
!macroend

; Only PerMark's own value is ever deleted from an extension's list: the key holds the other programs' too.
!macro PERMARK_EXT_WRITE EXT PROGID
  WriteRegStr HKCU "${PERMARK_CLASSES_KEY}\${EXT}\OpenWithProgids" "${PROGID}" ""
  WriteRegStr HKCU "${PERMARK_APP_KEY}\Capabilities\FileAssociations" "${EXT}" "${PROGID}"
!macroend

!macro PERMARK_EXT_DELETE EXT PROGID
  DeleteRegValue HKCU "${PERMARK_CLASSES_KEY}\${EXT}\OpenWithProgids" "${PROGID}"
  !insertmacro PERMARK_PRUNE "${EXT}\OpenWithProgids"
  !insertmacro PERMARK_PRUNE "${EXT}\shell"
  !insertmacro PERMARK_PRUNE "${EXT}"
!macroend

; Every group (FileGroup in the app) with its extensions.
!macro PERMARK_TYPES OP
  !insertmacro PERMARK_PROGID_${OP} "PerMark.Markdown" "$(TypeMarkdown)"
  !insertmacro PERMARK_EXT_${OP} ".md" "PerMark.Markdown"
  !insertmacro PERMARK_EXT_${OP} ".markdown" "PerMark.Markdown"
  !insertmacro PERMARK_EXT_${OP} ".mdown" "PerMark.Markdown"
  !insertmacro PERMARK_EXT_${OP} ".mkd" "PerMark.Markdown"
  !insertmacro PERMARK_PROGID_${OP} "PerMark.Text" "$(TypeText)"
  !insertmacro PERMARK_EXT_${OP} ".txt" "PerMark.Text"
  !insertmacro PERMARK_PROGID_${OP} "PerMark.Json" "$(TypeJson)"
  !insertmacro PERMARK_EXT_${OP} ".json" "PerMark.Json"
  !insertmacro PERMARK_EXT_${OP} ".jsonl" "PerMark.Json"
  !insertmacro PERMARK_PROGID_${OP} "PerMark.Xml" "$(TypeXml)"
  !insertmacro PERMARK_EXT_${OP} ".xml" "PerMark.Xml"
  !insertmacro PERMARK_PROGID_${OP} "PerMark.Yaml" "$(TypeYaml)"
  !insertmacro PERMARK_EXT_${OP} ".yaml" "PerMark.Yaml"
  !insertmacro PERMARK_EXT_${OP} ".yml" "PerMark.Yaml"
  !insertmacro PERMARK_PROGID_${OP} "PerMark.Toml" "$(TypeToml)"
  !insertmacro PERMARK_EXT_${OP} ".toml" "PerMark.Toml"
  !insertmacro PERMARK_PROGID_${OP} "PerMark.Ini" "$(TypeIni)"
  !insertmacro PERMARK_EXT_${OP} ".ini" "PerMark.Ini"
  !insertmacro PERMARK_EXT_${OP} ".cfg" "PerMark.Ini"
  !insertmacro PERMARK_PROGID_${OP} "PerMark.Properties" "$(TypeProperties)"
  !insertmacro PERMARK_EXT_${OP} ".properties" "PerMark.Properties"
!macroend

!macro PERMARK_CAPABILITIES_WRITE
  WriteRegStr HKCU "${PERMARK_APP_KEY}\Capabilities" "ApplicationName" "PerMark"
  WriteRegStr HKCU "${PERMARK_APP_KEY}\Capabilities" "ApplicationDescription" "$(PermarkAbout)"
  WriteRegStr HKCU "${PERMARK_APP_KEY}\Capabilities" "ApplicationIcon" "$\"$PermarkExe$\",0"
  WriteRegStr HKCU "${PERMARK_REGAPPS_KEY}" "PerMark" "${PERMARK_APP_KEY}\Capabilities"
!macroend

!macro PERMARK_CAPABILITIES_DELETE
  DeleteRegValue HKCU "${PERMARK_REGAPPS_KEY}" "PerMark"
  DeleteRegKey HKCU "${PERMARK_APP_KEY}\Capabilities"
!macroend

; ---- File manager entries -------------------------------------------------------------------------------------------

!macro PERMARK_VERB_WRITE KEY LABEL COMMAND
  WriteRegStr HKCU "${PERMARK_CLASSES_KEY}\${KEY}" "" "${LABEL}"
  WriteRegStr HKCU "${PERMARK_CLASSES_KEY}\${KEY}" "Icon" "$\"$PermarkExe$\",0"
  WriteRegStr HKCU "${PERMARK_CLASSES_KEY}\${KEY}\command" "" "${COMMAND}"
!macroend

!macro PERMARK_VERB_DELETE KEY LABEL COMMAND
  DeleteRegKey HKCU "${PERMARK_CLASSES_KEY}\${KEY}"
!macroend

; %1 = the folder | file that was clicked; %V = the folder whose background was clicked.
!macro PERMARK_MENU OP
  !insertmacro PERMARK_VERB_${OP} "Directory\shell\PerMark" "$PermarkLabel" "$\"$PermarkExe$\" $\"%1$\""
  !insertmacro PERMARK_VERB_${OP} "Directory\Background\shell\PerMark" "$PermarkLabel" "$\"$PermarkExe$\" $\"%V$\""
  !insertmacro PERMARK_VERB_${OP} "SystemFileAssociations\.md\shell\PerMark" "$PermarkLabel" "$\"$PermarkExe$\" $\"%1$\""
  !insertmacro PERMARK_VERB_${OP} "SystemFileAssociations\.markdown\shell\PerMark" "$PermarkLabel" "$\"$PermarkExe$\" $\"%1$\""
  !insertmacro PERMARK_VERB_${OP} "SystemFileAssociations\.mdown\shell\PerMark" "$PermarkLabel" "$\"$PermarkExe$\" $\"%1$\""
  !insertmacro PERMARK_VERB_${OP} "SystemFileAssociations\.mkd\shell\PerMark" "$PermarkLabel" "$\"$PermarkExe$\" $\"%1$\""
!macroend

!macro PERMARK_NEW OP
  !insertmacro PERMARK_VERB_${OP} "Directory\shell\PerMarkNew" "$PermarkNewLabel" "$\"$PermarkExe$\" --new-markdown $\"%1$\""
  !insertmacro PERMARK_VERB_${OP} "Directory\Background\shell\PerMarkNew" "$PermarkNewLabel" "$\"$PermarkExe$\" --new-markdown $\"%V$\""
!macroend

; ---- Empty keys left behind -----------------------------------------------------------------------------------------
; Writing creates the keys above an entry; deleting it leaves them, empty. Each is deleted when nothing at all is in it
; (no subkey, no value, no default value). DeleteRegKey /ifempty alone only looks at subkeys and would take a key's
; values with it. Windows' own first-level keys (Directory, SystemFileAssociations) are never deleted.

!macro PERMARK_PRUNE KEY
  Push $R0
  Push $R1
  ClearErrors
  EnumRegKey $R0 HKCU "${PERMARK_CLASSES_KEY}\${KEY}" 0
  ${IfNot} ${Errors}
  ${AndIf} $R0 == ""
    ClearErrors
    ReadRegStr $R1 HKCU "${PERMARK_CLASSES_KEY}\${KEY}" ""
    ${If} ${Errors}
      EnumRegValue $R1 HKCU "${PERMARK_CLASSES_KEY}\${KEY}" 0
      ${If} $R1 == ""
        DeleteRegKey /ifempty HKCU "${PERMARK_CLASSES_KEY}\${KEY}"
      ${EndIf}
    ${EndIf}
  ${EndIf}
  ClearErrors
  Pop $R1
  Pop $R0
!macroend

; Deepest first.
!macro PERMARK_ENTRIES_TIDY
  !insertmacro PERMARK_PRUNE "Directory\shell"
  !insertmacro PERMARK_PRUNE "Directory\Background\shell"
  !insertmacro PERMARK_PRUNE "Directory\Background"
  !insertmacro PERMARK_PRUNE "SystemFileAssociations\.md\shell"
  !insertmacro PERMARK_PRUNE "SystemFileAssociations\.md"
  !insertmacro PERMARK_PRUNE "SystemFileAssociations\.markdown\shell"
  !insertmacro PERMARK_PRUNE "SystemFileAssociations\.markdown"
  !insertmacro PERMARK_PRUNE "SystemFileAssociations\.mdown\shell"
  !insertmacro PERMARK_PRUNE "SystemFileAssociations\.mdown"
  !insertmacro PERMARK_PRUNE "SystemFileAssociations\.mkd\shell"
  !insertmacro PERMARK_PRUNE "SystemFileAssociations\.mkd"
!macroend

; ---- Choices --------------------------------------------------------------------------------------------------------

; VAR ("1" on, "0" off, "" leave alone) when the page did not ask: the recorded choice; a fresh install that cannot ask
; (silent, passive) takes the default (on); an update with nothing recorded changes nothing.
!macro PERMARK_RESOLVE VAR CHOICE
  ${If} ${VAR} == ""
    ClearErrors
    ReadRegDWORD $R0 HKCU "${PERMARK_APP_KEY}" "${CHOICE}"
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

; VAR = the words of an entry: an update keeps the words already there (the app rewords them with its own language),
; anything else uses the installer's language.
!macro PERMARK_TEXT VAR KEY INSTALLER_TEXT
  StrCpy ${VAR} ""
  ${If} $UpdateMode = 1
    ReadRegStr ${VAR} HKCU "${PERMARK_CLASSES_KEY}\${KEY}" ""
  ${EndIf}
  ${If} ${VAR} == ""
    StrCpy ${VAR} "${INSTALLER_TEXT}"
  ${EndIf}
!macroend

!macro PERMARK_ENTRY_APPLY VAR CHOICE OP KEY INSTALLER_TEXT LABELVAR
  !insertmacro PERMARK_RESOLVE ${VAR} "${CHOICE}"
  ${If} ${VAR} == "1"
    !insertmacro PERMARK_TEXT ${LABELVAR} "${KEY}" "${INSTALLER_TEXT}"
    !insertmacro ${OP} WRITE
    WriteRegDWORD HKCU "${PERMARK_APP_KEY}" "${CHOICE}" 1
  ${ElseIf} ${VAR} == "0"
    !insertmacro ${OP} DELETE
    !insertmacro PERMARK_ENTRIES_TIDY
    WriteRegDWORD HKCU "${PERMARK_APP_KEY}" "${CHOICE}" 0
  ${EndIf}
!macroend

; ---- The welcome page: short text, the types, the two entries -------------------------------------------------------

; the page's own text ends with the title of the check grid (a label of ours over that text would not show)
!define MUI_WELCOMEPAGE_TEXT "$(PermarkWelcome)$\r$\n$\r$\n$(PermarkTypesTitle)"

!macro PERMARK_TYPE_BOX VAR X Y LABEL NAME
  ${NSD_CreateCheckbox} ${X} ${Y} 48u 11u "${LABEL}"
  Pop ${VAR}
  SetCtlColors ${VAR} "000000" "FFFFFF"
  ; the welcome text is a tall label: raise the box above it
  System::Call 'user32::SetWindowPos(p ${VAR}, p 0, i 0, i 0, i 0, i 0, i 3)'
  ; $R9 = ";markdown;json;" (recorded | default); StrLoc is declared by the template
  ${StrLoc} $R8 "$R9" "${NAME};" ">"
  ${If} $R8 != ""
    ${NSD_Check} ${VAR}
  ${EndIf}
!macroend

!macro PERMARK_TYPE_READ VAR NAME
  ${NSD_GetState} ${VAR} $0
  ${If} $0 = ${BST_CHECKED}
    StrCpy $PermarkTypes "$PermarkTypes${NAME};"
  ${EndIf}
!macroend

!macro PERMARK_ENTRY_BOX VAR Y H LABEL CHOICE
  ClearErrors
  ReadRegDWORD $0 HKCU "${PERMARK_APP_KEY}" "${CHOICE}"
  ${If} ${Errors}
    StrCpy $0 1
  ${EndIf}
  ${NSD_CreateCheckbox} 120u ${Y} 195u ${H} "${LABEL}"
  Pop ${VAR}
  SetCtlColors ${VAR} "000000" "FFFFFF"
  System::Call 'user32::SetWindowPos(p ${VAR}, p 0, i 0, i 0, i 0, i 0, i 3)'
  ${If} $0 <> 0
    ${NSD_Check} ${VAR}
  ${EndIf}
!macroend

Function PermarkWelcomeShow
  Push $0
  Push $R8
  Push $R9
  ; the types start as recorded by an earlier install, else Markdown only
  ClearErrors
  ReadRegStr $R9 HKCU "${PERMARK_APP_KEY}" "${PERMARK_TYPES_CHOICE}"
  ${If} ${Errors}
    StrCpy $R9 "markdown;"
  ${EndIf}
  StrCpy $R9 ";$R9"
  ClearErrors

  !insertmacro PERMARK_TYPE_BOX $PermarkBoxMarkdown 120u 110u "Markdown" ";markdown"
  !insertmacro PERMARK_TYPE_BOX $PermarkBoxText 169u 110u "$(PermarkText)" ";text"
  !insertmacro PERMARK_TYPE_BOX $PermarkBoxJson 218u 110u "JSON" ";json"
  !insertmacro PERMARK_TYPE_BOX $PermarkBoxXml 267u 110u "XML" ";xml"
  !insertmacro PERMARK_TYPE_BOX $PermarkBoxYaml 120u 121u "YAML" ";yaml"
  !insertmacro PERMARK_TYPE_BOX $PermarkBoxToml 169u 121u "TOML" ";toml"
  !insertmacro PERMARK_TYPE_BOX $PermarkBoxIni 218u 121u "INI" ";ini"
  !insertmacro PERMARK_TYPE_BOX $PermarkBoxProperties 267u 121u "$(PermarkProperties)" ";properties"

  !insertmacro PERMARK_ENTRY_BOX $PermarkMenuBox 141u 16u "$(OpenWithMenuOption)" "${PERMARK_MENU_CHOICE}"
  !insertmacro PERMARK_ENTRY_BOX $PermarkNewBox 162u 20u "$(NewMarkdownOption)" "${PERMARK_NEW_CHOICE}"
  Pop $R9
  Pop $R8
  Pop $0
FunctionEnd

Function PermarkWelcomeLeave
  Push $0
  ${NSD_GetState} $PermarkMenuBox $0
  ${If} $0 = ${BST_CHECKED}
    StrCpy $PermarkMenu "1"
  ${Else}
    StrCpy $PermarkMenu "0"
  ${EndIf}
  ${NSD_GetState} $PermarkNewBox $0
  ${If} $0 = ${BST_CHECKED}
    StrCpy $PermarkNew "1"
  ${Else}
    StrCpy $PermarkNew "0"
  ${EndIf}
  ; "none;" marks "the page asked and nothing was ticked" (not the same as "not asked")
  StrCpy $PermarkTypes ""
  !insertmacro PERMARK_TYPE_READ $PermarkBoxMarkdown "markdown"
  !insertmacro PERMARK_TYPE_READ $PermarkBoxText "text"
  !insertmacro PERMARK_TYPE_READ $PermarkBoxJson "json"
  !insertmacro PERMARK_TYPE_READ $PermarkBoxXml "xml"
  !insertmacro PERMARK_TYPE_READ $PermarkBoxYaml "yaml"
  !insertmacro PERMARK_TYPE_READ $PermarkBoxToml "toml"
  !insertmacro PERMARK_TYPE_READ $PermarkBoxIni "ini"
  !insertmacro PERMARK_TYPE_READ $PermarkBoxProperties "properties"
  ${If} $PermarkTypes == ""
    StrCpy $PermarkTypes "none;"
  ${EndIf}
  Pop $0
FunctionEnd

; The welcome page is the first page the template inserts after this file: these apply to it.
!define MUI_PAGE_CUSTOMFUNCTION_SHOW PermarkWelcomeShow
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE PermarkWelcomeLeave

; ---- Hooks ----------------------------------------------------------------------------------------------------------

!macro NSIS_HOOK_POSTINSTALL
  ; $LANGUAGE = the Windows language id the installer ran in
  StrCpy $R8 ""
  ${If} $LANGUAGE = 1046
    StrCpy $R8 "pt-BR"
  ${ElseIf} $LANGUAGE = 3082
  ${OrIf} $LANGUAGE = 1034
    StrCpy $R8 "es"
  ${ElseIf} $LANGUAGE = 1031
    StrCpy $R8 "de"
  ${ElseIf} $LANGUAGE = 2052
    StrCpy $R8 "zh-CN"
  ${ElseIf} $LANGUAGE = 1040
    StrCpy $R8 "it"
  ${ElseIf} $LANGUAGE = 1033
    StrCpy $R8 "en"
  ${EndIf}
  ${If} $R8 != ""
    ; an update keeps what an earlier install recorded; a fresh install | a reinstall by hand records its language
    ReadRegStr $R9 HKCU "${PERMARK_APP_KEY}" "Language"
    ${If} $UpdateMode <> 1
    ${OrIf} $R9 == ""
      WriteRegStr HKCU "${PERMARK_APP_KEY}" "Language" "$R8"
    ${EndIf}
  ${EndIf}

  StrCpy $PermarkExe "$INSTDIR\${MAINBINARYNAME}.exe"

  ; "Open with" for every type, pointing at the files just installed (an update too)
  !insertmacro PERMARK_TYPES WRITE
  !insertmacro PERMARK_CAPABILITIES_WRITE

  !insertmacro PERMARK_ENTRY_APPLY $PermarkMenu "${PERMARK_MENU_CHOICE}" PERMARK_MENU "Directory\shell\PerMark" "$(OpenWithMenuLabel)" $PermarkLabel
  !insertmacro PERMARK_ENTRY_APPLY $PermarkNew "${PERMARK_NEW_CHOICE}" PERMARK_NEW "Directory\shell\PerMarkNew" "$(NewMarkdownLabel)" $PermarkNewLabel

  !insertmacro UPDATEFILEASSOC

  ; The default app: only when the page asked (never on an update, a silent | passive install). A changed or first choice
  ; with a type ticked hands the person to Windows' Default Apps page for PerMark.
  ${If} $UpdateMode <> 1
  ${AndIf} $PermarkTypes != ""
    ClearErrors
    ReadRegStr $R9 HKCU "${PERMARK_APP_KEY}" "${PERMARK_TYPES_CHOICE}"
    ${If} ${Errors}
    ${OrIf} $R9 != $PermarkTypes
      WriteRegStr HKCU "${PERMARK_APP_KEY}" "${PERMARK_TYPES_CHOICE}" "$PermarkTypes"
      ${If} $PermarkTypes != "none;"
        ExecShell "open" "${PERMARK_DEFAULT_APPS}"
      ${EndIf}
    ${EndIf}
    ClearErrors
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; An update's uninstall step keeps everything: the install that follows rewrites it.
  ${If} $UpdateMode <> 1
    !insertmacro PERMARK_MENU DELETE
    !insertmacro PERMARK_NEW DELETE
    !insertmacro PERMARK_ENTRIES_TIDY
    !insertmacro PERMARK_TYPES DELETE
    !insertmacro PERMARK_CAPABILITIES_DELETE
    !insertmacro UPDATEFILEASSOC
  ${EndIf}

  ; "Delete the application data": the recorded language and choices, and PerMark's folders in the profile
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    DeleteRegKey HKCU "${PERMARK_APP_KEY}"
    SetShellVarContext current
    RmDir /r "$APPDATA\PerMark"
    RmDir /r "$LOCALAPPDATA\PerMark"
  ${EndIf}
!macroend
