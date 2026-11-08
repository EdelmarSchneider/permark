// Errors in words: backend errors by kind (`error.<kind>`), anything else as "something went wrong".

import type { ApiError, IpcError } from "../api";
import type { ErrorKind } from "../api/generated/ErrorKind";
import { t, type MessageKey } from "./i18n";

export function errorKey(kind: ErrorKind): MessageKey {
  switch (kind) {
    case "notFound":
      return "error.notFound";
    case "pathEscape":
      return "error.pathEscape";
    case "invalidPath":
      return "error.invalidPath";
    case "denied":
      return "error.denied";
    case "io":
      return "error.io";
    case "exists":
      return "error.exists";
    case "binary":
      return "error.binary";
    case "tooLarge":
      return "error.tooLarge";
    case "encoding":
      return "error.encoding";
    case "unsupported":
      return "error.unsupported";
    case "invalidData":
      return "error.invalidData";
    case "conflict":
      return "error.conflict";
    case "unavailable":
      return "error.unavailable";
    case "superseded":
      return "error.superseded";
    case "internal":
      return "error.internal";
    case "invalidName":
      return "error.invalidName";
    case "intoItself":
      return "error.intoItself";
    case "otherRoot":
      return "error.otherRoot";
    case "protected":
      return "error.protected";
    case "overlap":
      return "error.overlap";
    case "trash":
      return "error.trash";
    case "partlyMoved":
      return "error.partlyMoved";
    case "editorNotSet":
      return "error.editorNotSet";
    case "editorFailed":
      return "error.editorFailed";
    case "settingsUnreadable":
      return "error.settingsUnreadable";
    default:
      return kind satisfies never;
  }
}

/** Why a settings file can't be read (`detail` of `settingsUnreadable`, mirror of Rust `settings::Unreadable::code`);
 *  a reason not known here falls back to the kind's own words. */
function settingsUnreadableKey(detail: string | null): MessageKey {
  switch (detail) {
    case "notText":
      return "error.settingsUnreadable.notText";
    case "tooLarge":
      return "error.settingsUnreadable.tooLarge";
    case "locked":
      return "error.settingsUnreadable.locked";
    default:
      return "error.settingsUnreadable";
  }
}

export function ipcErrorText(error: IpcError): string {
  const key =
    error.kind === "settingsUnreadable"
      ? settingsUnreadableKey(error.detail)
      : errorKey(error.kind);
  return t(key, { subject: error.subject ?? "", detail: error.detail ?? "" });
}

export function apiErrorText(error: ApiError): string {
  switch (error.kind) {
    case "backend":
      return ipcErrorText(error.error);
    case "command":
    case "window":
      return t("error.unknown", { message: error.message });
    default:
      return error satisfies never;
  }
}
