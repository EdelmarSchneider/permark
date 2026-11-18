<script lang="ts">
  // Rows of a change in the Assistant's views (a run of the copy's changes, what a question would write): each line
  // kept whole on one row, the box scrolls sideways. Wrapping would break a table row of Markdown at its pipes and
  // leave a lone "|" on a row of its own.
  import DiffLines from "../../core/editing/DiffLines.svelte";
  import type { DiffRow } from "../../core/editing/diffRows";

  type Props = { rows: readonly DiffRow[] };
  let { rows }: Props = $props();
</script>

<div class="box">
  <div class="lines"><DiffLines {rows} /></div>
</div>

<style>
  .box {
    overflow-x: auto;
    font-family: var(--font-code);
    font-size: var(--fs-ui-sm);
  }
  /* as wide as the longest line: every row's color reaches the end of what scrolls into view */
  .lines {
    width: max-content;
    min-width: 100%;
  }
  .lines :global(.row) {
    white-space: pre;
    overflow-wrap: normal;
  }
  /* unchanged lines only place the change: they step back */
  .lines :global(.row.same) {
    color: var(--muted);
  }
</style>
