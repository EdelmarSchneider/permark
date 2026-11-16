; Tauri's own installer strings (the pages and messages of its NSIS template) in French, in place of Tauri's file:
; `bundle.windows.nsis.customLanguageFiles` in tauri.conf.json points here. Tauri's file (CLI 2.12.3) reads "Déja",
; "La version ancien de ...", a reinstall subtitle that asks for the install location, and has literal {{product_name}}
; placeholders. Same names as Tauri's English.nsh (2.12.4 renamed choowHowToInstall to chooseHowToInstall: both are
; here, same words). Tauri copies the file as it is and adds the byte order mark itself, so the file has none; the
; product name is ${PRODUCTNAME}. A no-break space stands before : ; ? !
LangString addOrReinstall ${LANG_FRENCH} "Ajouter ou réinstaller des composants"
LangString alreadyInstalled ${LANG_FRENCH} "Déjà installé"
LangString alreadyInstalledLong ${LANG_FRENCH} "${PRODUCTNAME} ${VERSION} est déjà installé. Sélectionnez l'opération que vous souhaitez effectuer, puis cliquez sur Suivant pour continuer."
LangString appRunning ${LANG_FRENCH} "${PRODUCTNAME} est en cours d'exécution. Veuillez fermer l'application avant de réessayer."
LangString appRunningOkKill ${LANG_FRENCH} "${PRODUCTNAME} est en cours d'exécution.$\nCliquez sur OK pour fermer l'application."
LangString chooseMaintenanceOption ${LANG_FRENCH} "Choisissez l'opération de maintenance à effectuer."
LangString chooseHowToInstall ${LANG_FRENCH} "Choisissez comment installer ${PRODUCTNAME}."
LangString choowHowToInstall ${LANG_FRENCH} "Choisissez comment installer ${PRODUCTNAME}."
LangString createDesktop ${LANG_FRENCH} "Créer un raccourci sur le bureau"
LangString dontUninstall ${LANG_FRENCH} "Ne pas désinstaller"
LangString dontUninstallDowngrade ${LANG_FRENCH} "Ne pas désinstaller (cet installateur ne permet pas de revenir à une version antérieure sans désinstaller)"
LangString failedToKillApp ${LANG_FRENCH} "La fermeture de ${PRODUCTNAME} a échoué. Veuillez fermer l'application et réessayer."
LangString installingWebview2 ${LANG_FRENCH} "Installation de WebView2..."
LangString newerVersionInstalled ${LANG_FRENCH} "Une version plus récente de ${PRODUCTNAME} est déjà installée. Il n'est pas recommandé d'installer une version antérieure. Si vous souhaitez vraiment installer cette version, il est conseillé de désinstaller d'abord la version actuelle. Sélectionnez l'opération que vous souhaitez effectuer, puis cliquez sur Suivant pour continuer."
LangString older ${LANG_FRENCH} "plus ancienne"
LangString olderOrUnknownVersionInstalled ${LANG_FRENCH} "Une version $R4 de ${PRODUCTNAME} est installée sur le système. Il est recommandé de désinstaller la version actuelle avant d'installer celle-ci. Sélectionnez l'opération que vous souhaitez effectuer, puis cliquez sur Suivant pour continuer."
LangString silentDowngrades ${LANG_FRENCH} "Cet installateur ne permet pas de revenir à une version antérieure. L'installation silencieuse ne peut pas continuer : veuillez utiliser l'installation avec interface graphique.$\n"
LangString unableToUninstall ${LANG_FRENCH} "Impossible de désinstaller le programme."
LangString uninstallApp ${LANG_FRENCH} "Désinstaller ${PRODUCTNAME}"
LangString uninstallBeforeInstalling ${LANG_FRENCH} "Désinstaller avant d'installer"
LangString unknown ${LANG_FRENCH} "inconnue"
LangString webview2AbortError ${LANG_FRENCH} "L'installation de WebView2 a échoué. L'application ne peut pas s'exécuter sans WebView2. Veuillez relancer l'installation."
LangString webview2DownloadError ${LANG_FRENCH} "Erreur : le téléchargement de WebView2 a échoué - $0"
LangString webview2DownloadSuccess ${LANG_FRENCH} "Le composant WebView2 a été téléchargé."
LangString webview2Downloading ${LANG_FRENCH} "Téléchargement du composant WebView2..."
LangString webview2InstallError ${LANG_FRENCH} "Erreur : l'installation de WebView2 a échoué avec le code d'erreur $1"
LangString webview2InstallSuccess ${LANG_FRENCH} "L'installation de WebView2 a réussi"
LangString deleteAppData ${LANG_FRENCH} "Supprimer les données de l'application"
