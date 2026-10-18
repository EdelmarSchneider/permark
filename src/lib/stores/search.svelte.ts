// State of the search panel: debounced queries, grouped results, keyboard position and indexing progress.

import { SvelteSet } from "svelte/reactivity";
import { api } from "../api";
import { errorMessage } from "../errors";
import { flattenResults, groupHits, hitKey, type HitGroup, type ResultRow } from "../searchModel";
import { hasAllTags, unknownTags } from "../tagQuery";
import { TagQueryState } from "../tagQueryState.svelte";
import type { SearchHit } from "../types";
import { quiet } from "./quiet.svelte";
import { tags } from "./tags.svelte";

const DEBOUNCE_MS = 150;
const RESULT_LIMIT = 100;
/** With `tag:` filters the backend is asked for more, because the hits are filtered afterwards. */
const FILTERED_LIMIT = 200;

export type SearchStatus = "idle" | "loading" | "ready" | "error";

class SearchStore {
  /** The box: `tag:` chips and the text (which may end in a `tag:x` being typed). */
  readonly box = new TagQueryState(() => tags.counts);
  status = $state<SearchStatus>("idle");
  error = $state<string | null>(null);
  /** Hits before silenced paths are left out. */
  private found = $state.raw<SearchHit[]>([]);
  hits = $derived(
    quiet.includeQuiet ? this.found : this.found.filter((hit) => !quiet.isQuiet(hit.path)),
  );
  /** The query the current `hits` answer (the one to pre-fill the find bar with). */
  answered = $state("");
  collapsed = new SvelteSet<string>();
  /** Key of the active row: a group path or a hit key. */
  active = $state<string | null>(null);

  groups = $derived<HitGroup[]>(
    groupHits(this.hits, (path) => tags.index.get(path)?.description ?? ""),
  );
  /** The text in the box. */
  get query(): string {
    return this.box.text;
  }
  /** Tags nobody has, in the chips or being typed: the search cannot find anything. */
  unknown = $derived([
    ...unknownTags(this.box.chips, tags.known),
    ...(this.box.partial !== null && this.box.partial !== "" && this.box.suggestions.length === 0
      ? [this.box.partial]
      : []),
  ]);
  /** Only tags and no words: nothing to search for yet. */
  tagsOnly = $derived(this.box.chips.length > 0 && this.box.words === "");
  rows = $derived<ResultRow[]>(flattenResults(this.groups, this.collapsed));

  private timer: ReturnType<typeof setTimeout> | null = null;
  private seq = 0;

  rowKey(row: ResultRow): string {
    return row.type === "group" ? `group:${row.group.path}` : hitKey(row.hit);
  }

  setQuery(value: string): void {
    this.box.setText(value);
    this.runSoon();
  }

  /** A chip was taken out, or a suggested tag chosen. */
  changedChips(): void {
    this.runSoon();
  }

  private runSoon(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    if (this.box.isEmpty) {
      this.reset(false);
      return;
    }
    this.timer = setTimeout(() => void this.run(), DEBOUNCE_MS);
  }

  /** Enter in the query box: search right away. */
  runNow(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    void this.run();
  }

  async run(): Promise<void> {
    const query = this.box.words;
    const wanted = this.box.chips;
    if (query === "") {
      // nothing to search for (only tags, or nothing at all)
      this.reset(false);
      if (wanted.length > 0) this.status = "ready";
      return;
    }
    const seq = ++this.seq;
    this.status = "loading";
    try {
      const found = await api.search(query, wanted.length > 0 ? FILTERED_LIMIT : RESULT_LIMIT);
      if (seq !== this.seq) return;
      const hits =
        wanted.length > 0
          ? found
              .filter((h) => hasAllTags(tags.index.get(h.path)?.tags, wanted))
              .slice(0, RESULT_LIMIT)
          : found;
      this.found = hits;
      this.answered = query;
      this.status = "ready";
      this.error = null;
      this.collapsed.clear();
      this.active = this.rows.length > 0 ? this.rowKey(this.rows[0]) : null;
    } catch (error) {
      if (seq !== this.seq) return;
      this.status = "error";
      this.error = errorMessage(error);
    }
  }

  reset(clearQuery = true): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.seq++;
    if (clearQuery) this.box.clear();
    this.found = [];
    this.answered = "";
    this.status = "idle";
    this.error = null;
    this.active = null;
    this.collapsed.clear();
  }

  toggleGroup(path: string, collapse?: boolean): void {
    const shouldCollapse = collapse ?? !this.collapsed.has(path);
    if (shouldCollapse) this.collapsed.add(path);
    else this.collapsed.delete(path);
  }

  get activeIndex(): number {
    return this.rows.findIndex((r) => this.rowKey(r) === this.active);
  }

  move(delta: number): void {
    if (this.rows.length === 0) return;
    const i = this.activeIndex;
    const next = Math.min(this.rows.length - 1, Math.max(0, (i < 0 ? 0 : i) + delta));
    this.active = this.rowKey(this.rows[next]);
  }
}

export const search = new SearchStore();
