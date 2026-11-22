<script lang="ts">
  // A formula drawn by KaTeX (this view and KaTeX load when the first formula comes near the screen), with chemistry
  // (`\ce`, `\pu`: mhchem adds its commands to this same KaTeX). One that KaTeX cannot read shows as its source with an
  // error mark; nothing in it can run or load anything (`trust` stays off).
  // Drawn as the view mounts, never after: whoever is told of the mount (a table measuring its columns again) sees the
  // formula, not its TeX source.
  import katex from "katex";
  import "katex/contrib/mhchem";
  import type { EmbedProps } from "../../core/plugins/props";
  import "katex/dist/katex.min.css";

  let { source, display }: EmbedProps = $props();

  const html = $derived.by((): string | null => {
    try {
      return katex.renderToString(source, {
        displayMode: display,
        throwOnError: true,
        output: "htmlAndMathml",
        strict: "ignore",
        trust: false,
        maxExpand: 1000,
        maxSize: 50,
      });
    } catch {
      return null;
    }
  });
</script>

{#if html === null}<code class="pm-math-error" title={source}>{source}</code
  >{:else}{@html html}{/if}

<style>
  .pm-math-error {
    font-family: var(--font-code);
    color: var(--git-deleted-text);
    border: 0;
    background: none;
  }
  :global(.md .katex-display) {
    margin: 0.8em 0;
    overflow-x: auto;
    overflow-y: hidden;
  }
</style>
