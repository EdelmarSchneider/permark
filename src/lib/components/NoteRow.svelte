<script lang="ts">
  // One review note in the Notes tab: a check to mark it done, the quoted passage with the note and where it is (a click
  // goes to the passage), and the reply an agent left when it resolved it.
  import { t } from "../i18n";
  import { documentTitle, excerpt, trail } from "../notes";
  import { app } from "../stores/app.svelte";
  import { clock } from "../stores/clock.svelte";
  import { notes } from "../stores/notes.svelte";
  import { ago } from "../time";
  import type { Note } from "../types";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";

  type Props = {
    note: Note;
    /** Name the document instead of the section (the folded list of done notes mixes documents). */
    showPath?: boolean;
  };
  let { note, showPath = false }: Props = $props();

  const meta = $derived(
    [
      showPath ? documentTitle(note.path).name : trail(note.headingPath),
      t("notes.line", { line: note.line }),
      ago(note.createdMs, clock.now),
    ]
      .filter((part) => part !== "")
      .join(" · "),
  );
  const lost = $derived(
    note.state === "documentGone"
      ? { name: t("notes.state.documentGone"), hint: t("notes.state.documentGoneHint") }
      : note.state === "passageChanged"
        ? { name: t("notes.state.passageChanged"), hint: t("notes.state.passageChangedHint") }
        : null,
  );
  const check = $derived(note.done ? t("notes.reopen") : t("notes.markDone"));
</script>

<div class={["note-row", note.done && "is-done"]} data-note={note.id}>
  <input
    class="note-check"
    type="checkbox"
    checked={note.done}
    aria-label={check}
    title={check}
    onchange={(e) => void notes.setDone(note.id, e.currentTarget.checked)}
  />
  <button
    type="button"
    class="note-body"
    title={t("notes.jump")}
    onclick={() => void app.openNote(note)}
  >
    <span class="note-quote">{excerpt(note.quote)}</span>
    <span class="note-text">{note.text}</span>
    <span class="note-meta">{meta}</span>
  </button>
  <IconButton
    small
    class="note-delete"
    icon="trash"
    label={t("notes.delete")}
    onclick={() => void notes.remove(note.id)}
  />
  {#if lost}
    <span class="note-lost" title={lost.hint}><Icon name="alert" size="xs" />{lost.name}</span>
  {/if}
  {#if note.reply}
    <p class="note-reply">
      <Icon name="corner-down-right" size="xs" />
      <span class="sr-only">{t("notes.reply")}: </span>{note.reply}
    </p>
  {/if}
</div>
