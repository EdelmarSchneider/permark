; Tauri's own installer strings (the pages and messages of its NSIS template) in European Portuguese, in place of
; Tauri's file: `bundle.windows.nsis.customLanguageFiles` in tauri.conf.json points here. Tauri's words, with the
; literal {{product_name}} placeholders it leaves unfilled replaced. Same names as Tauri's English.nsh (Tauri CLI 2.12.3;
; 2.12.4 renamed choowHowToInstall to chooseHowToInstall: both are here, same words). Tauri copies the file as it is and
; adds the byte order mark itself, so the file has none; the product name is ${PRODUCTNAME}.
LangString addOrReinstall ${LANG_PORTUGUESE} "Adicionar ou reinstalar componentes"
LangString alreadyInstalled ${LANG_PORTUGUESE} "Já instalado"
LangString alreadyInstalledLong ${LANG_PORTUGUESE} "O ${PRODUCTNAME} ${VERSION} já está instalado. Selecione a operação que deseja realizar e clique em Seguinte para continuar."
LangString appRunning ${LANG_PORTUGUESE} "O ${PRODUCTNAME} está em execução. Feche-o primeiro e tente novamente."
LangString appRunningOkKill ${LANG_PORTUGUESE} "O ${PRODUCTNAME} está em execução.$\nClique em OK para o encerrar."
LangString chooseMaintenanceOption ${LANG_PORTUGUESE} "Escolha a opção de manutenção a realizar."
LangString chooseHowToInstall ${LANG_PORTUGUESE} "Escolha como deseja instalar o ${PRODUCTNAME}."
LangString choowHowToInstall ${LANG_PORTUGUESE} "Escolha como deseja instalar o ${PRODUCTNAME}."
LangString createDesktop ${LANG_PORTUGUESE} "Criar atalho no ambiente de trabalho"
LangString dontUninstall ${LANG_PORTUGUESE} "Não desinstalar"
LangString dontUninstallDowngrade ${LANG_PORTUGUESE} "Não desinstalar (instalar uma versão anterior sem desinstalar está desativado neste instalador)"
LangString failedToKillApp ${LANG_PORTUGUESE} "Não foi possível encerrar o ${PRODUCTNAME}. Feche-o primeiro e tente novamente."
LangString installingWebview2 ${LANG_PORTUGUESE} "A instalar o WebView2..."
LangString newerVersionInstalled ${LANG_PORTUGUESE} "Uma versão mais recente do ${PRODUCTNAME} já está instalada. Não é recomendada a instalação de uma versão mais antiga. Se realmente deseja instalar esta versão mais antiga, é melhor desinstalar primeiro a versão atual. Selecione a operação que deseja realizar e clique em Seguinte para continuar."
LangString older ${LANG_PORTUGUESE} "mais antiga"
LangString olderOrUnknownVersionInstalled ${LANG_PORTUGUESE} "Uma versão $R4 do ${PRODUCTNAME} está instalada no sistema. Recomenda-se desinstalar a versão atual antes de instalar. Selecione a operação que deseja realizar e clique em Seguinte para continuar."
LangString silentDowngrades ${LANG_PORTUGUESE} "Este instalador não permite instalar uma versão anterior, pelo que a instalação silenciosa não pode continuar. Utilize o instalador com interface gráfica.$\n"
LangString unableToUninstall ${LANG_PORTUGUESE} "Não foi possível desinstalar."
LangString uninstallApp ${LANG_PORTUGUESE} "Desinstalar o ${PRODUCTNAME}"
LangString uninstallBeforeInstalling ${LANG_PORTUGUESE} "Desinstalar antes de instalar"
LangString unknown ${LANG_PORTUGUESE} "desconhecida"
LangString webview2AbortError ${LANG_PORTUGUESE} "Não foi possível instalar o WebView2. A aplicação não pode ser executada sem ele. Tente reiniciar o instalador."
LangString webview2DownloadError ${LANG_PORTUGUESE} "Erro: não foi possível transferir o WebView2 - $0"
LangString webview2DownloadSuccess ${LANG_PORTUGUESE} "Instalador do WebView2 transferido com sucesso"
LangString webview2Downloading ${LANG_PORTUGUESE} "A transferir o instalador do WebView2..."
LangString webview2InstallError ${LANG_PORTUGUESE} "Erro: a instalação do WebView2 falhou com o código $1"
LangString webview2InstallSuccess ${LANG_PORTUGUESE} "WebView2 instalado com sucesso"
LangString deleteAppData ${LANG_PORTUGUESE} "Eliminar os dados da aplicação"

; Stock NSIS texts of this language's pages, in other words: "Bem vindo" lacks its hyphen, and the Brazilian gerunds
; ("Concluindo", "Instalando", "está sendo instalado") become European Portuguese. Tauri includes this file after the
; language is inserted, so these later texts win (makensis warns 6030 for each Modern UI one).
LangString MUI_TEXT_WELCOME_INFO_TITLE ${LANG_PORTUGUESE} "Bem-vindo ao Assistente de Instalação do $(^NameDA)"
LangString MUI_TEXT_INSTALLING_TITLE ${LANG_PORTUGUESE} "A instalar"
LangString MUI_TEXT_INSTALLING_SUBTITLE ${LANG_PORTUGUESE} "Aguarde enquanto o $(^NameDA) é instalado."
LangString MUI_TEXT_FINISH_INFO_TITLE ${LANG_PORTUGUESE} "A concluir o Assistente de Instalação do $(^NameDA)"
LangString MUI_UNTEXT_UNINSTALLING_TITLE ${LANG_PORTUGUESE} "A desinstalar"
LangString MUI_UNTEXT_UNINSTALLING_SUBTITLE ${LANG_PORTUGUESE} "Aguarde enquanto o $(^NameDA) é desinstalado."
