<script lang="ts">
  // Draws nothing. While the outline shows, it works out the tokens of each section for the outline column: the lines
  // of the section (its heading to the next of the same or a higher level) counted by the first tokenizer chosen.
  import { api } from "../../api";
  import { createAnalysisText } from "../../core/doc/analysisText.svelte";
  import type { OutlineWatcherProps } from "../../core/plugins/props";
  import { lineCount, sectionRanges } from "./model";
  import { tokens } from "./store.svelte";
  import { sections } from "./sections.svelte";

  let { items }: OutlineWatcherProps = $props();

  const source = createAnalysisText();
  const primary = $derived(tokens.primary);
  const ready = $derived(primary !== null && tokens.ready(primary));
  const on = $derived(tokens.state?.outline ?? false);

  $effect(() => {
    const info = primary;
    const text = source.text;
    const path = source.path;
    const headings = items;
    // a buffer being edited no longer matches the headings of the saved page: the column waits for the save
    if (!on || !ready || info === null || path === null || text === "" || source.editing) return;
    let stale = false;
    const ranges = sectionRanges(headings, lineCount(text));
    void api
      .pluginCall("token-count.count", { tokenizers: [info.id], text, parts: ranges })
      .then((result) => {
        if (stale || !result.ok) return;
        const found = result.value.find((r) => r.tokenizer === info.id);
        if (found?.status !== "ok") return;
        sections.set({
          path,
          tokenizer: info.id,
          approx: !info.exact,
          counts: found.parts,
        });
      });
    return () => {
      stale = true;
    };
  });
</script>
