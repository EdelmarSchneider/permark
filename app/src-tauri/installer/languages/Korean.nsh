; Tauri's own installer strings (the pages and messages of its NSIS template) in Korean, in place of Tauri's file:
; `bundle.windows.nsis.customLanguageFiles` in tauri.conf.json points here. Tauri's file (CLI 2.12.3) ends the reinstall
; subtitle with "..", reads "알 수 없음 버전" when the installed version is unknown, and has literal {{product_name}}
; placeholders. Same names as Tauri's English.nsh (2.12.4 renamed choowHowToInstall to chooseHowToInstall: both are
; here, same words). Tauri copies the file as it is and adds the byte order mark itself, so the file has none; the
; product name is ${PRODUCTNAME}.
LangString addOrReinstall ${LANG_KOREAN} "구성 요소 추가 및 재설치"
LangString alreadyInstalled ${LANG_KOREAN} "이미 설치되어 있습니다"
LangString alreadyInstalledLong ${LANG_KOREAN} "${PRODUCTNAME} ${VERSION}이(가) 이미 설치되어 있습니다. 수행하려는 작업을 선택하고 '다음'을 클릭하여 계속하세요."
LangString appRunning ${LANG_KOREAN} "${PRODUCTNAME}이(가) 실행 중입니다. 먼저 닫은 후 다시 시도하세요."
LangString appRunningOkKill ${LANG_KOREAN} "${PRODUCTNAME}이(가) 실행 중입니다.$\n'확인'을 누르면 프로그램을 종료합니다."
LangString chooseMaintenanceOption ${LANG_KOREAN} "수행하려는 관리 옵션을 선택하세요."
LangString chooseHowToInstall ${LANG_KOREAN} "${PRODUCTNAME}의 설치 방법을 선택하세요."
LangString choowHowToInstall ${LANG_KOREAN} "${PRODUCTNAME}의 설치 방법을 선택하세요."
LangString createDesktop ${LANG_KOREAN} "바탕 화면 바로 가기 만들기"
LangString dontUninstall ${LANG_KOREAN} "제거하지 않기"
LangString dontUninstallDowngrade ${LANG_KOREAN} "제거하지 않기 (이 설치 프로그램에서는 제거하지 않고 다운그레이드할 수 없습니다)"
LangString failedToKillApp ${LANG_KOREAN} "${PRODUCTNAME}을(를) 종료하지 못했습니다. 먼저 닫은 후 다시 시도하세요."
LangString installingWebview2 ${LANG_KOREAN} "WebView2를 설치하는 중입니다..."
LangString newerVersionInstalled ${LANG_KOREAN} "${PRODUCTNAME}의 최신 버전이 이미 설치되어 있습니다. 이전 버전은 설치하지 않는 것이 좋습니다. 이 이전 버전을 꼭 설치하려면 먼저 현재 버전을 제거하는 것이 좋습니다. 수행하려는 작업을 선택하고 '다음'을 클릭하여 계속하세요."
LangString older ${LANG_KOREAN} "이전"
LangString olderOrUnknownVersionInstalled ${LANG_KOREAN} "시스템에 ${PRODUCTNAME}의 $R4 버전이 설치되어 있습니다. 설치하기 전에 현재 버전을 제거하는 것이 좋습니다. 수행하려는 작업을 선택하고 '다음'을 클릭하여 계속하세요."
LangString silentDowngrades ${LANG_KOREAN} "이 설치 프로그램에서는 다운그레이드할 수 없으므로 자동 설치를 계속할 수 없습니다. 대신 그래픽 인터페이스 설치 프로그램을 사용하세요.$\n"
LangString unableToUninstall ${LANG_KOREAN} "제거할 수 없습니다."
LangString uninstallApp ${LANG_KOREAN} "${PRODUCTNAME} 제거하기"
LangString uninstallBeforeInstalling ${LANG_KOREAN} "설치하기 전에 제거하기"
LangString unknown ${LANG_KOREAN} "알 수 없는"
LangString webview2AbortError ${LANG_KOREAN} "WebView2를 설치하지 못했습니다. WebView2가 없으면 앱을 실행할 수 없습니다. 설치 프로그램을 다시 시작해 보세요."
LangString webview2DownloadError ${LANG_KOREAN} "오류: WebView2를 다운로드하지 못했습니다 - $0"
LangString webview2DownloadSuccess ${LANG_KOREAN} "WebView2 부트스트래퍼를 다운로드했습니다."
LangString webview2Downloading ${LANG_KOREAN} "WebView2 부트스트래퍼를 다운로드하는 중..."
LangString webview2InstallError ${LANG_KOREAN} "오류: WebView2를 설치하지 못했습니다(종료 코드 $1)."
LangString webview2InstallSuccess ${LANG_KOREAN} "WebView2를 설치했습니다."
LangString deleteAppData ${LANG_KOREAN} "애플리케이션 데이터 삭제하기"
