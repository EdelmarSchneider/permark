<script lang="ts">
  // A text file: line numbers, monospace, highlighted by its extension when the highlight plugin is on. A very long
  // file shows only the lines near the screen (plain), so it opens at once and never freezes the window.
  import { conditions } from "../../core/conditions/context.svelte";
  import { t } from "../../core/i18n";
  import { contributions } from "../../core/plugins/instance.svelte";
  import type { ViewerProps } from "../../core/plugins/props";

  let { view }: ViewerProps = $props();

  /** From this many lines on, only the lines in view are drawn. */
  const VIRTUAL_FROM = 20_000;
  /** Line height in pixels (the code font size times 1.6, as the CSS sets it). */
  const ROW = 21.6;
  const BUFFER = 40;

  const body = $derived(view.body.kind === "text" ? view.body : null);
  const text = $derived(body?.text ?? "");
  const lines = $derived(text.split("\n"));
  // a final line break ends the last line; it does not start another
  const count = $derived(text.endsWith("\n") ? lines.length - 1 : lines.length);
  const numbers = $derived(Array.from({ length: count }, (_, i) => i + 1).join("\n"));
  const code = $derived(
    contributions.visible("embed", conditions.current).find((e) => e.language === "@code"),
  );
  const virtual = $derived(count >= VIRTUAL_FROM);

  let host: HTMLElement | undefined = $state();
  let first = $state(0);
  let visible = $state(60);

  // the lines in view follow the pane that scrolls (the document area's)
  $effect(() => {
    const element = host;
    const pane = element?.closest<HTMLElement>("[data-scroller]");
    if (!virtual || !element || !pane) return;
    const update = (): void => {
      const top = pane.scrollTop - element.offsetTop;
      first = Math.max(0, Math.floor(top / ROW) - BUFFER);
      visible = Math.ceil(pane.clientHeight / ROW) + BUFFER * 2;
    };
    update();
    pane.addEventListener("scroll", update, { passive: true });
    window.addEventListener("resize", update);
    return () => {
      pane.removeEventListener("scroll", update);
      window.removeEventListener("resize", update);
    };
  });

  const window_ = $derived(lines.slice(first, first + visible));
</script>

<div class="text" bind:this={host} data-language={body?.language ?? ""}>
  {#if virtual}
    <div class="virtual" style:height={`${count * ROW}px`}>
      <div class="slice" style:transform={`translateY(${first * ROW}px)`}>
        {#each window_ as line, at (first + at)}
          <div class="row">
            <span class="no">{first + at + 1}</span><span class="line">{line}</span>
          </div>
        {/each}
      </div>
    </div>
    <p class="note">{t("text.plainBecauseLong")}</p>
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
    </div>
  {/if}
</div>

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
    height: 21.6px;
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
  .note {
    grid-column: 1 / -1;
    margin: 0;
    padding: 6px 14px;
    border-top: 1px solid var(--border);
    color: var(--muted);
    font: 12px var(--font-ui);
  }
</style>
