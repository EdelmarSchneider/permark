<script lang="ts">
  // A text file: line numbers, monospace, highlighted by its extension when the highlight plugin is on. A very long
  // file shows only the lines near the screen (plain), so it opens at once and never freezes the window; a file too
  // large to read whole shows its first part and asks for the rest as the reader comes to it. Lines that changed on
  // disk glow for a moment. Secret values are masked until clicked.
  import { untrack } from "svelte";
  import { api } from "../../api";
  import { conditions } from "../../core/conditions/context.svelte";
  import {
    findCodeSecrets,
    MASK,
    privateKeyLines,
    publicKeyLines,
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
  import { contributions } from "../../core/plugins/instance.svelte";
  import type { ViewerProps } from "../../core/plugins/props";

  let { view }: ViewerProps = $props();

  /** From this many lines (or characters) on, only the lines in view are drawn. */
  const VIRTUAL_FROM = 20_000;
  const VIRTUAL_CHARS = 200_000;
  /** Line height in pixels until it is measured (the code font size times 1.6, as the CSS sets it). */
  const ROW = 21.6;
  const BUFFER = 40;
  /** The padding above the first line of the plain view. */
  const PAD = 14;

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
  const numbers = $derived(
    virtual ? "" : Array.from({ length: Math.max(count, 1) }, (_, i) => i + 1).join("\n"),
  );
  const code = $derived(
    contributions.visible("embed", conditions.current).find((e) => e.language === "@code"),
  );

  let host: HTMLElement | undefined = $state();
  let top = $state(0);
  let visible = $state(60);
  let row = $state(ROW);

  // the lines in view follow the pane that scrolls (the document area's)
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

  const shown = $derived(lines.slice(top, top + visible));

  /** Asks for the next chunk when the lines in view are close to the end of what is held. */
  async function more(): Promise<void> {
    // lines of a version of the file that has gone are let go
    if (grown !== null && grown.owner !== view) grown = null;
    const mine = held;
    if (partial === null || mine.next === null || !wantsMore(mine, top, visible)) return;
    grown = loading(mine, { status: "loading" });
    const result = await api.documentMore(view.path, mine.next, view.size, view.mtimeMs);
    // another version of the file came meanwhile: that one starts again
    if (grown?.owner !== view || grown.load.status !== "loading") return;
    if (!result.ok) {
      const gone = result.error.kind === "backend" && result.error.error.kind === "conflict";
      if (gone) documents.reload();
      grown = loading(mine, { status: gone ? "idle" : "failed" });
      return;
    }
    grown = append(mine, result.value);
    void more();
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

  type Piece = { text: string; secret: string | null };

  /** Lines (0-based) inside a private key block: masked whole (a line alone does not show it is one). */
  const keyBody = $derived(privateKeyLines(lines));
  /** Lines of certificates | public keys: never masked. */
  const publicBody = $derived(publicKeyLines(lines));

  /** A line cut where its secret values are (a line of CRLF text loses its carriage return). */
  function pieces(raw: string, no: number): Piece[] {
    const line = raw.endsWith("\r") ? raw.slice(0, -1) : raw;
    const lead = line.length - line.trimStart().length;
    const spans = keyBody.has(no - 1)
      ? [{ from: lead, to: line.length }]
      : line.length > 4000 || publicBody.has(no - 1)
        ? []
        : findCodeSecrets(line);
    if (spans.length === 0) return [{ text: line, secret: null }];
    const open = shownSecrets.view === view ? shownSecrets.keys : new Set<string>();
    const out: Piece[] = [];
    let at = 0;
    for (const span of spans) {
      out.push({ text: line.slice(at, span.from), secret: null });
      const key = `${no}:${span.from}`;
      const value = line.slice(span.from, span.to);
      out.push({ text: open.has(key) ? value : MASK, secret: open.has(key) ? null : key });
      at = span.to;
    }
    out.push({ text: line.slice(at), secret: null });
    return out;
  }

  function reveal(key: string): void {
    const keys = shownSecrets.view === view ? shownSecrets.keys : new Set<string>();
    shownSecrets = { view, keys: new Set([...keys, key]) };
  }

  // ---- glow: the lines that changed on disk ---------------------------------------------------------------------

  const glow = $derived(documents.lineGlow?.path === view.path ? documents.lineGlow : null);
</script>

<div class="text" bind:this={host} data-language={body?.language ?? ""} style:--row={`${row}px`}>
  {#if virtual}
    <div class="virtual" style:height={`${count * row}px`}>
      <div class="slice" style:transform={`translateY(${top * row}px)`}>
        {#each shown as line, at (top + at)}
          <div class="row">
            <span class="no">{top + at + 1}</span><span class="line"
              >{#each pieces(line, top + at + 1) as piece, i (i)}{#if piece.secret !== null}<button
                    type="button"
                    class="secret"
                    title={t("config.reveal")}
                    aria-label={t("config.reveal")}
                    onclick={() => reveal(piece.secret ?? "")}>{piece.text}</button
                  >{:else}{piece.text}{/if}{/each}</span
            >
          </div>
        {/each}
      </div>
      {@render glowing(0)}
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
    <pre class="gutter" aria-hidden="true">{numbers}</pre>
    <div class="code">
      {#if code}
        {#await code.view() then loaded}
          {@const Code = loaded.default}
          <Code source={text} info={body?.language ?? ""} display={false} />
        {/await}
      {:else}
        <pre><code>{text}</code></pre>
      {/if}
      {@render glowing(PAD)}
    </div>
  {/if}
</div>

{#snippet glowing(pad: number)}
  {#if glow !== null}
    <div class="glows" aria-hidden="true">
      {#each glow.added as range (range.first)}
        <div
          class="glow"
          style:top={`${pad + (range.first - 1) * row}px`}
          style:height={`${(range.last - range.first + 1) * row}px`}
        ></div>
      {/each}
      {#each glow.removedAfter as after (after)}
        <div class="gone" style:top={`${pad + after * row - 2}px`}></div>
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
  .no {
    flex: none;
    width: 5.5em;
    padding-right: 10px;
    color: var(--faint);
    text-align: right;
    user-select: none;
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
  @keyframes line-glow {
    from {
      opacity: 1;
    }
    to {
      opacity: 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .glow {
      animation: none;
      opacity: 0.5;
    }
  }
</style>
