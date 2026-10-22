<script lang="ts">
  // The viewer of the config formats: the sheet, read only. A file that cannot be shown as a sheet (too large, not
  // valid for its syntax) is shown as text by the text viewer, not here. A line asked for (a code reference) brings
  // the row holding it into view, tinted a moment (rows of tables stay where they are: the sheet opens at its top).
  import { tick, untrack } from "svelte";
  import type { ViewerProps } from "../plugins/props";
  import { documents } from "../doc/document.svelte";
  import { chooseMode } from "../editing/choose";
  import ConfigSheet from "./ConfigSheet.svelte";
  import { Matches } from "./matches.svelte";

  let { view }: ViewerProps = $props();

  const sheet = $derived(view.body.kind === "sheet" ? view.body.sheet : null);
  const editable = $derived(view.editable.editable === "surgical");
  const matches = new Matches();
  let box: HTMLElement | undefined = $state();
  let handled = 0;

  // an ignore file counts what its patterns catch, once it is shown (again when the file changes)
  $effect(() => {
    if (sheet?.kind !== "ignore") return;
    matches.request(view.path, null, true);
    return () => matches.stop();
  });

  $effect(() => {
    const request = documents.lineRequest;
    if (!request || !box || request.path !== view.path || request.seq === handled) return;
    handled = request.seq;
    untrack(() => void locate(request.line));
  });

  /** The row whose line holds `line` (else the nearest row above it), at the top third of the pane, tinted. */
  async function locate(line: number): Promise<void> {
    await tick();
    const rows = Array.from(box?.querySelectorAll<HTMLElement>("[data-line]") ?? []);
    let target: HTMLElement | undefined;
    for (const row of rows) {
      const at = Number(row.dataset["line"]);
      if (Number.isFinite(at) && at <= line && at >= Number(target?.dataset["line"] ?? 0))
        target = row;
    }
    const pane = box?.closest<HTMLElement>("[data-scroller]");
    const row = target;
    if (!row || !pane) return;
    const y = row.getBoundingClientRect().top - pane.getBoundingClientRect().top + pane.scrollTop;
    pane.scrollTop = Math.max(0, y - pane.clientHeight / 3);
    row.classList.remove("pm-located");
    void row.offsetWidth;
    row.classList.add("pm-located");
    setTimeout(() => row.classList.remove("pm-located"), 1600);
  }
</script>

{#if sheet}
  <div class="sheet-view" bind:this={box}>
    <ConfigSheet
      {sheet}
      editable={false}
      source={editable ? (line) => chooseMode("source", line) : null}
      matches={sheet.kind === "ignore" ? matches.counts : null}
      capped={matches.capped}
    />
  </div>
{/if}

<style>
  .sheet-view {
    display: contents;
  }
  /* the row a reference asked for: the reading tint, fading out (static under reduced motion) */
  .sheet-view :global(.pm-located) {
    animation: sheet-locate var(--dur-locate) ease-out both;
  }
  @keyframes sheet-locate {
    0%,
    30% {
      background: var(--mark);
    }
    100% {
      background: transparent;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .sheet-view :global(.pm-located) {
      animation: none;
      background: color-mix(in srgb, var(--mark) 55%, transparent);
    }
  }
</style>
