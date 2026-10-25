<script lang="ts">
  // The why + what to do of "Live: off", as a quiet line where the Live chip leads (the Changes panel): seen by
  // everyone who opens it, mouse or keyboard. Nothing while live updates work.
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import { t } from "../i18n";
  import { liveOff } from "./liveSignal.svelte";
</script>

{#if liveOff.off}
  <p class="live-off-note">
    <TriangleAlert size={13} aria-hidden="true" />
    <span><strong>{t("status.liveOff")}.</strong> {t("status.liveOffHint")}</span>
  </p>
{/if}

<style>
  .live-off-note {
    display: flex;
    align-items: flex-start;
    gap: var(--sp-2);
    margin: 0;
    padding: var(--sp-2) var(--sp-3);
    border-bottom: 1px solid var(--border);
    color: var(--text);
    font-size: var(--fs-ui-sm);
  }
  .live-off-note :global(svg) {
    flex: none;
    margin-top: 2px;
    color: var(--git-modified-text);
  }
</style>
