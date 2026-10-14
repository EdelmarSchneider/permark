// Mock vault used by `mockApi` (VITE_MOCK=1): a realistic tree, documents shaped like the backend's
// rendered HTML (top-level blocks with `data-sourcepos`, heading anchors, alerts, `language-mermaid`
// code, `data-math-style` spans, footnotes, task lists), git state and a tiny section search.

import { resolveRelative, splitWorktree } from "../paths";
import { hashString } from "../text";
import type {
  ChangedSince,
  DocumentView,
  GitSnapshot,
  LinkTarget,
  SearchHit,
  TreeNode,
  VaultInfo,
} from "../types";

export const MOCK_ROOT = "C:/Users/demo/trailmark-docs";
export const MOCK_NOW = Date.now();
const SEC = 1000;
const MIN = 60 * SEC;

// ---------------------------------------------------------------------------------------------
// Block builders
// ---------------------------------------------------------------------------------------------

type Block = {
  html: string;
  text: string;
  lines: number;
  heading?: { level: number; text: string };
};

export function esc(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function plain(html: string): string {
  return html
    .replace(/<[^>]*>/g, " ")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&amp;/g, "&")
    .replace(/\s+/g, " ")
    .trim();
}

function slug(text: string): string {
  return text
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s-]/gu, "")
    .trim()
    .replace(/\s+/g, "-");
}

const raw = (html: string, lines = 1): Block => ({ html, text: plain(html), lines });
const h = (level: number, text: string): Block => {
  const id = slug(text);
  return {
    html: `<h${level}><a href="#${id}" aria-hidden="true" class="anchor" id="${id}"></a>${esc(text)}</h${level}>`,
    text,
    lines: 1,
    heading: { level, text },
  };
};
const p = (inner: string, lines = 1): Block => raw(`<p>${inner}</p>`, lines);
const ul = (items: string[]): Block =>
  raw(`<ul>${items.map((i) => `<li>${i}</li>`).join("")}</ul>`, items.length);
const tasks = (items: [boolean, string][]): Block =>
  raw(
    `<ul>${items
      .map(
        ([done, text]) =>
          `<li><input type="checkbox"${done ? ' checked=""' : ""} disabled="" /> ${text}</li>`,
      )
      .join("")}</ul>`,
    items.length,
  );
const code = (lang: string | null, source: string): Block =>
  raw(
    `<pre><code${lang ? ` class="language-${lang}"` : ""}>${esc(source)}\n</code></pre>`,
    source.split("\n").length + 2,
  );
const alert = (kind: string, inner: string): Block =>
  raw(
    `<div class="markdown-alert markdown-alert-${kind}"><p class="markdown-alert-title">${
      kind[0].toUpperCase() + kind.slice(1)
    }</p><p>${inner}</p></div>`,
    3,
  );
const table = (head: string[], rows: string[][]): Block =>
  raw(
    `<table><thead><tr>${head.map((c) => `<th>${c}</th>`).join("")}</tr></thead><tbody>${rows
      .map((r) => `<tr>${r.map((c) => `<td>${c}</td>`).join("")}</tr>`)
      .join("")}</tbody></table>`,
    rows.length + 2,
  );
const quote = (inner: string, cite: string): Block =>
  raw(`<blockquote><p>${inner}</p><p><em>${cite}</em></p></blockquote>`, 3);

/** Small stable string hash, so equal blocks get equal hashes (like the backend's per-block hashes). */
export const hashBlock = hashString;

/** Adds `data-sourcepos` to the root element of a block, like comrak does. */
function withSourcepos(html: string, start: number, lines: number): string {
  return html.replace(/^<(\w+)/, `<$1 data-sourcepos="${start}:1-${start + lines - 1}:1"`);
}

// ---------------------------------------------------------------------------------------------
// Documents
// ---------------------------------------------------------------------------------------------

type MockDoc = {
  kind: DocumentView["kind"];
  title: string | null;
  blocks: (rev: number) => Block[];
  frontmatter?: [string, string][];
  encodingNote?: string;
  /** Age of the file on disk at startup, in ms. */
  age: number;
};

const doc = (
  title: string,
  blocks: Block[] | ((rev: number) => Block[]),
  age = 3 * 60 * MIN,
  extra: Partial<MockDoc> = {},
): MockDoc => ({
  kind: "markdown",
  title,
  blocks: typeof blocks === "function" ? blocks : () => blocks,
  age,
  ...extra,
});

function liveFeed(rev: number): Block[] {
  const toastParagraph =
    rev % 4 < 2 ? [p("Each save shows its own toast, so nothing is missed.")] : [];
  const changelog = Array.from(
    { length: rev },
    (_, i) => `Edit ${i + 1}: refined the grouping rules`,
  );
  return [
    p(
      "The feed answers one question: <em>what changed while I was looking elsewhere?</em> It lists every file the watcher reports, keyed by path, so a file saved twelve times stays one entry that moves back to the top.",
      3,
    ),
    alert(
      "note",
      "Temporary files (<code>*.tmp</code>, <code>~$*</code>, swap files) and build folders never reach the feed. The filter runs in the core, before events are batched.",
    ),
    h(2, "Event model"),
    p(
      'Events come from two sources. The <a href="watcher.md">file watcher</a> reports single saves; the git-dir watcher reports operations such as checkout or rebase, which arrive as one bulk batch.<sup class="footnote-ref"><a href="#fn-1" id="fnref-1" data-footnote-ref>1</a></sup>',
      2,
    ),
    table(
      ["Event", "Source", "Feed", "Toast"],
      [
        ["created", "watcher", "new entry", "yes"],
        ["modified", "watcher", "updated in place", "yes"],
        ["removed", "watcher", "kept, struck through", "yes"],
        ["checkout", "git operation", "one collapsed entry", "no"],
      ],
    ),
    h(3, "Sources"),
    p("The file watcher and the git-dir watcher are the only two sources; nothing polls.", 1),
    h(3, "Batches"),
    p("A batch closes after 150 ms of quiet, or after one second at the most.", 1),
    code(
      "ts",
      `// One batch per quiet period (150 ms), or at most every second.
type FsBatch = {
  changes: FsChange[];
  bulk: null | { count: number; reason: "gitOperation" | "large" | "overflow" };
  rescan: boolean;
};`,
    ),
    h(2, "Grouping"),
    p(
      rev > 0
        ? `Bursts of saves collapse into one entry and <strong>never produce more than one toast</strong>: while a burst lasts, the toast updates its counter in place instead of stacking (revision ${rev}).`
        : "Bursts of saves collapse into one entry and <strong>never produce more than one toast</strong>: while a burst lasts, the toast updates its counter in place instead of stacking.",
      2,
    ),
    ...toastParagraph,
    p(
      "Follow mode (<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>L</kbd>) opens the latest changed doc at most once every three seconds, and never for a bulk batch.",
      2,
    ),
    quote(
      "The feed is only useful if it is quiet. If people stop looking at it, it has failed.",
      "Design review, 2026-10-12",
    ),
    h(2, "Pipeline"),
    code(
      "mermaid",
      `graph LR
  Watcher[Watcher] --> Aggregator[Aggregator]
  Aggregator --> Reconcile[Reconcile with disk]
  Reconcile --> Feed[Feed and toast]`,
    ),
    h(2, "Rollout"),
    tasks([
      [true, "Aggregate events (150 ms quiet, 1 s max)"],
      [true, "Reconcile with disk and drop duplicates"],
      [true, "Bulk detection for git operations"],
      [false, "Follow dwell: at most one jump every 3 s"],
      [false, "Worktree labels in the feed"],
    ]),
    p(
      'See <a href="../old/spec.md">the old spec</a> for the first draft, the <a href="../world/lore.md#characters">lore notes</a> for naming, or the <a href="#cost-model">cost model</a> below.',
      2,
    ),
    h(2, "Cost model"),
    p(
      'Reconciling a batch of <span data-math-style="inline">n</span> files costs <span data-math-style="inline">O(n \\log n)</span> with the sorted merge.',
    ),
    p('<span data-math-style="display">\\int_0^1 x^2\\,dx = \\frac{1}{3}</span>'),
    p('A broken formula keeps its source: <span data-math-style="inline">\\frac{1}{</span>.'),
    h(2, "Notes"),
    raw(
      "<dl><dt>Bulk</dt><dd>A batch of more than 200 events, or any git operation.</dd><dt>Burst</dt><dd>Normal saves that arrive within ten seconds of each other.</dd></dl>",
      4,
    ),
    raw(
      "<details><summary>Why not poll?</summary><p>Polling wastes battery and misses short-lived files. The native watcher is cheaper and exact.</p></details>",
      3,
    ),
    alert("tip", "Use <kbd>F7</kbd> to jump between the blocks that just changed."),
    alert("warning", "Large checkouts can touch thousands of files; they are grouped on purpose."),
    alert("caution", "Do not edit <code>.trailmark/cache</code> by hand."),
    alert(
      "important",
      "The feed is keyed by path: renames move the entry, they do not duplicate it.",
    ),
    p(
      'The pipeline in one picture: <img src="../images/pipeline.svg" alt="Pipeline diagram" width="520" />',
      1,
    ),
    p(
      'H<sub>2</sub>O and E = mc<sup>2</sup>, plus <del>strikethrough</del> and an autolink: <a href="https://example.com/feed">https://example.com/feed</a>.',
    ),
    ...(changelog.length > 0 ? [h(2, "Changelog"), ul(changelog)] : []),
    raw(
      '<section class="footnotes" data-footnotes><ol><li id="fn-1"><p>Git operations are detected from the git directory, not from the file events. <a href="#fnref-1" class="footnote-backref" data-footnote-backref data-footnote-backref-idx="1" aria-label="Back to reference 1">↩</a></p></li></ol></section>',
      3,
    ),
  ];
}

const loreBlocks: Block[] = [
  p("Notes on the people and places of the series. Keep this file the single source of truth.", 1),
  h(2, "Characters"),
  p(
    "Character notes: tall, quiet, a cartographer who trusts maps more than people. Her descriptions change per book as she ages.",
  ),
  h(3, "Aria"),
  p(
    "Aria carries a brass compass. Physical <strong>descriptions</strong> must stay consistent with book one: grey eyes, a scar over the left brow.",
  ),
  h(3, "Bram"),
  p(
    "The character sheet lists his scars in order. Keep descriptions short; the narrator never describes him twice in one chapter.",
  ),
  h(2, "World"),
  p(
    "The coast is mapped in three layers: the harbor charts, the inland roads and the old tunnels. Each layer has its own style guide.",
  ),
  h(2, "Magic"),
  p(
    "Magic costs memory. Every spell removes one concrete detail from the caster's past, which is why descriptions of old places grow vague.",
  ),
  code("yaml", "cost:\n  memory: 1\n  recoverable: false"),
];

const styleGuideBlocks: Block[] = [
  h(2, "Writing"),
  h(3, "Character descriptions"),
  p(
    "Introduce a character with one concrete detail, not a list. Longer descriptions belong in lore.md.",
  ),
  h(2, "Review checklist"),
  p("Check every new character against the glossary before merging. Keep the tone consistent."),
];

const outlineBlocks: Block[] = [
  h(2, "Book one"),
  p(
    "Chapter 1 introduces Aria at the harbor. Character descriptions are kept to a single paragraph.",
  ),
  h(2, "Book two"),
  p("The tunnels. Bram's backstory returns; see the character sheet."),
  h(2, "Book three"),
  p("The memory cost of magic becomes the central problem."),
];

/** A drawing 1000 px wide, for the wide-image case of the reading-width breakout. */
const WIDE_IMAGE =
  "data:image/svg+xml," +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="160" viewBox="0 0 1000 160"><rect width="1000" height="160" rx="12" fill="#e8ecff"/><rect x="20" y="20" width="960" height="120" rx="8" fill="none" stroke="#4f6bed" stroke-width="2" stroke-dasharray="8 6"/><text x="500" y="88" text-anchor="middle" font-family="sans-serif" font-size="22" fill="#4f6bed">An image 1000 px wide</text></svg>',
  );

/**
 * Blocks that need more room than the text measure, between ordinary paragraphs (reading width, section 15).
 * A save (`rev` > 0) edits each wide block and drops one paragraph, to exercise glow and removed markers.
 */
const readingWidthBlocks = (rev: number): Block[] => [
  h(1, "Reading width"),
  p(
    "Text reads best in a column of about seventy characters, so paragraphs, lists, headings and quotes stay at the measure. Tables, code, diagrams, formulas and pictures often need more room than that, and they are allowed to take it.",
    3,
  ),
  h(2, "A table that needs room"),
  table(
    ["Event", "Source", "Debounce", "Grouping", "Feed row", "Toast", "Follow", "Notes"],
    [
      [
        "created",
        "file watcher",
        "150 ms",
        "by path",
        "new entry",
        "yes",
        "yes",
        rev > 0 ? "A new file is a changed file (edited)." : "A new file is a changed file.",
      ],
      [
        "modified",
        "file watcher",
        "150 ms",
        "by path",
        "updated in place",
        "yes",
        "yes",
        "Twelve saves stay one row.",
      ],
      [
        "removed",
        "file watcher",
        "150 ms",
        "by path",
        "kept, struck through",
        "yes",
        "no",
        "The row stays until the next scan.",
      ],
      [
        "checkout",
        "git operation",
        "none",
        "one bulk batch",
        "one collapsed row",
        "no",
        "no",
        "Never produces a toast or a jump.",
      ],
    ],
  ),
  p("A narrow table stays at the measure, because its content does not need more.", 1),
  table(
    ["Mode", "Width"],
    [
      ["Comfortable", "760 px"],
      ["Wide", "1100 px"],
      ["Full", "the pane"],
    ],
  ),
  h(2, "A long line of code"),
  code(
    "ts",
    `const summary = events.filter((event) => event.kind !== '${rev > 0 ? "deleted" : "removed"}').map((event) => ({ path: event.path, at: event.atMs, group: groupOf(event), hidden: isHiddenByFilter(event.path) })).sort((a, b) => b.at - a.at);`,
  ),
  ...(rev > 0 ? [] : [p("Short code stays at the measure too.", 1)]),
  code("ts", "const answer = 42;"),
  h(2, "A diagram"),
  code(
    "mermaid",
    `graph LR
  Watcher[File watcher] --> Debounce[Debounce 150 ms] --> Group[Group by path] --> Reconcile[Reconcile with disk] --> Feed[Feed row] --> Toast[Toast] --> Follow[Follow jump] --> Outline[Outline ${rev > 0 ? "refresh" : "update"}]`,
  ),
  h(2, "A formula"),
  p(
    '<span data-math-style="display">S_n = \\sum_{k=1}^{n} \\frac{(-1)^{k+1}}{k} \\cdot \\left(\\frac{a_k + b_k + c_k + d_k + e_k + f_k + g_k + h_k + i_k + j_k + l_k + m_k}{\\sqrt{1 + x_k^2 + y_k^2 + z_k^2}}\\right) + \\int_0^\\infty \\frac{e^{-t}}{1 + t^2}\\,dt</span>',
  ),
  h(2, "A wide picture"),
  p(`<img src="${WIDE_IMAGE}" alt="An image 1000 px wide" width="1000" />`),
  p(
    "Back to ordinary text: the paragraphs around the blocks above keep their measure, so the eye never has to cross a very long line.",
    2,
  ),
  ul(["Lists stay at the measure", "so do headings", "and quotes"]),
  quote("A wide block is the exception, not the layout.", "Design notes"),
];

/** Filler paragraphs, so that a section is tall enough to scroll through. */
const filler = (topic: string, count = 3): Block[] =>
  Array.from({ length: count }, (_, i) =>
    p(
      `${topic}, paragraph ${i + 1}. Text like this makes the section tall enough to scroll, so the outline has a section to follow: the agent writes, the reader scrolls, and the heading that is current moves with the page.`,
      2,
    ),
  );

/**
 * A document with nested headings for the outline: a skipped level, a repeated heading text and a heading too long
 * for the panel. A save (`rev` > 0) rewrites one paragraph under "Risks" and adds a section under "Phase two".
 */
const rolloutBlocks = (rev: number): Block[] => [
  h(1, "Rollout plan"),
  p(
    "A plan with nested sections, to try the outline: collapse, follow, click, and live changes.",
    2,
  ),
  h(2, "Phase one"),
  ...filler("Phase one", 2),
  h(3, "Scope"),
  ...filler("Phase one scope", 3),
  h(3, "Risks"),
  p(
    rev > 0
      ? `The main risk is a slow disk, and since revision ${rev} also a full one.`
      : "The main risk is a slow disk.",
    1,
  ),
  ul(["Slow disk", "Network drive", "Antivirus scanning the folder"]),
  h(4, "Mitigations"),
  ...filler("Mitigations", 2),
  h(2, "Phase two"),
  ...filler("Phase two", 2),
  h(3, "Scope"),
  ...filler("Phase two scope", 3),
  h(4, "A heading that skips a level"),
  ...filler("The skipped level", 2),
  ...(rev > 0 ? [h(3, `New in revision ${rev}`), ...filler("The new section", 2)] : []),
  h(
    2,
    "A heading so long that it cannot possibly fit in the narrow right panel without being cut short",
  ),
  ...filler("The long heading", 3),
  h(2, "Appendix"),
  ...filler("Appendix", 4),
];

/** 1,312 headings, for the cut at 1,000 ("Showing 1,000 of 1,312"). */
const longOutlineBlocks: Block[] = [
  h(1, "Every heading"),
  ...Array.from({ length: 1311 }, (_, i): Block[] => [
    h(i % 9 === 0 ? 2 : 3, `Heading ${i + 1}`),
    p(`Text under heading ${i + 1}.`, 1),
  ]).flat(),
];

const docs: Record<string, MockDoc> = {
  "docs/specs/live-feed.md": doc(
    "Live changes feed",
    (rev) => [h(1, "Live changes feed"), ...liveFeed(rev)],
    12 * SEC,
    {
      frontmatter: [
        ["status", "Draft"],
        ["owner", "planning-agent"],
        ["updated", "2026-10-13"],
        ["tags", "feed, watcher"],
      ],
    },
  ),
  "docs/specs/watcher.md": doc(
    "Watcher",
    [
      h(1, "Watcher"),
      p(
        'The watcher turns raw file events into batches for the <a href="live-feed.md#event-model">feed</a>. See also the <a href="../research/watchers.md">research notes</a>.',
      ),
      h(2, "Debounce"),
      p(
        "Events are held for 150 ms of quiet, with a hard limit of one second, then reconciled with the disk.",
      ),
      code(
        "rust",
        "fn flush(&mut self) -> Vec<FsChange> {\n    // reconcile with disk: stat + hash, drop duplicates\n    self.pending.drain().filter(|c| c.changed()).collect()\n}",
      ),
      h(2, "Limits"),
      table(
        ["Setting", "Value"],
        [
          ["quiet period", "150 ms"],
          ["max wait", "1 s"],
          ["bulk threshold", "200 events"],
        ],
      ),
    ],
    40 * MIN,
  ),
  "docs/specs/glossary.md": doc("Glossary", [
    h(1, "Glossary"),
    raw(
      "<dl><dt>Baseline</dt><dd>The point in time changes are measured against.</dd><dt>Bulk</dt><dd>A batch that is shown as one feed entry.</dd><dt>Worktree</dt><dd>A linked checkout of the same repository.</dd></dl>",
      4,
    ),
  ]),
  "docs/research/storage.md": doc(
    "Storage",
    [
      h(1, "Storage"),
      p(
        "State lives in AppData by default and in <code>.trailmark/</code> when the project opts in.",
      ),
      ul([
        "Local: nothing is written to the project",
        "Project: <code>config.toml</code> is shared, <code>local/</code> is ignored",
      ]),
    ],
    45 * MIN,
  ),
  "docs/research/watchers.md": doc(
    "Watchers",
    [
      h(1, "Watchers"),
      p("Comparison of file watching strategies on Windows, macOS and Linux."),
      h(2, "Native"),
      p(
        "ReadDirectoryChangesW is exact but can overflow on large trees; the overflow flag triggers a rescan.",
      ),
      h(2, "Polling"),
      p("Not used in the MVP; local disks only."),
    ],
    4 * MIN,
    { frontmatter: [["tags", "research, watcher"]] },
  ),
  "docs/research/index.md": doc(
    "Research",
    [
      h(1, "Research"),
      p('Start with <a href="storage.md">storage</a> or <a href="watchers.md">watchers</a>.'),
    ],
    2 * 24 * 60 * MIN,
    { encodingNote: "Decoded from UTF-16" },
  ),
  "docs/world/lore.md": doc("Lore", [h(1, "Lore"), ...loreBlocks], 26 * 60 * MIN),
  "docs/world/outline.md": doc("Outline", [h(1, "Outline"), ...outlineBlocks], 26 * 60 * MIN),
  "docs/world/style-guide.md": doc(
    "Style guide",
    [h(1, "Style guide"), ...styleGuideBlocks],
    26 * 60 * MIN,
  ),
  "docs/design/reading-width.md": doc("Reading width", readingWidthBlocks),
  "docs/design/outline.md": doc("Rollout plan", rolloutBlocks),
  "docs/design/long-outline.md": doc("Every heading", longOutlineBlocks),
  "docs/design/wireframes.md": doc("Wireframes", [
    h(1, "Wireframes"),
    p("Layouts for the welcome screen, the IDE mode and the search panel."),
    h(2, "IDE mode"),
    p("Activity bar, sidebar, document, right panel and a status bar."),
  ]),
  "plan.md": doc(
    "Plan",
    [
      h(1, "Plan"),
      p(
        'The work is split in three tasks: the <a href="docs/research/storage.md">core</a>, the UI and the integration.',
      ),
      tasks([
        [true, "Scaffold"],
        [false, "Core"],
        [false, "UI"],
      ]),
    ],
    90 * SEC,
  ),
  "README.md": doc("TrailMark docs", [
    h(1, "TrailMark docs"),
    p(
      'Specs live in <a href="docs/specs/live-feed.md">docs/specs</a>, research in <a href="docs/research/index.md">docs/research</a>. Open the <a href="plan.md">plan</a> for the current state.',
    ),
    p("Everything here is mock data for the UI."),
  ]),
  "CHANGELOG.md": doc("Changelog", [
    h(1, "Changelog"),
    h(2, "0.1.0"),
    ul(["First release candidate"]),
  ]),
  "assets/big-api.md": { kind: "tooLarge", title: null, blocks: () => [], age: 0 },
  "assets/report.pdf": { kind: "binary", title: null, blocks: () => [], age: 0 },
  "data/config.json": {
    kind: "json",
    title: null,
    age: 5 * 60 * MIN,
    blocks: () => [
      raw(
        `<pre><code class="language-json">${esc('{\n  "name": "trailmark-docs",\n  "version": "0.1.0",\n  "ignore": ["**/*.tmp", "build/"]\n}')}\n</code></pre>`,
        6,
      ),
    ],
  },
  "src/main.rs": {
    kind: "text",
    title: null,
    age: 5 * 60 * MIN,
    blocks: () => [code("rust", 'fn main() {\n    println!("hello from the mock vault");\n}')],
  },
  "@wt/feature-ui/README.md": doc("Feature UI", [
    h(1, "Feature UI"),
    p("A worktree checkout used by the UI agent."),
  ]),
  "@wt/feature-ui/docs/ui-notes.md": doc(
    "UI notes",
    [
      h(1, "UI notes"),
      p("Notes written by the UI agent in its own worktree."),
      ul(["Tokens first", "Then the shell", "Then the doc view"]),
    ],
    60 * SEC,
  ),
  "@wt/fix-links/docs/links.md": doc("Link fixes", [
    h(1, "Link fixes"),
    p("Normalizing <code>%20</code> and backslashes in links."),
  ]),
};

/** Documents of the single-file ("simple") mock vault. */
const simpleDocs: Record<string, MockDoc> = {
  "proposal.md": doc("Proposal: shared glossary", [
    h(1, "Proposal: shared glossary"),
    p(
      'We propose one glossary per repository, linked from every README. See <a href="template.md">the template</a>.',
      2,
    ),
    alert("tip", "Images next to this file load as usual."),
    h(2, "Next steps"),
    tasks([
      [true, "Draft the proposal"],
      [false, "Review with the team"],
    ]),
  ]),
  "template.md": doc("Glossary template", [
    h(1, "Glossary template"),
    p('Back to the <a href="proposal.md">proposal</a>.'),
  ]),
};

// ---------------------------------------------------------------------------------------------
// Mutable mock state (edited by the mock driver)
// ---------------------------------------------------------------------------------------------

export type MockVaultKind = "ide" | "simple";

export const mockState = {
  vault: "ide" as MockVaultKind,
  revisions: new Map<string, number>(),
  modified: new Map<string, number>(),
  deleted: new Set<string>(),
  created: new Set<string>(),
};

function registry(): Record<string, MockDoc> {
  return mockState.vault === "simple" ? simpleDocs : docs;
}

export function docExists(path: string): boolean {
  return path in registry() && !mockState.deleted.has(path);
}

/** The front matter pairs of a mock document, if it has any. */
export function mockFrontmatter(path: string): [string, string][] | null {
  return registry()[path]?.frontmatter ?? null;
}

export function getDocument(path: string): DocumentView {
  const entry = registry()[path];
  if (!entry || mockState.deleted.has(path)) throw new Error(`file not found: ${path}`);
  const rev = mockState.revisions.get(path) ?? 0;
  const blocks = [...entry.blocks(rev)];
  if (rev > 0 && entry.kind === "markdown" && path !== "docs/specs/live-feed.md") {
    blocks.push(p(`Edit ${rev}: this paragraph was added by the mock driver.`));
  }
  const hasFrontmatter = entry.frontmatter && entry.frontmatter.length > 0;
  let line = hasFrontmatter ? (entry.frontmatter?.length ?? 0) + 3 : 1;
  const html: string[] = [];
  const hashes: string[] = [];
  for (const block of blocks) {
    hashes.push(hashBlock(block.html));
    html.push(
      entry.kind === "markdown" ? withSourcepos(block.html, line, block.lines) : block.html,
    );
    line += block.lines + 1;
  }
  return {
    path,
    kind: entry.kind,
    html: html.join("\n"),
    blocks: hashes,
    title: entry.title,
    frontmatter: entry.frontmatter ?? null,
    modifiedMs: mockState.modified.get(path) ?? MOCK_NOW - entry.age,
    sizeBytes: new TextEncoder().encode(html.join("\n")).length,
    encodingNote: entry.encodingNote ?? null,
  };
}

/** Simulates an agent saving a document: bumps its revision and modification time. */
export function editDocument(path: string): void {
  mockState.revisions.set(path, (mockState.revisions.get(path) ?? 0) + 1);
  mockState.modified.set(path, Date.now());
  mockState.deleted.delete(path);
}

export function deleteDocument(path: string): void {
  mockState.deleted.add(path);
}

export function restoreDocument(path: string): void {
  mockState.deleted.delete(path);
  mockState.modified.set(path, Date.now());
}

/** Documents that exist in the current mock vault. */
export function allDocPaths(): string[] {
  return Object.keys(registry()).filter((p) => !mockState.deleted.has(p));
}

// ---------------------------------------------------------------------------------------------
// Vault, tree, git
// ---------------------------------------------------------------------------------------------

const dir = (path: string, children: TreeNode[]): TreeNode => ({
  name: path.split("/").pop() ?? path,
  path,
  kind: "dir",
  children,
});
const file = (path: string): TreeNode => ({
  name: path.split("/").pop() ?? path,
  path,
  kind: "file",
});

export const mockWorktrees = [
  {
    name: "feature-ui",
    path: "C:/Users/demo/trailmark-docs/.worktrees/feature-ui",
    branch: "feature/ui",
  },
  { name: "fix-links", path: "C:/Users/demo/trailmark-fix-links", branch: "fix/links" },
];

export const mockIdeVault: VaultInfo = {
  root: MOCK_ROOT,
  name: "trailmark-docs",
  mode: "ide",
  initialFile: null,
  worktrees: mockWorktrees,
  liveUpdates: true,
  treeDefault: null,
};

export const mockSimpleVault: VaultInfo = {
  root: "C:/Users/demo/Downloads",
  name: "Downloads",
  mode: "simple",
  initialFile: "proposal.md",
  worktrees: [],
  liveUpdates: true,
  treeDefault: null,
};

export function mockTree(): TreeNode {
  if (mockState.vault === "simple") {
    return {
      name: "Downloads",
      path: "",
      kind: "dir",
      children: [file("proposal.md"), file("template.md")],
    };
  }
  const exists = (p: string): boolean => !mockState.deleted.has(p);
  const keep = (nodes: TreeNode[]): TreeNode[] =>
    nodes.filter((n) => n.kind !== "file" || exists(n.path));
  return {
    name: "trailmark-docs",
    path: "",
    kind: "dir",
    children: [
      dir("assets", keep([file("assets/big-api.md"), file("assets/report.pdf")])),
      dir("data", [file("data/config.json")]),
      dir("docs", [
        dir("docs/design", [
          file("docs/design/long-outline.md"),
          file("docs/design/outline.md"),
          file("docs/design/reading-width.md"),
          file("docs/design/wireframes.md"),
        ]),
        dir("docs/images", [file("docs/images/pipeline.svg")]),
        dir("docs/research", [
          file("docs/research/index.md"),
          file("docs/research/storage.md"),
          file("docs/research/watchers.md"),
        ]),
        dir(
          "docs/specs",
          keep([
            file("docs/specs/glossary.md"),
            file("docs/specs/live-feed.md"),
            file("docs/specs/watcher.md"),
          ]),
        ),
        dir("docs/world", [
          file("docs/world/lore.md"),
          file("docs/world/outline.md"),
          file("docs/world/style-guide.md"),
        ]),
      ]),
      dir("src", [file("src/main.rs")]),
      file("CHANGELOG.md"),
      file("plan.md"),
      file("README.md"),
      {
        name: "feature-ui",
        path: "@wt/feature-ui",
        kind: "worktree",
        children: [
          dir("@wt/feature-ui/docs", [file("@wt/feature-ui/docs/ui-notes.md")]),
          file("@wt/feature-ui/README.md"),
        ],
      },
      {
        name: "fix-links",
        path: "@wt/fix-links",
        kind: "worktree",
        children: [dir("@wt/fix-links/docs", [file("@wt/fix-links/docs/links.md")])],
      },
    ],
  };
}

const HEAD_OID = "9f3c2e1d8e7b6a5f4c3d2e1f0a9b8c7d6e5f4a3b";

export const mockGit = {
  repoState: "clean" as GitSnapshot["repoState"],
  gitError: null as string | null,
  isRepo: true,
  generation: 1,
  baseline: "lastVisit" as "lastVisit" | "head",
};

export function mockSnapshot(kind: "lastVisit" | "head" = mockGit.baseline): GitSnapshot {
  if (mockState.vault === "simple") {
    return {
      generation: mockGit.generation,
      isRepo: false,
      gitError: null,
      head: { branch: null, oid: null, detached: false, unborn: false },
      repoState: "clean",
      baseline: { kind, oid: null },
      files: {},
      truncated: false,
    };
  }
  return {
    generation: mockGit.generation,
    isRepo: mockGit.isRepo,
    gitError: mockGit.gitError,
    head: { branch: "main", oid: HEAD_OID, detached: false, unborn: false },
    repoState: mockGit.repoState,
    baseline: {
      kind,
      oid: kind === "head" ? HEAD_OID : "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678",
    },
    files: mockGit.gitError
      ? {}
      : {
          "docs/specs/live-feed.md": { state: "modified", staged: false },
          "docs/specs/watcher.md": { state: "added", staged: true },
          "docs/research/storage.md": { state: "untracked", staged: false },
          "docs/research/watchers.md": { state: "modified", staged: false },
          "plan.md": { state: "modified", staged: false },
        },
    truncated: false,
  };
}

export function mockSince(): ChangedSince {
  if (mockState.vault === "simple") return { sinceMs: null, files: [] };
  const mtime = (p: string): number => mockState.modified.get(p) ?? MOCK_NOW - docs[p].age;
  return {
    sinceMs: MOCK_NOW - 3 * 60 * MIN,
    files: [
      {
        path: "docs/specs/live-feed.md",
        state: "modified",
        modifiedMs: mtime("docs/specs/live-feed.md"),
      },
      { path: "docs/specs/watcher.md", state: "added", modifiedMs: mtime("docs/specs/watcher.md") },
      {
        path: "docs/research/storage.md",
        state: "untracked",
        modifiedMs: mtime("docs/research/storage.md"),
      },
      {
        path: "docs/research/watchers.md",
        state: "modified",
        modifiedMs: mtime("docs/research/watchers.md"),
      },
      { path: "plan.md", state: "modified", modifiedMs: mtime("plan.md") },
    ],
  };
}

// ---------------------------------------------------------------------------------------------
// Links
// ---------------------------------------------------------------------------------------------

export function resolveMockLink(from: string, href: string): LinkTarget {
  if (href.startsWith("#")) return { kind: "anchor", anchor: href.slice(1) };
  if (/^[a-z][a-z0-9+.-]+:/i.test(href)) return { kind: "external", url: href };
  const hash = href.indexOf("#");
  const pathPart = hash < 0 ? href : href.slice(0, hash);
  const anchor = hash < 0 ? null : href.slice(hash + 1);
  const resolved = resolveRelative(from, pathPart);
  if (resolved === null) return { kind: "missing", path: pathPart };
  for (const candidate of [resolved, `${resolved}.md`]) {
    if (docExists(candidate)) return { kind: "internal", path: candidate, anchor };
  }
  return { kind: "missing", path: resolved };
}

// ---------------------------------------------------------------------------------------------
// Section search
// ---------------------------------------------------------------------------------------------

type Chunk = { path: string; headingPath: string[]; text: string; startLine: number };

function chunksOf(path: string): Chunk[] {
  const entry = registry()[path];
  if (!entry || entry.kind !== "markdown" || mockState.deleted.has(path)) return [];
  const blocks = entry.blocks(mockState.revisions.get(path) ?? 0);
  let line = entry.frontmatter ? entry.frontmatter.length + 3 : 1;
  const stack: { level: number; text: string }[] = [];
  const chunks: Chunk[] = [];
  let current: Chunk | null = null;
  for (const block of blocks) {
    if (block.heading) {
      while (stack.length > 0 && stack[stack.length - 1].level >= block.heading.level) stack.pop();
      stack.push(block.heading);
      current = { path, headingPath: stack.map((s) => s.text), text: block.text, startLine: line };
      chunks.push(current);
    } else {
      if (!current) {
        current = { path, headingPath: [], text: "", startLine: line };
        chunks.push(current);
      }
      current.text += ` ${block.text}`;
    }
    line += block.lines + 1;
  }
  return chunks;
}

function snippetOf(text: string, terms: string[]): string {
  const lower = text.toLowerCase();
  const first = Math.min(...terms.map((t) => lower.indexOf(t)).filter((i) => i >= 0));
  const start = Math.max(0, (Number.isFinite(first) ? first : 0) - 40);
  let slice = esc(text.slice(start, start + 170).trim());
  for (const term of terms) {
    slice = slice.replace(
      new RegExp(`(${term.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")})`, "gi"),
      "<mark>$1</mark>",
    );
  }
  return `${start > 0 ? "…" : ""}${slice}${start + 170 < text.length ? "…" : ""}`;
}

export function searchDocs(query: string, limit: number): SearchHit[] {
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (terms.length === 0) return [];
  const hits: SearchHit[] = [];
  for (const path of allDocPaths()) {
    if (splitWorktree(path).worktree !== null) continue;
    for (const chunk of chunksOf(path)) {
      const haystack = `${chunk.headingPath.join(" ")} ${chunk.text}`.toLowerCase();
      if (!terms.every((t) => haystack.includes(t))) continue;
      const score =
        terms.reduce((sum, t) => sum + haystack.split(t).length - 1, 0) +
        (chunk.headingPath.join(" ").toLowerCase().includes(terms[0]) ? 2 : 0);
      hits.push({
        path,
        headingPath: chunk.headingPath,
        snippet: snippetOf(chunk.text, terms),
        startLine: chunk.startLine,
        score,
      });
    }
  }
  return hits.sort((a, b) => b.score - a.score).slice(0, limit);
}

export const mockRecents = [
  { root: MOCK_ROOT, name: "trailmark-docs", lastOpenedMs: MOCK_NOW - 5 * MIN },
  { root: "F:/work/big-repo", name: "big-repo", lastOpenedMs: MOCK_NOW - 26 * 60 * MIN },
  { root: "D:/old/notes", name: "notes", lastOpenedMs: MOCK_NOW - 9 * 24 * 60 * MIN },
];
