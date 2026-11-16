; Tauri's own installer strings (the pages and messages of its NSIS template) in Spanish, in place of Tauri's file:
; `bundle.windows.nsis.customLanguageFiles` in tauri.conf.json points here. Tauri's file (CLI 2.12.3) drops the article
; in "los datos de aplicación" and has literal {{product_name}} placeholders. Same names as Tauri's English.nsh (2.12.4
; renamed choowHowToInstall to chooseHowToInstall: both are here, same words). Tauri copies the file as it is and adds
; the byte order mark itself, so the file has none; the product name is ${PRODUCTNAME}.
LangString addOrReinstall ${LANG_SPANISHINTERNATIONAL} "Añadir o reinstalar componentes"
LangString alreadyInstalled ${LANG_SPANISHINTERNATIONAL} "Ya está instalado"
LangString alreadyInstalledLong ${LANG_SPANISHINTERNATIONAL} "${PRODUCTNAME} ${VERSION} ya está instalado. Seleccione la operación que desee realizar y pulse Siguiente para continuar."
LangString appRunning ${LANG_SPANISHINTERNATIONAL} "${PRODUCTNAME} está abierto. Ciérrelo e inténtelo de nuevo."
LangString appRunningOkKill ${LANG_SPANISHINTERNATIONAL} "${PRODUCTNAME} está abierto.$\nPulse Aceptar para cerrarlo."
LangString chooseMaintenanceOption ${LANG_SPANISHINTERNATIONAL} "Elija la operación de mantenimiento que desee realizar."
LangString chooseHowToInstall ${LANG_SPANISHINTERNATIONAL} "Elija cómo desea instalar ${PRODUCTNAME}."
LangString choowHowToInstall ${LANG_SPANISHINTERNATIONAL} "Elija cómo desea instalar ${PRODUCTNAME}."
LangString createDesktop ${LANG_SPANISHINTERNATIONAL} "Crear acceso directo en el escritorio"
LangString dontUninstall ${LANG_SPANISHINTERNATIONAL} "No desinstalar"
LangString dontUninstallDowngrade ${LANG_SPANISHINTERNATIONAL} "No desinstalar (este instalador no permite instalar una versión anterior sin desinstalar)"
LangString failedToKillApp ${LANG_SPANISHINTERNATIONAL} "No se ha podido cerrar ${PRODUCTNAME}. Ciérrelo e inténtelo de nuevo."
LangString installingWebview2 ${LANG_SPANISHINTERNATIONAL} "Instalando WebView2..."
LangString newerVersionInstalled ${LANG_SPANISHINTERNATIONAL} "Ya está instalada una versión más reciente de ${PRODUCTNAME}. No se recomienda que instale una versión anterior. Si realmente desea instalar esta versión anterior, es recomendable desinstalar la versión actual antes de continuar. Seleccione la operación que desee realizar y pulse Siguiente para continuar."
LangString older ${LANG_SPANISHINTERNATIONAL} "anterior"
LangString olderOrUnknownVersionInstalled ${LANG_SPANISHINTERNATIONAL} "Una versión $R4 de ${PRODUCTNAME} está instalada en su sistema. Es recomendable desinstalar la versión actual antes de continuar. Seleccione la operación que desee realizar y pulse Siguiente para continuar."
LangString silentDowngrades ${LANG_SPANISHINTERNATIONAL} "Este instalador no permite instalar una versión anterior, así que la instalación silenciosa no puede continuar. Use el instalador con interfaz gráfica.$\n"
LangString unableToUninstall ${LANG_SPANISHINTERNATIONAL} "No se ha podido desinstalar."
LangString uninstallApp ${LANG_SPANISHINTERNATIONAL} "Desinstalar ${PRODUCTNAME}"
LangString uninstallBeforeInstalling ${LANG_SPANISHINTERNATIONAL} "Desinstalar antes de instalar"
LangString unknown ${LANG_SPANISHINTERNATIONAL} "desconocida"
LangString webview2AbortError ${LANG_SPANISHINTERNATIONAL} "No se ha podido instalar WebView2. La aplicación no puede funcionar sin él. Intente reiniciar el instalador."
LangString webview2DownloadError ${LANG_SPANISHINTERNATIONAL} "Error: no se ha podido descargar WebView2 - $0"
LangString webview2DownloadSuccess ${LANG_SPANISHINTERNATIONAL} "El instalador de WebView2 se ha descargado correctamente."
LangString webview2Downloading ${LANG_SPANISHINTERNATIONAL} "Descargando el instalador de WebView2..."
LangString webview2InstallError ${LANG_SPANISHINTERNATIONAL} "Error: la instalación de WebView2 falló con el código $1."
LangString webview2InstallSuccess ${LANG_SPANISHINTERNATIONAL} "WebView2 se ha instalado correctamente."
LangString deleteAppData ${LANG_SPANISHINTERNATIONAL} "Eliminar los datos de la aplicación"

; Stock NSIS texts of this language's pages, in other words. NSIS' Spanish file leaves the space labels unquoted, so
; their trailing space is lost ("Espacio requerido:0.0 KB"). Tauri includes this file after the language is inserted,
; so these later texts win.
LangString ^SpaceAvailable ${LANG_SPANISHINTERNATIONAL} "Espacio disponible: "
LangString ^SpaceRequired ${LANG_SPANISHINTERNATIONAL} "Espacio requerido: "
