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

  // The web view would load a link of the drawing in place of the app page (the unsaved Source text gone with it), or
  // open it in a window of its own on a middle click. `inert` already stops that, but not every web view honors it.
  // Every click is held, not only one on a link: a `<use>` copy of a link takes the click as itself, outside any link,
  // and the picture has no click behavior of its own to keep.
  function holdClick(event: MouseEvent): void {
    event.preventDefault();
  }
</script>

{#if embed}
  {#if shown.trim() !== ""}
    {#key shown}
      {#await embed.view() then loaded}
        {@const View = loaded.default}
        <!-- A picture of the block being typed, like Edit's opened block: inert keeps its links out of the tab order
             (a Tab stop that does nothing) in every redraw, since it sits on this wrapper and not on the drawn links.
             The panel around it still scrolls. -->
        <div inert onclickcapture={holdClick} onauxclickcapture={holdClick}>
          <View source={shown} info={language} display={false} />
        </div>
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
