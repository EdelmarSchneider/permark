<script lang="ts">
  // A diagram embed for tests: the shared diagram figure, drawn at once in the theme asked ("fail" as source: cannot
  // be drawn; "link": the drawing also holds a link to a page). The drawing names its theme in a `desc`.
  import DiagramView from "../doc/DiagramView.svelte";
  import type { EmbedProps } from "../plugins/props";

  let { source, theme }: EmbedProps = $props();

  function draw(text: string, drawIn: "light" | "dark"): Promise<string | null> {
    if (text.trim() === "fail") return Promise.resolve(null);
    const link = text.trim() === "link" ? `<a href="other.md"><text x="1" y="8">go</text></a>` : "";
    return Promise.resolve(
      `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><desc>${drawIn}</desc><rect width="10" height="10"/>${link}</svg>`,
    );
  }
</script>

<DiagramView {source} {theme} {draw} />
