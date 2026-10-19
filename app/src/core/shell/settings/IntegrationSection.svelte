<script lang="ts">
  // Integration: what plugins that are on put in the settings (the agent connection, which git, the Notes folder). A
  // feature that does not exist in this build has no row: nothing here is a dead control.
  import { conditions } from "../../conditions/context.svelte";
  import { t } from "../../i18n";
  import { contributions } from "../../plugins/instance.svelte";
  import Card from "./Card.svelte";

  const sections = $derived(contributions.visible("settingsSection", conditions.current));
</script>

{#if sections.length === 0}
  <p class="none">{t("settings.integrationNone")}</p>
{:else}
  {#each sections as section (section.owner + section.id)}
    <section class="group" aria-labelledby={`int-${section.owner}-${section.id}`}>
      <h3 id={`int-${section.owner}-${section.id}`}>{t(section.label)}</h3>
      <Card>
        {#await section.view() then loaded}
          {@const Section = loaded.default}
          <Section />
        {/await}
      </Card>
    </section>
  {/each}
{/if}

<style>
  .none {
    margin: 0;
    padding: var(--sp-4);
    color: var(--muted);
  }
  .group + .group {
    margin-top: var(--sp-5);
  }
  h3 {
    margin: 0 0 var(--sp-2);
    padding-left: var(--sp-1);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
</style>
