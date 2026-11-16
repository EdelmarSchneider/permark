; Tauri's own installer strings (the pages and messages of its NSIS template) in Brazilian Portuguese, in place of
; Tauri's file: `bundle.windows.nsis.customLanguageFiles` in tauri.conf.json points here. Tauri's file (CLI 2.12.3) has
; typos on the reinstall page ("primeirl", "executare clique", "versão ... instalado") and literal {{product_name}}
; placeholders. Same names as Tauri's English.nsh (2.12.4 renamed choowHowToInstall to chooseHowToInstall: both are
; here, same words). Tauri copies the file as it is and adds the byte order mark itself, so the file has none; the
; product name is ${PRODUCTNAME}.
LangString addOrReinstall ${LANG_PORTUGUESEBR} "Adicionar ou reinstalar componentes"
LangString alreadyInstalled ${LANG_PORTUGUESEBR} "Já instalado"
LangString alreadyInstalledLong ${LANG_PORTUGUESEBR} "O ${PRODUCTNAME} ${VERSION} já está instalado. Selecione a operação que deseja realizar e clique em Próximo para continuar."
LangString appRunning ${LANG_PORTUGUESEBR} "O ${PRODUCTNAME} está em execução. Feche-o e tente novamente."
LangString appRunningOkKill ${LANG_PORTUGUESEBR} "O ${PRODUCTNAME} está em execução.$\nClique em OK para fechá-lo."
LangString chooseMaintenanceOption ${LANG_PORTUGUESEBR} "Escolha a opção de manutenção a realizar."
LangString chooseHowToInstall ${LANG_PORTUGUESEBR} "Escolha como deseja instalar o ${PRODUCTNAME}."
LangString choowHowToInstall ${LANG_PORTUGUESEBR} "Escolha como deseja instalar o ${PRODUCTNAME}."
LangString createDesktop ${LANG_PORTUGUESEBR} "Criar atalho na área de trabalho"
LangString dontUninstall ${LANG_PORTUGUESEBR} "Não desinstalar"
LangString dontUninstallDowngrade ${LANG_PORTUGUESEBR} "Não desinstalar (este instalador não permite instalar uma versão anterior sem desinstalar)"
LangString failedToKillApp ${LANG_PORTUGUESEBR} "Não foi possível fechar o ${PRODUCTNAME}. Feche-o e tente novamente."
LangString installingWebview2 ${LANG_PORTUGUESEBR} "Instalando o WebView2..."
LangString newerVersionInstalled ${LANG_PORTUGUESEBR} "Uma versão mais recente do ${PRODUCTNAME} já está instalada. Não é recomendado instalar uma versão anterior. Se você realmente deseja instalar esta versão anterior, é melhor desinstalar a versão atual primeiro. Selecione a operação que deseja realizar e clique em Próximo para continuar."
LangString older ${LANG_PORTUGUESEBR} "mais antiga"
LangString olderOrUnknownVersionInstalled ${LANG_PORTUGUESEBR} "Uma versão $R4 do ${PRODUCTNAME} está instalada no seu sistema. É recomendado desinstalar a versão atual antes de prosseguir com a instalação. Selecione a operação que deseja realizar e clique em Próximo para continuar."
LangString silentDowngrades ${LANG_PORTUGUESEBR} "Este instalador não permite instalar uma versão anterior, então a instalação silenciosa não pode continuar. Use o instalador com interface gráfica.$\n"
LangString unableToUninstall ${LANG_PORTUGUESEBR} "Não foi possível desinstalar."
LangString uninstallApp ${LANG_PORTUGUESEBR} "Desinstalar o ${PRODUCTNAME}"
LangString uninstallBeforeInstalling ${LANG_PORTUGUESEBR} "Desinstalar antes de instalar"
LangString unknown ${LANG_PORTUGUESEBR} "desconhecida"
LangString webview2AbortError ${LANG_PORTUGUESEBR} "Não foi possível instalar o WebView2. O programa não funciona sem ele. Tente executar o instalador novamente."
LangString webview2DownloadError ${LANG_PORTUGUESEBR} "Erro: não foi possível baixar o WebView2 - $0"
LangString webview2DownloadSuccess ${LANG_PORTUGUESEBR} "Instalador do WebView2 baixado com sucesso"
LangString webview2Downloading ${LANG_PORTUGUESEBR} "Baixando o instalador do WebView2..."
LangString webview2InstallError ${LANG_PORTUGUESEBR} "Erro: a instalação do WebView2 falhou com o código $1"
LangString webview2InstallSuccess ${LANG_PORTUGUESEBR} "WebView2 instalado com sucesso"
LangString deleteAppData ${LANG_PORTUGUESEBR} "Remover os dados do programa"

; Stock NSIS texts of this language's pages, in other words: the informal "pra" and "O Instalador" become "para" and
; "o instalador". Tauri includes this file after the language is inserted, so these later texts win (makensis warns
; 6030 for each Modern UI one).
LangString ^DirText ${LANG_PORTUGUESEBR} "O instalador instalará o $(^NameDA) na seguinte pasta. Para instalar em uma pasta diferente, clique em Procurar e selecione outra pasta. $_CLICK"
LangString ^ClickInstall ${LANG_PORTUGUESEBR} "Clique em Instalar para iniciar a instalação."
LangString ^ClickUninstall ${LANG_PORTUGUESEBR} "Clique em Desinstalar para iniciar a desinstalação."
LangString MUI_TEXT_FINISH_INFO_TEXT ${LANG_PORTUGUESEBR} "O $(^NameDA) foi instalado no seu computador.$\r$\n$\r$\nClique em Concluir para fechar o instalador."
