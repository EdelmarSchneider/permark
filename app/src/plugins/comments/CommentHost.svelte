<script lang="ts">
  // Always there while the plugin is on (a status item that draws nothing in the bar): the field of the comment being
  // written, and the small "Comment" button that follows a selection in the page.
  import MessageSquarePlus from "@lucide/svelte/icons/message-square-plus";
  import type { PageSelection } from "../../core/plugins/props";
  import { documents } from "../../core/doc/document.svelte";
  import { t } from "../../core/i18n";
  import CommentField from "./CommentField.svelte";
  import { readSelection } from "./marks";
  import { comments } from "./store.svelte";

  let selection = $state.raw<PageSelection | null>(null);
  const draft = $derived(comments.draft);

  // the selection of the page: a button appears next to it
  $effect(() => {
    const read = (): void => {
      selection = documents.path === null ? null : readSelection();
    };
    document.addEventListener("selectionchange", read);
    return () => document.removeEventListener("selectionchange", read);
  });

  const at = $derived.by(() => {
    const rect = selection?.rect;
    if (!rect) return null;
    const left = Math.max(8, Math.min(window.innerWidth - 120, rect.right - 8));
    return { left, top: Math.min(window.innerHeight - 40, rect.bottom + 6) };
  });

  function begin(): void {
    const now = selection;
    if (now && comments.begin(now)) selection = null;
  }
</script>

{#if draft}
  <CommentField />
{:else if selection && at}
  <button
    type="button"
    class="add"
    style:left="{at.left}px"
    style:top="{at.top}px"
    title={t("comments.addHint")}
    onmousedown={(e) => e.preventDefault()}
    onclick={begin}
  >
    <MessageSquarePlus size={14} aria-hidden="true" />
    {t("comments.add")}
  </button>
{/if}

<style>
  .add {
    position: fixed;
    z-index: 45;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px var(--sp-2);
    border: 1px solid var(--border-strong);
    border-radius: 999px;
    background: var(--surface);
    box-shadow: var(--shadow-pop);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui-sm);
    cursor: pointer;
  }
  .add:hover {
    background: var(--hover);
  }
</style>
