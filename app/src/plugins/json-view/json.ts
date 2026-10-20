// JSON text -> a read-only tree, exactly as written: numbers keep their digits (no rounding of big integers), keys keep
// their order (and duplicates), strings are decoded. A text that is not JSON (comments, trailing commas) is refused with
// the place it went wrong, and the viewer shows it as text.

import type { TreeNode } from "../../core/doc/tree";
import { tn } from "../../core/i18n";

export type JsonParse = { ok: true; root: TreeNode } | { ok: false; at: number };

/** Deepest nesting read (the recursion must not run out of stack on `[[[[...`). */
const MAX_DEPTH = 200;
const NUMBER = /-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?/y;

class Failure extends Error {
  constructor(readonly at: number) {
    super("not JSON");
  }
}

class Reader {
  at = 0;
  constructor(readonly text: string) {}

  skip(): void {
    while (this.at < this.text.length && " \t\n\r".includes(this.text.charAt(this.at))) this.at++;
  }

  fail(): never {
    throw new Failure(this.at);
  }

  expect(char: string): void {
    if (this.text.charAt(this.at) !== char) this.fail();
    this.at++;
  }

  string(): string {
    this.expect('"');
    let out = "";
    for (;;) {
      const char = this.text.charAt(this.at);
      if (this.at >= this.text.length || char < " ") this.fail();
      this.at++;
      if (char === '"') return out;
      if (char !== "\\") {
        out += char;
        continue;
      }
      const escape = this.text.charAt(this.at++);
      switch (escape) {
        case '"':
        case "\\":
        case "/":
          out += escape;
          break;
        case "b":
          out += "\b";
          break;
        case "f":
          out += "\f";
          break;
        case "n":
          out += "\n";
          break;
        case "r":
          out += "\r";
          break;
        case "t":
          out += "\t";
          break;
        case "u": {
          const hex = this.text.slice(this.at, this.at + 4);
          if (!/^[0-9a-fA-F]{4}$/.test(hex)) this.fail();
          out += String.fromCharCode(Number.parseInt(hex, 16));
          this.at += 4;
          break;
        }
        default:
          this.at--;
          this.fail();
      }
    }
  }

  value(label: string | null, depth: number): TreeNode {
    if (depth > MAX_DEPTH) this.fail();
    this.skip();
    const char = this.text.charAt(this.at);
    if (char === "{") return this.object(label, depth);
    if (char === "[") return this.array(label, depth);
    if (char === '"') return leaf(label, "string", JSON.stringify(this.string()));
    for (const word of ["true", "false", "null"] as const) {
      if (this.text.startsWith(word, this.at)) {
        this.at += word.length;
        return leaf(label, word === "null" ? "null" : "boolean", word);
      }
    }
    NUMBER.lastIndex = this.at;
    const match = NUMBER.exec(this.text);
    if (!match) this.fail();
    this.at += match[0].length;
    return leaf(label, "number", match[0]);
  }

  object(label: string | null, depth: number): TreeNode {
    this.expect("{");
    const members: TreeNode[] = [];
    this.skip();
    if (this.text.charAt(this.at) === "}") {
      this.at++;
    } else {
      for (;;) {
        this.skip();
        const key = this.string();
        this.skip();
        this.expect(":");
        members.push(this.value(key, depth + 1));
        this.skip();
        const next = this.text.charAt(this.at++);
        if (next === "}") break;
        if (next !== ",") {
          this.at--;
          this.fail();
        }
      }
    }
    return container(label, "object", members, tn("tree.keys", members.length));
  }

  array(label: string | null, depth: number): TreeNode {
    this.expect("[");
    const items: TreeNode[] = [];
    this.skip();
    if (this.text.charAt(this.at) === "]") {
      this.at++;
    } else {
      for (;;) {
        items.push(this.value(String(items.length), depth + 1));
        this.skip();
        const next = this.text.charAt(this.at++);
        if (next === "]") break;
        if (next !== ",") {
          this.at--;
          this.fail();
        }
      }
    }
    return container(label, "array", items, tn("tree.items", items.length));
  }
}

function leaf(label: string | null, kind: TreeNode["kind"], summary: string): TreeNode {
  return { label, kind, summary, children: null };
}

function container(
  label: string | null,
  kind: "object" | "array",
  children: TreeNode[],
  summary: string,
): TreeNode {
  return { label, kind, summary, children: children.length === 0 ? null : () => children };
}

/** The tree of a JSON text, or where it stopped being JSON. */
export function parseJson(text: string): JsonParse {
  const reader = new Reader(text);
  try {
    const root = reader.value(null, 0);
    reader.skip();
    if (reader.at < text.length) reader.fail();
    return { ok: true, root };
  } catch (error) {
    if (error instanceof Failure) return { ok: false, at: error.at };
    throw error;
  }
}
