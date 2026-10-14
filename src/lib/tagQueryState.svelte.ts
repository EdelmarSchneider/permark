// The state of a search box that understands `tag:` filters: the chips of finished tokens, the text
// still in the box, and the suggestions for the token being typed. Quick open and Search each own one.

import { rankTags, type NamedCount, type TagMatch } from "./tagModel";
import { absorbTokens, parseTagQuery, withoutPartial } from "./tagQuery";
import { normalizeTag } from "./tagText";

const MAX_SUGGESTIONS = 8;

export class TagQueryState {
  /** Tags the results must have (finished `tag:x` tokens). */
  chips = $state.raw<string[]>([]);
  /** What is in the input box, including a `tag:x` token still being typed. */
  text = $state("");
  /** The highlighted suggestion. */
  active = $state(0);

  private readonly counts: () => readonly NamedCount[];

  constructor(counts: () => readonly NamedCount[]) {
    this.counts = counts;
  }

  /** The unfinished `tag:x` token ("" right after `tag:`), or null. */
  partial = $derived(parseTagQuery(this.text).partial);
  /** The words to search for: the text without any `tag:` token. */
  words = $derived(parseTagQuery(this.text).text);
  /** Tags that fit the token being typed (not the ones already chosen). */
  get suggestions(): TagMatch[] {
    const partial = this.partial;
    if (partial === null) return [];
    return rankTags(partial, this.counts(), new Set(this.chips)).slice(0, MAX_SUGGESTIONS);
  }

  /** Whether anything is typed or chosen. */
  get isEmpty(): boolean {
    return this.text.trim() === "" && this.chips.length === 0;
  }

  /** The input changed: finished `tag:x` tokens move into the chips. */
  setText(value: string): void {
    const next = absorbTokens(this.chips, value);
    this.chips = next.chips;
    this.text = next.text;
    this.active = 0;
  }

  /** A suggestion was chosen: it becomes a chip and the unfinished token leaves the box. */
  pick(name: string): void {
    if (!this.chips.includes(name)) this.chips = [...this.chips, name];
    this.text = withoutPartial(this.text);
    this.active = 0;
  }

  remove(name: string): void {
    this.chips = this.chips.filter((c) => c !== name);
  }

  clear(): void {
    this.chips = [];
    this.text = "";
    this.active = 0;
  }

  /**
   * Keys of the box that belong to the tag filters. Returns true when the key was used: Up and Down
   * move through the suggestions, Enter or Tab take one, and Backspace in an empty box drops the last chip.
   */
  handleKey(e: KeyboardEvent): boolean {
    const count = this.suggestions.length;
    if (this.partial !== null && count > 0 && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      this.active = (this.active + (e.key === "ArrowDown" ? 1 : -1) + count) % count;
      return true;
    }
    if (this.partial !== null && (e.key === "Enter" || e.key === "Tab") && !e.shiftKey) {
      const chosen = this.suggestions[this.active]?.tag.name ?? normalizeTag(this.partial);
      if (chosen === null) return false;
      this.pick(chosen);
      return true;
    }
    if (e.key === "Backspace" && this.text === "" && this.chips.length > 0) {
      this.remove(this.chips[this.chips.length - 1]);
      return true;
    }
    return false;
  }
}
