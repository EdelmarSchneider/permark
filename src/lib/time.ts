import { intlTag, t } from "./i18n";

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

/** Longer age for sentences, worded by the language: "just now", "12 seconds ago", "há 4 minutos", "vor 2 Stunden". */
export function ago(ms: number, now: number): string {
  const diff = Math.max(0, now - ms);
  if (diff < 5 * SECOND) return t("time.justNow");
  const words = new Intl.RelativeTimeFormat(intlTag(), { numeric: "always" });
  if (diff < MINUTE) return words.format(-Math.floor(diff / SECOND), "second");
  if (diff < HOUR) return words.format(-Math.floor(diff / MINUTE), "minute");
  if (diff < DAY) return words.format(-Math.floor(diff / HOUR), "hour");
  return words.format(-Math.floor(diff / DAY), "day");
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

/** "10:42" in local time: 24 hours in every language of the app. */
export function clock(ms: number): string {
  const d = new Date(ms);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** "Mon 18:40" within the last week, "Oct 3, 18:40" before that. */
export function baselineLabel(ms: number, now: number): string {
  const d = new Date(ms);
  if (now - ms < 6 * DAY) {
    return `${d.toLocaleDateString(intlTag(), { weekday: "short" })} ${clock(ms)}`;
  }
  return `${d.toLocaleDateString(intlTag(), { month: "short", day: "numeric" })}, ${clock(ms)}`;
}
