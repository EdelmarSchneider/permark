; Tauri's own installer strings (the pages and messages of its NSIS template) in Ukrainian, in place of Tauri's file:
; `bundle.windows.nsis.customLanguageFiles` in tauri.conf.json points here. Tauri's file has literal {{product_name}}
; placeholders it leaves unfilled, exclamation marks and "Додати/Перевстановити"; these are plain sentences. Same names
; as Tauri's English.nsh (Tauri CLI 2.12.3; 2.12.4 renamed choowHowToInstall to chooseHowToInstall: both are here, same
; words). Tauri copies the file as it is and adds the byte order mark itself, so the file has none; the product name is
; ${PRODUCTNAME}. `older` and `unknown` fill "$R4 версію" (feminine accusative, like "версію").
LangString addOrReinstall ${LANG_UKRAINIAN} "Додати або перевстановити компоненти"
LangString alreadyInstalled ${LANG_UKRAINIAN} "Уже встановлено"
LangString alreadyInstalledLong ${LANG_UKRAINIAN} "${PRODUCTNAME} ${VERSION} уже встановлено. Виберіть дію, яку потрібно виконати, і натисніть Далі, щоб продовжити."
LangString appRunning ${LANG_UKRAINIAN} "${PRODUCTNAME} запущено. Спершу закрийте програму, а потім спробуйте ще раз."
LangString appRunningOkKill ${LANG_UKRAINIAN} "${PRODUCTNAME} запущено.$\nНатисніть OK, щоб закрити програму."
LangString chooseMaintenanceOption ${LANG_UKRAINIAN} "Виберіть дію, яку потрібно виконати."
LangString chooseHowToInstall ${LANG_UKRAINIAN} "Виберіть спосіб встановлення ${PRODUCTNAME}."
LangString choowHowToInstall ${LANG_UKRAINIAN} "Виберіть спосіб встановлення ${PRODUCTNAME}."
LangString createDesktop ${LANG_UKRAINIAN} "Створити ярлик на робочому столі"
LangString dontUninstall ${LANG_UKRAINIAN} "Не видаляти"
LangString dontUninstallDowngrade ${LANG_UKRAINIAN} "Не видаляти (ця програма встановлення не може встановити старішу версію, не видаливши поточну)"
LangString failedToKillApp ${LANG_UKRAINIAN} "Не вдалося закрити ${PRODUCTNAME}. Спершу закрийте програму, а потім спробуйте ще раз."
LangString installingWebview2 ${LANG_UKRAINIAN} "Встановлення WebView2..."
LangString newerVersionInstalled ${LANG_UKRAINIAN} "Уже встановлено новішу версію ${PRODUCTNAME}. Встановлювати старішу версію не рекомендується. Якщо ви справді хочете встановити цю старішу версію, спершу видаліть поточну. Виберіть дію, яку потрібно виконати, і натисніть Далі, щоб продовжити."
LangString older ${LANG_UKRAINIAN} "старішу"
LangString olderOrUnknownVersionInstalled ${LANG_UKRAINIAN} "У вашій системі встановлено $R4 версію ${PRODUCTNAME}. Перед встановленням рекомендується видалити поточну версію. Виберіть дію, яку потрібно виконати, і натисніть Далі, щоб продовжити."
LangString silentDowngrades ${LANG_UKRAINIAN} "Ця програма встановлення не може встановити старішу версію. Тихе встановлення неможливо продовжити; скористайтеся програмою встановлення з графічним інтерфейсом.$\n"
LangString unableToUninstall ${LANG_UKRAINIAN} "Не вдалося видалити."
LangString uninstallApp ${LANG_UKRAINIAN} "Видалити ${PRODUCTNAME}"
LangString uninstallBeforeInstalling ${LANG_UKRAINIAN} "Видалити перед встановленням"
LangString unknown ${LANG_UKRAINIAN} "невідому"
LangString webview2AbortError ${LANG_UKRAINIAN} "Не вдалося встановити WebView2. Без нього програма не може працювати. Спробуйте перезапустити програму встановлення."
LangString webview2DownloadError ${LANG_UKRAINIAN} "Помилка: не вдалося завантажити WebView2 - $0"
LangString webview2DownloadSuccess ${LANG_UKRAINIAN} "Інсталятор WebView2 успішно завантажено"
LangString webview2Downloading ${LANG_UKRAINIAN} "Завантаження інсталятора WebView2..."
LangString webview2InstallError ${LANG_UKRAINIAN} "Помилка: не вдалося встановити WebView2, код виходу $1"
LangString webview2InstallSuccess ${LANG_UKRAINIAN} "WebView2 успішно встановлено"
LangString deleteAppData ${LANG_UKRAINIAN} "Видалити дані програми"
