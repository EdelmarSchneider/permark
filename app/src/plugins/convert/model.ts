// The converter's rules, pure: what can be converted, how a table of the page becomes a source, how a count reads.

import type { MessageKey } from "../../core/i18n";

/** The sources the backend reads (ids), in the order the dialog lists them. */
export const SOURCES = ["json", "jsonl", "yaml", "xml", "csv", "tsv", "toon", "toml"] as const;
export type SourceId = (typeof SOURCES)[number];

/** Extensions | fence languages and the source they are. */
const BY_WORD: Readonly<Record<string, SourceId>> = {
  json: "json",
  jsonc: "json",
  geojson: "json",
  jsonl: "jsonl",
  ndjson: "jsonl",
  yaml: "yaml",
  yml: "yaml",
  xml: "xml",
  csv: "csv",
  tsv: "tsv",
  toon: "toon",
  toml: "toml",
};

/** The source a file name | fence language names (`null`: none the converter reads). */
export function sourceOf(word: string): SourceId | null {
  const lower = word.toLowerCase();
  const ext = lower.includes(".") ? (lower.split(".").at(-1) ?? "") : lower;
  return BY_WORD[ext] ?? null;
}

/** A file the converter can read, by its name. */
export function convertible(path: string): boolean {
  const name = path.split("/").at(-1) ?? path;
  return name.includes(".") && sourceOf(name) !== null;
}

/** A cell for CSV: quoted when it holds a comma, a quote or a line break. */
function csvCell(text: string): string {
  return /[",\r\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

/** A table of the page as CSV (header row first): the converter guesses the types of its cells. */
export function tableAsCsv(rows: readonly (readonly string[])[]): string {
  return rows.map((cells) => cells.map(csvCell).join(",")).join("\n") + "\n";
}

/** The cells of a rendered table, header first (text as the reader sees it). */
export function cellsOf(table: HTMLTableElement): string[][] {
  return Array.from(table.rows).map((row) =>
    Array.from(row.cells).map((cell) => (cell.textContent ?? "").trim()),
  );
}

/** `-42 %` | `+12 %` | `0 %`: the change from the source, rounded. */
export function change(tokens: number, source: number): string {
  if (source <= 0) return "";
  const percent = Math.round(((tokens - source) / source) * 100);
  if (percent === 0) return "0 %";
  return percent < 0 ? `−${-percent} %` : `+${percent} %`;
}

/** A count with its mark: `~1,240` for an estimate, `1,240` when exact. */
export function count(tokens: number, approx: boolean, locale: string): string {
  const number = new Intl.NumberFormat(locale).format(tokens);
  return approx ? `~${number}` : number;
}

/** The name of a target in the dialog. */
export function targetKey(id: string): MessageKey {
  switch (id) {
    case "toon":
      return "convert.target.toon";
    case "json":
      return "convert.target.json";
    case "jsonl":
      return "convert.target.jsonl";
    case "csv":
      return "convert.target.csv";
    case "tsv":
      return "convert.target.tsv";
    case "yaml":
      return "convert.target.yaml";
    case "jsonPretty":
      return "convert.target.jsonPretty";
    default:
      return "convert.target.other";
  }
}

/** The name of a source format. */
export function sourceKey(id: string): MessageKey {
  switch (id) {
    case "json":
      return "convert.source.json";
    case "jsonl":
      return "convert.source.jsonl";
    case "yaml":
      return "convert.source.yaml";
    case "xml":
      return "convert.source.xml";
    case "csv":
      return "convert.source.csv";
    case "tsv":
      return "convert.source.tsv";
    case "toon":
      return "convert.source.toon";
    case "toml":
      return "convert.source.toml";
    default:
      return "convert.target.other";
  }
}

/** Why a target fits or not, in words. */
export function reasonKey(reason: string): MessageKey {
  switch (reason) {
    case "tabular":
      return "convert.reason.tabular";
    case "smaller":
      return "convert.reason.smaller";
    case "compact":
      return "convert.reason.compact";
    case "lines":
      return "convert.reason.lines";
    case "readable":
      return "convert.reason.readable";
    case "noGain":
      return "convert.reason.noGain";
    case "deep":
      return "convert.reason.deep";
    case "notUniform":
      return "convert.reason.notUniform";
    case "notTable":
      return "convert.reason.notTable";
    case "notArray":
      return "convert.reason.notArray";
    default:
      return "convert.reason.failed";
  }
}

/** What a conversion lost, in words. */
export function noteKey(note: string): MessageKey {
  switch (note) {
    case "comments":
      return "convert.note.comments";
    case "anchors":
      return "convert.note.anchors";
    case "tags":
      return "convert.note.tags";
    case "keys":
      return "convert.note.keys";
    case "documents":
      return "convert.note.documents";
    case "dates":
      return "convert.note.dates";
    case "notNumbers":
      return "convert.note.notNumbers";
    case "types":
      return "convert.note.types";
    case "uneven":
      return "convert.note.uneven";
    case "duplicateColumns":
      return "convert.note.duplicateColumns";
    case "nulls":
      return "convert.note.nulls";
    case "instructions":
      return "convert.note.instructions";
    case "doctype":
      return "convert.note.doctype";
    case "whitespace":
      return "convert.note.whitespace";
    case "mixed":
      return "convert.note.mixed";
    case "namespaces":
      return "convert.note.namespaces";
    case "entities":
      return "convert.note.entities";
    default:
      return "convert.note.other";
  }
}

/** What the counts belong to: the token count plugin's tokenizer (product names), or the app's own estimate. */
export function tokenizerName(id: string | null): string | null {
  switch (id) {
    case "o200k":
      return "GPT-4o";
    case "cl100k":
      return "GPT-4";
    case "claude":
      return "Claude";
    case "gemini":
      return "Gemini";
    case "chars4":
      return "÷ 4";
    default:
      return null;
  }
}
