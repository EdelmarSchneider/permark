<script lang="ts">
  // The Notes tab of the right panel (docs/design/v0.4.md section 3): the open notes of the project grouped by document,
  // the ones that are done folded away. A click on a note goes to its passage.
  import { tick } from "svelte";
  import { t, tn } from "../i18n";
  import { documentTitle } from "../notes";
  import { docs } from "../stores/docs.svelte";
  import { notes } from "../stores/notes.svelte";
  import Icon from "./Icon.svelte";
  import NoteRow from "./NoteRow.svelte";

  type Props = { active: boolean };
  let { active }: Props = $props();

  let doneOpen = $state(false);
  let list: HTMLElement | undefined = $state();
  /** The request to show a note (a click on its bar) that this panel has carried out. */
  let handled = 0;

  // A bar in the margin was clicked: the note is scrolled into view and marked for a moment.
  $effect(() => {
    const focus = notes.focus;
    if (!focus || !active || focus.n === handled) return;
    handled = focus.n;
    if (notes.all.find((note) => note.id === focus.id)?.done) doneOpen = true;
    void tick().then(() => {
      const row = list?.querySelector<HTMLElement>(`[data-note="${CSS.escape(focus.id)}"]`);
      if (!row) return;
      row.scrollIntoView({ block: "nearest" });
      row.classList.remove("is-pulse");
      void row.offsetWidth; // play the mark again when the same note is shown twice
      row.classList.add("is-pulse");
    });
  });
</script>

<div class="notes-panel">
  {#if notes.all.length === 0}
    <div class="notes-empty">
      <Icon name="note" />
      <strong>{t("notes.empty.title")}</strong>
      <p>{t("notes.empty.text")}</p>
    </div>
  {:else}
    <div bind:this={list} class="scroll notes-scroll" role="region" aria-label={t("notes.list")}>
      {#if notes.groups.length === 0}
        <p class="notes-all-done">{t("notes.allDone")}</p>
      {/if}
      {#each notes.groups as group (group.path)}
        {@const title = documentTitle(group.path)}
        <section
          class={["notes-group", group.path === docs.path && "is-current"]}
          aria-label={t("notes.groupLabel", { path: group.path })}
        >
          <h3 class="notes-group__title" title={group.path}>
            <Icon name="file-text" size="sm" />
            <span class="notes-group__name">{title.name}</span>
            {#if title.folder !== ""}<span class="notes-group__folder">{title.folder}</span>{/if}
          </h3>
          <ul class="notes-list">
            {#each group.notes as note (note.id)}
              <li><NoteRow {note} /></li>
            {/each}
          </ul>
        </section>
      {/each}
      {#if notes.done.length > 0}
        <section class="notes-group notes-group--done">
          <button
            type="button"
            class="notes-fold"
            aria-expanded={doneOpen}
            onclick={() => (doneOpen = !doneOpen)}
          >
            <Icon name="chevron-right" size="sm" class={doneOpen ? "is-open" : ""} />
            {tn("notes.done", notes.done.length)}
          </button>
          {#if doneOpen}
            <ul class="notes-list">
              {#each notes.done as note (note.id)}
                <li><NoteRow {note} showPath /></li>
              {/each}
            </ul>
          {/if}
        </section>
      {/if}
    </div>
  {/if}
</div>
