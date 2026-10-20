// Which highlight.js language a fence names: by id, display name or alias, case-insensitive (pure; no grammar loaded).

import { HLJS_LANGUAGES } from "./languageData";

export type Resolved = { id: string; name: string };

/** Names that mean "this is not code": shown as written, no label, no guessing. */
const PLAIN = new Set(["text", "txt", "plain", "plaintext", "none", "nohighlight", "raw"]);

const BY_NAME = (() => {
  const map = new Map<string, Resolved>();
  for (const [id, name] of HLJS_LANGUAGES) {
    const resolved = { id, name };
    map.set(id.toLowerCase(), resolved);
  }
  // aliases never take a name that is already a language of its own
  for (const [id, name, aliases] of HLJS_LANGUAGES) {
    for (const alias of aliases) {
      const key = alias.toLowerCase();
      if (!map.has(key)) map.set(key, { id, name });
    }
  }
  return map;
})();

/** The first word of a fence's info string (`rust title="x"` -> `rust`). */
export function fenceWord(info: string): string {
  return info.trim().split(/[\s{,:]/)[0] ?? "";
}

export function isPlainName(name: string): boolean {
  return PLAIN.has(name.toLowerCase());
}

/** The language a name stands for, or null when highlight.js does not have it. */
export function resolveLanguage(name: string): Resolved | null {
  return BY_NAME.get(name.trim().toLowerCase()) ?? null;
}

/** Display name of a grammar id. */
export function grammarName(id: string): string {
  return BY_NAME.get(id.toLowerCase())?.name ?? id;
}
