// The hand-off dialog's arithmetic: which sessions are in, the notes a preview calls for, how a session and a size
// read. Pure.

import type { HandOffPreview, HandOffSessionView } from "../../api";
import type { MessageKey } from "../../core/i18n";

export type Ready = Extract<HandOffPreview, { state: "ready" }>;

/** The sessions that go in (checked, not left out for size). */
export function included(preview: Ready): HandOffSessionView[] {
  return preview.sessions.filter((s) => s.checked && !s.dropped);
}

/** The checked ids with `id` flipped. */
export function toggled(preview: Ready, id: number): number[] {
  const checked = preview.sessions.filter((s) => s.checked).map((s) => s.id);
  return checked.includes(id)
    ? checked.filter((c) => c !== id)
    : [...checked, id].sort((a, b) => a - b);
}

/** Whether confirming hands off anything. */
export function canConfirm(preview: Ready): boolean {
  return included(preview).length > 0 || preview.outside.files > 0;
}

/** The notes of a preview, in the order they read best. */
export function notes(preview: Ready): MessageKey[] {
  const out: MessageKey[] = [];
  switch (preview.case) {
    case "outsideOnly":
      out.push("agent.handoff.outsideOnly");
      break;
    case "first":
      if (preview.sessions.length > 1) out.push("agent.handoff.first");
      break;
    case "stale":
      if (preview.allStale) out.push("agent.handoff.allStale");
      break;
    case "one":
    case "several":
    case "nothing":
      break;
    default:
      preview.case satisfies never;
  }
  if (preview.trimmed) out.push("agent.handoff.trimmed");
  if (preview.upgraded) out.push("agent.handoff.upgraded");
  return out;
}

/** `~340`, `~1.2k`, `~12k` (tokens are approximate). */
export function approx(n: number, locale: string): string {
  if (n < 1000) return new Intl.NumberFormat(locale).format(n);
  const format = new Intl.NumberFormat(locale, { maximumFractionDigits: n < 10_000 ? 1 : 0 });
  return `${format.format(n / 1000)}k`;
}

/** A session's day and hours: `{ day, from, to }` in the interface's language. */
export function when(
  session: HandOffSessionView,
  locale: string,
): { day: string; from: string; to: string } {
  const day = new Date(session.start).toLocaleDateString(locale, {
    weekday: "short",
    month: "short",
    day: "numeric",
  });
  const hour = (ms: number): string =>
    new Date(ms).toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" });
  return { day, from: hour(session.start), to: hour(session.end) };
}
