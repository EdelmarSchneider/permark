<script lang="ts">
  // A diagram: "drawing", then the drawing, or the source with a note when it cannot be drawn. Redrawn when the theme
  // or the light tone changes. The engines (Mermaid, PlantUML) are plugins; this is the shape they share.
  import { t } from "../i18n";
  import { look } from "../look.svelte";
  import { viewBoxWidth } from "./diagram";

  type Props = {
    source: string;
    /** Draws `source` for the theme: SVG text, or null when it cannot be drawn. */
    draw: (source: string, theme: "light" | "dark") => Promise<string | null>;
  };
  let { source, draw }: Props = $props();

  type Drawn =
    | { status: "pending" }
    | { status: "drawn"; svg: string; width: number | null }
    | { status: "failed" };
  let drawn = $state.raw<Drawn>({ status: "pending" });
  let host: HTMLElement | undefined = $state();

  // the block that holds the diagram is a diagram while this lives (the page lets it outgrow the text column)
  $effect(() => {
    const holder = host?.parentElement;
    holder?.classList.add("diagram");
    return () => holder?.classList.remove("diagram");
  });

  // draws (again) when the source, the theme or the tone changes; an older answer never overwrites a newer
  $effect(() => {
    const theme = look.theme;
    void look.tone;
    let stale = false;
    void draw(source, theme).then((svg) => {
      if (stale) return;
      if (svg === null) {
        drawn = { status: "failed" };
        return;
      }
      const box = /viewBox="([^"]*)"/.exec(svg);
      drawn = { status: "drawn", svg, width: viewBoxWidth(box?.[1]) };
    });
    return () => {
      stale = true;
    };
  });
</script>

<figure
  bind:this={host}
  class="figure"
  class:is-pending={drawn.status === "pending"}
  class:is-failed={drawn.status === "failed"}
>
  {#if drawn.status === "drawn"}
    <div class="drawing" style:width={drawn.width === null ? undefined : `${drawn.width}px`}>
      <!-- the drawing was cleaned of scripts, foreign content and remote references -->
      {@html drawn.svg}
    </div>
  {:else if drawn.status === "failed"}
    <pre class="source"><code>{source}</code></pre>
    <figcaption class="message">{t("doc.diagramFailed")}</figcaption>
  {:else}
    <span class="pending">{t("doc.diagramRendering")}</span>
  {/if}
</figure>

<style>
  .figure {
    margin: 0;
    padding: 18px 12px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    text-align: center;
    overflow-x: auto;
  }
  .figure.is-pending {
    min-height: 64px;
    color: var(--faint);
    font-size: 12px;
  }
  .figure.is-failed {
    padding: 0;
    border: 0;
    text-align: left;
  }
  .drawing {
    max-width: 100%;
    margin: 0 auto;
  }
  .drawing :global(svg) {
    max-width: 100%;
    height: auto;
  }
  .source {
    white-space: pre;
  }
  .message {
    margin: 6px 0 0;
    font-size: 12px;
    color: var(--muted);
  }
</style>
