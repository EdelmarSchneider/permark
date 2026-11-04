<script lang="ts">
  // JSON Lines: one record per non-blank line, each line its own `parseJson` tree. Read-only; Source is the held
  // text. Trees are built only for the records near the screen. An unfinished `Held.open` line is not a record until
  // the load is `done` or `capped`. Jump asks `documentMore` until that record, the file ends, or `MAX_HELD_CHARS`.
  import { api } from "../../api";
  import { documents } from "../../core/doc/document.svelte";
  import {
    append,
    loading,
    megabytes,
    start,
    wantsMore,
    MAX_HELD_CHARS,
    type Held,
  } from "../../core/doc/largeText";
  import TreeRow from "../../core/doc/TreeRow.svelte";
  import { t } from "../../core/i18n";
  import type { ViewerProps } from "../../core/plugins/props";
  import { untrack } from "svelte";
  import { parseJson, type JsonParse } from "./json";
  import { heldText, lastIsFinal, recordsOf, type JsonlRecord } from "./jsonl";

  let { view }: ViewerProps = $props();

  /** Records drawn past the ones on screen (both sides). */
  const BUFFER = 8;
  /** Height of a record until it is measured. */
  const ROW = 28;

  type Mode = "tree" | "source";
  type Drawn = JsonlRecord & { position: number; parsed: JsonParse };

  const body = $derived(view.body.kind === "text" ? view.body : null);
  const partial = $derived(body?.partial ?? null);
  const text = $derived(body?.text ?? "");
  const first = $derived(start(view, text, partial === null ? null : partial.loaded));
  let grown = $state.raw<Held | null>(null);
  const held = $derived(grown !== null && grown.owner === view ? grown : first);
  const records = $derived(recordsOf(held.lines, lastIsFinal(held.open, held.load.status)));
  const source = $derived(heldText(held.lines, held.open));

  let chosen = $state<Mode>("tree");
  let query = $state("");
  let top = $state(0);
  let visible = $state(BUFFER * 2);
  let sizeState = $state.raw<{ owner: object; sizes: ReadonlyMap<number, number> } | null>(null);
  const sizes = $derived(
    sizeState !== null && sizeState.owner === view ? sizeState.sizes : new Map<number, number>(),
  );

  /** Pixel offset of each record, then the total. Rebuilt when the records or a measured height change. */
  const placed = $derived.by(() => {
    const next: number[] = [0];
    for (const rec of records) {
      const prev = next[next.length - 1] ?? 0;
      next.push(prev + (sizes.get(rec.index) ?? ROW));
    }
    return next;
  });

  const drawn = $derived.by(() => {
    if (chosen !== "tree") return [];
    const end = Math.min(records.length, top + visible);
    const startAt = Math.min(Math.max(top, 0), end);
    const out: Drawn[] = [];
    for (let position = startAt; position < end; position += 1) {
      const rec = records[position];
      if (!rec) continue;
      out.push({ ...rec, position, parsed: parseJson(rec.text) });
    }
    return out;
  });

  let host: HTMLElement | undefined = $state();
  let slice: HTMLElement | undefined = $state();
  /** Where the pane was while Tree was showing. Source shares the pane. */
  let treeScroll = 0;

  function pick(option: Mode): void {
    if (option === chosen) return;
    const pane = host?.closest<HTMLElement>("[data-scroller]");
    if (option === "source") {
      if (pane) treeScroll = pane.scrollTop;
      chosen = "source";
      return;
    }
    chosen = "tree";
    if (pane && pane.scrollTop !== treeScroll) pane.scrollTop = treeScroll;
  }

  function indexAt(prefix: readonly number[], y: number): number {
    if (y <= 0) return 0;
    let lo = 0;
    let hi = Math.max(0, prefix.length - 1);
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if ((prefix[mid] ?? 0) <= y) lo = mid;
      else hi = mid - 1;
    }
    return lo;
  }

  /** How many records cover `viewport` pixels from `start`, at least one. */
  function fitCount(prefix: readonly number[], start: number, viewport: number): number {
    const origin = prefix[start] ?? 0;
    let i = start;
    const last = Math.max(0, prefix.length - 1);
    while (i < last && (prefix[i] ?? 0) - origin < viewport) i += 1;
    return Math.max(1, i - start);
  }

  // records follow the pane only while Tree is showing. `chosen` is not a dependency: a scroll
  // or a clamp while Source is up must leave the window, and a new `view` for the same path
  // (this instance stays; DocumentArea keys on the path) must leave Source.
  $effect(() => {
    const element = host;
    const prefix = placed;
    if (!element) return;
    const pane = element.closest<HTMLElement>("[data-scroller]");
    if (!pane) return;
    const update = (): void => {
      if (untrack(() => chosen) !== "tree") return;
      const bar = element.querySelector(".bar")?.getBoundingClientRect().height ?? 0;
      const above = pane.scrollTop - element.offsetTop - bar;
      const at = indexAt(prefix, above);
      const nextTop = Math.max(0, at - BUFFER);
      top = nextTop;
      visible = Math.max(fitCount(prefix, nextTop, pane.clientHeight) + BUFFER, BUFFER * 2);
      queueMicrotask(() => void more());
    };
    update();
    pane.addEventListener("scroll", update, { passive: true });
    window.addEventListener("resize", update);
    return () => {
      pane.removeEventListener("scroll", update);
      window.removeEventListener("resize", update);
    };
  });

  function measure(root: HTMLElement): void {
    const owner = view;
    const current =
      sizeState !== null && sizeState.owner === owner ? sizeState.sizes : new Map<number, number>();
    const next = new Map(current);
    let changed = false;
    for (const el of root.querySelectorAll<HTMLElement>("[data-record]")) {
      const index = Number(el.dataset.record);
      const height = el.offsetHeight;
      if (!Number.isInteger(index) || height <= 0 || next.get(index) === height) continue;
      next.set(index, height);
      changed = true;
    }
    if (changed) sizeState = { owner, sizes: next };
  }

  // a record grows when its tree opens: the rows after it move down
  $effect(() => {
    const root = slice;
    if (!root || typeof ResizeObserver === "undefined") return;
    void drawn;
    const observer = new ResizeObserver(() => measure(root));
    for (const el of root.querySelectorAll("[data-record]")) observer.observe(el);
    measure(root);
    return () => observer.disconnect();
  });

  let pending: Promise<void> | null = null;

  function canLoadMore(mine: Held): boolean {
    return partial !== null && mine.next !== null && mine.load.status === "idle";
  }

  async function more(): Promise<void> {
    if (grown !== null && grown.owner !== view) grown = null;
    const mine = held;
    if (pending !== null || !canLoadMore(mine)) return;
    if (!wantsMore(mine, top, visible, records.length)) return;
    if (await next(mine)) void more();
  }

  async function next(mine: Held): Promise<boolean> {
    if (mine.next === null || pending !== null) return false;
    const asked = nextOf(mine, mine.next);
    pending = asked.then(() => undefined);
    try {
      return await asked;
    } finally {
      pending = null;
    }
  }

  async function nextOf(mine: Held, offset: number): Promise<boolean> {
    grown = loading(mine, { status: "loading" });
    const result = await api.documentMore(view.path, offset, view.size, view.mtimeMs);
    if (grown?.owner !== view || grown.load.status !== "loading") return false;
    if (!result.ok) {
      const gone = result.error.kind === "backend" && result.error.error.kind === "conflict";
      if (gone) documents.reload();
      grown = loading(mine, { status: gone ? "idle" : "failed" });
      return false;
    }
    // `append` stops at `MAX_HELD_CHARS` (`capped`). No second cap.
    grown = append(mine, result.value);
    return true;
  }

  function again(): void {
    if (grown?.owner === view) grown = loading(grown, { status: "idle" });
    void more();
  }

  /** A whole record number, or null when the field is not one. */
  function asked(raw: string): number | null {
    const trimmed = raw.trim();
    if (!/^[0-9]+$/.test(trimmed)) return null;
    const n = Number(trimmed);
    return Number.isSafeInteger(n) ? n : null;
  }

  function reveal(position: number): void {
    if (position < 0 || position >= records.length) return;
    top = Math.max(0, position - BUFFER);
    const need = position - top + BUFFER + 1;
    if (need > visible) visible = need;
    const pane = host?.closest<HTMLElement>("[data-scroller]");
    if (!pane || !host) return;
    const bar = host.querySelector(".bar")?.getBoundingClientRect().height ?? 0;
    const y = (placed[position] ?? 0) + bar;
    pane.scrollTop = Math.max(0, host.offsetTop + y - pane.clientHeight / 3);
  }

  async function jumpTo(n: number | null): Promise<void> {
    if (n === null || n < 1) return;
    const owner = view;
    for (;;) {
      while (pending !== null) await pending;
      if (view !== owner) return;
      const mine = held;
      const found = recordsOf(mine.lines, lastIsFinal(mine.open, mine.load.status));
      if (n <= found.length) {
        reveal(n - 1);
        return;
      }
      // past the records that are final, and nothing more to load: no move
      if (!canLoadMore(mine)) return;
      const came = await next(mine);
      if (!came) {
        if (pending !== null) continue;
        return;
      }
    }
  }
</script>

<div class="jsonl" bind:this={host}>
  <div class="bar">
    <div class="segments" role="radiogroup" aria-label={t("structured.mode")}>
      {#each ["tree", "source"] as const as option (option)}
        <button
          type="button"
          class="segment"
          role="radio"
          aria-checked={chosen === option}
          onclick={() => pick(option)}
        >
          {t(option === "tree" ? "structured.tree" : "structured.source")}
        </button>
      {/each}
    </div>
    <form
      class="jump"
      onsubmit={(event) => {
        event.preventDefault();
        void jumpTo(asked(query));
      }}
    >
      <label>
        {t("jsonl.jump")}
        <input
          type="text"
          inputmode="numeric"
          autocomplete="off"
          spellcheck="false"
          bind:value={query}
        />
      </label>
      <button type="submit">{t("jsonl.go")}</button>
    </form>
  </div>
  {#if chosen === "source"}
    <pre class="source"><code>{source}</code></pre>
  {:else if records.length === 0}
    <p class="empty">{t("jsonl.empty")}</p>
  {:else}
    <div class="virtual" style:height={`${placed[records.length] ?? 0}px`} role="list">
      <div class="slice" bind:this={slice}>
        {#each drawn as item (item.index)}
          <article
            class="record"
            class:bad={!item.parsed.ok}
            data-record={item.index}
            data-line={item.line}
            style:transform={`translateY(${placed[item.position] ?? 0}px)`}
            role="listitem"
          >
            <span class="index">{item.index}</span>
            {#if item.parsed.ok}
              <div class="tree" role="tree">
                <TreeRow node={item.parsed.root} depth={0} open />
              </div>
            {:else}
              <p class="fail">{t("jsonl.invalid", { line: item.line, at: item.parsed.at + 1 })}</p>
            {/if}
          </article>
        {/each}
      </div>
    </div>
  {/if}
  {#if partial !== null}
    <p class="note" role="status">
      {#if held.load.status === "loading"}
        {t("text.loadingMore")}
      {:else if held.load.status === "failed"}
        {t("text.loadFailed")}
        <button type="button" class="btn btn--sm" onclick={again}>{t("text.retry")}</button>
      {:else if held.load.status === "capped"}
        {t("text.capped", { held: megabytes(MAX_HELD_CHARS), total: megabytes(partial.total) })}
      {:else if held.load.status === "done"}
        {t("text.allLoaded", { total: megabytes(partial.total) })}
      {:else}
        {t("text.partial", { held: megabytes(held.chars), total: megabytes(partial.total) })}
      {/if}
    </p>
  {/if}
</div>

<style>
  .jsonl {
    padding: var(--sp-3) var(--sp-5) var(--sp-8);
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
    min-height: 28px;
    margin-bottom: var(--sp-2);
  }
  .segments {
    display: inline-grid;
    grid-template-columns: repeat(2, auto);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    overflow: hidden;
  }
  .segment {
    height: 24px;
    padding: 0 12px;
    border: 0;
    border-right: 1px solid var(--border);
    background: var(--bg);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    cursor: pointer;
  }
  .segment:last-child {
    border-right: 0;
  }
  .segment[aria-checked="true"] {
    background: var(--accent-soft);
    color: var(--accent-text);
    font-weight: 600;
  }
  .jump {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .jump input {
    width: 5em;
    margin-left: 6px;
    padding: 2px 6px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: inherit;
  }
  .jump button {
    height: 24px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font-size: var(--fs-ui-sm);
    cursor: pointer;
  }
  .virtual {
    position: relative;
  }
  .record {
    position: absolute;
    left: 0;
    right: 0;
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 8px;
    align-items: start;
    padding: 2px 0;
    border-top: 1px solid var(--border);
  }
  .record.bad {
    background: var(--mark);
  }
  .index {
    min-width: 2.5em;
    padding-top: 1px;
    color: var(--muted);
    font: 12px/1.55 var(--font-code);
    text-align: right;
  }
  .fail,
  .empty,
  .note {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .fail {
    color: var(--text);
  }
  .note {
    margin-top: var(--sp-2);
  }
  .source {
    margin: 0;
    padding: 12px 14px;
    overflow-x: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--surface);
    font: var(--fs-code) / 1.6 var(--font-code);
    white-space: pre;
  }
</style>
