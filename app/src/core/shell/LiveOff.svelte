<script lang="ts">
  // The project's folder cannot report its changes (a network drive, WSL, out of watches): PerMark cannot follow them
  // live, and says so in the status bar while it lasts. Single files and the Notes never follow live: nothing there.
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import { conditions } from "../conditions/context.svelte";
  import { t } from "../i18n";

  const off = $derived(
    conditions.current.workspace === "project" &&
      conditions.current.liveUpdates.status === "unavailable",
  );
</script>

<span class="live-off" role="status">
  {#if off}
    <span class="item" title={t("status.liveOffHint")}>
      <TriangleAlert size={13} aria-hidden="true" />
      {t("status.liveOff")}<span class="sr-only">. {t("status.liveOffHint")}</span>
    </span>
  {/if}
</span>

<style>
  /* no box of its own (no gap in the bar while empty); the live region stays to say it when it comes */
  .live-off {
    display: contents;
  }
  .item {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 0 6px;
    color: var(--git-modified-text);
    cursor: default;
  }
</style>
