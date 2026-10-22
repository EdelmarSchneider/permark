<script lang="ts">
  // A sidebar tab showing one project's items (a count on the project's detail card led here): says which project and
  // offers "Show all". Takes the focus when it appears (the person came for that list), so the next Tab reaches the
  // items.
  import { t } from "../i18n";

  type Props = { name: string; clear: () => void };
  let { name, clear }: Props = $props();

  let line: HTMLElement | undefined = $state();
  $effect(() => {
    line?.focus();
  });
</script>

<div class="only" role="group" aria-label={t("projects.only", { name })}>
  <span class="text" tabindex="-1" bind:this={line}>{t("projects.only", { name })}</span>
  <button type="button" class="btn btn--sm" onclick={clear}>{t("projects.showAll")}</button>
</div>

<style>
  .only {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-2) var(--sp-3);
    border-bottom: 1px solid var(--border);
    background: var(--surface-2);
    font-size: var(--fs-ui-sm);
  }
  .text {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
    border-radius: var(--radius-control);
  }
  .text:focus {
    outline: none;
  }
  .text:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
