; Tauri's own installer strings (the pages and messages of its NSIS template) in German, in place of Tauri's German file:
; `bundle.windows.nsis.customLanguageFiles` in tauri.conf.json points here. Tauri's file (CLI 2.12.3) lacks "older" and
; "unknown" (it names the second "unbekannt"), so the reinstall page read "Eine -Version von ..."; it also has English
; messages, literal {{product_name}} placeholders and informal "du" labels. Same names as Tauri's English.nsh
; (2.12.4 renamed choowHowToInstall to chooseHowToInstall: both are here, same words). Tauri copies the file as it is and
; adds the byte order mark itself, so the file has none; the product name is ${PRODUCTNAME}.
LangString addOrReinstall ${LANG_GERMAN} "Komponenten hinzufügen oder neu installieren"
LangString alreadyInstalled ${LANG_GERMAN} "Bereits installiert"
LangString alreadyInstalledLong ${LANG_GERMAN} "${PRODUCTNAME} ${VERSION} ist bereits installiert. Wählen Sie den gewünschten Vorgang aus und klicken Sie auf Weiter, um fortzufahren."
LangString appRunning ${LANG_GERMAN} "${PRODUCTNAME} wird ausgeführt. Schließen Sie es zuerst und versuchen Sie es dann erneut."
LangString appRunningOkKill ${LANG_GERMAN} "${PRODUCTNAME} wird ausgeführt.$\nKlicken Sie auf OK, um es zu beenden."
LangString chooseMaintenanceOption ${LANG_GERMAN} "Wählen Sie die auszuführende Wartungsoption."
LangString chooseHowToInstall ${LANG_GERMAN} "Wählen Sie, wie Sie ${PRODUCTNAME} installieren möchten."
LangString choowHowToInstall ${LANG_GERMAN} "Wählen Sie, wie Sie ${PRODUCTNAME} installieren möchten."
LangString createDesktop ${LANG_GERMAN} "Desktop-Verknüpfung erstellen"
LangString dontUninstall ${LANG_GERMAN} "Nicht deinstallieren"
LangString dontUninstallDowngrade ${LANG_GERMAN} "Nicht deinstallieren (ein Downgrade ohne Deinstallation ist bei diesem Installationsprogramm deaktiviert)"
LangString failedToKillApp ${LANG_GERMAN} "${PRODUCTNAME} konnte nicht beendet werden. Schließen Sie es zuerst und versuchen Sie es dann erneut."
LangString installingWebview2 ${LANG_GERMAN} "WebView2 wird installiert..."
LangString newerVersionInstalled ${LANG_GERMAN} "Eine neuere Version von ${PRODUCTNAME} ist bereits installiert. Es wird nicht empfohlen, eine ältere Version zu installieren. Wenn Sie diese ältere Version wirklich installieren möchten, deinstallieren Sie besser zuerst die aktuelle Version. Wählen Sie den gewünschten Vorgang aus und klicken Sie auf Weiter, um fortzufahren."
LangString older ${LANG_GERMAN} "ältere"
LangString olderOrUnknownVersionInstalled ${LANG_GERMAN} "Auf Ihrem System ist eine $R4 Version von ${PRODUCTNAME} installiert. Es wird empfohlen, die aktuelle Version vor der Installation zu deinstallieren. Wählen Sie den gewünschten Vorgang aus und klicken Sie auf Weiter, um fortzufahren."
LangString silentDowngrades ${LANG_GERMAN} "Downgrades sind bei diesem Installationsprogramm deaktiviert. Die unbeaufsichtigte Installation kann nicht fortgesetzt werden; verwenden Sie stattdessen das Installationsprogramm mit grafischer Oberfläche.$\n"
LangString unableToUninstall ${LANG_GERMAN} "Die Deinstallation ist fehlgeschlagen."
LangString uninstallApp ${LANG_GERMAN} "${PRODUCTNAME} deinstallieren"
LangString uninstallBeforeInstalling ${LANG_GERMAN} "Vor der Installation deinstallieren"
LangString unknown ${LANG_GERMAN} "unbekannte"
LangString webview2AbortError ${LANG_GERMAN} "Die Installation von WebView2 ist fehlgeschlagen. Die Anwendung kann ohne WebView2 nicht ausgeführt werden. Starten Sie das Installationsprogramm erneut."
LangString webview2DownloadError ${LANG_GERMAN} "Fehler: Herunterladen von WebView2 fehlgeschlagen - $0"
LangString webview2DownloadSuccess ${LANG_GERMAN} "WebView2-Bootstrapper erfolgreich heruntergeladen"
LangString webview2Downloading ${LANG_GERMAN} "WebView2-Bootstrapper wird heruntergeladen..."
LangString webview2InstallError ${LANG_GERMAN} "Fehler: Die Installation von WebView2 ist mit dem Exit-Code $1 fehlgeschlagen"
LangString webview2InstallSuccess ${LANG_GERMAN} "WebView2 erfolgreich installiert"
LangString deleteAppData ${LANG_GERMAN} "Anwendungsdaten löschen"
