<script lang="ts">
  // Integration: the default app (every build), then what plugins that are on put in the settings (file manager
  // entries, updates, which git, the Notes folder; the agent connection has its own section). A feature this build
  // lacks has no row.
  import { conditions } from "../../conditions/context.svelte";
  import { t } from "../../i18n";
  import { contributions } from "../../plugins/instance.svelte";
  import Card from "./Card.svelte";
  import DefaultAppCard from "./DefaultAppCard.svelte";

  const sections = $derived(contributions.visible("settingsSection", conditions.current));
</script>

<section class="group" aria-labelledby="int-core-default-app">
  <h3 id="int-core-default-app">{t("defaultApp.title")}</h3>
  <Card>
    <DefaultAppCard />
  </Card>
</section>
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

<style>
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
