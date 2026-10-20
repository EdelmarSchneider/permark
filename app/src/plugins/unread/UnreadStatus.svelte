<script lang="ts">
  // The number of unread documents in the status bar; a click shows the list. Nothing when everything is read.
  import { tn } from "../../core/i18n";
  import { ui } from "../../core/shell/ui.svelte";
  import { unread } from "./store.svelte";

  const count = $derived(unread.unread.length);
</script>

{#if count > 0}
  <button
    type="button"
    class="chip"
    title={tn("unread.statusHint", count)}
    onclick={() => ui.showSidebar("unread.panel")}
  >
    <span class="dot" aria-hidden="true"></span>
    {tn("unread.status", count)}
  </button>
{/if}

<style>
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 0 6px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  .chip:hover {
    background: var(--hover);
    color: var(--text);
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
  }
</style>
