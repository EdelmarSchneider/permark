// What the correction bar says about a failed run: its own words for the two causes the person fixes on the Local AI
// page (no model for the job; engine not on this computer yet: Test there gets it), else the Test model words.

import type { ApiError } from "../../api";
import { t } from "../../core/i18n";
import { testErrorText } from "../local-ai/words";

export function failureText(error: ApiError): string {
  if (error.kind === "backend" && error.error.kind === "unavailable") {
    const { subject, detail } = error.error;
    if (subject === "model" && detail === "noModel") return t("correction.failed.noModel");
    if (subject === "engine" && detail === "notAvailable") return t("correction.failed.noEngine");
  }
  return testErrorText(error);
}
