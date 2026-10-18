<script lang="ts">
  // One comment in the Comments tab: a check to mark it done, the quoted passage with the comment and where it is (a click
  // goes to the passage), the button that puts it on the clipboard as a prompt for an agent, and the reply an agent left
  // when it resolved it.
  import { t } from "../i18n";
  import { documentTitle, excerpt, trail } from "../comments";
  import { app } from "../stores/app.svelte";
  import { clock } from "../stores/clock.svelte";
  import { comments } from "../stores/comments.svelte";
  import { ago } from "../time";
  import type { Comment } from "../types";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";

  type Props = {
    comment: Comment;
    /** Name the document instead of the section (the folded list of done comments mixes documents). */
    showPath?: boolean;
  };
  let { comment, showPath = false }: Props = $props();

  const meta = $derived(
    [
      showPath ? documentTitle(comment.path).name : trail(comment.headingPath),
      t("comments.line", { line: comment.line }),
      ago(comment.createdMs, clock.now),
    ]
      .filter((part) => part !== "")
      .join(" · "),
  );
  const lost = $derived(
    comment.state === "documentGone"
      ? { name: t("comments.state.documentGone"), hint: t("comments.state.documentGoneHint") }
      : comment.state === "passageChanged"
        ? { name: t("comments.state.passageChanged"), hint: t("comments.state.passageChangedHint") }
        : null,
  );
  const check = $derived(comment.done ? t("comments.reopen") : t("comments.markDone"));
</script>

<div class={["comment-row", comment.done && "is-done"]} data-comment={comment.id}>
  <input
    class="comment-check"
    type="checkbox"
    checked={comment.done}
    aria-label={check}
    title={check}
    onchange={(e) => void comments.setDone(comment.id, e.currentTarget.checked)}
  />
  <button
    type="button"
    class="comment-body"
    title={t("comments.jump")}
    onclick={() => void app.openComment(comment)}
  >
    <span class="comment-quote">{excerpt(comment.quote)}</span>
    <span class="comment-text">{comment.text}</span>
    <span class="comment-meta">{meta}</span>
  </button>
  {#if !comment.done}
    <IconButton
      small
      class="comment-send"
      icon="external"
      label={t("comments.send")}
      onclick={() => void comments.sendToAgent(comment.id)}
    />
  {/if}
  <IconButton
    small
    class="comment-delete"
    icon="trash"
    label={t("comments.delete")}
    onclick={() => void comments.remove(comment.id)}
  />
  {#if lost}
    <span class="comment-lost" title={lost.hint}><Icon name="alert" size="xs" />{lost.name}</span>
  {/if}
  {#if comment.reply}
    <p class="comment-reply">
      <Icon name="corner-down-right" size="xs" />
      <span class="sr-only">{t("comments.reply")}: </span>{comment.reply}
    </p>
  {/if}
</div>
