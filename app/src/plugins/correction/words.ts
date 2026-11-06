// What the correction bar says about a failed run: its own words for no model on the job (fixed on the Local AI page),
// the Test model words for the rest (`notAvailable` = no engine built for this computer; a missing one the run gets).

import type { ApiError } from "../../api";
import { t } from "../../core/i18n";
import { testErrorText } from "../local-ai/words";

export function failureText(error: ApiError): string {
  if (error.kind === "backend" && error.error.kind === "unavailable") {
    const { subject, detail } = error.error;
    if (subject === "model" && detail === "noModel") return t("correction.failed.noModel");
  }
  return testErrorText(error);
}
