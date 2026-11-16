<script lang="ts">
  // The plugin's menus over the window: a marked word's menu (shown once its corrections are in), the language
  // picker (its list, or the default for new documents in place; Left goes back) and the download question. Each
  // menu gives the keyboard back to what had it when it closes (the editor, with its caret).
  import ContextMenu from "../../core/explorer/ContextMenu.svelte";
  import { t } from "../../core/i18n";
  import DownloadAsk from "./DownloadAsk.svelte";
  import { defaultEntries, languageEntries, wordEntries } from "./menus";
  import type { SpellingStore } from "./store.svelte";

  type Props = { store: SpellingStore };
  let { store }: Props = $props();

  const word = $derived.by(() => {
    const at = store.menuAt;
    const found = store.suggested;
    if (at === null || found?.word !== at.word) return null;
    return { at, entries: wordEntries(store, at, found.list) };
  });

  const picker = $derived.by(() => {
    const at = store.picker;
    if (at === null) return null;
    if (at.level === "default") {
      return {
        at,
        entries: defaultEntries(store),
        label: t("spelling.languages.defaultTitle"),
        back: () => store.openPicker(at.x, at.y, "languages", true),
        start: undefined,
      };
    }
    const entries = languageEntries(store, at);
    const opener = entries.findIndex((entry) => entry.kind === "item" && entry.opens === true);
    return {
      at,
      entries,
      label: t("spelling.languages.label"),
      back: undefined,
      start: at.back ? opener : undefined,
    };
  });
</script>

{#if word !== null}
  {#key word.at}
    <ContextMenu
      x={word.at.x}
      y={word.at.y}
      entries={word.entries}
      label={t("spelling.menu.label", { word: word.at.word })}
      close={() => store.closeMenu()}
    />
  {/key}
{/if}
{#if picker !== null}
  <!-- the default's list replaces the language list in place: a fresh menu (keyboard on its first entry) -->
  {#key picker.at}
    <ContextMenu
      x={picker.at.x}
      y={picker.at.y}
      entries={picker.entries}
      label={picker.label}
      close={() => store.closePicker()}
      back={picker.back}
      start={picker.start}
    />
  {/key}
{/if}
{#if store.asking !== null}
  <DownloadAsk {store} asked={store.asking} />
{/if}
