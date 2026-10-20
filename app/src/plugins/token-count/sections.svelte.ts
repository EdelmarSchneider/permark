// The tokens of each section of the outline, as the watcher last worked them out; the column reads them for the text on
// screen only (a figure made for other text is not shown).

import { documents } from "../../core/doc/document.svelte";
import { language, t } from "../../core/i18n";
import type { OutlineCell } from "../../core/plugins/registry.svelte";
import { tokens } from "./store.svelte";
import type { TokenizerId } from "./tokenizers";

type Sections = {
  /** The document the figures are of. */
  path: string;
  tokenizer: TokenizerId;
  approx: boolean;
  counts: readonly number[];
};

class SectionFigures {
  #latest = $state.raw<Sections | null>(null);

  set(next: Sections): void {
    this.#latest = next;
  }

  clear(): void {
    this.#latest = null;
  }

  /** The figures that belong to the page and the tokenizer now. */
  #current(): Sections | null {
    const latest = this.#latest;
    const primary = tokens.primary;
    if (latest === null || primary === null || latest.tokenizer !== primary.id) return null;
    return latest.path === documents.path ? latest : null;
  }

  header(): OutlineCell | null {
    const primary = tokens.primary;
    if (!(tokens.state?.outline ?? false) || primary === null || !tokens.ready(primary))
      return null;
    const current = this.#current();
    const approx = current?.approx ?? !primary.exact;
    return {
      text: `${approx ? "≈ " : ""}${t("tokens.outline.header")}`,
      title: t("tokens.outline.title", { tokenizer: primary.short }),
    };
  }

  cell(index: number): OutlineCell | null {
    const value = this.#current()?.counts[index];
    if (value === undefined) return null;
    return { text: new Intl.NumberFormat(language.locale).format(value) };
  }
}

export const sections = new SectionFigures();
