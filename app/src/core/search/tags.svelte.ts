// The tags a search is narrowed to, and the list of tags the finder can offer. The `tags` plugin feeds both (nothing
// while it is off); the Search panel, Quick open and Search everywhere read them. Core knows only names and counts.

import { fuzzyMatch } from "./fuzzy";

export type TagHit = { name: string; count: number };

/** What the tags plugin offers the finder: every tag with the number of files that have it. */
export type TagProvider = { list(): readonly TagHit[] };

export class SearchTags {
  /** The tags a search is narrowed to: files that have all of them. */
  selected = $state.raw<readonly string[]>([]);
  provider = $state.raw<TagProvider | null>(null);

  /** The plugin is on. */
  get available(): boolean {
    return this.provider !== null;
  }

  setProvider(provider: TagProvider | null): void {
    this.provider = provider;
    if (provider === null) this.selected = [];
  }

  add(name: string): void {
    if (!this.selected.includes(name)) this.selected = [...this.selected, name];
  }

  remove(name: string): void {
    this.selected = this.selected.filter((n) => n !== name);
  }

  toggle(name: string): void {
    if (this.selected.includes(name)) this.remove(name);
    else this.add(name);
  }

  clear(): void {
    if (this.selected.length > 0) this.selected = [];
  }

  /** The tags whose name matches the words (all of them for an empty box), best first, not yet selected first. */
  hits(query: string): readonly TagHit[] {
    const all = this.provider?.list() ?? [];
    const text = query.trim();
    if (text === "") return all.slice(0, 200);
    const ranked: { hit: TagHit; score: number }[] = [];
    for (const hit of all) {
      const found = fuzzyMatch(text, hit.name);
      if (found) ranked.push({ hit, score: found.score });
    }
    ranked.sort((a, b) => b.score - a.score || b.hit.count - a.hit.count);
    return ranked.slice(0, 200).map((r) => r.hit);
  }
}

export const searchTags = new SearchTags();
