<script lang="ts">
  // A formula drawn by KaTeX (loaded when the first formula comes near the screen). One that KaTeX cannot read shows
  // as its source with an error mark; nothing in it can run or load anything (`trust` stays off).
  import type { EmbedProps } from "../../core/plugins/props";
  import "katex/dist/katex.min.css";

  let { source, display }: EmbedProps = $props();

  let html = $state<string | null>(null);
  let failed = $state(false);

  $effect(() => {
    let stale = false;
    void import("katex").then((module) => {
      if (stale) return;
      try {
        html = module.default.renderToString(source, {
          displayMode: display,
          throwOnError: true,
          output: "htmlAndMathml",
          strict: "ignore",
          trust: false,
          maxExpand: 1000,
          maxSize: 50,
        });
        failed = false;
      } catch {
        failed = true;
      }
    });
    return () => {
      stale = true;
    };
  });
</script>

{#if failed}<code class="pm-math-error" title={source}>{source}</code
  >{:else if html !== null}{@html html}{:else}{source}{/if}

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
