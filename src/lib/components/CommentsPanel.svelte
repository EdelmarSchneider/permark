<script lang="ts">
  // The Comments tab of the right panel (docs/design/v0.4.md section 3): the open comments of the project grouped by document,
  // the ones that are done folded away, and a way to send them all to an agent as a prompt. A click on a comment goes to
  // its passage.
  import { tick } from "svelte";
  import { t, tn } from "../i18n";
  import { documentTitle } from "../comments";
  import { docs } from "../stores/docs.svelte";
  import { comments } from "../stores/comments.svelte";
  import Icon from "./Icon.svelte";
  import CommentRow from "./CommentRow.svelte";

  type Props = { active: boolean };
  let { active }: Props = $props();

  let doneOpen = $state(false);
  let list: HTMLElement | undefined = $state();
  /** The request to show a comment (a click on its bar) that this panel has carried out. */
  let handled = 0;

  // A bar in the margin was clicked: the comment is scrolled into view and marked for a moment.
  $effect(() => {
    const focus = comments.focus;
    if (!focus || !active || focus.n === handled) return;
    handled = focus.n;
    if (comments.all.find((comment) => comment.id === focus.id)?.done) doneOpen = true;
    void tick().then(() => {
      const row = list?.querySelector<HTMLElement>(`[data-comment="${CSS.escape(focus.id)}"]`);
      if (!row) return;
      row.scrollIntoView({ block: "nearest" });
      row.classList.remove("is-pulse");
      void row.offsetWidth; // play the mark again when the same comment is shown twice
      row.classList.add("is-pulse");
    });
  });
</script>

<div class="comments-panel">
  {#if comments.all.length === 0}
    <div class="comments-empty">
      <Icon name="comment" />
      <strong>{t("comments.empty.title")}</strong>
      <p>{t("comments.empty.text")}</p>
    </div>
  {:else}
    {#if comments.count > 0}
      <div class="comments-send">
        <button
          type="button"
          class="comments-send__button"
          onclick={() => void comments.sendAllToAgent()}
        >
          <Icon name="external" size="sm" />
          {t("comments.sendAll", { n: comments.count })}
        </button>
      </div>
    {/if}
    <div
      bind:this={list}
      class="scroll comments-scroll"
      role="region"
      aria-label={t("comments.list")}
    >
      {#if comments.groups.length === 0}
        <p class="comments-all-done">{t("comments.allDone")}</p>
      {/if}
      {#each comments.groups as group (group.path)}
        {@const title = documentTitle(group.path)}
        <section
          class={["comments-group", group.path === docs.path && "is-current"]}
          aria-label={t("comments.groupLabel", { path: group.path })}
        >
          <h3 class="comments-group__title" title={group.path}>
            <Icon name="file-text" size="sm" />
            <span class="comments-group__name">{title.name}</span>
            {#if title.folder !== ""}<span class="comments-group__folder">{title.folder}</span>{/if}
          </h3>
          <ul class="comments-list">
            {#each group.comments as comment (comment.id)}
              <li><CommentRow {comment} /></li>
            {/each}
          </ul>
        </section>
      {/each}
      {#if comments.done.length > 0}
        <section class="comments-group comments-group--done">
          <button
            type="button"
            class="comments-fold"
            aria-expanded={doneOpen}
            onclick={() => (doneOpen = !doneOpen)}
          >
            <Icon name="chevron-right" size="sm" class={doneOpen ? "is-open" : ""} />
            {tn("comments.done", comments.done.length)}
          </button>
          {#if doneOpen}
            <ul class="comments-list">
              {#each comments.done as comment (comment.id)}
                <li><CommentRow {comment} showPath /></li>
              {/each}
            </ul>
          {/if}
        </section>
      {/if}
    </div>
  {/if}
</div>
