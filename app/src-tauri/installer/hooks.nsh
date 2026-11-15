; Hooks of the Windows (NSIS) installer, wired in by `bundle.windows.nsis.installerHooks` in tauri.conf.json.
; Everything here writes under HKEY_CURRENT_USER: no administrator rights.
;
; 1. LANGUAGE. The installer's language is recorded as the app's language code (en, pt-BR, es, de, zh-CN, it, ja,
;    ko, fr, pt-PT, ca, gl, ro) in HKCU\Software\PerMark\Language. The app reads it once, at its first start, when no
;    language was picked in it.
;
; 2. "OPEN WITH". Every file type PerMark reads is registered so it is offered under "Open with" (a ProgID per group of
;    types, the extension's OpenWithProgids list, and the app's capabilities for the Default Apps page). The default app
;    of a type is NEVER set here: Windows protects it. The welcome page asks "Open these files with PerMark" (Markdown
;    checked, the other types unchecked; the title of the check grid says Windows confirms); with a type checked, the
;    installer opens Windows' Default Apps page for PerMark at the end, where the person confirms. An update asks nothing
;    and opens nothing.
;
; 3. FILE MANAGER ENTRIES. "Open with PerMark" (folders, the background of a folder, Markdown files) and "New Markdown
;    file" (folders, the background of a folder), each a check box on the welcome page, also switchable in the app's
;    settings. The choice is remembered as HKCU\Software\PerMark\OpenWithMenu and \NewMarkdownMenu (1 on, 0 off): an
;    update keeps it (rewrites what is on so it points at the new files, never adds what is off).
;
; 4. COMMAND LINE. "Add the permark command to PATH", a check box on the welcome page, off unless an earlier install
;    recorded it on: the install folder goes once at the end of the user's PATH (HKCU\Environment\Path), so `permark mcp`
;    and `permark context` run from any terminal started afterwards. The choice is remembered as
;    HKCU\Software\PerMark\AddToPath (1 on, 0 off) and the entry written as \PathEntry: only that entry is ever removed
;    (box unticked, uninstall), never one the person wrote. An update leaves PATH as it is. A PATH this installer cannot
;    rewrite whole (see "Command line" below) is never changed: the installer says so and names the folder to add.
;
; Uninstalling removes everything above (and the keys it leaves empty); with "Delete the application data" it also removes
; HKCU\Software\PerMark and PerMark's folders in the profile. An update's uninstall step removes nothing.
; The layout is the one the app writes (crates/plugins/shell-integration, crates/core/src/desktop); a Rust test keeps them
; equal. A test installer may define the four roots below before including this file (path-check.nsi does).

!ifndef PERMARK_APP_KEY
  !define PERMARK_APP_KEY "Software\PerMark"
!endif
!ifndef PERMARK_CLASSES_KEY
  !define PERMARK_CLASSES_KEY "Software\Classes"
!endif
!ifndef PERMARK_REGAPPS_KEY
  !define PERMARK_REGAPPS_KEY "Software\RegisteredApplications"
!endif
!ifndef PERMARK_ENV_KEY
  !define PERMARK_ENV_KEY "Environment"
!endif

!define PERMARK_MENU_CHOICE "OpenWithMenu"
!define PERMARK_NEW_CHOICE "NewMarkdownMenu"
!define PERMARK_TYPES_CHOICE "DefaultTypes"
!define PERMARK_PATH_CHOICE "AddToPath"
!define PERMARK_PATH_ENTRY "PathEntry"
!define PERMARK_DEFAULT_APPS "ms-settings:defaultapps?registeredAppUser=PerMark"

; ---- Words ----------------------------------------------------------------------------------------------------------
; Windows language ids: 1033 en, 1046 pt-BR, 3082 es, 1031 de, 2052 zh-CN, 1040 it, 1041 ja, 1042 ko, 1036 fr,
; 2070 pt-PT, 1027 ca, 1110 gl, 1048 ro.

LangString PermarkWelcome 1033 "PerMark is a unified document workspace: one place to read, write and organize Markdown and the files around it.$\r$\n$\r$\nSetup will install PerMark for you. No administrator rights are needed."
LangString PermarkWelcome 1046 "O PerMark é um espaço de trabalho unificado para documentos: um só lugar para ler, escrever e organizar Markdown e os arquivos ao redor.$\r$\n$\r$\nO instalador vai instalar o PerMark para você. Não é preciso ser administrador."
LangString PermarkWelcome 3082 "PerMark es un espacio de trabajo unificado para documentos: un solo lugar para leer, escribir y organizar Markdown y los archivos que lo rodean.$\r$\n$\r$\nEl instalador instalará PerMark para usted. No se necesitan permisos de administrador."
LangString PermarkWelcome 1031 "PerMark ist ein einheitlicher Arbeitsplatz für Dokumente: ein Ort zum Lesen, Schreiben und Ordnen von Markdown und den Dateien drumherum.$\r$\n$\r$\nDas Setup installiert PerMark für Sie. Administratorrechte sind nicht nötig."
LangString PermarkWelcome 2052 "PerMark 是统一文档工作区：在一个地方阅读、编写和整理 Markdown 及其周边文件。$\r$\n$\r$\n安装程序将为您安装 PerMark，无需管理员权限。"
LangString PermarkWelcome 1040 "PerMark è uno spazio di lavoro unificato per i documenti: un unico posto per leggere, scrivere e organizzare Markdown e i file che lo circondano.$\r$\n$\r$\nIl programma di installazione installerà PerMark per te. Non servono i permessi di amministratore."
LangString PermarkWelcome 1041 "PerMark は統合ドキュメントワークスペースです。Markdown とその周辺のファイルを、ひとつの場所で読み、書き、整理できます。$\r$\n$\r$\nこのセットアップで PerMark をインストールします。管理者権限は必要ありません。"
LangString PermarkWelcome 1042 "PerMark는 통합 문서 작업 공간입니다. Markdown과 그 주변 파일을 한곳에서 읽고, 쓰고, 정리할 수 있습니다.$\r$\n$\r$\n설치 프로그램이 PerMark를 설치합니다. 관리자 권한은 필요하지 않습니다."
LangString PermarkWelcome 1036 "PerMark est un espace de travail unifié pour vos documents : un seul endroit pour lire, écrire et organiser le Markdown et les fichiers qui l'entourent.$\r$\n$\r$\nLe programme d'installation va installer PerMark pour vous. Aucun droit d'administrateur n'est nécessaire."
LangString PermarkWelcome 2070 "O PerMark é um espaço de trabalho unificado para documentos: um só local para ler, escrever e organizar Markdown e os ficheiros à sua volta.$\r$\n$\r$\nO programa de configuração vai instalar o PerMark para si. Não são necessários direitos de administrador."
LangString PermarkWelcome 1027 "El PerMark és un espai de treball unificat per a documents: un sol lloc per a llegir, escriure i organitzar el Markdown i els fitxers que l'acompanyen.$\r$\n$\r$\nAquest assistent instal·larà el PerMark al vostre compte. No calen permisos d'administrador."
LangString PermarkWelcome 1110 "PerMark é un espazo de traballo unificado para documentos: un único lugar para ler, escribir e organizar Markdown e os ficheiros que o acompañan.$\r$\n$\r$\nO asistente instalará PerMark para vostede. Non se necesitan permisos de administrador."
LangString PermarkWelcome 1048 "PerMark este un spațiu de lucru unificat pentru documente: un singur loc în care să citești, să scrii și să organizezi Markdown și fișierele din jurul lui.$\r$\n$\r$\nProgramul de instalare va instala PerMark pentru tine. Nu sunt necesare drepturi de administrator."

LangString PermarkTypesTitle 1033 "Open these files with PerMark (Windows asks you to confirm in its Settings after the installation):"
LangString PermarkTypesTitle 1046 "Abrir estes arquivos com o PerMark (o Windows pede sua confirmação nas Configurações depois da instalação):"
LangString PermarkTypesTitle 3082 "Abrir estos archivos con PerMark (Windows le pedirá que lo confirme en Configuración después de la instalación):"
LangString PermarkTypesTitle 1031 "Diese Dateien mit PerMark öffnen (Windows bittet Sie nach der Installation in den Einstellungen um Bestätigung):"
LangString PermarkTypesTitle 2052 "用 PerMark 打开这些文件（安装后 Windows 会请您在“设置”中确认）："
LangString PermarkTypesTitle 1040 "Apri questi file con PerMark (dopo l'installazione Windows ti chiederà di confermare nelle Impostazioni):"
LangString PermarkTypesTitle 1041 "これらのファイルを PerMark で開く (インストール後、Windows の設定で確認を求められます):"
LangString PermarkTypesTitle 1042 "다음 파일을 PerMark로 열기 (설치 후 Windows 설정에서 확인을 요청합니다):"
LangString PermarkTypesTitle 1036 "Ouvrir ces fichiers avec PerMark (Windows vous demandera de confirmer dans ses Paramètres après l'installation) :"
LangString PermarkTypesTitle 2070 "Abrir estes ficheiros com o PerMark (o Windows pede-lhe que confirme nas Definições após a instalação):"
LangString PermarkTypesTitle 1027 "Obre aquests fitxers amb el PerMark (el Windows us demanarà que ho confirmeu a la Configuració després de la instal·lació):"
LangString PermarkTypesTitle 1110 "Abrir estes ficheiros con PerMark (Windows pediralle que o confirme na Configuración despois da instalación):"
LangString PermarkTypesTitle 1048 "Deschide aceste fișiere cu PerMark (Windows îți va cere să confirmi în Setări după instalare):"

LangString PermarkText 1033 "Text"
LangString PermarkText 1046 "Texto"
LangString PermarkText 3082 "Texto"
LangString PermarkText 1031 "Text"
LangString PermarkText 2052 "文本"
LangString PermarkText 1040 "Testo"
LangString PermarkText 1041 "テキスト"
LangString PermarkText 1042 "텍스트"
LangString PermarkText 1036 "Texte"
LangString PermarkText 2070 "Texto"
LangString PermarkText 1027 "Text"
LangString PermarkText 1110 "Texto"
LangString PermarkText 1048 "Text"

LangString PermarkProperties 1033 "Properties"
LangString PermarkProperties 1046 "Properties"
LangString PermarkProperties 3082 "Properties"
LangString PermarkProperties 1031 "Properties"
LangString PermarkProperties 2052 "Properties"
LangString PermarkProperties 1040 "Properties"
LangString PermarkProperties 1041 "Properties"
LangString PermarkProperties 1042 "Properties"
LangString PermarkProperties 1036 "Properties"
LangString PermarkProperties 2070 "Properties"
LangString PermarkProperties 1027 "Properties"
LangString PermarkProperties 1110 "Properties"
LangString PermarkProperties 1048 "Properties"

LangString OpenWithMenuLabel 1033 "Open with PerMark"
LangString OpenWithMenuLabel 1046 "Abrir com o PerMark"
LangString OpenWithMenuLabel 3082 "Abrir con PerMark"
LangString OpenWithMenuLabel 1031 "Mit PerMark öffnen"
LangString OpenWithMenuLabel 2052 "用 PerMark 打开"
LangString OpenWithMenuLabel 1040 "Apri con PerMark"
LangString OpenWithMenuLabel 1041 "PerMark で開く"
LangString OpenWithMenuLabel 1042 "PerMark로 열기"
LangString OpenWithMenuLabel 1036 "Ouvrir avec PerMark"
LangString OpenWithMenuLabel 2070 "Abrir com o PerMark"
LangString OpenWithMenuLabel 1027 "Obre amb PerMark"
LangString OpenWithMenuLabel 1110 "Abrir con PerMark"
LangString OpenWithMenuLabel 1048 "Deschide cu PerMark"

LangString NewMarkdownLabel 1033 "New Markdown file with PerMark"
LangString NewMarkdownLabel 1046 "Novo arquivo Markdown com o PerMark"
LangString NewMarkdownLabel 3082 "Nuevo archivo Markdown con PerMark"
LangString NewMarkdownLabel 1031 "Neue Markdown-Datei mit PerMark"
LangString NewMarkdownLabel 2052 "用 PerMark 新建 Markdown 文件"
LangString NewMarkdownLabel 1040 "Nuovo file Markdown con PerMark"
LangString NewMarkdownLabel 1041 "PerMark で新しい Markdown ファイルを作成"
LangString NewMarkdownLabel 1042 "PerMark로 새 Markdown 파일 만들기"
LangString NewMarkdownLabel 1036 "Nouveau fichier Markdown avec PerMark"
LangString NewMarkdownLabel 2070 "Novo ficheiro Markdown com o PerMark"
LangString NewMarkdownLabel 1027 "Fitxer Markdown nou amb PerMark"
LangString NewMarkdownLabel 1110 "Novo ficheiro Markdown con PerMark"
LangString NewMarkdownLabel 1048 "Fișier Markdown nou cu PerMark"

LangString OpenWithMenuOption 1033 "Add “Open with PerMark” to the right-click menu"
LangString OpenWithMenuOption 1046 "Adicionar “Abrir com o PerMark” ao menu de clique direito"
LangString OpenWithMenuOption 3082 "Agregar “Abrir con PerMark” al menú de clic derecho"
LangString OpenWithMenuOption 1031 "„Mit PerMark öffnen“ zum Kontextmenü hinzufügen"
LangString OpenWithMenuOption 2052 "在右键菜单中添加“用 PerMark 打开”"
LangString OpenWithMenuOption 1040 "Aggiungi “Apri con PerMark” al menu contestuale"
LangString OpenWithMenuOption 1041 "右クリックメニューに「PerMark で開く」を追加"
LangString OpenWithMenuOption 1042 "오른쪽 클릭 메뉴에 “PerMark로 열기” 추가"
LangString OpenWithMenuOption 1036 "Ajouter « Ouvrir avec PerMark » au menu contextuel"
LangString OpenWithMenuOption 2070 "Adicionar “Abrir com o PerMark” ao menu de contexto"
LangString OpenWithMenuOption 1027 "Afegeix «Obre amb PerMark» al menú contextual"
LangString OpenWithMenuOption 1110 "Engadir «Abrir con PerMark» ao menú contextual"
LangString OpenWithMenuOption 1048 "Adaugă „Deschide cu PerMark” în meniul de clic dreapta"

LangString NewMarkdownOption 1033 "Add “New Markdown file” to the right-click menu of folders"
LangString NewMarkdownOption 1046 "Adicionar “Novo arquivo Markdown” ao menu de clique direito das pastas"
LangString NewMarkdownOption 3082 "Agregar “Nuevo archivo Markdown” al menú de clic derecho de las carpetas"
LangString NewMarkdownOption 1031 "„Neue Markdown-Datei“ zum Kontextmenü von Ordnern hinzufügen"
LangString NewMarkdownOption 2052 "在文件夹的右键菜单中添加“新建 Markdown 文件”"
LangString NewMarkdownOption 1040 "Aggiungi “Nuovo file Markdown” al menu contestuale delle cartelle"
LangString NewMarkdownOption 1041 "フォルダーの右クリックメニューに「新しい Markdown ファイル」を追加"
LangString NewMarkdownOption 1042 "폴더의 오른쪽 클릭 메뉴에 “새 Markdown 파일” 추가"
LangString NewMarkdownOption 1036 "Ajouter « Nouveau fichier Markdown » au menu contextuel des dossiers"
LangString NewMarkdownOption 2070 "Adicionar “Novo ficheiro Markdown” ao menu de contexto das pastas"
LangString NewMarkdownOption 1027 "Afegeix «Fitxer Markdown nou» al menú contextual de les carpetes"
LangString NewMarkdownOption 1110 "Engadir «Novo ficheiro Markdown» ao menú contextual dos cartafoles"
LangString NewMarkdownOption 1048 "Adaugă „Fișier Markdown nou” în meniul de clic dreapta al dosarelor"

LangString PathOption 1033 "Add the permark command to PATH"
LangString PathOption 1046 "Adicionar o comando permark ao PATH"
LangString PathOption 3082 "Agregar el comando permark al PATH"
LangString PathOption 1031 "Den Befehl permark zum PATH hinzufügen"
LangString PathOption 2052 "将 permark 命令添加到 PATH"
LangString PathOption 1040 "Aggiungi il comando permark al PATH"
LangString PathOption 1041 "permark コマンドを PATH に追加"
LangString PathOption 1042 "permark 명령을 PATH에 추가"
LangString PathOption 1036 "Ajouter la commande permark au PATH"
LangString PathOption 2070 "Adicionar o comando permark ao PATH"
LangString PathOption 1027 "Afegeix l'ordre permark al PATH"
LangString PathOption 1110 "Engadir a orde permark ao PATH"
LangString PathOption 1048 "Adaugă comanda permark în PATH"

LangString PathNotAdded 1033 "The permark command was not added to PATH: the installer cannot change your PATH safely (it is probably too long). To use the command, add this folder to PATH yourself:$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotAdded 1046 "O comando permark não foi adicionado ao PATH: o instalador não consegue alterar seu PATH com segurança (provavelmente ele é longo demais). Para usar o comando, adicione você mesmo esta pasta ao PATH:$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotAdded 3082 "El comando permark no se agregó al PATH: el instalador no puede modificar su PATH de forma segura (probablemente es demasiado largo). Para usar el comando, agregue usted mismo esta carpeta al PATH:$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotAdded 1031 "Der Befehl permark wurde nicht zum PATH hinzugefügt: Das Setup kann Ihren PATH nicht sicher ändern (er ist wahrscheinlich zu lang). Um den Befehl zu verwenden, fügen Sie diesen Ordner selbst zum PATH hinzu:$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotAdded 2052 "permark 命令未添加到 PATH：安装程序无法安全地修改您的 PATH（可能太长）。如需使用该命令，请自行将此文件夹添加到 PATH：$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotAdded 1040 "Il comando permark non è stato aggiunto al PATH: il programma di installazione non può modificare il PATH in modo sicuro (probabilmente è troppo lungo). Per usare il comando, aggiungi tu stesso questa cartella al PATH:$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotAdded 1041 "permark コマンドは PATH に追加されませんでした。セットアップは PATH を安全に変更できません (長すぎる可能性があります)。コマンドを使うには、このフォルダーを自分で PATH に追加してください:$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotAdded 1042 "permark 명령이 PATH에 추가되지 않았습니다. 설치 프로그램이 PATH를 안전하게 변경할 수 없습니다 (너무 길 수 있습니다). 명령을 사용하려면 이 폴더를 직접 PATH에 추가하세요:$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotAdded 1036 "La commande permark n'a pas été ajoutée au PATH : le programme d'installation ne peut pas modifier votre PATH sans risque (il est probablement trop long). Pour utiliser la commande, ajoutez vous-même ce dossier au PATH :$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotAdded 2070 "O comando permark não foi adicionado ao PATH: o programa de configuração não consegue alterar o seu PATH em segurança (provavelmente é demasiado longo). Para usar o comando, adicione esta pasta ao PATH manualmente:$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotAdded 1027 "L'ordre permark no s'ha afegit al PATH: l'instal·lador no pot modificar el PATH de manera segura (probablement és massa llarg). Per a fer servir l'ordre, afegiu vós mateix aquesta carpeta al PATH:$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotAdded 1110 "A orde permark non se engadiu ao PATH: o instalador non pode modificar o seu PATH de forma segura (probablemente é demasiado longo). Para usar a orde, engada vostede mesmo este cartafol ao PATH:$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotAdded 1048 "Comanda permark nu a fost adăugată în PATH: programul de instalare nu poate modifica PATH-ul în siguranță (probabil este prea lung). Pentru a folosi comanda, adaugă singur acest dosar în PATH:$\r$\n$\r$\n$PermarkPathDir"

LangString PathNotRemoved 1033 "The permark command was not removed from PATH: the uninstaller cannot change your PATH safely (it is probably too long). You can remove this folder from PATH yourself:$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotRemoved 1046 "O comando permark não foi removido do PATH: o desinstalador não consegue alterar seu PATH com segurança (provavelmente ele é longo demais). Você pode remover esta pasta do PATH manualmente:$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotRemoved 3082 "El comando permark no se quitó del PATH: el desinstalador no puede modificar su PATH de forma segura (probablemente es demasiado largo). Puede quitar usted mismo esta carpeta del PATH:$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotRemoved 1031 "Der Befehl permark wurde nicht aus dem PATH entfernt: Die Deinstallation kann Ihren PATH nicht sicher ändern (er ist wahrscheinlich zu lang). Sie können diesen Ordner selbst aus dem PATH entfernen:$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotRemoved 2052 "permark 命令未从 PATH 中移除：卸载程序无法安全地修改您的 PATH（可能太长）。您可以自行从 PATH 中移除此文件夹：$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotRemoved 1040 "Il comando permark non è stato rimosso dal PATH: il programma di disinstallazione non può modificare il PATH in modo sicuro (probabilmente è troppo lungo). Puoi rimuovere tu stesso questa cartella dal PATH:$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotRemoved 1041 "permark コマンドは PATH から削除されませんでした。アンインストーラーは PATH を安全に変更できません (長すぎる可能性があります)。このフォルダーは自分で PATH から削除できます:$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotRemoved 1042 "permark 명령이 PATH에서 제거되지 않았습니다. 제거 프로그램이 PATH를 안전하게 변경할 수 없습니다 (너무 길 수 있습니다). 이 폴더는 직접 PATH에서 제거할 수 있습니다:$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotRemoved 1036 "La commande permark n'a pas été retirée du PATH : le programme de désinstallation ne peut pas modifier votre PATH sans risque (il est probablement trop long). Vous pouvez retirer vous-même ce dossier du PATH :$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotRemoved 2070 "O comando permark não foi removido do PATH: o programa de desinstalação não consegue alterar o seu PATH em segurança (provavelmente é demasiado longo). Pode remover esta pasta do PATH manualmente:$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotRemoved 1027 "L'ordre permark no s'ha tret del PATH: el desinstal·lador no pot modificar el PATH de manera segura (probablement és massa llarg). Podeu treure vós mateix aquesta carpeta del PATH:$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotRemoved 1110 "A orde permark non se quitou do PATH: o desinstalador non pode modificar o seu PATH de forma segura (probablemente é demasiado longo). Pode quitar vostede mesmo este cartafol do PATH:$\r$\n$\r$\n$PermarkPathDir"
LangString PathNotRemoved 1048 "Comanda permark nu a fost eliminată din PATH: programul de dezinstalare nu poate modifica PATH-ul în siguranță (probabil este prea lung). Poți elimina singur acest dosar din PATH:$\r$\n$\r$\n$PermarkPathDir"

LangString PermarkAbout 1033 "Read, write and organize Markdown and the files around it."
LangString PermarkAbout 1046 "Leia, escreva e organize Markdown e os arquivos ao redor dele."
LangString PermarkAbout 3082 "Lea, escriba y organice Markdown y los archivos que lo rodean."
LangString PermarkAbout 1031 "Markdown und die Dateien drumherum lesen, schreiben und ordnen."
LangString PermarkAbout 2052 "阅读、编写和整理 Markdown 及其周边文件。"
LangString PermarkAbout 1040 "Leggi, scrivi e organizza Markdown e i file che lo circondano."
LangString PermarkAbout 1041 "Markdown とその周辺のファイルを読み、書き、整理します。"
LangString PermarkAbout 1042 "Markdown과 그 주변 파일을 읽고, 쓰고, 정리합니다."
LangString PermarkAbout 1036 "Lire, écrire et organiser le Markdown et les fichiers qui l'entourent."
LangString PermarkAbout 2070 "Ler, escrever e organizar Markdown e os ficheiros à sua volta."
LangString PermarkAbout 1027 "Llegiu, escriviu i organitzeu el Markdown i els fitxers que l'acompanyen."
LangString PermarkAbout 1110 "Lea, escriba e organice Markdown e os ficheiros que o acompañan."
LangString PermarkAbout 1048 "Citește, scrie și organizează Markdown și fișierele din jurul lui."

LangString TypeMarkdown 1033 "Markdown document"
LangString TypeMarkdown 1046 "Documento Markdown"
LangString TypeMarkdown 3082 "Documento de Markdown"
LangString TypeMarkdown 1031 "Markdown-Dokument"
LangString TypeMarkdown 2052 "Markdown 文档"
LangString TypeMarkdown 1040 "Documento Markdown"
LangString TypeMarkdown 1041 "Markdown ドキュメント"
LangString TypeMarkdown 1042 "Markdown 문서"
LangString TypeMarkdown 1036 "Document Markdown"
LangString TypeMarkdown 2070 "Documento Markdown"
LangString TypeMarkdown 1027 "Document Markdown"
LangString TypeMarkdown 1110 "Documento Markdown"
LangString TypeMarkdown 1048 "Document Markdown"
LangString TypeText 1033 "Text document"
LangString TypeText 1046 "Documento de texto"
LangString TypeText 3082 "Documento de texto"
LangString TypeText 1031 "Textdokument"
LangString TypeText 2052 "文本文档"
LangString TypeText 1040 "Documento di testo"
LangString TypeText 1041 "テキスト ドキュメント"
LangString TypeText 1042 "텍스트 문서"
LangString TypeText 1036 "Document texte"
LangString TypeText 2070 "Documento de texto"
LangString TypeText 1027 "Document de text"
LangString TypeText 1110 "Documento de texto"
LangString TypeText 1048 "Document text"
LangString TypeJson 1033 "JSON file"
LangString TypeJson 1046 "Arquivo JSON"
LangString TypeJson 3082 "Archivo JSON"
LangString TypeJson 1031 "JSON-Datei"
LangString TypeJson 2052 "JSON 文件"
LangString TypeJson 1040 "File JSON"
LangString TypeJson 1041 "JSON ファイル"
LangString TypeJson 1042 "JSON 파일"
LangString TypeJson 1036 "Fichier JSON"
LangString TypeJson 2070 "Ficheiro JSON"
LangString TypeJson 1027 "Fitxer JSON"
LangString TypeJson 1110 "Ficheiro JSON"
LangString TypeJson 1048 "Fișier JSON"
LangString TypeXml 1033 "XML file"
LangString TypeXml 1046 "Arquivo XML"
LangString TypeXml 3082 "Archivo XML"
LangString TypeXml 1031 "XML-Datei"
LangString TypeXml 2052 "XML 文件"
LangString TypeXml 1040 "File XML"
LangString TypeXml 1041 "XML ファイル"
LangString TypeXml 1042 "XML 파일"
LangString TypeXml 1036 "Fichier XML"
LangString TypeXml 2070 "Ficheiro XML"
LangString TypeXml 1027 "Fitxer XML"
LangString TypeXml 1110 "Ficheiro XML"
LangString TypeXml 1048 "Fișier XML"
LangString TypeYaml 1033 "YAML file"
LangString TypeYaml 1046 "Arquivo YAML"
LangString TypeYaml 3082 "Archivo YAML"
LangString TypeYaml 1031 "YAML-Datei"
LangString TypeYaml 2052 "YAML 文件"
LangString TypeYaml 1040 "File YAML"
LangString TypeYaml 1041 "YAML ファイル"
LangString TypeYaml 1042 "YAML 파일"
LangString TypeYaml 1036 "Fichier YAML"
LangString TypeYaml 2070 "Ficheiro YAML"
LangString TypeYaml 1027 "Fitxer YAML"
LangString TypeYaml 1110 "Ficheiro YAML"
LangString TypeYaml 1048 "Fișier YAML"
LangString TypeToml 1033 "TOML file"
LangString TypeToml 1046 "Arquivo TOML"
LangString TypeToml 3082 "Archivo TOML"
LangString TypeToml 1031 "TOML-Datei"
LangString TypeToml 2052 "TOML 文件"
LangString TypeToml 1040 "File TOML"
LangString TypeToml 1041 "TOML ファイル"
LangString TypeToml 1042 "TOML 파일"
LangString TypeToml 1036 "Fichier TOML"
LangString TypeToml 2070 "Ficheiro TOML"
LangString TypeToml 1027 "Fitxer TOML"
LangString TypeToml 1110 "Ficheiro TOML"
LangString TypeToml 1048 "Fișier TOML"
LangString TypeIni 1033 "Settings file"
LangString TypeIni 1046 "Arquivo de configuração"
LangString TypeIni 3082 "Archivo de configuración"
LangString TypeIni 1031 "Einstellungsdatei"
LangString TypeIni 2052 "设置文件"
LangString TypeIni 1040 "File di impostazioni"
LangString TypeIni 1041 "設定ファイル"
LangString TypeIni 1042 "설정 파일"
LangString TypeIni 1036 "Fichier de configuration"
LangString TypeIni 2070 "Ficheiro de definições"
LangString TypeIni 1027 "Fitxer de configuració"
LangString TypeIni 1110 "Ficheiro de configuración"
LangString TypeIni 1048 "Fișier de setări"
LangString TypeProperties 1033 "Properties file"
LangString TypeProperties 1046 "Arquivo de propriedades"
LangString TypeProperties 3082 "Archivo de propiedades"
LangString TypeProperties 1031 "Properties-Datei"
LangString TypeProperties 2052 "Properties 文件"
LangString TypeProperties 1040 "File di proprietà"
LangString TypeProperties 1041 "Properties ファイル"
LangString TypeProperties 1042 "Properties 파일"
LangString TypeProperties 1036 "Fichier de propriétés"
LangString TypeProperties 2070 "Ficheiro de propriedades"
LangString TypeProperties 1027 "Fitxer de propietats"
LangString TypeProperties 1110 "Ficheiro de propiedades"
LangString TypeProperties 1048 "Fișier de proprietăți"

; ---- State ----------------------------------------------------------------------------------------------------------

; "1" | "0" ticked on the welcome page; "" when the page did not show (silent, passive, update).
Var PermarkMenu
Var PermarkNew
Var PermarkPath
; "markdown;json;" ticked on the welcome page; "" when the page did not show.
Var PermarkTypes
Var PermarkExe
Var PermarkLabel
Var PermarkNewLabel
Var PermarkMenuBox
Var PermarkNewBox
Var PermarkPathBox
Var PermarkBoxMarkdown
Var PermarkBoxText
Var PermarkBoxJson
Var PermarkBoxXml
Var PermarkBoxYaml
Var PermarkBoxToml
Var PermarkBoxIni
Var PermarkBoxProperties
; the PATH steps (see "Command line")
Var PermarkPathType
Var PermarkPathValue
Var PermarkPathKept
Var PermarkPathFound
Var PermarkPathDir

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
; (silent, passive) takes DEFAULT; an update with nothing recorded changes nothing.
!macro PERMARK_RESOLVE VAR CHOICE DEFAULT
  ${If} ${VAR} == ""
    ClearErrors
    ReadRegDWORD $R0 HKCU "${PERMARK_APP_KEY}" "${CHOICE}"
    ${If} ${Errors}
      ${If} $UpdateMode = 1
        StrCpy ${VAR} ""
      ${Else}
        StrCpy ${VAR} "${DEFAULT}"
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
  !insertmacro PERMARK_RESOLVE ${VAR} "${CHOICE}" "1"
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

; ---- Command line: the install folder on the user's PATH ------------------------------------------------------------
; Per user (the install is): HKCU\Environment\Path only, never the machine's PATH. NSIS strings hold NSIS_MAX_STRLEN - 1
; characters (1023 in the NSIS Tauri uses). ReadRegStr gives "" for a longer value, and a string built past the limit is
; cut short: writing either back would wipe the person's other entries, the classic installer PATH damage. So the value's
; type and size are read first, a PATH that could not be rewritten whole is never written, and every write keeps the type
; the value had (REG_EXPAND_SZ keeps entries such as %USERPROFILE%\bin unexpanded).

; PermarkPathType = "2" REG_EXPAND_SZ | "1" REG_SZ, with PermarkPathValue = the PATH as stored; "0" = no PATH value yet;
; "" = it cannot be changed safely (too long for an NSIS string, or not text).
!macro PERMARK_PATH_READ
  Push $0
  Push $1
  Push $2
  Push $3
  StrCpy $PermarkPathType ""
  StrCpy $PermarkPathValue ""
  ; 0x80000001 = HKEY_CURRENT_USER, 0x1 = KEY_QUERY_VALUE; result 2 = ERROR_FILE_NOT_FOUND
  System::Call 'advapi32::RegOpenKeyExW(p 0x80000001, w "${PERMARK_ENV_KEY}", i 0, i 0x1, *p .r1) i .r0'
  ${If} $0 = 2
    StrCpy $PermarkPathType "0"
  ${ElseIf} $0 = 0
    ; no buffer: only the type ($2) and the size in bytes with the closing NUL ($3), whatever the length
    System::Call 'advapi32::RegQueryValueExW(p r1, w "Path", p 0, *i .r2, p 0, *i .r3) i .r0'
    System::Call 'advapi32::RegCloseKey(p r1)'
    ${If} $0 = 2
      StrCpy $PermarkPathType "0"
    ${ElseIf} $0 = 0
      IntOp $3 $3 / 2
      ${If} $3 < ${NSIS_MAX_STRLEN}
        ${If} $2 = 1
        ${OrIf} $2 = 2
          ClearErrors
          ReadRegStr $PermarkPathValue HKCU "${PERMARK_ENV_KEY}" "Path"
          ${IfNot} ${Errors}
            StrCpy $PermarkPathType $2
          ${EndIf}
        ${EndIf}
      ${EndIf}
    ${EndIf}
  ${EndIf}
  ClearErrors
  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend

; VAR as entries are compared: outer quotes off, variables expanded, closing backslashes off (NSIS compares strings
; without case, as Windows compares paths). VAR != $R0.
!macro PERMARK_PATH_NORMAL VAR
  Push $R0
  StrCpy $R0 ${VAR} 1
  ${If} $R0 == '"'
    StrCpy ${VAR} ${VAR} "" 1
    StrCpy $R0 ${VAR} "" -1
    ${If} $R0 == '"'
      StrCpy ${VAR} ${VAR} -1
    ${EndIf}
  ${EndIf}
  ExpandEnvStrings ${VAR} ${VAR}
  ${Do}
    StrCpy $R0 ${VAR} "" -1
    ${If} $R0 != "\"
      ${ExitDo}
    ${EndIf}
    StrCpy ${VAR} ${VAR} -1
  ${Loop}
  Pop $R0
!macroend

; Walks PermarkPathValue entry by entry. PermarkPathFound = "1" when an entry is the folder PermarkPathDir (normalized);
; PermarkPathKept = the value without those entries, every other entry and separator as it was (empty ones too).
!macro PERMARK_PATH_SCAN
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  StrCpy $PermarkPathKept ""
  StrCpy $PermarkPathFound "0"
  ; $0 = position, $1 = the character there, $2 = the entry so far, $3 = entries kept
  StrCpy $0 0
  StrCpy $2 ""
  StrCpy $3 0
  ${Do}
    StrCpy $1 $PermarkPathValue 1 $0
    ${If} $1 == ";"
    ${OrIf} $1 == ""
      StrCpy $4 $2
      !insertmacro PERMARK_PATH_NORMAL $4
      ${If} $4 == $PermarkPathDir
        StrCpy $PermarkPathFound "1"
      ${Else}
        ${If} $3 > 0
          StrCpy $PermarkPathKept "$PermarkPathKept;"
        ${EndIf}
        StrCpy $PermarkPathKept "$PermarkPathKept$2"
        IntOp $3 $3 + 1
      ${EndIf}
      ${If} $1 == ""
        ${ExitDo}
      ${EndIf}
      StrCpy $2 ""
    ${Else}
      StrCpy $2 "$2$1"
    ${EndIf}
    IntOp $0 $0 + 1
  ${Loop}
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend

; Writes PermarkPathValue with the type the PATH had; a new PATH gets REG_EXPAND_SZ, Windows' own type for it.
!macro PERMARK_PATH_WRITE
  ClearErrors
  ${If} $PermarkPathType == "1"
    WriteRegStr HKCU "${PERMARK_ENV_KEY}" "Path" "$PermarkPathValue"
  ${Else}
    WriteRegExpandStr HKCU "${PERMARK_ENV_KEY}" "Path" "$PermarkPathValue"
  ${EndIf}
!macroend

; Explorer reloads the environment on this message, so terminals started afterwards see the new PATH (open ones keep
; theirs). SMTO_ABORTIFHUNG (0x2): a hung window is skipped instead of holding the installer up.
!macro PERMARK_PATH_ANNOUNCE
  System::Call 'user32::SendMessageTimeoutW(p ${HWND_BROADCAST}, i ${WM_SETTINGCHANGE}, p 0, w "Environment", i 0x2, i 5000, p 0)'
!macroend

; Adds $INSTDIR at the end of the PATH unless an entry already is that folder (then nothing is recorded: the entry is not
; the installer's to remove). The folder written is recorded as \PathEntry.
!macro PERMARK_PATH_ADD
  Push $0
  Push $1
  StrCpy $PermarkPathDir "$INSTDIR"
  !insertmacro PERMARK_PATH_NORMAL $PermarkPathDir
  !insertmacro PERMARK_PATH_READ
  StrCpy $PermarkPathFound "0"
  ${If} $PermarkPathType != ""
    !insertmacro PERMARK_PATH_SCAN
  ${EndIf}
  ${If} $PermarkPathFound != "1"
    ; the new value (old value, ";", folder) must stay readable by ReadRegStr, which holds NSIS_MAX_STRLEN - 2
    ; characters (it keeps one for the closing NUL): else no later uninstall could remove the entry
    StrLen $0 $PermarkPathValue
    StrLen $1 $PermarkPathDir
    IntOp $0 $0 + $1
    IntOp $0 $0 + 2
    ; a ";" in the folder's name would split it into two entries
    ${StrLoc} $1 "$PermarkPathDir" ";" ">"
    ${If} $PermarkPathType == ""
    ${OrIf} $0 >= ${NSIS_MAX_STRLEN}
    ${OrIf} $1 != ""
      DetailPrint "$(PathNotAdded)"
      ${If} $PassiveMode <> 1
        MessageBox MB_ICONINFORMATION|MB_OK "$(PathNotAdded)" /SD IDOK
      ${EndIf}
    ${Else}
      ${If} $PermarkPathValue != ""
        StrCpy $1 $PermarkPathValue "" -1
        ${If} $1 != ";"
          StrCpy $PermarkPathValue "$PermarkPathValue;"
        ${EndIf}
      ${EndIf}
      StrCpy $PermarkPathValue "$PermarkPathValue$PermarkPathDir"
      !insertmacro PERMARK_PATH_WRITE
      ${IfNot} ${Errors}
        WriteRegStr HKCU "${PERMARK_APP_KEY}" "${PERMARK_PATH_ENTRY}" "$PermarkPathDir"
        !insertmacro PERMARK_PATH_ANNOUNCE
      ${EndIf}
    ${EndIf}
  ${EndIf}
  ClearErrors
  Pop $1
  Pop $0
!macroend

; Removes the recorded entry (every entry that is that folder) and the record. A PATH that cannot be rewritten whole is
; left as it is and the record kept, so a later uninstall can still remove the entry. A PATH left empty is deleted: only
; the installer's entry was in it.
!macro PERMARK_PATH_REMOVE
  ReadRegStr $PermarkPathDir HKCU "${PERMARK_APP_KEY}" "${PERMARK_PATH_ENTRY}"
  ${If} $PermarkPathDir != ""
    !insertmacro PERMARK_PATH_NORMAL $PermarkPathDir
    !insertmacro PERMARK_PATH_READ
    ${If} $PermarkPathType == ""
      DetailPrint "$(PathNotRemoved)"
      ${If} $PassiveMode <> 1
        MessageBox MB_ICONINFORMATION|MB_OK "$(PathNotRemoved)" /SD IDOK
      ${EndIf}
    ${Else}
      ${If} $PermarkPathType != "0"
        !insertmacro PERMARK_PATH_SCAN
        ${If} $PermarkPathFound == "1"
          ${If} $PermarkPathKept == ""
            DeleteRegValue HKCU "${PERMARK_ENV_KEY}" "Path"
          ${Else}
            StrCpy $PermarkPathValue $PermarkPathKept
            !insertmacro PERMARK_PATH_WRITE
          ${EndIf}
          !insertmacro PERMARK_PATH_ANNOUNCE
        ${EndIf}
      ${EndIf}
      DeleteRegValue HKCU "${PERMARK_APP_KEY}" "${PERMARK_PATH_ENTRY}"
    ${EndIf}
  ${EndIf}
  ClearErrors
!macroend

; Asked on the welcome page: done as ticked. Not asked: an update leaves PATH as it is (same folder; an entry the person
; removed by hand stays removed); a silent | passive install adds the entry only when an earlier install recorded it on.
!macro PERMARK_PATH_APPLY
  ${If} $PermarkPath != ""
  ${OrIf} $UpdateMode <> 1
    !insertmacro PERMARK_RESOLVE $PermarkPath "${PERMARK_PATH_CHOICE}" "0"
    ${If} $PermarkPath == "1"
      !insertmacro PERMARK_PATH_ADD
      WriteRegDWORD HKCU "${PERMARK_APP_KEY}" "${PERMARK_PATH_CHOICE}" 1
    ${Else}
      !insertmacro PERMARK_PATH_REMOVE
      WriteRegDWORD HKCU "${PERMARK_APP_KEY}" "${PERMARK_PATH_CHOICE}" 0
    ${EndIf}
  ${EndIf}
!macroend

; ---- The welcome page: short text, the types, the three entries -----------------------------------------------------
; Everything under the page's title sits in fixed boxes (dialog units, x 120..315, y 42..192 of 193), each as tall as the
; longest of the six languages needs, so no text can run into the next control (the template's own text label is one 130u
; box, tall enough to cover the grid below it; ours stays empty and our boxes replace it):
;   42  the welcome text: two paragraphs, <= 6 lines
;   96  the title of the check grid, <= 3 lines (it carries the sentence "Windows asks you to confirm")
;  123  the grid: 2 rows of 4 boxes, widths 52 + 44 + 42 + 57 (the last holds "Properties", the widest label)
;  147  the two file manager entries, 2 lines each
;  181  the PATH option, 1 line (the shortest words on the page: no language wraps them)
; Windows CJK fonts make a line ~8.5u, Latin ~8u; the lines above are measured in all six languages with the dialog font.

!define MUI_WELCOMEPAGE_TEXT " "

; raise a control above the template's tall (empty) text label
!macro PERMARK_RAISE VAR
  System::Call 'user32::SetWindowPos(p ${VAR}, p 0, i 0, i 0, i 0, i 0, i 3)'
!macroend

!macro PERMARK_LABEL Y H TEXT
  ${NSD_CreateLabel} 120u ${Y} 195u ${H} "${TEXT}"
  Pop $0
  SetCtlColors $0 "000000" "FFFFFF"
  !insertmacro PERMARK_RAISE $0
!macroend

!macro PERMARK_TYPE_BOX VAR X Y W LABEL NAME
  ${NSD_CreateCheckbox} ${X} ${Y} ${W} 11u "${LABEL}"
  Pop ${VAR}
  SetCtlColors ${VAR} "000000" "FFFFFF"
  !insertmacro PERMARK_RAISE ${VAR}
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

; ticked as recorded by an earlier install, else as DEFAULT (1 | 0)
!macro PERMARK_ENTRY_BOX VAR Y H LABEL CHOICE DEFAULT
  ClearErrors
  ReadRegDWORD $0 HKCU "${PERMARK_APP_KEY}" "${CHOICE}"
  ${If} ${Errors}
    StrCpy $0 ${DEFAULT}
  ${EndIf}
  ${NSD_CreateCheckbox} 120u ${Y} 195u ${H} "${LABEL}"
  Pop ${VAR}
  SetCtlColors ${VAR} "000000" "FFFFFF"
  !insertmacro PERMARK_RAISE ${VAR}
  ${If} $0 <> 0
    ${NSD_Check} ${VAR}
  ${EndIf}
!macroend

; VAR = "1" | "0" as BOX was left
!macro PERMARK_ENTRY_READ BOX VAR
  ${NSD_GetState} ${BOX} $0
  ${If} $0 = ${BST_CHECKED}
    StrCpy ${VAR} "1"
  ${Else}
    StrCpy ${VAR} "0"
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

  !insertmacro PERMARK_LABEL 42u 52u "$(PermarkWelcome)"
  !insertmacro PERMARK_LABEL 96u 25u "$(PermarkTypesTitle)"

  !insertmacro PERMARK_TYPE_BOX $PermarkBoxMarkdown 120u 123u 52u "Markdown" ";markdown"
  !insertmacro PERMARK_TYPE_BOX $PermarkBoxText 172u 123u 44u "$(PermarkText)" ";text"
  !insertmacro PERMARK_TYPE_BOX $PermarkBoxJson 216u 123u 42u "JSON" ";json"
  !insertmacro PERMARK_TYPE_BOX $PermarkBoxXml 258u 123u 57u "XML" ";xml"
  !insertmacro PERMARK_TYPE_BOX $PermarkBoxYaml 120u 134u 52u "YAML" ";yaml"
  !insertmacro PERMARK_TYPE_BOX $PermarkBoxToml 172u 134u 44u "TOML" ";toml"
  !insertmacro PERMARK_TYPE_BOX $PermarkBoxIni 216u 134u 42u "INI" ";ini"
  !insertmacro PERMARK_TYPE_BOX $PermarkBoxProperties 258u 134u 57u "$(PermarkProperties)" ";properties"

  !insertmacro PERMARK_ENTRY_BOX $PermarkMenuBox 147u 16u "$(OpenWithMenuOption)" "${PERMARK_MENU_CHOICE}" 1
  !insertmacro PERMARK_ENTRY_BOX $PermarkNewBox 164u 17u "$(NewMarkdownOption)" "${PERMARK_NEW_CHOICE}" 1
  !insertmacro PERMARK_ENTRY_BOX $PermarkPathBox 181u 11u "$(PathOption)" "${PERMARK_PATH_CHOICE}" 0
  Pop $R9
  Pop $R8
  Pop $0
FunctionEnd

Function PermarkWelcomeLeave
  Push $0
  !insertmacro PERMARK_ENTRY_READ $PermarkMenuBox $PermarkMenu
  !insertmacro PERMARK_ENTRY_READ $PermarkNewBox $PermarkNew
  !insertmacro PERMARK_ENTRY_READ $PermarkPathBox $PermarkPath
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
  ${ElseIf} $LANGUAGE = 1041
    StrCpy $R8 "ja"
  ${ElseIf} $LANGUAGE = 1042
    StrCpy $R8 "ko"
  ${ElseIf} $LANGUAGE = 1036
    StrCpy $R8 "fr"
  ${ElseIf} $LANGUAGE = 2070
    StrCpy $R8 "pt-PT"
  ${ElseIf} $LANGUAGE = 1027
    StrCpy $R8 "ca"
  ${ElseIf} $LANGUAGE = 1110
    StrCpy $R8 "gl"
  ${ElseIf} $LANGUAGE = 1048
    StrCpy $R8 "ro"
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
  !insertmacro PERMARK_PATH_APPLY

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
    ; before the application data goes: the record names the PATH entry to remove (the choice itself stays recorded)
    !insertmacro PERMARK_PATH_REMOVE
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
