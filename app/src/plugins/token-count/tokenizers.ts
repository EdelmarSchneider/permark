// The tokenizers the person can choose: ids (as the settings and `.permark/config.toml` keep them), how they are told
// apart on screen (exact or approximate), and the effective choice of a project. Mirror of the Rust crate's table.

import { t, type MessageKey } from "../../core/i18n";

export type TokenizerId = "o200k" | "cl100k" | "claude" | "gemini" | "chars4";

/** Most tokenizers shown at once. */
export const MAX_TOKENIZERS = 3;

export type TokenizerInfo = {
  id: TokenizerId;
  /** What the status bar and the popover call it (product names stay; the rule of thumb is translated). */
  short: string;
  /** Counted by the model's own tokenizer, or estimated. */
  exact: boolean;
  /** The models it counts for. */
  label: MessageKey;
  /** One line on why it is exact or approximate. */
  why: MessageKey;
};

export const TOKENIZERS: readonly TokenizerInfo[] = [
  {
    id: "o200k",
    short: "GPT-4o",
    exact: true,
    label: "tokens.label.o200k",
    why: "tokens.why.exact",
  },
  {
    id: "cl100k",
    short: "GPT-4",
    exact: true,
    label: "tokens.label.cl100k",
    why: "tokens.why.exact",
  },
  {
    id: "claude",
    short: "Claude",
    exact: false,
    label: "tokens.label.claude",
    why: "tokens.why.claude",
  },
  {
    id: "gemini",
    short: "Gemini",
    exact: false,
    label: "tokens.label.gemini",
    why: "tokens.why.gemini",
  },
  {
    id: "chars4",
    // read at draw time: follows the interface language
    get short() {
      return t("tokens.short.chars4");
    },
    exact: false,
    label: "tokens.label.chars4",
    why: "tokens.why.chars4",
  },
];

export function infoOf(id: string): TokenizerInfo | null {
  return TOKENIZERS.find((info) => info.id === id) ?? null;
}

function known(ids: readonly string[]): TokenizerInfo[] {
  const out: TokenizerInfo[] = [];
  for (const id of ids) {
    const info = infoOf(id);
    if (info !== null && !out.includes(info) && out.length < MAX_TOKENIZERS) out.push(info);
  }
  return out;
}

/**
 * The tokenizers a project counts with: its own list when it names a known one, else the app's; unknown ids and
 * repeats dropped, at most three, first = primary.
 */
export function effective(app: readonly string[], project: readonly string[]): TokenizerInfo[] {
  const own = known(project);
  return own.length > 0 ? own : known(app);
}
