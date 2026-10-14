import { LOCALE, t, tn } from "./i18n";

const SECOND = 1000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** Compact age for lists: "just now", "12s", "4m", "3h", "2d". */
export function relativeTime(ms: number, now: number): string {
  const diff = Math.max(0, now - ms);
  if (diff < 5 * SECOND) return t("time.justNow");
  if (diff < MINUTE) return t("time.seconds", { n: Math.floor(diff / SECOND) });
  if (diff < HOUR) return t("time.minutes", { n: Math.floor(diff / MINUTE) });
  if (diff < DAY) return t("time.hours", { n: Math.floor(diff / HOUR) });
  return t("time.days", { n: Math.floor(diff / DAY) });
}

/** Longer age for sentences: "just now", "12 seconds ago", "4 minutes ago". */
export function ago(ms: number, now: number): string {
  const diff = Math.max(0, now - ms);
  if (diff < 5 * SECOND) return t("time.justNow");
  if (diff < MINUTE) return tn("time.ago.seconds", Math.floor(diff / SECOND));
  if (diff < HOUR) return tn("time.ago.minutes", Math.floor(diff / MINUTE));
  if (diff < DAY) return tn("time.ago.hours", Math.floor(diff / HOUR));
  return tn("time.ago.days", Math.floor(diff / DAY));
}

/** Header text for a document's modification time: "saved just now", "saved 12s ago". */
export function savedLabel(modifiedMs: number, now: number): string {
  const diff = Math.max(0, now - modifiedMs);
  if (diff < 5 * SECOND) return t("doc.savedNow");
  return t("doc.saved", { time: relativeTime(modifiedMs, now) });
}

function pad(n: number): string {
  return String(n).padStart(2, "0");
}

/** "10:42" in local time. */
export function clock(ms: number): string {
  const d = new Date(ms);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** "Mon 18:40" within the last week, "Oct 3, 18:40" before that. */
export function baselineLabel(ms: number, now: number): string {
  const d = new Date(ms);
  if (now - ms < 6 * DAY) {
    return `${d.toLocaleDateString(LOCALE, { weekday: "short" })} ${clock(ms)}`;
  }
  return `${d.toLocaleDateString(LOCALE, { month: "short", day: "numeric" })}, ${clock(ms)}`;
}
