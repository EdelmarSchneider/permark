<script lang="ts">
  // A small button: the data as JSON on the clipboard (a check mark when it worked, a word when it did not).
  import Braces from "@lucide/svelte/icons/braces";
  import Check from "@lucide/svelte/icons/check";
  import { t } from "../../core/i18n";
  import { copyAsJson } from "./copyJson";

  type Props = {
    /** The TOON text (asked when pressed: a file is read then). */
    text: () => Promise<string | null>;
    /** Icon only (a code block's corner). */
    compact?: boolean;
  };
  let { text, compact = false }: Props = $props();

  let state = $state<"idle" | "done" | "failed">("idle");

  async function copy(): Promise<void> {
    const source = await text();
    state = source !== null && (await copyAsJson(source)) ? "done" : "failed";
  }

  // the check mark (or the word) goes after a moment
  $effect(() => {
    if (state === "idle") return;
    const timer = setTimeout(() => (state = "idle"), 1500);
    return () => clearTimeout(timer);
  });
</script>

<button
  type="button"
  class={compact ? "icon-btn icon-btn--sm copy-json" : "btn btn--sm copy-json"}
  title={t("toon.copyJson")}
  aria-label={t("toon.copyJson")}
  onclick={() => void copy()}
>
  {#if state === "done"}<Check size={14} />{:else}<Braces size={14} />{/if}
  {#if !compact}{state === "failed" ? t("toon.copyFailed") : t("toon.copyJson")}{/if}
</button>
