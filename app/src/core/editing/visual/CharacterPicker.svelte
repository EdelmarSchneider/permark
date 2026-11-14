<script lang="ts">
  // The popup behind the toolbar's emoji + symbols buttons: the groups the settings payload ships, searchable by
  // name. The payload is the source (no character table here), the filtering is `characters.ts` over
  // `core/search/fuzzy.ts`, and a click hands the character to the editor through the `run` path.
  import { api } from "../../../api";
  import type { CharacterGroup } from "../../../api";
  import { t, type MessageKey } from "../../i18n";
  import { scanGroups, type PickerKind } from "./characters";

  type Props = {
    /** The payload's emoji group, or the symbol groups. */
    kind: PickerKind;
    /** Insert the picked character at the caret and hand the keyboard back to the editor. */
    apply: (character: string) => void;
    /** Close the popup without inserting. */
    close: () => void;
    /** The editor's popover key handling (Escape closes, focus returns). */
    popoverKey: (event: KeyboardEvent) => void;
  };
  let { kind, apply, close, popoverKey }: Props = $props();

  let groups = $state.raw<CharacterGroup[]>([]);
  let query = $state("");

  const shown = $derived(scanGroups(kind, groups, query));

  /** The groups the groups file names these get a label of their own; any other name shows as it is. */
  const GROUP_KEYS: Record<string, MessageKey> = {
    emoji: "picker.group.emoji",
    greek: "picker.group.greek",
    operators: "picker.group.operators",
    relations: "picker.group.relations",
    arrows: "picker.group.arrows",
    units: "picker.group.units",
    scripts: "picker.group.scripts",
  };

  const labelOf = (name: string): string => {
    const key = GROUP_KEYS[name.trim().toLowerCase()];
    return key === undefined ? name : t(key);
  };

  // The groups, as the settings ship them: the boot read plus every settings change, like the shell's own
  // settings-following store (`core/shell/arrangement.svelte.ts`).
  $effect(() => {
    let gone = false;
    let off: (() => void) | null = null;
    void api.settingsGet().then((result) => {
      if (!gone && result.ok) groups = result.value.characters;
    });
    void api
      .on("settings-changed", (event) => {
        groups = event.settings.characters;
      })
      .then((stop) => {
        if (gone) stop();
        else off = stop;
      });
    return () => {
      gone = true;
      off?.();
    };
  });
</script>

<div class="picker">
  <!-- svelte-ignore a11y_autofocus -->
  <input
    class="picker__search"
    type="search"
    placeholder={t("picker.search")}
    aria-label={t("picker.search")}
    autofocus
    value={query}
    onkeydown={popoverKey}
    oninput={(e) => (query = e.currentTarget.value)}
  />
  {#if shown.length === 0}
    <p class="picker__empty">{t("picker.empty")}</p>
  {:else}
    {#each shown as group (group.name)}
      {@const label = labelOf(group.name)}
      <div class="picker__group" role="group" aria-label={label}>
        <h3 class="picker__title">{label}</h3>
        <div class="picker__grid">
          {#each group.entries as entry (entry.char)}
            <button
              type="button"
              class="picker__char"
              title={entry.names[0] ?? entry.char}
              onclick={() => apply(entry.char)}>{entry.char}</button
            >
          {/each}
        </div>
      </div>
    {/each}
  {/if}
  <div class="picker__foot">
    <button type="button" class="btn btn--sm btn--ghost" onclick={close}
      >{t("common.cancel")}</button
    >
  </div>
</div>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    padding: var(--sp-3);
    max-height: 22rem;
    overflow-y: auto;
  }
  .picker__search {
    height: 2rem;
    padding: 0 var(--sp-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font-size: var(--fs-ui);
  }
  .picker__group {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }
  .picker__title {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
  .picker__grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(2rem, 1fr));
    gap: var(--sp-1);
  }
  .picker__char {
    height: 2rem;
    padding: 0;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
    font-size: 1.125rem;
    line-height: 1;
    cursor: pointer;
  }
  .picker__char:hover {
    background: var(--hover);
  }
  .picker__empty {
    margin: 0;
    color: var(--faint);
    font-size: var(--fs-ui-sm);
  }
  .picker__foot {
    display: flex;
    justify-content: flex-end;
    gap: var(--sp-2);
  }
</style>
