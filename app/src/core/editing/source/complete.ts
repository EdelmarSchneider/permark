// Completions without AI: project files as link targets (relative to the document), headings after `#` in a link
// (another document's from the backend, this one's from its own text), and words already written in the document.

import type {
  Completion,
  CompletionContext,
  CompletionResult,
  CompletionSource,
} from "@codemirror/autocomplete";

export type CompletionHost = {
  /** Project path of the document being edited. */
  docPath(): string;
  /** Files a link may point at (project paths; empty for a single file). */
  linkTargets(): Promise<string[]>;
  /** Heading anchors of another document. */
  headings(path: string): Promise<{ id: string; label: string }[]>;
};

const DOCUMENT = /\.(md|markdown|mdown|mkd|mdx)$/i;

/** `to` written relative to the folder of `from` (both project paths, `/` separated). */
export function relativeLink(from: string, to: string): string {
  const fromDir = from.split("/").slice(0, -1);
  const target = to.split("/");
  let common = 0;
  while (
    common < fromDir.length &&
    common < target.length - 1 &&
    fromDir[common] === target[common]
  ) {
    common += 1;
  }
  const up = fromDir.slice(common).map(() => "..");
  const rest = target.slice(common);
  return [...up, ...rest].map(encodeSegment).join("/");
}

function encodeSegment(segment: string): string {
  return segment === ".."
    ? segment
    : segment.replace(/ /g, "%20").replace(/[()]/g, encodeURIComponent);
}

/** A relative link written in `from` back to a project path (null when it climbs out). */
export function resolveLink(from: string, link: string): string | null {
  const parts = from.split("/").slice(0, -1);
  for (const raw of decodeURIComponentSafe(link).split("/")) {
    if (raw === "" || raw === ".") continue;
    if (raw === "..") {
      if (parts.length === 0) return null;
      parts.pop();
    } else parts.push(raw);
  }
  return parts.join("/");
}

function decodeURIComponentSafe(text: string): string {
  try {
    return decodeURIComponent(text);
  } catch {
    return text;
  }
}

/** Heading anchors the way the reading view makes them: lower case, letters | digits | `-` `_`, repeats get `-n`. */
export function headingAnchors(text: string): { id: string; label: string }[] {
  const seen = new Map<string, number>();
  const out: { id: string; label: string }[] = [];
  let fence: string | null = null;
  for (const line of text.split("\n")) {
    const opening = /^ {0,3}(`{3,}|~{3,})/.exec(line);
    if (opening) {
      const marker = opening[1] ?? "";
      if (fence === null) fence = marker;
      else if (marker[0] === fence[0] && marker.length >= fence.length) fence = null;
      continue;
    }
    if (fence !== null) continue;
    const match = /^ {0,3}#{1,6}[ \t]+(.*?)[ \t#]*$/.exec(line);
    if (!match) continue;
    const label = (match[1] ?? "").trim();
    const base = label
      .toLowerCase()
      .replace(/[^\p{L}\p{N}\p{M} _-]/gu, "")
      .replace(/ /g, "-");
    const count = seen.get(base) ?? 0;
    seen.set(base, count + 1);
    out.push({ id: count === 0 ? base : `${base}-${count}`, label });
  }
  return out;
}

/** Words of at least 4 characters, most used first. */
export function wordsOf(text: string, skip = ""): string[] {
  const counts = new Map<string, number>();
  for (const match of text.matchAll(/[\p{L}\p{N}_][\p{L}\p{N}\p{M}_'-]{3,}/gu)) {
    const word = match[0];
    if (word === skip) continue;
    counts.set(word, (counts.get(word) ?? 0) + 1);
  }
  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .map(([word]) => word);
}

export function completionSources(host: CompletionHost): CompletionSource[] {
  let targets: Promise<string[]> | null = null;
  const files = (): Promise<string[]> => (targets ??= host.linkTargets());

  const linkTarget = async (context: CompletionContext): Promise<CompletionResult | null> => {
    const match = context.matchBefore(/\]\([^)\s#]*$/);
    if (!match) return null;
    const typed = match.text.slice(2);
    const doc = host.docPath();
    const options: Completion[] = (await files())
      .filter((path) => path !== doc)
      .map((path) => ({
        label: relativeLink(doc, path),
        boost: DOCUMENT.test(path) ? 1 : 0,
        type: DOCUMENT.test(path) ? "text" : "variable",
      }));
    if (typed === "" && !context.explicit && options.length > 200) options.length = 200;
    return { from: match.from + 2, options, validFor: /^[^)\s#]*$/ };
  };

  const heading = async (context: CompletionContext): Promise<CompletionResult | null> => {
    const match = context.matchBefore(/\]\([^)\s#]*#[^)\s]*$/);
    if (!match) return null;
    const hash = match.text.indexOf("#");
    const link = match.text.slice(2, hash);
    const doc = host.docPath();
    let anchors: { id: string; label: string }[];
    if (link === "") anchors = headingAnchors(context.state.doc.toString());
    else {
      const target = resolveLink(doc, link);
      anchors = target === null ? [] : await host.headings(target);
    }
    return {
      from: match.from + hash + 1,
      options: anchors.map((a) => ({ label: a.id, detail: a.label, type: "keyword" })),
      validFor: /^[^)\s]*$/,
    };
  };

  // words of a document version are counted once (CodeMirror's text is immutable)
  const counted = new WeakMap<object, string[]>();
  const wordsAt = (context: CompletionContext): string[] => {
    const doc = context.state.doc;
    let list = counted.get(doc);
    if (!list) {
      list = wordsOf(doc.toString());
      counted.set(doc, list);
    }
    return list;
  };

  const words = (context: CompletionContext): CompletionResult | null => {
    const match = context.matchBefore(/[\p{L}\p{N}_][\p{L}\p{N}\p{M}_'-]*$/u);
    if (!match || (match.text.length < 2 && !context.explicit)) return null;
    // not inside a link target
    const before = context.state.sliceDoc(Math.max(0, match.from - 300), match.from);
    if (/\]\([^)]*$/.test(before)) return null;
    const options = wordsAt(context)
      .filter((word) => word !== match.text)
      .slice(0, 300)
      .map((label) => ({ label, type: "text" }));
    return { from: match.from, options, validFor: /^[\p{L}\p{N}\p{M}_'-]*$/u };
  };

  return [heading, linkTarget, words];
}
