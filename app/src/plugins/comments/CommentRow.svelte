<script lang="ts">
  // One comment in the Comments tab: where it is, the quoted passage, what was written, the answers under it, and what
  // can be done with it (go to the passage, mark done | open again, reply, send to the agent, delete).
  import CornerDownRight from "@lucide/svelte/icons/corner-down-right";
  import Check from "@lucide/svelte/icons/check";
  import Reply from "@lucide/svelte/icons/reply";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Send from "@lucide/svelte/icons/send";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import type { CommentView } from "../../api";
  import { language, t } from "../../core/i18n";
  import { excerpt, trail } from "./model";
  import { comments } from "./store.svelte";

  type Props = { comment: CommentView; marked?: boolean };
  let { comment, marked = false }: Props = $props();

  let replying = $state(false);
  let draft = $state("");
  let deleting = $state(false);

  const where = $derived(
    [trail(comment.headingPath), t("comments.line", { line: comment.line })]
      .filter((part) => part !== "")
      .join(" · "),
  );
  const when = $derived(
    new Date(comment.createdMs).toLocaleString(language.locale, {
      dateStyle: "medium",
      timeStyle: "short",
    }),
  );

  async function sendReply(): Promise<void> {
    if (await comments.reply(comment.id, draft)) {
      draft = "";
      replying = false;
    }
  }

  function onreplykey(event: KeyboardEvent): void {
    if (event.isComposing || event.keyCode === 229) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      replying = false;
    } else if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void sendReply();
    }
  }
</script>

<article class="comment" class:marked class:done={comment.done} data-comment={comment.id}>
  <button
    type="button"
    class="where"
    title={t("comments.jump")}
    disabled={comment.state === "documentMissing"}
    onclick={() => comments.jump(comment)}
  >
    {where}
  </button>
  {#if comment.state === "passageChanged"}
    <span class="state" title={t("comments.state.passageChangedHint")}>
      {t("comments.state.passageChanged")}
    </span>
  {:else if comment.state === "documentMissing"}
    <span class="state warn" title={t("comments.state.documentGoneHint")}>
      {t("comments.state.documentGone")}
    </span>
  {/if}
  <p class="quote">{excerpt(comment.quote)}</p>
  <p class="text">{comment.text}</p>
  <p class="when">
    {when}
    {#if comment.done}
      · {comment.doneBy === "agent" ? t("comments.doneByAgent") : t("comments.doneByYou")}
    {/if}
  </p>
  {#if comment.reply !== null}
    <p class="reply"><CornerDownRight size={12} aria-hidden="true" /> {comment.reply}</p>
  {/if}
  {#each comment.replies as reply (reply.atMs + reply.text)}
    <p class="reply">
      <CornerDownRight size={12} aria-hidden="true" />
      <span class="by">{reply.by === "agent" ? t("comments.byAgent") : t("comments.byYou")}</span>
      {reply.text}
    </p>
  {/each}

  {#if replying}
    <textarea
      rows="2"
      bind:value={draft}
      placeholder={t("comments.replyPlaceholder")}
      aria-label={t("comments.reply")}
      onkeydown={onreplykey}></textarea>
    <div class="row">
      <button type="button" class="btn btn--sm btn--primary" onclick={() => void sendReply()}>
        {t("comments.reply")}
      </button>
      <button type="button" class="btn btn--sm" onclick={() => (replying = false)}>
        {t("common.cancel")}
      </button>
    </div>
  {:else}
    <div class="actions">
      {#if comment.done}
        <button
          type="button"
          class="icon"
          title={t("comments.reopen")}
          aria-label={t("comments.reopen")}
          onclick={() => void comments.setDone(comment.id, false)}
        >
          <RotateCcw size={14} />
        </button>
      {:else}
        <button
          type="button"
          class="icon"
          title={t("comments.markDone")}
          aria-label={t("comments.markDone")}
          onclick={() => void comments.setDone(comment.id, true)}
        >
          <Check size={14} />
        </button>
      {/if}
      <button
        type="button"
        class="icon"
        title={t("comments.reply")}
        aria-label={t("comments.reply")}
        onclick={() => (replying = true)}
      >
        <Reply size={14} />
      </button>
      {#if !comment.done}
        <button
          type="button"
          class="icon"
          title={t("comments.send")}
          aria-label={t("comments.send")}
          onclick={() => void comments.sendToAgent(comment.id)}
        >
          <Send size={14} />
        </button>
      {/if}
      <span class="spacer"></span>
      {#if deleting}
        <button
          type="button"
          class="confirm"
          onclick={() => void comments.remove(comment.id)}
          onblur={() => (deleting = false)}
        >
          {t("comments.deleteConfirm")}
        </button>
      {:else}
        <button
          type="button"
          class="icon"
          title={t("comments.delete")}
          aria-label={t("comments.delete")}
          onclick={() => (deleting = true)}
        >
          <Trash2 size={14} />
        </button>
      {/if}
    </div>
  {/if}
</article>

<style>
  .comment {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--sp-2) var(--sp-3);
    border-bottom: 1px solid var(--border);
  }
  .comment.marked {
    background: var(--accent-soft);
  }
  .comment.done .text,
  .comment.done .quote {
    opacity: 0.7;
  }
  .where {
    align-self: flex-start;
    max-width: 100%;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--accent-text);
    font: inherit;
    font-size: var(--fs-ui-xs);
    text-align: left;
    overflow-wrap: anywhere;
    cursor: pointer;
  }
  .where:hover:not(:disabled) {
    text-decoration: underline;
  }
  .where:disabled {
    color: var(--muted);
    cursor: default;
  }
  .state {
    align-self: flex-start;
    padding: 0 6px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .state.warn {
    background: color-mix(in srgb, var(--git-deleted) 14%, transparent);
    color: var(--git-deleted-text);
  }
  .quote {
    margin: 2px 0;
    padding-left: var(--sp-2);
    border-left: 3px solid var(--accent-muted);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    overflow-wrap: anywhere;
  }
  .text {
    margin: 0;
    font-size: var(--fs-ui);
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }
  .when {
    margin: 0;
    color: var(--faint);
    font-size: var(--fs-ui-xs);
  }
  .reply {
    display: flex;
    gap: 4px;
    margin: 2px 0 0;
    padding-left: var(--sp-2);
    color: var(--text);
    font-size: var(--fs-ui-sm);
    overflow-wrap: anywhere;
  }
  .by {
    color: var(--muted);
    font-weight: 600;
  }
  textarea {
    width: 100%;
    margin-top: var(--sp-1);
    padding: var(--sp-1) var(--sp-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui-sm);
    resize: none;
  }
  .row,
  .actions {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    margin-top: var(--sp-1);
  }
  .spacer {
    flex: 1;
  }
  .icon {
    display: inline-grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover {
    background: var(--hover);
    color: var(--text);
  }
  .confirm {
    padding: 0 var(--sp-2);
    border: 1px solid var(--git-deleted);
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--git-deleted-text);
    font: inherit;
    font-size: var(--fs-ui-xs);
    cursor: pointer;
  }
</style>
