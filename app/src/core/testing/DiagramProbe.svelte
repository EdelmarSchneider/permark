<script lang="ts">
  // A diagram embed for tests: the shared diagram figure, drawn at once in the theme asked ("fail" as source: cannot
  // be drawn). The drawing names its theme in a `desc`.
  import DiagramView from "../doc/DiagramView.svelte";
  import type { EmbedProps } from "../plugins/props";

  let { source, theme }: EmbedProps = $props();

  function draw(text: string, drawIn: "light" | "dark"): Promise<string | null> {
    if (text.trim() === "fail") return Promise.resolve(null);
    return Promise.resolve(
      `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><desc>${drawIn}</desc><rect width="10" height="10"/></svg>`,
    );
  }
</script>

<DiagramView {source} {theme} {draw} />
