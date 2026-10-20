<script lang="ts">
  // The drawing of the diagram the caret is in, in the panel beside Source: the embed of its language (Mermaid,
  // PlantUML), redrawn 300 ms after the typing stops. A diagram that cannot be drawn shows its note (the embed's own);
  // no plugin for the language -> one line saying which to turn on.
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
  {#if shown.trim() !== ""}
    {#key shown}
      {#await embed.view() then loaded}
        {@const View = loaded.default}
        <View source={shown} info={language} display={false} />
      {:catch}
        <p class="note">{t("edit.diagramFailed")}</p>
      {/await}
    {/key}
  {/if}
{:else}
  <p class="note">{t("edit.diagramNoPlugin")}</p>
{/if}

<style>
  .note {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
</style>
