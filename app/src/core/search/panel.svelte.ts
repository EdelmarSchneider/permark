// State of the Search panel (one per project window): the words, the answer, the folded documents, the row the
// keyboard is on and how far the first index build is.

import type { Api, ApiError, IndexPhase, SectionHit, Unlisten } from "../../api";
import { flattenRows, groupHits, type PanelRow } from "./panelModel";

/** Wait after the last key before the backend is asked. */
export const PANEL_DEBOUNCE_MS = 150;
const LIMIT = 100;

export type Answer =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; hits: readonly SectionHit[]; partial: boolean }
  | { status: "failed"; error: ApiError };

export type Progress = { phase: IndexPhase; done: number; total: number };

export class SearchPanelStore {
  query = $state("");
  answer = $state.raw<Answer>({ status: "idle" });
  progress = $state.raw<Progress | null>(null);
  collapsed = $state.raw<ReadonlySet<string>>(new Set());
  active = $state(0);
  /** Also search what the person silenced (only offered when the silence plugin is on). */
  includeSilenced = $state(false);
  readonly #api: Api;
  #seq = 0;
  #timer: ReturnType<typeof setTimeout> | null = null;

  constructor(api: Api) {
    this.#api = api;
  }

  groups = $derived(this.answer.status === "ready" ? groupHits(this.answer.hits) : []);
  rows: readonly PanelRow[] = $derived(flattenRows(this.groups, this.collapsed));

  get current(): PanelRow | null {
    return this.rows[Math.min(this.active, this.rows.length - 1)] ?? null;
  }

  /** Listens to the index of the project at `root`; resolves to teardown. */
  async start(root: string): Promise<Unlisten> {
    const off = await this.#api.on("index-progress", (e) => {
      if (e.root !== root) return;
      this.progress = { phase: e.phase, done: e.done, total: e.total };
      // the answer was partial: it is worth asking again now that the index is complete
      if (e.phase === "ready" && this.answer.status === "ready" && this.answer.partial) {
        this.#schedule(0);
      }
    });
    const status = await this.#api.searchStatus();
    if (status.ok && this.progress === null) {
      this.progress = {
        phase: status.value.phase,
        done: status.value.done,
        total: status.value.total,
      };
    }
    return () => {
      off();
      this.dispose();
    };
  }

  setQuery(text: string): void {
    this.query = text;
    this.active = 0;
    this.#schedule(PANEL_DEBOUNCE_MS);
  }

  setIncludeSilenced(on: boolean): void {
    this.includeSilenced = on;
    this.#schedule(0);
  }

  /** Esc: the box empties and the answer goes. */
  clear(): void {
    this.query = "";
    this.#seq += 1;
    this.#cancel();
    this.answer = { status: "idle" };
    this.active = 0;
  }

  retry(): void {
    this.#schedule(0);
  }

  toggle(path: string): void {
    const next = new Set(this.collapsed);
    if (!next.delete(path)) next.add(path);
    this.collapsed = next;
  }

  move(delta: 1 | -1): void {
    const count = this.rows.length;
    if (count === 0) return;
    this.active = (Math.min(this.active, count - 1) + delta + count) % count;
  }

  dispose(): void {
    this.#seq += 1;
    this.#cancel();
  }

  #cancel(): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
  }

  #schedule(wait: number): void {
    this.#cancel();
    const seq = ++this.#seq;
    if (this.query.trim() === "") {
      this.answer = { status: "idle" };
      return;
    }
    const run = (): void => void this.#run(seq);
    if (wait === 0) run();
    else this.#timer = setTimeout(run, wait);
  }

  async #run(seq: number): Promise<void> {
    const query = this.query.trim();
    if (this.answer.status !== "ready") this.answer = { status: "loading" };
    const result = await this.#api.searchSections(query, LIMIT, {
      includeSilenced: this.includeSilenced,
      tags: [],
    });
    if (seq !== this.#seq) return;
    this.answer = result.ok
      ? { status: "ready", hits: result.value.hits, partial: result.value.partial }
      : { status: "failed", error: result.error };
  }
}
