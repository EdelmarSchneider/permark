<script lang="ts">
  // One line of words in the Assistant's views, in the look of what it means: `quiet` (a hint), `muted` (an
  // explanation), `refusal` (why something did not happen), `warning` (what to mind). The one place these looks are
  // set for the plugin's views, so a change of look is made once.
  import type { Snippet } from "svelte";

  type Props = {
    tone?: "quiet" | "muted" | "refusal" | "warning";
    /** Said by assistive technology at once (a refusal right after the person's action). */
    alert?: boolean;
    id?: string;
    children: Snippet;
  };
  let { tone = "quiet", alert = false, id, children }: Props = $props();
</script>

<p class={["note", tone]} role={alert ? "alert" : undefined} {id}>{@render children()}</p>

<style>
  .note {
    margin: 0;
    font-size: var(--fs-ui-sm);
    line-height: 1.5;
    overflow-wrap: anywhere;
  }
  .quiet {
    color: var(--faint);
  }
  .muted {
    color: var(--muted);
  }
  .refusal {
    color: var(--git-deleted-text);
  }
  .warning {
    color: var(--git-modified-text);
  }
</style>
