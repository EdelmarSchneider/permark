// Config files for the mock backend. The sheets come from the REAL parsers: `cargo test` renders the fixture files
// (`fixtures/*.sample`) and writes `generated/mock-documents.json`. An edit here only moves one value on its line and
// patches the sheet the same way (the backend does it with the parser's spans).

import { mockView } from "./mockDocs";
import type {
  Api,
  Body,
  ConfigEdit,
  ConfigEdited,
  IgnoreMatches,
  Result,
  Row,
  Sheet,
} from "./types";

const ok = <T>(value: T): Promise<Result<T>> => Promise.resolve({ ok: true, value });

function refused<T>(
  command: "config_edit" | "config_sheet",
  path: string,
  kind: "notFound" | "invalidData" | "unsupported",
  detail: string,
): Promise<Result<T>> {
  return Promise.resolve({
    ok: false,
    error: { kind: "backend", command, error: { kind, subject: path, detail } },
  });
}

export type ConfigApi = Pick<Api, "configSheet" | "configEdit" | "ignoreMatches">;

type Token = { start: number; end: number; raw: string };

/** The value token of a `key = value` | `key: value` | `KEY=value` line (quotes included). */
function tokenIn(line: string): Token | null {
  const match =
    /^(\s*(?:export\s+)?[^=:#;]*?\s*[=:]\s*)("(?:[^"\\]|\\.)*"|'[^']*'|.*?)(\s+[#;].*)?\s*$/.exec(
      line,
    );
  const head = match?.[1];
  const raw = match?.[2];
  if (head === undefined || raw === undefined) return null;
  return { start: head.length, end: head.length + raw.length, raw };
}

function unquote(raw: string): string {
  if (
    raw.length >= 2 &&
    (raw.startsWith('"') || raw.startsWith("'")) &&
    raw.endsWith(raw[0] ?? "")
  ) {
    const inner = raw.slice(1, -1);
    return raw.startsWith('"') ? inner.replace(/\\(["\\])/g, "$1").replace(/\\n/g, "\n") : inner;
  }
  return raw;
}

function textRows(sheet: Sheet): Row[] {
  return sheet.groups.flatMap((g) => g.rows);
}

/** The sheet with the scalar values read again from the lines of `text` (rows keep their lines). */
function patched(sheet: Sheet, text: string): Sheet {
  const lines = text.split("\n");
  return {
    ...sheet,
    groups: sheet.groups.map((group) => ({
      ...group,
      rows: group.rows.map((row) => {
        if (row.value.type !== "scalar") return row;
        const token = tokenIn(lines[row.line - 1] ?? "");
        if (token === null || row.value.edit.editable === "readOnly") return row;
        return { ...row, value: { ...row.value, text: unquote(token.raw) } };
      }),
    })),
  };
}

/** Ignore files: groups of a comment block, rows = pattern lines (the backend's rule). */
function ignoreSheet(text: string): Sheet {
  const groups: Sheet["groups"] = [
    { path: "", title: "", line: 1, end: 1, comment: null, rows: [] },
  ];
  let block: string[] = [];
  let blockLine = 0;
  let previousComment = false;
  text.split("\n").forEach((raw, n) => {
    const line = raw.trimEnd();
    const lineNo = n + 1;
    if (line === "") {
      previousComment = false;
      return;
    }
    if (line.trimStart().startsWith("#")) {
      if (!previousComment) {
        block = [];
        blockLine = lineNo;
      }
      previousComment = true;
      const words = line.trimStart().slice(1).trim();
      if (words !== "") block.push(words);
      return;
    }
    previousComment = false;
    if (block.length > 0) {
      groups.push({
        path: `section-${blockLine}`,
        title: block.join(" "),
        line: blockLine,
        end: lineNo,
        comment: null,
        rows: [],
      });
      block = [];
    }
    const group = groups[groups.length - 1];
    if (group === undefined) return;
    group.end = Math.max(group.end, lineNo);
    group.rows.push({
      path: `L${lineNo}`,
      key: "",
      line: lineNo,
      end: lineNo,
      value: { type: "pattern", path: `L${lineNo}`, text: line, negated: line.startsWith("!") },
      comment: null,
    });
  });
  return { kind: "ignore", groups: groups.filter((g) => g.rows.length > 0) };
}

/** A sheet of the plain `key: value` | `key = value` lines of a text (properties of a front matter in the mock). */
function plainSheet(text: string): Sheet | null {
  const rows: Row[] = [];
  text.split("\n").forEach((line, n) => {
    const match = /^([\w.-]+)\s*[:=]\s*(.*?)\s*$/.exec(line);
    const key = match?.[1];
    const value = match?.[2];
    if (key === undefined || value === undefined || value.startsWith("[")) return;
    const kind = /^-?\d+(\.\d+)?$/.test(value)
      ? "number"
      : value === "true" || value === "false"
        ? "bool"
        : "string";
    rows.push({
      path: key,
      key,
      line: n + 1,
      end: n + 1,
      value: {
        type: "scalar",
        path: key,
        kind,
        text: unquote(value),
        edit: { editable: "surgical" },
      },
      comment: null,
    });
  });
  return rows.length === 0
    ? null
    : {
        kind: "config",
        groups: [{ path: "", title: "", line: 1, end: rows.length, comment: null, rows }],
      };
}

function sheetBody(path: string, text: string): Body {
  const view = mockView(path, [], true);
  const body = view?.body;
  if (body?.kind !== "sheet") {
    const plain = /\.(ya?ml|toml)$/i.test(path) ? plainSheet(text) : null;
    return plain ? { kind: "sheet", sheet: plain } : { kind: "text", text, language: null };
  }
  if (body.sheet.kind === "ignore") return { kind: "sheet", sheet: ignoreSheet(text) };
  return { kind: "sheet", sheet: patched(body.sheet, text) };
}

function quoteLike(raw: string, text: string): string {
  if (raw.startsWith('"')) return `"${text.replace(/(["\\])/g, "\\$1")}"`;
  if (raw.startsWith("'") && !text.includes("'")) return `'${text}'`;
  return /[\s#]/.test(text) && raw !== "" ? `"${text}"` : text;
}

function editLines(
  path: string,
  text: string,
  edit: ConfigEdit,
  body: Body,
): Promise<Result<ConfigEdited>> {
  const lines = text.split("\n");
  const lineOf = (target: string): number => Number(target.replace(/^L/, "")) - 1;
  switch (edit.op) {
    case "set": {
      const at = lineOf(edit.path);
      if (lines[at] === undefined) return refused("config_edit", path, "notFound", "notFound");
      lines[at] = edit.value.text;
      break;
    }
    case "remove": {
      const at = lineOf(edit.path);
      if (lines[at] === undefined) return refused("config_edit", path, "notFound", "notFound");
      lines.splice(at, 1);
      break;
    }
    case "insert": {
      const after = edit.after === null ? lines.length - 1 : lineOf(edit.after);
      const last = lines[lines.length - 1] === "" ? lines.length - 1 : lines.length;
      lines.splice(Math.min(after + 1, last), 0, edit.text);
      break;
    }
    // the data edits (keys, fields, rows) are the TOON format's: the backend makes them
    case "rename":
    case "renameField":
    case "addKey":
    case "addRow":
    case "removeRow":
    case "duplicateRow":
    case "moveRow":
      return refused("config_edit", path, "unsupported", "unsupported");
    default:
      return edit satisfies never;
  }
  const next = lines.join("\n");
  return ok({ text: next, body: body.kind === "sheet" ? sheetBody(path, next) : body });
}

export function createMockConfig(textOf: (path: string) => string | null = () => null): ConfigApi {
  const mockFileText = (path: string): string => textOf(path) ?? "";
  return {
    configSheet: (path, text) => ok(sheetBody(path, text)),
    configEdit: (path, text, edit) => {
      const body = sheetBody(path, text);
      if (body.kind !== "sheet") return refused("config_edit", path, "unsupported", "unsupported");
      if (body.sheet.kind === "ignore") return editLines(path, text, edit, body);
      if (edit.op !== "set") return refused("config_edit", path, "unsupported", "unsupported");
      const row = textRows(body.sheet).find((r) => r.path === edit.path);
      if (row === undefined || row.value.type !== "scalar") {
        return refused("config_edit", path, "notFound", "notFound");
      }
      if (row.value.edit.editable === "readOnly") {
        return refused("config_edit", path, "invalidData", row.value.edit.reason);
      }
      const lines = text.split("\n");
      const line = lines[row.line - 1] ?? "";
      const token = tokenIn(line);
      if (token === null) return refused("config_edit", path, "notFound", "notFound");
      const written =
        edit.value.type === "text" ? quoteLike(token.raw, edit.value.text) : edit.value.text;
      lines[row.line - 1] = line.slice(0, token.start) + written + line.slice(token.end);
      const next = lines.join("\n");
      return ok({ text: next, body: sheetBody(path, next) });
    },
    ignoreMatches: (path, given) => {
      const text = given ?? mockFileText(path);
      const body = sheetBody(path, text);
      const counts =
        body.kind === "sheet"
          ? textRows(body.sheet)
              .filter((r) => r.value.type === "pattern")
              .map((r) => ({ line: r.line, count: (text.length * 7 + r.line * 5) % 17 }))
          : [];
      const result: IgnoreMatches = { counts, capped: false };
      return ok(result);
    },
  };
}
