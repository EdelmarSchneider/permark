; Tauri's own installer strings (the pages and messages of its NSIS template) in Indonesian, which Tauri does not
; translate: `bundle.windows.nsis.customLanguageFiles` in tauri.conf.json points here. Same names as Tauri's English.nsh
; (Tauri CLI 2.12.3; 2.12.4 renamed choowHowToInstall to chooseHowToInstall: both are here, same words). Tauri copies the file as it is and adds the byte order mark itself, so the file has none; the
; product name is ${PRODUCTNAME}.
LangString addOrReinstall ${LANG_INDONESIAN} "Tambah atau instal ulang komponen"
LangString alreadyInstalled ${LANG_INDONESIAN} "Sudah Terinstal"
LangString alreadyInstalledLong ${LANG_INDONESIAN} "${PRODUCTNAME} ${VERSION} sudah terinstal. Pilih tindakan yang ingin Anda lakukan, lalu klik Lanjut untuk melanjutkan."
LangString appRunning ${LANG_INDONESIAN} "${PRODUCTNAME} sedang berjalan. Tutup dulu, lalu coba lagi."
LangString appRunningOkKill ${LANG_INDONESIAN} "${PRODUCTNAME} sedang berjalan.$\nKlik OK untuk menutupnya."
LangString chooseMaintenanceOption ${LANG_INDONESIAN} "Pilih tindakan pemeliharaan yang ingin dilakukan."
LangString chooseHowToInstall ${LANG_INDONESIAN} "Pilih cara Anda ingin menginstal ${PRODUCTNAME}."
LangString choowHowToInstall ${LANG_INDONESIAN} "Pilih cara Anda ingin menginstal ${PRODUCTNAME}."
LangString createDesktop ${LANG_INDONESIAN} "Buat pintasan di desktop"
LangString dontUninstall ${LANG_INDONESIAN} "Jangan hapus instalasi"
LangString dontUninstallDowngrade ${LANG_INDONESIAN} "Jangan hapus instalasi (penginstal ini tidak dapat menginstal versi lebih lama tanpa menghapus versi saat ini)"
LangString failedToKillApp ${LANG_INDONESIAN} "Gagal menutup ${PRODUCTNAME}. Tutup dulu, lalu coba lagi."
LangString installingWebview2 ${LANG_INDONESIAN} "Menginstal WebView2..."
LangString newerVersionInstalled ${LANG_INDONESIAN} "Versi ${PRODUCTNAME} yang lebih baru sudah terinstal. Menginstal versi yang lebih lama tidak disarankan. Jika Anda benar-benar ingin menginstal versi lama ini, sebaiknya hapus dulu instalasi versi saat ini. Pilih tindakan yang ingin Anda lakukan, lalu klik Lanjut untuk melanjutkan."
LangString older ${LANG_INDONESIAN} "lebih lama"
LangString olderOrUnknownVersionInstalled ${LANG_INDONESIAN} "Versi $R4 dari ${PRODUCTNAME} terinstal di sistem Anda. Sebaiknya hapus instalasi versi saat ini sebelum menginstal. Pilih tindakan yang ingin Anda lakukan, lalu klik Lanjut untuk melanjutkan."
LangString silentDowngrades ${LANG_INDONESIAN} "Penginstal ini tidak dapat menginstal versi lebih lama. Instalasi senyap tidak dapat dilanjutkan; gunakan penginstal dengan antarmuka grafis.$\n"
LangString unableToUninstall ${LANG_INDONESIAN} "Tidak dapat menghapus instalasi."
LangString uninstallApp ${LANG_INDONESIAN} "Hapus ${PRODUCTNAME}"
LangString uninstallBeforeInstalling ${LANG_INDONESIAN} "Hapus instalasi sebelum menginstal"
LangString unknown ${LANG_INDONESIAN} "tidak dikenal"
LangString webview2AbortError ${LANG_INDONESIAN} "Gagal menginstal WebView2. Aplikasi tidak dapat berjalan tanpanya. Coba jalankan ulang penginstal."
LangString webview2DownloadError ${LANG_INDONESIAN} "Kesalahan: gagal mengunduh WebView2 - $0"
LangString webview2DownloadSuccess ${LANG_INDONESIAN} "Program awal WebView2 berhasil diunduh"
LangString webview2Downloading ${LANG_INDONESIAN} "Mengunduh program awal WebView2..."
LangString webview2InstallError ${LANG_INDONESIAN} "Kesalahan: instalasi WebView2 gagal dengan kode keluar $1"
LangString webview2InstallSuccess ${LANG_INDONESIAN} "WebView2 berhasil diinstal"
LangString deleteAppData ${LANG_INDONESIAN} "Hapus data aplikasi"

; Stock NSIS texts of this language's pages, in other words: NSIS' Indonesian file writes "di instal" apart, "anda" in
; lower case where the texts above say "Anda", and the archaic "sudah paripurna". Tauri includes this file after the
; language is inserted, so these later texts win (makensis warns 6030 for each Modern UI one).
LangString ^DirText ${LANG_INDONESIAN} "Program $(^NameDA) akan diinstal di lokasi berikut. Untuk memilih lokasi lain, tekan tombol Cari, lalu pilih lokasi yang Anda kehendaki. $_CLICK"
LangString MUI_TEXT_FINISH_SUBTITLE ${LANG_INDONESIAN} "Proses instalasi berhasil diselesaikan."
LangString MUI_TEXT_FINISH_INFO_TEXT ${LANG_INDONESIAN} "$(^NameDA) sudah diinstal di komputer Anda.$\r$\n$\r$\nTekan tombol Selesai untuk menutup program ini."
LangString MUI_UNTEXT_CONFIRM_SUBTITLE ${LANG_INDONESIAN} "Menghapus $(^NameDA) dari komputer Anda."
LangString MUI_UNTEXT_FINISH_SUBTITLE ${LANG_INDONESIAN} "Proses penghapusan berhasil diselesaikan."
