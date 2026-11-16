<script lang="ts">
  // Settings -> Integration -> Spelling: the language a document is checked in when it neither says nor shows one,
  // the person's own words (accepted in every document; each with its remove button, a field adds one) and the
  // dictionaries on this computer (each came from a choice in the status bar's language picker).
  import { onMount } from "svelte";
  import { sizeText, t } from "../../core/i18n";
  import ListEditor from "../../core/shell/settings/ListEditor.svelte";
  import SettingRow from "../../core/shell/settings/SettingRow.svelte";
  import { byName, languageLabel } from "./names";
  import { spelling, type SpellingStore } from "./store.svelte";

  type Props = { store?: SpellingStore };
  let { store = spelling }: Props = $props();

  const ids = $derived(byName(store.languages.map((l) => l.id)));
  const here = $derived(
    ids.flatMap((id) => {
      const language = store.languageOf(id);
      return language?.state.kind === "ready" ? [language] : [];
    }),
  );
  const words = $derived(store.personal ?? []);

  onMount(() => {
    void store.loadDefault();
    void store.loadPersonal();
  });

  /** The list as the person left it: the words added go in, the words taken out go. */
  function changeWords(next: string[]): void {
    for (const word of next) if (!words.includes(word)) void store.addToDictionary(word);
    for (const word of words) if (!next.includes(word)) void store.removeWord(word);
  }
</script>

<SettingRow
  label={t("spelling.settings.default")}
  labelFor="spelling-default"
  hint={t("spelling.settings.defaultHint")}
>
  <select
    id="spelling-default"
    class="field-input"
    value={store.defaultLanguage?.lang ?? ""}
    onchange={(e) => void store.setDefault(e.currentTarget.value)}
  >
    {#each ids as id (id)}
      <option value={id}>{languageLabel(id)}</option>
    {/each}
  </select>
</SettingRow>
<SettingRow label={t("spelling.settings.words")} hint={t("spelling.settings.wordsHint")} wide>
  <ListEditor
    items={words}
    label={t("spelling.settings.words")}
    placeholder={t("spelling.settings.wordsAdd")}
    onchange={changeWords}
  />
</SettingRow>
<SettingRow
  label={t("spelling.settings.dictionaries")}
  labelId="spelling-dictionaries"
  hint={t("spelling.settings.dictionariesHint")}
  wide
>
  {#if here.length === 0}
    <p class="none">{t("spelling.settings.none")}</p>
  {:else}
    <ul aria-labelledby="spelling-dictionaries">
      {#each here as language (language.id)}
        <li>
          <span>{languageLabel(language.id)}</span>
          <span class="size">{sizeText(language.bytes)}</span>
        </li>
      {/each}
    </ul>
  {/if}
</SettingRow>

<style>
  .none {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  ul {
    display: grid;
    gap: 2px;
    width: 100%;
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: var(--fs-ui-sm);
  }
  li {
    display: flex;
    justify-content: space-between;
    gap: var(--sp-3);
  }
  .size {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
</style>
