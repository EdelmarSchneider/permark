<script lang="ts">
  // The drawing beside a Mermaid block while its text is edited. A pause redraws it. A typo keeps the last
  // drawing of this same block and says the text cannot be drawn yet.
  import { tick } from "svelte";
  import { drawDiagram } from "../dom/enhance";
  import { t } from "../i18n";
  import { session } from "../stores/session.svelte";

  type Props = { source: string; fence: number };
  let { source, fence }: Props = $props();

  const DELAY_MS = 300;

  let svg = $state("");
  let failed = $state(false);
  let drawnFor = -1;
  let host: HTMLElement | undefined = $state();

  $effect(() => {
    const text = source;
    const id = fence;
    const theme = session.resolvedTheme;
    if (text.trim() === "") {
      svg = "";
      failed = false;
      drawnFor = id;
      return;
    }
    const timer = setTimeout(() => {
      void drawDiagram(text, theme).then(async (drawn) => {
        if (drawn) {
          svg = drawn;
          failed = false;
          drawnFor = id;
          await tick();
          const drawing = host?.querySelector("svg");
          const box = drawing?.getAttribute("viewBox")?.split(/[\s,]+/) ?? [];
          const width = Number(box[2]);
          if (drawing && Number.isFinite(width) && width > 0) {
            drawing.style.width = `${width}px`;
            drawing.style.maxWidth = "100%";
            drawing.style.height = "auto";
          }
        } else {
          failed = true;
          if (drawnFor !== id) svg = "";
        }
      });
    }, DELAY_MS);
    return () => clearTimeout(timer);
  });
</script>

<aside class="diagram-preview" aria-label={t("edit.diagramPreview")}>
  {#if svg}
    <div class="diagram" bind:this={host}>{@html svg}</div>
  {/if}
  {#if failed}
    <p class="diagram-preview__error">{t("doc.diagramFailed")}</p>
  {/if}
</aside>
