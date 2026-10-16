// What the header says about a picture, a sound or a video: its size on screen and on disk, and how long it plays. Pure.

import { intlTag, t } from "./i18n";
import type { AssetKind } from "./fileKinds";

/** What the webview learned about the file once it had decoded it. */
export type MediaFacts = {
  /** Pixels (a picture, a video). */
  width?: number;
  height?: number;
  /** Seconds (a sound, a video). */
  duration?: number;
};

/** A file size for a line of text, with the decimal mark of the interface's language: "312 KB", "1.5 MB". */
export function formatSize(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = Math.max(0, bytes);
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  // whole bytes and whole kilobytes need no decimals; the bigger units keep one
  const digits = unit <= 1 || value >= 100 ? 0 : 1;
  const text = value.toLocaleString(intlTag(), {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  });
  return `${text} ${units[unit]}`;
}

/** A length of time as a clock: "0:07", "3:12", "1:02:03". Nothing for a length that is not known. */
export function formatDuration(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "";
  const total = Math.round(seconds);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const rest = String(total % 60).padStart(2, "0");
  return hours > 0 ? `${hours}:${String(minutes).padStart(2, "0")}:${rest}` : `${minutes}:${rest}`;
}

/** The pixel size, as the header writes it: "800 × 450 px". Empty until the size is known. */
export function formatDimensions(facts: MediaFacts): string {
  const { width, height } = facts;
  if (!width || !height) return "";
  return t("media.dimensions", { width, height });
}

/** The parts of the header line for a file, in the order they are written; the ones not known yet are left out. */
export function mediaSummary(kind: AssetKind, sizeBytes: number, facts: MediaFacts): string[] {
  const parts: string[] = [];
  if (kind !== "audio") parts.push(formatDimensions(facts));
  if (kind !== "image" && facts.duration !== undefined) parts.push(formatDuration(facts.duration));
  parts.push(formatSize(sizeBytes));
  return parts.filter((part) => part !== "");
}
