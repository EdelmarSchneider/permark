<script lang="ts">
  // The notes shared with this project's agents, one row each (title + Stop sharing), and "+ Add note": an inline
  // picker (not a dialog) with a search field over the person's other notes, newest first. Enter | a click shares
  // and the picker stays open for the next one; Escape | Back close the picker alone (the popover around it stays:
  // the key is taken here first). One component for the status popover and the settings section; the parent draws
  // it only while the notes plugin answers.
  import X from "@lucide/svelte/icons/x";
  import { tick } from "svelte";
  import type { SharedNote } from "../../api";
  import { isComposition } from "../../core/commands/combo";
  import { t } from "../../core/i18n";
  import { moveActive } from "../../core/search/finderModel";
  import { fuzzyMatch } from "../../core/search/fuzzy";
  import { agent } from "./store.svelte";

  /** Candidates listed before anything is typed. */
  const NEWEST = 8;

  const id = $props.id();
  const notes = $derived(agent.sharedNotes);
  const shared = $derived(notes?.shared ?? []);
  const candidates = $derived(notes?.candidates ?? []);

  let picking = $state(false);
  let query = $state("");
  let active = $state(0);
  let said = $state("");
  let list: HTMLUListElement | null = $state(null);
  let add: HTMLButtonElement | null = $state(null);
  let field: HTMLInputElement | null = $state(null);
  let back: HTMLButtonElement | null = $state(null);

  /** Candidates the search keeps, still newest first (a picker of recent notes, not a ranking). */
  const shown = $derived(
    query.trim() === ""
      ? candidates.slice(0, NEWEST)
      : candidates.filter((note) => fuzzyMatch(query, titleOf(note)) !== null),
  );
  const current = $derived(shown[Math.min(active, shown.length - 1)] ?? null);
  const optionId = (note: SharedNote): string => `${id}-note-${note.id}`;

  function titleOf(note: SharedNote): string {
    return note.title === "" ? t("notes.untitled") : note.title;
  }

  async function openPicker(): Promise<void> {
    query = "";
    active = 0;
    picking = true;
    await tick();
    keepKeyboard();
  }

  /** The keyboard in the picker: on the search field, or on Back while there is nothing to search (never the page). */
  function keepKeyboard(): void {
    (field ?? back)?.focus();
  }

  async function closePicker(): Promise<void> {
    picking = false;
    await tick();
    add?.focus();
  }

  async function share(note: SharedNote | null): Promise<void> {
    if (note === null) return;
    if (!(await agent.shareNote(note.id))) return;
    said = t("agent.notes.added", { name: titleOf(note) });
    // the shared one leaves the list: the keyboard stays at its place, on the next one
    active = Math.max(Math.min(active, shown.length - 1), 0);
    await tick();
    if (picking) keepKeyboard();
  }

  async function stop(note: SharedNote, index: number): Promise<void> {
    if (!(await agent.unshareNote(note.id))) return;
    said = t("agent.notes.removed", { name: titleOf(note) });
    await tick();
    const buttons = Array.from(
      list?.querySelectorAll<HTMLButtonElement>("button[data-stop]") ?? [],
    );
    (buttons[index] ?? buttons[index - 1] ?? add)?.focus();
  }

  /** The option the keyboard is on stays in view. */
  async function keepInView(): Promise<void> {
    await tick();
    if (current !== null)
      document.getElementById(optionId(current))?.scrollIntoView({ block: "nearest" });
  }

  function onPickerKey(event: KeyboardEvent): void {
    if (isComposition(event) || event.altKey || event.ctrlKey || event.metaKey) return;
    switch (event.key) {
      case "Escape":
        // taken here, so the popover | the settings page around stays open
        event.preventDefault();
        void closePicker();
        return;
      case "ArrowDown":
      case "ArrowUp":
        if (event.target !== field) return;
        event.preventDefault();
        active = moveActive(active, event.key === "ArrowDown" ? 1 : -1, shown.length);
        void keepInView();
        return;
      case "Enter":
        if (event.target !== field) return;
        event.preventDefault();
        void share(current);
        return;
      default:
        return;
    }
  }
</script>

<div class="shared-notes">
  {#if shared.length === 0}
    <p class="muted">{t("agent.notes.empty")}</p>
  {:else}
    <ul class="rows" bind:this={list}>
      {#each shared as note, i (note.id)}
        {@const title = titleOf(note)}
        <li>
          <span class="note-title" {title}>{title}</span>
          <button
            type="button"
            class="icon"
            data-stop
            aria-label={t("agent.notes.stopAria", { name: title })}
            title={t("agent.notes.stop")}
            onclick={() => void stop(note, i)}
          >
            <X size={14} aria-hidden="true" />
          </button>
        </li>
      {/each}
    </ul>
  {/if}
  {#if picking}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="picker" onkeydown={onPickerKey}>
      <div class="picker-head">
        <button
          type="button"
          class="link"
          data-back
          bind:this={back}
          onclick={() => void closePicker()}
        >
          ‹ {t("agent.notes.back")}
        </button>
      </div>
      {#if candidates.length === 0}
        <p class="muted">{t("agent.notes.none")}</p>
      {:else}
        <input
          bind:this={field}
          class="field"
          type="search"
          role="combobox"
          aria-expanded="true"
          aria-controls="{id}-list"
          aria-activedescendant={current === null ? undefined : optionId(current)}
          aria-label={t("agent.notes.search")}
          placeholder={t("agent.notes.search")}
          autocomplete="off"
          spellcheck="false"
          value={query}
          oninput={(e) => {
            query = e.currentTarget.value;
            active = 0;
          }}
        />
        <ul class="options" id="{id}-list" role="listbox" aria-label={t("agent.notes.title")}>
          {#each shown as note (note.id)}
            {@const title = titleOf(note)}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <li
              id={optionId(note)}
              role="option"
              aria-selected={current?.id === note.id}
              class:active={current?.id === note.id}
              {title}
              onpointermove={() => (active = shown.indexOf(note))}
              onmousedown={(e) => e.preventDefault()}
              onclick={() => void share(note)}
            >
              {title}
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {:else}
    <button
      type="button"
      class="link"
      data-add-note
      bind:this={add}
      onclick={() => void openPicker()}
    >
      {t("agent.notes.add")}
    </button>
  {/if}
  <p class="sr-only" role="status">{said}</p>
</div>

<style>
  .shared-notes {
    display: grid;
    gap: var(--sp-2);
    min-width: 0;
    font-size: var(--fs-ui-sm);
  }
  .muted {
    margin: 0;
    color: var(--muted);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  /* five rows, then the list scrolls */
  .rows {
    display: grid;
    gap: 2px;
    max-height: calc(5 * 26px);
    overflow-y: auto;
  }
  .rows li {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    min-width: 0;
    min-height: 24px;
  }
  .note-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .icon {
    display: inline-flex;
    flex: none;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
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
  .link {
    justify-self: start;
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent-text);
    font: inherit;
    cursor: pointer;
  }
  .link:hover {
    text-decoration: underline;
  }
  .picker {
    display: grid;
    gap: var(--sp-2);
    min-width: 0;
  }
  .field {
    min-width: 0;
    padding: 4px var(--sp-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    outline: none;
  }
  .field:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-soft);
  }
  .options {
    max-height: calc(8 * 26px);
    overflow-y: auto;
  }
  .options li {
    padding: 3px var(--sp-2);
    overflow: hidden;
    border-radius: var(--radius-control);
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
  }
  .options li.active {
    background: var(--accent-soft);
  }
</style>
