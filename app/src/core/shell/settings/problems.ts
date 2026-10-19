// A refused setting in words: the backend's reason code -> one plain sentence.

import type { ApiError } from "../../../api";
import type { MessageKey } from "../../i18n";

/** The reason code the backend gave (`tooLong`, `unbalancedQuote`, ...); `other` when it gave none. */
export function reasonCode(error: ApiError): string {
  return error.kind === "backend" ? (error.error.detail ?? "other") : "other";
}

export function problemKey(code: string): MessageKey {
  switch (code) {
    case "tooLong":
      return "settings.problem.tooLong";
    case "tooMany":
      return "settings.problem.tooMany";
    case "empty":
      return "settings.problem.empty";
    case "control":
      return "settings.problem.control";
    case "group":
      return "settings.problem.group";
    case "link":
      return "settings.problem.link";
    case "folder":
      return "settings.problem.folder";
    case "unbalancedQuote":
      return "settings.problem.unbalancedQuote";
    case "programPlaceholder":
      return "settings.problem.programPlaceholder";
    case "unknownPlaceholder":
      return "settings.problem.unknownPlaceholder";
    case "noPath":
      return "settings.problem.noPath";
    case "notInitialized":
      return "settings.problem.notInitialized";
    default:
      return "settings.problem.other";
  }
}
