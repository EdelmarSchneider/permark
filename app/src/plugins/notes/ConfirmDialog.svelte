<script lang="ts">
  // Asks before something that cannot be taken back with a click: deleting pinned notes, taking off the list a file
  // that holds typed text.
  import { t, tn } from "../../core/i18n";
  import { shownName } from "./model";
  import NoteDialog from "./NoteDialog.svelte";
  import type { NotesStore } from "./store.svelte";

  type Props = { store: NotesStore };
  let { store }: Props = $props();

  const asked = $derived(store.confirm);
  const message = $derived.by(() => {
    if (asked.status === "deleteNotes") {
      if (asked.pinned.length === 1) {
        const name = asked.pinned[0] ?? "";
        const entry = store.entries.find((e) => e.path === name);
        return t("notes.deletePinned", { name: entry ? shownName(entry) : name });
      }
      return tn("notes.deletePinnedMany", asked.pinned.length);
    }
    if (asked.status === "removeFile") {
      const file = store.entries.find((e) => e.path === asked.path);
      return t("notes.file.removeUnsaved", { name: file?.name ?? "" });
    }
    return "";
  });
  const action = $derived(
    asked.status === "removeFile" ? t("notes.file.remove") : t("notes.delete"),
  );
</script>

{#if asked.status !== "closed"}
  <NoteDialog title={action} close={() => store.cancelConfirm()}>
    <p class="message">{message}</p>
    {#snippet footer()}
      <button type="button" class="btn" data-first onclick={() => store.cancelConfirm()}>
        {t("common.cancel")}
      </button>
      <button type="button" class="btn btn--primary" onclick={() => void store.confirmDelete()}>
        {action}
      </button>
    {/snippet}
  </NoteDialog>
{/if}

<style>
  .message {
    margin: 0;
    overflow-wrap: anywhere;
  }
</style>
