<script lang="ts">
  // The copy button of code blocks and data: a check mark when the copy worked, a warning when it did not, for a
  // moment. A screen reader hears the result from a live line beside the button: it exists before the first press (a
  // live region added together with its text is not announced) and a button's own content is not read as a region.
  import Check from "@lucide/svelte/icons/check";
  import CircleAlert from "@lucide/svelte/icons/circle-alert";
  import Copy from "@lucide/svelte/icons/copy";
  import { onDestroy, type Component } from "svelte";
  import { t } from "../i18n";

  type Props = {
    /** A text for the clipboard, or a step that copies and says whether it worked. */
    copy: string | (() => Promise<boolean>);
    /** The button's name and tooltip; also its words unless `compact`. */
    label: string;
    /** The words after a failed copy (not `compact`); `label` when absent. */
    failedLabel?: string;
    icon?: Component<{ size?: number }>;
    /** Icon only (a block's corner); else a small button with words. */
    compact?: boolean;
    /** Classes the place styles the button by. */
    class?: string;
  };
  let {
    copy,
    label,
    failedLabel,
    icon = Copy,
    compact = true,
    class: extra = "",
  }: Props = $props();

  type Shown = "idle" | "done" | "failed";
  /** How long the result stays on the button. */
  const SHOWN_MS = 1500;

  let shown = $state<Shown>("idle");
  let timer: ReturnType<typeof setTimeout> | undefined;

  const said = $derived.by((): string => {
    switch (shown) {
      case "idle":
        return "";
      case "done":
        return t("edit.copied");
      case "failed":
        return t("doc.copyFailed");
      default:
        return shown satisfies never;
    }
  });
  const Idle = $derived(icon);

  async function write(): Promise<boolean> {
    try {
      if (typeof copy !== "string") return await copy();
      await navigator.clipboard.writeText(copy);
      return true;
    } catch {
      // the system refused the clipboard (or the step failed): said, never silent
      return false;
    }
  }

  async function press(): Promise<void> {
    const worked = await write();
    clearTimeout(timer);
    shown = worked ? "done" : "failed";
    timer = setTimeout(() => (shown = "idle"), SHOWN_MS);
  }

  onDestroy(() => clearTimeout(timer));
</script>

<button
  type="button"
  class={[
    compact ? "icon-btn icon-btn--sm" : "btn btn--sm",
    extra,
    shown === "done" && "is-done",
    shown === "failed" && "is-failed",
  ]}
  aria-label={label}
  title={label}
  onclick={() => void press()}
>
  {#if shown === "done"}<Check size={14} />{:else if shown === "failed"}<CircleAlert
      size={14}
    />{:else}<Idle size={14} />{/if}
  {#if !compact}{shown === "failed" ? (failedLabel ?? label) : label}{/if}
</button>
<span class="sr-only" aria-live="polite">{said}</span>
