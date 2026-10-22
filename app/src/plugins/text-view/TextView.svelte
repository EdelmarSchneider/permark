<script lang="ts">
  // A text file: line numbers, monospace, highlighted by its extension when the highlight plugin is on. A very long
  // file shows only the lines near the screen (plain), so it opens at once and never freezes the window; a file too
  // large to read whole shows its first part and asks for the rest as the reader comes to it. Lines that changed on
  // disk glow for a moment; a line asked for (a code reference) comes into view, tinted a moment. Secret values are
  // masked until clicked. Long lines wrap (the setting): a wrapped line goes on a little to the right, its number on
  // its first row only; a very long file counts its rows without drawing them (wrapRows.ts) and draws only the rows
  // near the screen, even of a single line of many megabytes. Switching wraps keeps the line at the top in place.
  import { tick, untrack } from "svelte";
  import { api } from "../../api";
  import { conditions } from "../../core/conditions/context.svelte";
  import {
    findCodeSecrets,
    MASK,
    privateKeyLines,
    publicKeyLines,
    type Span,
  } from "../../core/config/secrets";
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
  import { t } from "../../core/i18n";
  import { look } from "../../core/look.svelte";
  import { contributions } from "../../core/plugins/instance.svelte";
  import type { ViewerProps } from "../../core/plugins/props";
  import {
    anchorAt,
    intoLine,
    leadFor,
    measure,
    MAX_LEAD,
    MIN_COLS,
    piecesFor,
    rowStarts,
    rowsRange,
    widthsOf,
    type Anchor,
    type Piece,
    type Widths,
  } from "./wrapRows";
  import { longLineSpans, LONG_LINE, type WindowCache } from "./longSecrets";

  let { view }: ViewerProps = $props();

  /** From this many lines (or characters) on, only the lines in view are drawn. */
  const VIRTUAL_FROM = 20_000;
  const VIRTUAL_CHARS = 200_000;
  /** Line height in pixels until it is measured (the code font size times 1.6, as the CSS sets it). */
  const ROW = 21.6;
  const BUFFER = 40;
  /** The padding above the first line of the plain view. */
  const PAD = 14;
  /** Wrapped: a line of at most this many rows is drawn whole; a longer one only by its rows near the screen. */
  const WHOLE_ROWS = 200;
  /** Room right of a wrapped row (pixels). */
  const PAD_RIGHT = 16;
  /** The text whose width gives the width of one character. */
  const PROBE = "0000000000";

  const body = $derived(view.body.kind === "text" ? view.body : null);
  const partial = $derived(body?.partial ?? null);
  const text = $derived(body?.text ?? "");
  // what is held: the text of the view, and the chunks that came after it (kept for this view only)
  const first = $derived(start(view, text, partial === null ? null : partial.loaded));
  let grown = $state.raw<Held | null>(null);
  const held = $derived(grown !== null && grown.owner === view ? grown : first);
  const lines = $derived(held.lines);
  const count = $derived(lines.length);
  const virtual = $derived(
    partial !== null || count >= VIRTUAL_FROM || text.length > VIRTUAL_CHARS,
  );
  const wrap = $derived(look.wrap);
  const numbers = $derived(
    virtual || wrap ? "" : Array.from({ length: Math.max(count, 1) }, (_, i) => i + 1).join("\n"),
  );
  const digits = $derived(String(Math.max(count, 1)).length);
  const code = $derived(
    contributions.visible("embed", conditions.current).find((e) => e.language === "@code"),
  );

  let host: HTMLElement | undefined = $state();
  let codeBox: HTMLElement | undefined = $state();
  /** First row drawn and how many (a row = a line, or a row of a wrapped line). */
  let top = $state(0);
  let visible = $state(60);
  let row = $state(ROW);
  /** Characters in a wrapped row (measured; until then a usual width). */
  let cols = $state(80);
  /** Goes up when the wrapped lines of a smaller file may have moved (their marks are placed again). */
  let layout = $state(0);

  // ---- rows of a very long text that wraps: counted, not drawn ----------------------------------------------------

  /** The columns of each line, kept while chunks come (only a wrapping very long text needs them). */
  let widthCache: ({ lines: readonly string[] } & Widths) | null = null;
  const widths = $derived.by((): Widths | null => {
    if (!virtual || !wrap) return null;
    const next = widthsOf(lines, widthCache);
    widthCache = { lines, ...next };
    return next;
  });
  const starts = $derived(widths === null ? null : rowStarts(widths.widths, cols, widths.leads));

  /** The indentation the further rows of line `i` (0-based) keep. */
  function leadOf(i: number): number {
    return widths === null ? 0 : leadFor(widths.leads[i] ?? 0, cols);
  }
  const totalRows = $derived(starts === null ? count : (starts[count] ?? 0));

  /** Rows above line `i` (0-based). */
  function rowStart(i: number): number {
    const at = Math.min(Math.max(i, 0), count);
    return starts === null ? at : (starts[at] ?? 0);
  }

  /** What is drawn: pieces of lines (a whole line, or some rows of a very long one) and the row they start at. */
  const drawn = $derived.by((): { top: number; pieces: Piece[] } => {
    if (starts !== null) return piecesFor(starts, top, visible, WHOLE_ROWS);
    const from = Math.min(top, count);
    const to = Math.min(count, top + visible);
    return {
      top: from,
      pieces: Array.from({ length: to - from }, (_, i) => ({ line: from + i, from: 0, to: 1 })),
    };
  });

  /** A piece that is a whole line (its secrets masked) rather than some rows of a very long one. */
  function whole(piece: Piece): boolean {
    return (
      starts === null ||
      (piece.from === 0 && piece.to === rowStart(piece.line + 1) - rowStart(piece.line))
    );
  }

  /** Where some rows of a very long line start and end, in characters. */
  function rangeOf(piece: Piece): [number, number] {
    const raw = lines[piece.line] ?? "";
    const line = raw.endsWith("\r") ? raw.slice(0, -1) : raw;
    return rowsRange(line, piece.from, piece.to, cols, widths?.leads[piece.line] ?? 0);
  }

  /** The indentation the further rows of a line of a smaller file keep. */
  function leadSmall(raw: string): number {
    return Math.min(measure(raw).lead, MAX_LEAD);
  }

  /** A line of the plain view without its carriage return, with its line break (a copy keeps one per line). */
  function plain(raw: string): string {
    return `${raw.endsWith("\r") ? raw.slice(0, -1) : raw}\n`;
  }

  // the rows in view follow the pane that scrolls (the document area's)
  $effect(() => {
    const element = host;
    if (!element) return;
    const measured = parseFloat(getComputedStyle(element).lineHeight) || ROW;
    untrack(() => (row = measured));
    const pane = element.closest<HTMLElement>("[data-scroller]");
    if (!virtual || !pane) return;
    const update = (): void => {
      const above = pane.scrollTop - element.offsetTop;
      top = Math.max(0, Math.floor(above / measured) - BUFFER);
      visible = Math.ceil(pane.clientHeight / measured) + BUFFER * 2;
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

  // a very long text that wraps: the characters a row holds follow the width of the pane
  $effect(() => {
    const element = host;
    if (!element || !virtual || !wrap) return;
    const fit = (): void => {
      const probe = element.querySelector<HTMLElement>(".probe");
      const char = (probe?.getBoundingClientRect().width ?? 0) / PROBE.length;
      // no layout (a hidden tab): the count stays until there is one
      if (!(char > 0)) return;
      // the text starts after the number and its column
      const line = element.querySelector<HTMLElement>(".row .line");
      const left = line
        ? line.getBoundingClientRect().left -
          element.getBoundingClientRect().left -
          element.clientLeft
        : 0;
      const room = element.clientWidth - left - PAD_RIGHT - 1;
      cols = Math.max(MIN_COLS, Math.floor(room / char));
    };
    fit();
    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", fit);
      return () => window.removeEventListener("resize", fit);
    }
    const observer = new ResizeObserver(fit);
    observer.observe(element);
    return () => observer.disconnect();
  });

  // a smaller file that wraps: its rows move when the width changes, the marks over them follow
  $effect(() => {
    const element = codeBox;
    if (!element || virtual || !wrap || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => (layout += 1));
    observer.observe(element);
    return () => observer.disconnect();
  });

  // ---- where a line is: the marks over it, a line asked for, the line kept at the top ----------------------------

  /** Where lines `first` .. `last` (1-based) are, in pixels from the top of what draws them. */
  function place(firstLine: number, lastLine: number): { top: number; height: number } {
    if (virtual) {
      const a = rowStart(firstLine - 1);
      const b = rowStart(lastLine);
      return { top: a * row, height: Math.max(1, b - a) * row };
    }
    if (wrap) {
      void layout;
      const box = drawnLines(firstLine, lastLine);
      if (box !== null) return box;
    }
    return { top: PAD + (firstLine - 1) * row, height: (lastLine - firstLine + 1) * row };
  }

  /** Where wrapped lines of a smaller file are drawn (`null`: not drawn yet, or no layout). */
  function drawnLines(firstLine: number, lastLine: number): { top: number; height: number } | null {
    const box = codeBox;
    if (!box) return null;
    const all = box.querySelectorAll<HTMLElement>(".ln");
    const a = all[firstLine - 1];
    const b = all[Math.min(lastLine, all.length) - 1];
    if (!a || !b) return null;
    const from = a.getBoundingClientRect();
    if (from.height === 0) return null;
    return {
      top: from.top - box.getBoundingClientRect().top,
      height: b.getBoundingClientRect().bottom - from.top,
    };
  }

  /** The edge under line `after` (1-based; 0 = above the first line). */
  function below(after: number): number {
    if (after <= 0) return place(1, 1).top;
    const at = place(after, after);
    return at.top + at.height;
  }

  /** What draws the lines (the marks are placed in it). */
  function linesBox(): HTMLElement | null {
    return (virtual ? host?.querySelector<HTMLElement>(".virtual") : codeBox) ?? null;
  }

  /** The line at the top of the pane as drawn now, and how far into it the pane starts. */
  function anchorNow(): Anchor | null {
    const pane = host?.closest<HTMLElement>("[data-scroller]");
    const drawnBox = linesBox();
    if (!host || !pane || !drawnBox) return null;
    const edge = pane.getBoundingClientRect().top;
    const boxTop = drawnBox.getBoundingClientRect().top;
    if (boxTop >= edge) return null;
    const rows = drawnBox.querySelectorAll<HTMLElement>(".row[data-line]");
    for (const element of rows) {
      const rect = element.getBoundingClientRect();
      // a row showing less than a pixel is passed over (rounding never anchors the line above)
      if (rect.bottom <= edge + 1 || rect.height === 0) continue;
      const line = Number(element.dataset.line);
      const from = Number(element.dataset.from);
      return { line, delta: edge - rect.top + from * row };
    }
    const spans = drawnBox.querySelectorAll<HTMLElement>(".ln");
    if (spans.length > 0) {
      // the first line whose bottom is under the edge
      let lo = 0;
      let hi = spans.length - 1;
      while (lo < hi) {
        const mid = (lo + hi) >> 1;
        if ((spans[mid]?.getBoundingClientRect().bottom ?? 0) > edge + 1) hi = mid;
        else lo = mid + 1;
      }
      const rect = spans[lo]?.getBoundingClientRect();
      if (rect && rect.height > 0) return { line: lo + 1, delta: edge - rect.top };
    }
    return anchorAt(edge - boxTop - (virtual ? 0 : PAD), row, count);
  }

  /**
   * Brings the anchored line back to the top of the pane: after a switch of wrapping, at its first row (where the
   * person read from); after a new width, as far into it as it was.
   */
  function restore(anchor: Anchor, switched: boolean): void {
    const pane = host?.closest<HTMLElement>("[data-scroller]");
    const drawnBox = linesBox();
    if (!pane || !drawnBox) return;
    const at = place(anchor.line, anchor.line);
    const into = switched
      ? intoLine(anchor.delta, row)
      : Math.max(0, Math.min(anchor.delta, at.height - 1));
    const y =
      drawnBox.getBoundingClientRect().top - pane.getBoundingClientRect().top + pane.scrollTop;
    pane.scrollTop = Math.max(0, y + at.top + into);
  }

  // wrapping switched | the rows got wider or narrower: the line at the top stays at the top
  let shownAs: { wrap: boolean; cols: number } | null = null;
  let anchor: { at: Anchor; switched: boolean } | null = null;
  $effect.pre(() => {
    const now = { wrap, cols: virtual && wrap ? cols : 0 };
    untrack(() => {
      if (shownAs !== null && (now.wrap !== shownAs.wrap || now.cols !== shownAs.cols)) {
        // a change before the last one was restored keeps that one's line (the drawing moved, the person did not)
        const at = anchor?.at ?? anchorNow();
        const switched = now.wrap !== shownAs.wrap || anchor?.switched === true;
        anchor = at === null ? null : { at, switched };
      }
      shownAs = now;
    });
  });
  $effect(() => {
    void wrap;
    void cols;
    untrack(() => {
      const kept = anchor;
      if (kept === null) return;
      void tick().then(() => {
        // a newer change took over meanwhile: that one restores
        if (anchor !== kept) return;
        anchor = null;
        restore(kept.at, kept.switched);
      });
    });
  });

  /** The request for a chunk on its way (one at a time). */
  let pending: Promise<void> | null = null;

  /** Asks for the next chunk when the rows in view are close to the end of what is held. */
  async function more(): Promise<void> {
    // lines of a version of the file that has gone are let go
    if (grown !== null && grown.owner !== view) grown = null;
    const mine = held;
    if (pending !== null || partial === null || mine.next === null) return;
    if (!wantsMore(mine, top, visible, totalRows)) return;
    // only a chunk that came goes on (a failed | superseded one waits for the person | the new version)
    if (await next(mine)) void more();
  }

  /** Asks for the chunk after what `mine` holds; true when it came. */
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
    // another version of the file came meanwhile: that one starts again
    if (grown?.owner !== view || grown.load.status !== "loading") return false;
    if (!result.ok) {
      const gone = result.error.kind === "backend" && result.error.error.kind === "conflict";
      if (gone) documents.reload();
      grown = loading(mine, { status: gone ? "idle" : "failed" });
      return false;
    }
    grown = append(mine, result.value);
    return true;
  }

  function again(): void {
    if (grown?.owner === view) grown = loading(grown, { status: "idle" });
    void more();
  }

  // ---- secrets: a value of a secret-looking key is a mask until it is clicked ----------------------------------

  let shownSecrets = $state.raw<{ view: object | null; keys: ReadonlySet<string> }>({
    view: null,
    keys: new Set(),
  });

  type Part = { text: string; secret: string | null };

  /** Lines (0-based) inside a private key block: masked whole (a line alone does not show it is one). */
  const keyBody = $derived(privateKeyLines(lines));
  /** Lines of certificates | public keys: never masked. */
  const publicBody = $derived(publicKeyLines(lines));

  /** The window scans of the very long lines of this text (line, 0-based -> its windows). */
  let longScans: { lines: readonly string[]; byLine: Map<number, WindowCache> } | null = null;

  /** Secret spans of line `i` (0-based) that reach into `from` .. `to`. */
  function spansOf(line: string, i: number, from: number, to: number): Span[] {
    if (keyBody.has(i)) return [{ from: line.length - line.trimStart().length, to: line.length }];
    if (publicBody.has(i)) return [];
    if (line.length <= LONG_LINE) return findCodeSecrets(line);
    if (longScans?.lines !== lines) longScans = { lines, byLine: new Map() };
    let cache = longScans.byLine.get(i);
    if (cache === undefined) {
      cache = new Map();
      longScans.byLine.set(i, cache);
    }
    return longLineSpans(line, from, to, cache);
  }

  /**
   * A line, or the part `range` of it (some rows of a very long one), cut where its secret values are (a line of CRLF
   * text loses its carriage return). A value begun on an earlier row stays hidden: its mask is drawn where it starts.
   */
  function pieces(raw: string, no: number, range?: [number, number]): Part[] {
    const line = raw.endsWith("\r") ? raw.slice(0, -1) : raw;
    const [start, end] = range ?? [0, line.length];
    const spans = spansOf(line, no - 1, start, end);
    if (spans.length === 0) return [{ text: line.slice(start, end), secret: null }];
    const open = shownSecrets.view === view ? shownSecrets.keys : new Set<string>();
    const out: Part[] = [];
    let at = start;
    for (const span of spans) {
      if (span.to <= start) continue;
      if (span.from >= end) break;
      const from = Math.max(span.from, start);
      const to = Math.min(span.to, end);
      out.push({ text: line.slice(at, from), secret: null });
      const key = `${no}:${span.from}`;
      if (open.has(key)) out.push({ text: line.slice(from, to), secret: null });
      else if (span.from >= start) out.push({ text: MASK, secret: key });
      at = to;
    }
    out.push({ text: line.slice(at, end), secret: null });
    return out;
  }

  function reveal(key: string): void {
    const keys = shownSecrets.view === view ? shownSecrets.keys : new Set<string>();
    shownSecrets = { view, keys: new Set([...keys, key]) };
  }

  // ---- glow: the lines that changed on disk ---------------------------------------------------------------------

  const glow = $derived(documents.lineGlow?.path === view.path ? documents.lineGlow : null);

  // ---- a line asked for: at the top third of the pane, tinted a moment ---------------------------------------------

  /** A large text loads at most this many more parts to reach a line; the rest comes by scrolling. */
  const MAX_PARTS = 10;
  const LOCATE_MS = 1600;
  /** The band covers the line, at most this many rows of it (a line of many rows: its start). */
  const BAND_ROWS = 4;
  let located = $state.raw<{ line: number; seq: number } | null>(null);
  let handled = 0;
  let locateTimer: ReturnType<typeof setTimeout> | undefined;
  // the tint's clock goes with the viewer
  $effect(() => () => clearTimeout(locateTimer));

  $effect(() => {
    const request = documents.lineRequest;
    if (!request || !host || request.path !== view.path || request.seq === handled) return;
    handled = request.seq;
    untrack(() => void bring(request.line, request.seq));
  });

  async function bring(line: number, seq: number): Promise<void> {
    const owner = view;
    for (let parts = 0; parts < MAX_PARTS; parts++) {
      while (pending !== null) await pending;
      if (view !== owner || handled !== seq) return;
      const mine = held;
      if (partial === null || mine.lines.length >= line || mine.next === null) break;
      if (mine.load.status === "failed" || mine.load.status === "capped") break;
      if (!(await next(mine))) break;
    }
    if (view !== owner || handled !== seq) return;
    const at = Math.min(line, Math.max(held.lines.length, 1));
    // a line past the end of a whole file is its last line, no note; a part not loaded yet is said
    if (at < line && held.next !== null) documents.notify(t("codeRef.lineFar", { line }));
    await tick();
    const pane = host?.closest<HTMLElement>("[data-scroller]");
    const drawnBox = linesBox();
    if (host && pane) {
      const origin = (drawnBox ?? host).getBoundingClientRect().top;
      const y = origin - pane.getBoundingClientRect().top + pane.scrollTop + place(at, at).top;
      pane.scrollTop = Math.max(0, y - pane.clientHeight / 3);
    }
    located = { line: at, seq };
    clearTimeout(locateTimer);
    locateTimer = setTimeout(() => {
      if (located?.seq === seq) located = null;
    }, LOCATE_MS);
  }
</script>

<div
  class="text"
  class:wrap
  bind:this={host}
  data-language={body?.language ?? ""}
  style:--row={`${row}px`}
  style:--cols={cols}
  style:--digits={digits}
>
  {#if virtual}
    <!-- not wrapping: the box scrolls sideways, so the keyboard can reach it (arrow keys scroll it) -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div
      class="virtual"
      style:height={`${totalRows * row}px`}
      tabindex={wrap ? undefined : 0}
      role={wrap ? undefined : "region"}
      aria-label={wrap ? undefined : t("text.scrollRegion")}
    >
      {#if wrap}<span class="probe" aria-hidden="true">{PROBE}</span>{/if}
      <div class="slice" style:transform={`translateY(${drawn.top * row}px)`}>
        {#each drawn.pieces as piece (`${piece.line}:${piece.from}`)}
          <div
            class="row"
            class:cont={piece.from > 0}
            data-line={piece.line + 1}
            data-from={piece.from}
          >
            <span class="no">{piece.from === 0 ? piece.line + 1 : ""}</span><span
              class="line"
              style:--lead={wrap ? leadOf(piece.line) : null}
              >{#each pieces(lines[piece.line] ?? "", piece.line + 1, whole(piece) ? undefined : rangeOf(piece)) as part, i (i)}{#if part.secret !== null}<button
                    type="button"
                    class="secret"
                    title={t("config.reveal")}
                    aria-label={t("config.reveal")}
                    onclick={() => reveal(part.secret ?? "")}>{part.text}</button
                  >{:else}{part.text}{/if}{/each}</span
            >
          </div>
        {/each}
      </div>
      {@render glowing()}
      {@render locating()}
    </div>
    {#if partial !== null}
      <p class="note" role="status">
        {#if held.load.status === "loading"}
          {t("text.loadingMore")}
        {:else if held.load.status === "failed"}
          {t("text.loadFailed")}
          <button type="button" class="btn btn--sm" onclick={again}>{t("text.retry")}</button>
        {:else if held.load.status === "capped"}
          {t("text.capped", {
            held: megabytes(MAX_HELD_CHARS),
            total: megabytes(partial.total),
          })}
        {:else if held.load.status === "done"}
          {t("text.allLoaded", { total: megabytes(partial.total) })}
        {:else}
          {t("text.partial", {
            held: megabytes(held.chars),
            total: megabytes(partial.total),
          })}
        {/if}
      </p>
    {:else}
      <p class="note">{t("text.plainBecauseLong")}</p>
    {/if}
  {:else}
    {#if !wrap}<pre class="gutter" aria-hidden="true">{numbers}</pre>{/if}
    <div class="code" bind:this={codeBox}>
      {#if code}
        {#await code.view() then loaded}
          {@const Code = loaded.default}
          <Code
            source={text}
            info={body?.language ?? ""}
            display={false}
            lines={wrap}
            region={wrap ? undefined : t("text.scrollRegion")}
          />
        {/await}
      {:else if wrap}
        <pre><code
            >{#each lines as line, i (i)}<span class="ln" style:--lead={leadSmall(line)}
                >{plain(line)}</span
              >{:else}<span class="ln">{"\n"}</span>{/each}</code
          ></pre>
      {:else}
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <pre tabindex="0" role="region" aria-label={t("text.scrollRegion")}><code>{text}</code
          ></pre>
      {/if}
      {@render glowing()}
      {@render locating()}
    </div>
  {/if}
</div>

{#snippet locating()}
  {#if located !== null}
    {@const at = place(located.line, located.line)}
    {#key located.seq}
      <div
        class="located"
        aria-hidden="true"
        style:top={`${at.top}px`}
        style:height={`${Math.min(at.height, BAND_ROWS * row)}px`}
      ></div>
    {/key}
  {/if}
{/snippet}

{#snippet glowing()}
  {#if glow !== null}
    <div class="glows" aria-hidden="true">
      {#each glow.added as range (range.first)}
        {@const at = place(range.first, range.last)}
        <div class="glow" style:top={`${at.top}px`} style:height={`${at.height}px`}></div>
      {/each}
      {#each glow.removedAfter as after (after)}
        <div class="gone" style:top={`${below(after) - 2}px`}></div>
      {/each}
    </div>
  {/if}
{/snippet}

<style>
  .text {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    margin: var(--sp-3) var(--sp-5) var(--sp-8);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--surface);
    font: var(--fs-code) / 1.6 var(--font-code);
    overflow: hidden;
  }
  .text.wrap {
    grid-template-columns: minmax(0, 1fr);
  }
  .gutter {
    margin: 0;
    padding: 14px 10px 14px 14px;
    color: var(--faint);
    text-align: right;
    font: inherit;
    user-select: none;
    border-right: 1px solid var(--border);
  }
  .code {
    position: relative;
    min-width: 0;
  }
  .code :global(pre) {
    margin: 0;
    padding: 14px 16px;
    overflow-x: auto;
    border: 0;
    border-radius: 0;
    background: transparent;
    font: inherit;
    white-space: pre;
  }
  .code :global(code) {
    font: inherit;
  }
  /* wrapped: one element per line, its number drawn in the room at the left (the same place as the gutter) */
  .wrap .code :global(pre) {
    --gutter: calc(var(--digits) * 1ch);
    padding-left: calc(14px + var(--gutter) + 10px + 1px + 16px);
    overflow-x: hidden;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    counter-reset: ln;
    background-image: linear-gradient(var(--border), var(--border));
    background-repeat: no-repeat;
    background-size: 1px 100%;
    background-position: calc(14px + var(--gutter) + 10px) 0;
  }
  /* a further row starts 2 to the right of the line's own indentation (`--lead`) */
  .wrap .code :global(.ln) {
    display: block;
    position: relative;
    padding-left: calc((2 + var(--lead, 0)) * 1ch);
    text-indent: calc((2 + var(--lead, 0)) * -1ch);
    counter-increment: ln;
  }
  .wrap .code :global(.ln::before) {
    content: counter(ln);
    position: absolute;
    top: 0;
    left: calc(-27px - var(--gutter));
    width: var(--gutter);
    color: var(--faint);
    text-align: right;
    text-indent: 0;
    white-space: nowrap;
  }
  .wrap .code :global(.secret) {
    text-indent: 0;
  }
  .virtual {
    grid-column: 1 / -1;
    position: relative;
    overflow-x: auto;
  }
  .slice {
    position: absolute;
    inset: 0 0 auto 0;
    min-width: max-content;
  }
  .row {
    display: flex;
    height: var(--row);
    white-space: pre;
  }
  /* the number column, as the gutter of a smaller file (a line between it and the text) */
  .no {
    flex: none;
    width: 5.5em;
    margin-right: 16px;
    padding-right: 10px;
    border-right: 1px solid var(--border);
    color: var(--faint);
    text-align: right;
    user-select: none;
  }
  /* not wrapping: the box takes the keyboard to scroll sideways */
  .virtual:focus,
  .code :global(pre:focus) {
    outline: none;
  }
  .virtual:focus-visible,
  .code :global(pre:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  /* wrapped rows: a row holds `--cols` characters, breaks after any one of them; its further rows start 2 to the right */
  .wrap .virtual {
    overflow-x: hidden;
  }
  .wrap .slice {
    min-width: 0;
  }
  .wrap .row {
    height: auto;
    min-height: var(--row);
  }
  .wrap .line {
    flex: none;
    box-sizing: content-box;
    width: calc((var(--cols) - 2 - var(--lead, 0)) * 1ch + 1px);
    padding-left: calc((2 + var(--lead, 0)) * 1ch);
    text-indent: calc((2 + var(--lead, 0)) * -1ch);
    white-space: break-spaces;
    word-break: break-all;
    line-break: anywhere;
    overflow-wrap: anywhere;
  }
  .wrap .row.cont .line {
    text-indent: 0;
  }
  .wrap .line .secret {
    text-indent: 0;
  }
  .probe {
    position: absolute;
    top: 0;
    left: 0;
    visibility: hidden;
    white-space: pre;
    pointer-events: none;
  }
  .secret {
    margin: 0;
    padding: 0 5px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface-2);
    color: var(--muted);
    font: inherit;
    letter-spacing: 0.04em;
    cursor: pointer;
  }
  .secret:hover,
  .secret:focus-visible {
    color: var(--text);
    border-color: var(--border-strong);
  }
  .note {
    grid-column: 1 / -1;
    margin: 0;
    padding: 6px 14px;
    border-top: 1px solid var(--border);
    color: var(--muted);
    font: 12px var(--font-ui);
  }
  /* lines that changed on disk: one rectangle each, fading out (opacity only) */
  .glows {
    position: absolute;
    inset: 0;
    pointer-events: none;
  }
  .glow {
    position: absolute;
    left: 0;
    right: 0;
    background: var(--live);
    animation: line-glow var(--dur-glow) ease-out forwards;
  }
  .gone {
    position: absolute;
    left: 0;
    right: 0;
    height: 3px;
    border-radius: 2px;
    background: var(--git-deleted);
    opacity: 0.55;
  }
  /* the line a reference asked for: a band in the reading tint, fading out (static under reduced motion) */
  .located {
    position: absolute;
    left: 0;
    right: 0;
    pointer-events: none;
    background: color-mix(in srgb, var(--mark) 55%, transparent);
    animation: line-locate var(--dur-locate) ease-out forwards;
  }
  @keyframes line-locate {
    0%,
    30% {
      opacity: 1;
    }
    to {
      opacity: 0;
    }
  }
  @keyframes line-glow {
    from {
      opacity: 1;
    }
    to {
      opacity: 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .glow,
    .located {
      animation: none;
      opacity: 0.5;
    }
  }
</style>
