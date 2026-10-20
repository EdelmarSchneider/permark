// What the folder count shows, as pure functions: the badge text, the words for a time, a reason, a refusal, and the
// plain text "Copy summary" puts on the clipboard.

import type { FolderJob, FolderSkip, FolderSummary } from "../../api";
import type { MessageKey, Params, PluralKey } from "../../core/i18n";
import type { TokenizerInfo } from "./tokenizers";

/** Folders over this many files are refused (the backend's limit; the dialog names it). */
export const REFUSE_FILES = 50_000;

/** A number as the badge writes it: whole below 10 000, then `12.3k`, then `1.2M`. */
export function shortNumber(n: number, locale: string): string {
  if (n < 10_000) return new Intl.NumberFormat(locale).format(n);
  const one = new Intl.NumberFormat(locale, { maximumFractionDigits: 1 });
  const thousands = Math.round(n / 100) / 10;
  if (thousands < 1000) return `${one.format(thousands)}k`;
  return `${one.format(Math.round(n / 100_000) / 10)}M`;
}

/** The badge of a counted folder: `≈` first when it is an estimate, `*` after it when the folder changed since. */
export function badgeText(tokens: number, approx: boolean, stale: boolean, locale: string): string {
  return `${approx ? "≈" : ""}${shortNumber(tokens, locale)}${stale ? "*" : ""}`;
}

/** How long to say a job takes: seconds up to a minute and a half, then whole minutes (rounded up). */
export function timeUnit(seconds: number): { unit: "seconds" | "minutes"; n: number } {
  return seconds < 90
    ? { unit: "seconds", n: Math.max(1, Math.round(seconds)) }
    : { unit: "minutes", n: Math.ceil(seconds / 60) };
}

export function skipKey(reason: FolderSkip): MessageKey {
  switch (reason) {
    case "binary":
      return "tokens.folder.skip.binary";
    case "tooLarge":
      return "tokens.folder.skip.tooLarge";
    case "unreadable":
      return "tokens.folder.skip.unreadable";
    default:
      return reason satisfies never;
  }
}

type Failed = Extract<FolderJob, { state: "failed" }>;

export function failedKey(reason: Failed["reason"]): MessageKey {
  switch (reason) {
    case "notFound":
      return "tokens.folder.failed.notFound";
    case "unavailable":
      return "tokens.folder.failed.unavailable";
    case "io":
      return "tokens.folder.failed.io";
    default:
      return reason satisfies never;
  }
}

/** The job is working: the status item shows its progress. */
export function isWorking(job: FolderJob): boolean {
  switch (job.state) {
    case "scanning":
    case "counting":
      return true;
    case "idle":
    case "confirm":
    case "refused":
    case "done":
    case "cancelled":
    case "failed":
      return false;
    default:
      return job satisfies never;
  }
}

/** The parent folder of a project path (`""` for a top-level one). */
export function parentOf(path: string): string {
  const at = path.lastIndexOf("/");
  return at < 0 ? "" : path.slice(0, at);
}

/** How the words of a summary are said: the window's translators, so the text can be built (and tested) apart. */
export type Say = {
  t: (key: MessageKey, params?: Params) => string;
  tn: (base: PluralKey, n: number, params?: Params) => string;
  number: (n: number) => string;
};

/** The reasons files were skipped, as one phrase: `7 not text, 2 over 5 MB`. */
export function skippedReasons(summary: FolderSummary, say: Say): string {
  const parts: [number, FolderSkip][] = [
    [summary.skippedBinary, "binary"],
    [summary.skippedLarge, "tooLarge"],
    [summary.skippedUnreadable, "unreadable"],
  ];
  return parts
    .filter(([n]) => n > 0)
    .map(([n, reason]) => `${say.number(n)} ${say.t(skipKey(reason))}`)
    .join(", ");
}

/** What a type row is called: `.md`, no extension, or all the others. */
export function typeName(ext: string, say: Say): string {
  if (ext === "") return say.t("tokens.folder.summary.noExt");
  if (ext === "*") return say.t("tokens.folder.summary.otherTypes");
  return `.${ext}`;
}

/** The folder as a person names it: its path, or the project for the project folder. */
export function folderName(folder: string, say: Say): string {
  return folder === "" ? say.t("tokens.folder.project") : folder;
}

/** The summary as plain text for a note or a prompt. */
export function summaryText(
  summary: FolderSummary,
  tokenizer: TokenizerInfo | null,
  say: Say,
): string {
  const name = tokenizer?.short ?? summary.tokenizer;
  const kind = say.t(summary.approx ? "tokens.badge.approx" : "tokens.badge.exact");
  const lines = [
    say.t("tokens.folder.copy.head", {
      folder: folderName(summary.folder, say),
      count: say.tn("tokens.count", summary.tokens, { n: say.number(summary.tokens) }),
      tokenizer: name,
      kind,
    }),
  ];
  const counted = say.tn("tokens.folder.summary.counted", summary.counted, {
    n: say.number(summary.counted),
  });
  if (summary.skipped > 0) {
    const skipped = say.tn("tokens.folder.summary.skipped", summary.skipped, {
      n: say.number(summary.skipped),
    });
    lines.push(
      say.t("tokens.folder.copy.files", {
        counted,
        skipped,
        reasons: skippedReasons(summary, say),
      }),
    );
  } else {
    lines.push(counted);
  }
  if (summary.byType.length > 0) {
    const list = summary.byType
      .map(
        (row) =>
          `${typeName(row.ext, say)} ${say.number(row.tokens)} (${say.tn(
            "tokens.folder.files",
            row.files,
            { n: say.number(row.files) },
          )})`,
      )
      .join(" · ");
    lines.push(say.t("tokens.folder.copy.types", { list }));
  }
  if (summary.top.length > 0) {
    const list = summary.top.map((file) => `${file.path} ${say.number(file.tokens)}`).join(" · ");
    lines.push(say.t("tokens.folder.copy.top", { list }));
  }
  return lines.join("\n");
}
