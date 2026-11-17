<script lang="ts">
  // "Expand all" and "Collapse all" for a read-only tree's bar.
  import ChevronsDownUp from "@lucide/svelte/icons/chevrons-down-up";
  import ChevronsUpDown from "@lucide/svelte/icons/chevrons-up-down";
  import { t } from "../i18n";

  let { onask }: { onask: (open: boolean) => void } = $props();
</script>

<span class="spreads">
  <button type="button" class="spread" onclick={() => onask(true)}>
    <ChevronsUpDown size={14} aria-hidden="true" />
    {t("structured.expandAll")}
  </button>
  <button type="button" class="spread" onclick={() => onask(false)}>
    <ChevronsDownUp size={14} aria-hidden="true" />
    {t("structured.collapseAll")}
  </button>
</span>

<style>
  .spreads {
    display: inline-flex;
    gap: 6px;
  }
  .spread {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font-size: var(--fs-ui-sm);
    cursor: pointer;
  }
  .spread:hover {
    background: var(--hover);
  }
  .spread :global(svg) {
    flex: none;
    color: var(--muted);
  }
</style>
