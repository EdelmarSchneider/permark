<script lang="ts">
  // A diagram drawn while its source is typed: the embed of its language (Mermaid, PlantUML), redrawn 300 ms after
  // the typing stops. Nothing when no plugin draws that language.
  import { conditions } from "../conditions/context.svelte";
  import { t } from "../i18n";
  import { contributions } from "../plugins/instance.svelte";

  type Props = { language: string; source: string };
  let { language, source }: Props = $props();

  let shown = $state("");

  const embed = $derived(
    contributions
      .visible("embed", conditions.current)
      .find((e) => e.language === (language === "puml" ? "plantuml" : language)) ??
      contributions.visible("embed", conditions.current).find((e) => e.language === language),
  );

  // debounce: a timer per change of the source, cleared on the next
  $effect(() => {
    const next = source;
    const timer = setTimeout(() => (shown = next), 300);
    return () => clearTimeout(timer);
  });
</script>

{#if embed}
  {#if shown !== ""}
    {#key shown}
      {#await embed.view() then loaded}
        {@const View = loaded.default}
        <View source={shown} info={language} display={false} />
      {/await}
    {/key}
  {/if}
{:else}
  <p class="none">{t("edit.diagramNoPlugin")}</p>
{/if}

<style>
  .none {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
</style>
