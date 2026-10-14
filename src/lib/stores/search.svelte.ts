// State of the search panel: debounced queries, grouped results, keyboard position and indexing progress.

import { SvelteSet } from "svelte/reactivity";
import { api } from "../api";
import { errorText } from "../errors";
import { flattenResults, groupHits, hitKey, type HitGroup, type ResultRow } from "../searchModel";
import type { SearchHit } from "../types";

const DEBOUNCE_MS = 150;
const RESULT_LIMIT = 100;

export type SearchStatus = "idle" | "loading" | "ready" | "error";

class SearchStore {
  query = $state("");
  status = $state<SearchStatus>("idle");
  error = $state<string | null>(null);
  hits = $state.raw<SearchHit[]>([]);
  /** The query the current `hits` answer (the one to pre-fill the find bar with). */
  answered = $state("");
  collapsed = new SvelteSet<string>();
  /** Key of the active row: a group path or a hit key. */
  active = $state<string | null>(null);

  groups = $derived<HitGroup[]>(groupHits(this.hits));
  rows = $derived<ResultRow[]>(flattenResults(this.groups, this.collapsed));

  private timer: ReturnType<typeof setTimeout> | null = null;
  private seq = 0;

  rowKey(row: ResultRow): string {
    return row.type === "group" ? `group:${row.group.path}` : hitKey(row.hit);
  }

  setQuery(value: string): void {
    this.query = value;
    if (this.timer !== null) clearTimeout(this.timer);
    if (value.trim() === "") {
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
    const query = this.query.trim();
    if (query === "") {
      this.reset(false);
      return;
    }
    const seq = ++this.seq;
    this.status = "loading";
    try {
      const hits = await api.search(query, RESULT_LIMIT);
      if (seq !== this.seq) return;
      this.hits = hits;
      this.answered = query;
      this.status = "ready";
      this.error = null;
      this.collapsed.clear();
      this.active = this.rows.length > 0 ? this.rowKey(this.rows[0]) : null;
    } catch (error) {
      if (seq !== this.seq) return;
      this.status = "error";
      this.error = errorText(error);
    }
  }

  reset(clearQuery = true): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.seq++;
    if (clearQuery) this.query = "";
    this.hits = [];
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
