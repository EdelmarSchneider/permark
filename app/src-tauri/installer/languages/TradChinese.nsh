; Tauri's own installer strings (the pages and messages of its NSIS template) in Traditional Chinese, in place of
; Tauri's file: `bundle.windows.nsis.customLanguageFiles` in tauri.conf.json points here. Tauri's file (CLI 2.12.3)
; reads "的 舊版 版本", uses the Mainland 數據 for data, says 你 where PerMark says 您, and has literal {{product_name}}
; placeholders. Same names as Tauri's English.nsh (2.12.4 renamed choowHowToInstall to chooseHowToInstall: both are
; here, same words). Tauri copies the file as it is and adds the byte order mark itself, so the file has none; the
; product name is ${PRODUCTNAME}.
LangString addOrReinstall ${LANG_TRADCHINESE} "新增或重新安裝元件"
LangString alreadyInstalled ${LANG_TRADCHINESE} "已安裝"
LangString alreadyInstalledLong ${LANG_TRADCHINESE} "${PRODUCTNAME} ${VERSION} 已經安裝。請選擇您要進行的操作，然後按「下一步」繼續。"
LangString appRunning ${LANG_TRADCHINESE} "${PRODUCTNAME} 正在執行中。請先關閉它，然後再試一次。"
LangString appRunningOkKill ${LANG_TRADCHINESE} "${PRODUCTNAME} 正在執行中。$\n按「確定」即可結束它。"
LangString chooseMaintenanceOption ${LANG_TRADCHINESE} "請選擇您要進行的維護選項。"
LangString chooseHowToInstall ${LANG_TRADCHINESE} "請選擇您要如何安裝 ${PRODUCTNAME}。"
LangString choowHowToInstall ${LANG_TRADCHINESE} "請選擇您要如何安裝 ${PRODUCTNAME}。"
LangString createDesktop ${LANG_TRADCHINESE} "建立桌面捷徑"
LangString dontUninstall ${LANG_TRADCHINESE} "不要解除安裝"
LangString dontUninstallDowngrade ${LANG_TRADCHINESE} "不要解除安裝（本安裝程式不允許未解除安裝就降級）"
LangString failedToKillApp ${LANG_TRADCHINESE} "無法結束 ${PRODUCTNAME}。請先關閉它，然後再試一次。"
LangString installingWebview2 ${LANG_TRADCHINESE} "正在安裝 WebView2..."
LangString newerVersionInstalled ${LANG_TRADCHINESE} "已安裝較新版本的 ${PRODUCTNAME}。不建議安裝舊版。如果您確實要安裝這個舊版，最好先解除安裝目前的版本。請選擇您要進行的操作，然後按「下一步」繼續。"
LangString older ${LANG_TRADCHINESE} "較舊"
LangString olderOrUnknownVersionInstalled ${LANG_TRADCHINESE} "您的系統已安裝$R4版本的 ${PRODUCTNAME}。建議您先解除安裝目前的版本，再進行安裝。請選擇您要進行的操作，然後按「下一步」繼續。"
LangString silentDowngrades ${LANG_TRADCHINESE} "本安裝程式不允許降級，因此無法繼續無訊息安裝。請改用圖形介面安裝程式。$\n"
LangString unableToUninstall ${LANG_TRADCHINESE} "無法解除安裝。"
LangString uninstallApp ${LANG_TRADCHINESE} "解除安裝 ${PRODUCTNAME}"
LangString uninstallBeforeInstalling ${LANG_TRADCHINESE} "安裝前先解除安裝"
LangString unknown ${LANG_TRADCHINESE} "未知"
LangString webview2AbortError ${LANG_TRADCHINESE} "無法安裝 WebView2。這個應用程式需要 WebView2 才能執行。請重新啟動安裝程式。"
LangString webview2DownloadError ${LANG_TRADCHINESE} "錯誤：WebView2 下載失敗 - $0"
LangString webview2DownloadSuccess ${LANG_TRADCHINESE} "WebView2 啟動載入器下載成功"
LangString webview2Downloading ${LANG_TRADCHINESE} "正在下載 WebView2 啟動載入器..."
LangString webview2InstallError ${LANG_TRADCHINESE} "錯誤：WebView2 安裝失敗，錯誤碼 $1"
LangString webview2InstallSuccess ${LANG_TRADCHINESE} "WebView2 安裝成功"
LangString deleteAppData ${LANG_TRADCHINESE} "刪除應用程式資料"
