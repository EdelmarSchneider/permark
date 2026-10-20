<script lang="ts">
  // One row of the settings page: what it is (label + hint) at the left, the control at the right. Stacks when the page
  // is narrow, or when `wide` (a long control: a text field, a list). Rows of one card are separated by a line.
  import type { Snippet } from "svelte";

  type Props = {
    label: string;
    hint?: string;
    /** The id of the control the label names (a `<label for>`), else the label is plain text with `labelId`. */
    labelFor?: string;
    labelId?: string;
    /** The control takes the row's full width under the label. */
    wide?: boolean;
    children: Snippet;
    /** A line under the control (a problem, a preview). */
    note?: Snippet;
  };
  let { label, hint, labelFor, labelId, wide = false, children, note }: Props = $props();
</script>

<div class="row" class:wide>
  <div class="text">
    {#if labelFor}
      <label class="name" for={labelFor}>{label}</label>
    {:else}
      <span class="name" id={labelId}>{label}</span>
    {/if}
    {#if hint}<span class="hint">{hint}</span>{/if}
  </div>
  <div class="control">{@render children()}</div>
  {#if note}<div class="note">{@render note()}</div>{/if}
</div>

<style>
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    column-gap: var(--sp-5);
    row-gap: var(--sp-2);
    padding: var(--sp-3) var(--sp-4);
  }
  .row.wide {
    grid-template-columns: minmax(0, 1fr);
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .name {
    font-size: var(--fs-ui);
    font-weight: 500;
  }
  .hint {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    overflow-wrap: anywhere;
  }
  .control {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: var(--sp-2);
    min-width: 0;
  }
  .wide .control {
    justify-content: stretch;
  }
  .wide .control > :global(input),
  .wide .control > :global(select) {
    flex: 1;
    min-width: 0;
  }
  .note {
    grid-column: 1 / -1;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    overflow-wrap: anywhere;
  }
  @media (max-width: 640px) {
    .row {
      grid-template-columns: minmax(0, 1fr);
    }
    .control {
      justify-content: flex-start;
    }
  }
</style>
