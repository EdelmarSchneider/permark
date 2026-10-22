<script lang="ts">
  // The agent connection's row in the gear popover (project windows): its state in a word and the way to its settings
  // section, one click from anywhere. Hidden until the state is known.
  import { t } from "../../core/i18n";
  import { ui } from "../../core/shell/ui.svelte";
  import { AGENT_PAGE } from "./offer.svelte";
  import { agent } from "./store.svelte";

  const state = $derived(
    agent.status === null
      ? null
      : !agent.enabled
        ? t("agent.popoverRow.off")
        : agent.connected.length > 0
          ? t("agent.popoverRow.connected")
          : t("agent.popoverRow.on"),
  );
</script>

{#if state !== null}
  <div class="row">
    <span class="name" id="pop-agent">{t("agent.section")}</span>
    <span class="state">{state}</span>
    <button
      type="button"
      class="btn btn--sm"
      aria-describedby="pop-agent"
      onclick={() => ui.openSettingsPage(AGENT_PAGE, "content")}
    >
      {agent.enabled ? t("agent.openSettings") : t("agent.setUp")}
    </button>
  </div>
{/if}

<style>
  .row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--sp-1) var(--sp-3);
    padding: 6px 10px;
  }
  .name {
    flex: 1;
    min-width: 0;
  }
  .state {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
</style>
