// A moment as people say it ("3 hours ago", "yesterday"), in the language of the interface. Pure.

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/**
 * Compact "3 min ago": Intl's narrow style, or short where a language's narrow past is a bare minus sign ("-3 min" in
 * French, European Portuguese and Romanian; short says "il y a 3 min").
 */
export function compactRelativeFormat(locale: string): Intl.RelativeTimeFormat {
  const narrow = new Intl.RelativeTimeFormat(locale, { numeric: "always", style: "narrow" });
  return /^[-\u2212]/.test(narrow.format(-1, "minute"))
    ? new Intl.RelativeTimeFormat(locale, { numeric: "always", style: "short" })
    : narrow;
}

/** `then` seen from `now` (both in ms); `locale` is a BCP 47 tag. A moment ahead of `now` reads as "now". */
export function relativeTime(then: number, now: number, locale: string): string {
  const rtf = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  const diff = Math.min(0, then - now);
  const age = -diff;
  if (age < 45_000) return rtf.format(0, "second");
  if (age < HOUR) return rtf.format(Math.round(diff / MINUTE), "minute");
  if (age < DAY) return rtf.format(Math.round(diff / HOUR), "hour");
  if (age < 30 * DAY) return rtf.format(Math.round(diff / DAY), "day");
  if (age < 365 * DAY) return rtf.format(Math.round(diff / (30 * DAY)), "month");
  return rtf.format(Math.round(diff / (365 * DAY)), "year");
}
