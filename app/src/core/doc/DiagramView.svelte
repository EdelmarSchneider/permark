<script lang="ts">
  // A diagram: "drawing", then the drawing, or the source with a note when it cannot be drawn. Redrawn when the theme
  // or the light tone changes. The engines (Mermaid, PlantUML, Graphviz) are plugins; this is the shape they share.
  import { t } from "../i18n";
  import { look } from "../look.svelte";
  import { viewBoxWidth, type DrawFailure } from "./diagram";

  type Props = {
    source: string;
    /** Draws `source` for the theme (`forced`: fixed by the caller, not the page's): SVG text, or null | a failure
     *  with its words when it cannot be drawn. */
    draw: (
      source: string,
      theme: "light" | "dark",
      forced: boolean,
    ) => Promise<string | DrawFailure | null>;
    /** The theme to draw in, whatever the page shows; absent: the page's theme and tone. */
    theme?: "light" | "dark" | undefined;
  };
  let { source, draw, theme }: Props = $props();

  type Drawn =
    | { status: "pending" }
    | { status: "drawn"; svg: string; width: number | null }
    | { status: "failed"; problem: string | null };
  let drawn = $state.raw<Drawn>({ status: "pending" });
  let host: HTMLElement | undefined = $state();

  // the block that holds the diagram is a diagram while this lives (the page lets it outgrow the text column)
  $effect(() => {
    const holder = host?.parentElement;
    holder?.classList.add("diagram");
    return () => holder?.classList.remove("diagram");
  });

  // a click on a <use> outside any link is held: the page resolves its `#id` against the whole page, so it may copy a
  // link of another drawing, and a click on that copy is retargeted to the <use>, past every link router
  $effect(() => {
    const figure = host;
    if (!figure) return;
    const holdCopies = (event: MouseEvent): void => {
      const use = event.target instanceof Element ? event.target.closest("use") : null;
      if (use !== null && use.closest("a") === null) event.preventDefault();
    };
    figure.addEventListener("click", holdCopies, true);
    figure.addEventListener("auxclick", holdCopies, true);
    return () => {
      figure.removeEventListener("click", holdCopies, true);
      figure.removeEventListener("auxclick", holdCopies, true);
    };
  });

  // draws (again) when the source, the theme or the tone changes; an older answer never overwrites a newer
  $effect(() => {
    const forced = theme !== undefined;
    const drawIn = theme ?? look.theme;
    // a forced theme does not follow the page's tone
    if (!forced) void look.tone;
    let stale = false;
    void draw(source, drawIn, forced).then((svg) => {
      if (stale) return;
      if (typeof svg !== "string") {
        drawn = { status: "failed", problem: svg?.problem ?? null };
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
    <!-- the engine's words go inside the note: an export keeps the note and drops the source. The note comes first:
         a source refused for its size would push it thousands of lines down -->
    <figcaption class="message">
      {t("doc.diagramFailed")}
      {#if drawn.problem}<span class="problem">{drawn.problem}</span>{/if}
    </figcaption>
    <!-- a scroller of its own, so the keyboard can read a long source -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <pre class="source" tabindex="0" role="region" aria-label={t("doc.diagramSource")}><code
        >{source}</code
      ></pre>
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
    /* also the drawing's box: its cleaned CSS keeps every part in the flow, and in-flow overflow (transforms,
       margins, shadows) is clipped here */
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
    max-height: 24em;
    overflow: auto;
  }
  .message {
    margin: 0 0 6px;
    font-size: 12px;
    color: var(--muted);
  }
  .problem {
    display: block;
    font-family: var(--font-code);
  }
</style>
