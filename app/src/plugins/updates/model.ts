// What the updater's state means on screen: the status-bar chip (only when something waits for the person) and the
// line of the settings card. Pure: the store feeds it the state the backend announces.

import type { UpdateState } from "../../api";
import { t } from "../../core/i18n";

/** The chip of the status bar; `null` = no chip (nothing waits for the person). */
export type Chip = { tone: "info" | "busy" | "ready" | "problem"; text: string };

export function chipOf(state: UpdateState): Chip | null {
  switch (state.state) {
    case "idle":
    case "checking":
    case "upToDate":
      return null;
    case "available":
      return { tone: "info", text: t("updates.chip.available", { version: state.version }) };
    case "downloading":
      return {
        tone: "busy",
        text:
          state.percent === null
            ? t("updates.chip.downloading")
            : t("updates.chip.downloadingPercent", { percent: state.percent }),
      };
    case "ready":
      return { tone: "ready", text: t("updates.chip.ready") };
    case "error":
      return { tone: "problem", text: t("updates.chip.error") };
    default:
      return state satisfies never;
  }
}

/** One sentence about where the updater stands. */
export function statusLine(state: UpdateState): string {
  switch (state.state) {
    case "idle":
      return t("updates.status.idle");
    case "checking":
      return t("updates.status.checking");
    case "upToDate":
      return t("updates.status.upToDate");
    case "available":
      return t("updates.status.available", { version: state.version });
    case "downloading":
      return state.percent === null
        ? t("updates.status.downloading", { version: state.version })
        : t("updates.status.downloadingPercent", {
            version: state.version,
            percent: state.percent,
          });
    case "ready":
      return t("updates.status.ready", { version: state.version });
    case "error":
      switch (state.stage) {
        case "check":
          return t("updates.status.checkFailed");
        case "download":
          return t("updates.status.downloadFailed", { message: state.message });
        case "install":
          return t("updates.status.installFailed", { message: state.message });
        default:
          return state.stage satisfies never;
      }
    default:
      return state satisfies never;
  }
}

/** The one action that moves the updater on, for the state it is in (`null`: none to offer). */
export type Action = "check" | "download" | "restart";

export function actionOf(state: UpdateState): Action | null {
  switch (state.state) {
    case "idle":
    case "upToDate":
    case "error":
      return "check";
    case "available":
      return "download";
    case "ready":
      return "restart";
    case "checking":
    case "downloading":
      return null;
    default:
      return state satisfies never;
  }
}
