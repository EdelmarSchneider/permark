// The state of the in-app updater as a pure state machine. The store (`stores/updates.svelte.ts`) feeds it
// the outcome of each step and the UI renders the resulting phase; nothing here touches Tauri.
//
//   idle -> checking -> available -> downloading -> installing
//              |            ^             |
//              |            +-- (failed) -+
//              +-> upToDate / failed (only after a manual check) -> idle
//
// A check that the app starts by itself is silent: no update and network errors both end in `idle`.

import { t } from "./i18n";

export type UpdateState =
  | { phase: "idle" }
  | { phase: "checking"; manual: boolean }
  /** A manual check found nothing newer. */
  | { phase: "upToDate" }
  /** A manual check could not reach the update server. */
  | { phase: "failed"; message: string }
  /** `error` is set when the last attempt to install this update failed. */
  | { phase: "available"; version: string; error?: string }
  | {
      phase: "downloading";
      version: string;
      downloaded: number;
      /** Size of the download in bytes, once the server told us. */
      total: number | null;
    }
  | { phase: "installing"; version: string };

export type UpdateEvent =
  | { type: "checkStarted"; manual: boolean }
  | { type: "found"; version: string }
  | { type: "none" }
  | { type: "checkFailed"; message: string }
  | { type: "installRequested" }
  | { type: "downloadStarted"; total: number | null }
  | { type: "downloadProgress"; chunk: number }
  | { type: "downloadFinished" }
  | { type: "installFailed"; message: string }
  /** The "up to date" or "failed" message was seen (the settings popover closed). */
  | { type: "dismissResult" };

/** Progress of the download, as the platform layer reports it. */
export type DownloadProgress =
  | { kind: "started"; total: number | null }
  | { kind: "chunk"; bytes: number }
  | { kind: "finished" };

export const INITIAL_UPDATE_STATE: UpdateState = { phase: "idle" };

/** Applies one event. An event that makes no sense in the current phase (a late result) returns the same state. */
export function reduceUpdate(state: UpdateState, event: UpdateEvent): UpdateState {
  switch (event.type) {
    case "checkStarted":
      return state.phase === "idle" || state.phase === "upToDate" || state.phase === "failed"
        ? { phase: "checking", manual: event.manual }
        : state;
    case "found":
      return state.phase === "checking" ? { phase: "available", version: event.version } : state;
    case "none":
      return state.phase === "checking"
        ? state.manual
          ? { phase: "upToDate" }
          : { phase: "idle" }
        : state;
    case "checkFailed":
      return state.phase === "checking"
        ? state.manual
          ? { phase: "failed", message: event.message }
          : { phase: "idle" }
        : state;
    case "installRequested":
      return state.phase === "available"
        ? { phase: "downloading", version: state.version, downloaded: 0, total: null }
        : state;
    case "downloadStarted":
      return state.phase === "downloading" ? { ...state, total: event.total } : state;
    case "downloadProgress":
      return state.phase === "downloading"
        ? { ...state, downloaded: state.downloaded + event.chunk }
        : state;
    case "downloadFinished":
      return state.phase === "downloading"
        ? { phase: "installing", version: state.version }
        : state;
    case "installFailed":
      return state.phase === "downloading" || state.phase === "installing"
        ? {
            phase: "available",
            version: state.version,
            error: event.message,
          }
        : state;
    case "dismissResult":
      return state.phase === "upToDate" || state.phase === "failed" ? { phase: "idle" } : state;
  }
}

/** Download progress from 0 to 100, or null while the size is unknown. */
export function updatePercent(state: UpdateState): number | null {
  if (state.phase !== "downloading" || state.total === null || state.total <= 0) return null;
  return Math.min(100, Math.floor((state.downloaded / state.total) * 100));
}

/** A check, download or install is running. */
export function isBusy(state: UpdateState): boolean {
  return (
    state.phase === "checking" || state.phase === "downloading" || state.phase === "installing"
  );
}

/** One line telling where the updater stands ("Update available: v0.2.0"); empty while there is nothing to say. */
export function describeUpdate(state: UpdateState): string {
  switch (state.phase) {
    case "idle":
      return "";
    case "checking":
      return t("update.checking");
    case "upToDate":
      return t("update.upToDate");
    case "failed":
      return t("update.checkFailed");
    case "available":
      return t("update.available", { version: state.version });
    case "downloading": {
      const percent = updatePercent(state);
      return percent === null
        ? t("update.downloading", { version: state.version })
        : t("update.downloadingPercent", { version: state.version, percent });
    }
    case "installing":
      return t("update.installing", { version: state.version });
  }
}
