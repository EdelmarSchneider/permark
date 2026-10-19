// "3 min ago" in the language of the interface.

import { language } from "../../core/i18n";

export function ago(atMs: number, now: number): string {
  const seconds = Math.max(0, Math.round((now - atMs) / 1000));
  const rtf = new Intl.RelativeTimeFormat(language.locale, { numeric: "always", style: "narrow" });
  if (seconds < 60) return rtf.format(-seconds, "second");
  if (seconds < 3600) return rtf.format(-Math.round(seconds / 60), "minute");
  if (seconds < 86_400) return rtf.format(-Math.round(seconds / 3600), "hour");
  return rtf.format(-Math.round(seconds / 86_400), "day");
}

/** The name of a project path, and the folder it is in. */
export function split(path: string): { name: string; folder: string } {
  const at = path.lastIndexOf("/");
  return at < 0
    ? { name: path, folder: "" }
    : { name: path.slice(at + 1), folder: path.slice(0, at) };
}
