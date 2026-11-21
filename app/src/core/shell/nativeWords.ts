// Words of the app's own system dialogs (a page that does not answer, the system web view that stopped): the backend
// draws them but holds no translations, so each view sends them in the interface language.

import type { NativeWords } from "../../api";
import { t } from "../i18n";

/** The system dialogs' words in the interface language (reads it: a language change gives new words). Every field,
 *  the optional ones too: one left out stays English in its dialog. */
export function nativeWords(): Required<NativeWords> {
  return {
    notResponding: t("stuck.title"),
    notRespondingDetail: t("stuck.detail"),
    closeAnyway: t("stuck.closeAnyway"),
    wait: t("stuck.wait"),
    engineStopped: t("stuck.engineStopped"),
    windowFailed: t("stuck.windowFailed"),
    viewFailedExit: t("stuck.viewFailedExit"),
  };
}
