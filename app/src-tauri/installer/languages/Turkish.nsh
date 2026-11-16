; Tauri's own installer strings (the pages and messages of its NSIS template) in Turkish, in place of Tauri's file:
; `bundle.windows.nsis.customLanguageFiles` in tauri.conf.json points here. Tauri's words, with the literal
; {{product_name}} placeholders it leaves unfilled replaced, and "${PRODUCTNAME}'i kaldır" (a suffix that fits only
; some names) written without a suffix. Same names as Tauri's English.nsh (Tauri CLI 2.12.3; 2.12.4 renamed
; choowHowToInstall to chooseHowToInstall: both are here, same words). Tauri copies the file as it is and adds the byte
; order mark itself, so the file has none; the product name is ${PRODUCTNAME}.
LangString addOrReinstall ${LANG_TURKISH} "Bileşen Ekle/Yeniden Yükle"
LangString alreadyInstalled ${LANG_TURKISH} "Daha Önceden Yüklenmiş"
LangString alreadyInstalledLong ${LANG_TURKISH} "${PRODUCTNAME} ${VERSION} daha önceden yüklenmiş. Gerçekleştirmek istediğiniz işlemi seçin ve devam etmek için İleri'ye tıklayın."
LangString appRunning ${LANG_TURKISH} "${PRODUCTNAME} çalışır durumda. Lütfen önce uygulamayı kapatın ve sonra tekrar deneyin."
LangString appRunningOkKill ${LANG_TURKISH} "${PRODUCTNAME} çalışır durumda.$\nUygulamayı sonlandırmak için Tamam'a tıklayın."
LangString chooseMaintenanceOption ${LANG_TURKISH} "Gerçekleştirmek istediğiniz bakım seçeneğini belirleyin."
LangString chooseHowToInstall ${LANG_TURKISH} "${PRODUCTNAME} uygulamasını nasıl yüklemek istediğinizi seçin."
LangString choowHowToInstall ${LANG_TURKISH} "${PRODUCTNAME} uygulamasını nasıl yüklemek istediğinizi seçin."
LangString createDesktop ${LANG_TURKISH} "Masaüstü kısayolu oluştur"
LangString dontUninstall ${LANG_TURKISH} "Kaldırma işlemini gerçekleştirme"
LangString dontUninstallDowngrade ${LANG_TURKISH} "Kaldırma işlemini gerçekleştirme (Kaldırma işlemi yapmadan sürüm düşürme bu yükleyici için devre dışı bırakılmıştır)"
LangString failedToKillApp ${LANG_TURKISH} "${PRODUCTNAME} sonlandırılamadı. Lütfen önce kapatın, sonra tekrar deneyin."
LangString installingWebview2 ${LANG_TURKISH} "WebView2 yükleniyor..."
LangString newerVersionInstalled ${LANG_TURKISH} "${PRODUCTNAME} uygulamasının daha yeni bir sürümü zaten yüklü. Daha eski bir sürümü yüklemeniz önerilmez. Bu eski sürümü gerçekten yüklemek istiyorsanız, önce mevcut sürümü kaldırmanız daha uygundur. Gerçekleştirmek istediğiniz işlemi seçin ve devam etmek için İleri'ye tıklayın."
LangString older ${LANG_TURKISH} "daha eski"
LangString olderOrUnknownVersionInstalled ${LANG_TURKISH} "Sisteminizde ${PRODUCTNAME} uygulamasının $R4 sürümü yüklü. Yükleme işleminden önce mevcut sürümü kaldırmanız önerilir. Gerçekleştirmek istediğiniz işlemi seçin ve devam etmek için İleri'ye tıklayın."
LangString silentDowngrades ${LANG_TURKISH} "Bu yükleyici için sürüm düşürme işlemleri devre dışı bırakıldı, sessiz yükleyici ile devam edilemiyor, lütfen bunun yerine grafik arayüz yükleyicisini kullanın.$\n"
LangString unableToUninstall ${LANG_TURKISH} "Kaldırma işlemi gerçekleştirilemiyor."
LangString uninstallApp ${LANG_TURKISH} "${PRODUCTNAME} uygulamasını kaldır"
LangString uninstallBeforeInstalling ${LANG_TURKISH} "Yükleme yapmadan önce kaldırın"
LangString unknown ${LANG_TURKISH} "bilinmeyen"
LangString webview2AbortError ${LANG_TURKISH} "WebView2 yüklenemedi. Uygulama bu bileşen olmadan çalışamaz. Yükleyiciyi yeniden başlatmayı deneyin."
LangString webview2DownloadError ${LANG_TURKISH} "Hata: WebView2 indirilemedi - $0"
LangString webview2DownloadSuccess ${LANG_TURKISH} "WebView2 önyükleyicisi başarıyla indirildi"
LangString webview2Downloading ${LANG_TURKISH} "WebView2 önyükleyicisi indiriliyor..."
LangString webview2InstallError ${LANG_TURKISH} "Hata: WebView2 yüklemesi $1 hata koduyla başarısız oldu."
LangString webview2InstallSuccess ${LANG_TURKISH} "WebView2 başarıyla yüklendi"
LangString deleteAppData ${LANG_TURKISH} "Uygulama verilerini sil"

; Stock NSIS texts of this language's pages, in other words: the finish text has a double space and lacks the dotless ı
; ("bilgisayariniza"); the directory subtitle and the uninstall progress page have typos ("şeçiniz", "Litfen",
; "Tamamlandır"). Tauri includes this file after the language is inserted, so these later texts win (makensis warns
; 6030 for each Modern UI one).
LangString MUI_TEXT_DIRECTORY_SUBTITLE ${LANG_TURKISH} "$(^NameDA) programını kurmak istediğiniz dizini seçiniz."
LangString MUI_TEXT_FINISH_INFO_TEXT ${LANG_TURKISH} "$(^NameDA) bilgisayarınıza yüklendi.$\r$\n$\r$\nKurulumu sonlandırmak için 'Bitir'e basın."
LangString MUI_UNTEXT_UNINSTALLING_SUBTITLE ${LANG_TURKISH} "Lütfen $(^NameDA) programı sisteminizden kaldırılırken bekleyiniz."
LangString MUI_UNTEXT_FINISH_TITLE ${LANG_TURKISH} "Kaldırma İşlemi Tamamlandı"
