<script lang="ts">
  // Reading: the language, the look (theme + light tone) and whether long lines of text wrap (everywhere). Reading width
  // and zoom are per project and act on the shown document, so they live in the document header's reading menu.
  import type { LanguagePref, ThemePref, Tone } from "../../../api";
  import { keyHint } from "../../commands/hint";
  import { setWrap } from "../../doc/wrapLines";
  import { language, LOCALE_NAMES, LOCALES, t, type MessageKey } from "../../i18n";
  import { look } from "../../look.svelte";
  import type { WindowStore } from "../../window.svelte";
  import Segments from "./Segments.svelte";
  import SettingRow from "./SettingRow.svelte";

  type Props = { store: WindowStore };
  let { store }: Props = $props();

  const THEMES: { value: ThemePref; label: MessageKey }[] = [
    { value: "system", label: "settings.themeSystem" },
    { value: "light", label: "settings.themeLight" },
    { value: "dark", label: "settings.themeDark" },
  ];
  const TONES: { value: Tone; label: MessageKey }[] = [
    { value: "white", label: "settings.toneWhite" },
    { value: "paper", label: "settings.tonePaper" },
    { value: "sage", label: "settings.toneSage" },
    { value: "gray", label: "settings.toneGray" },
  ];

  function parseLanguage(value: string): LanguagePref {
    if (value === "system") return "system";
    return LOCALES.find((locale) => locale === value) ?? "system";
  }

  const wrapKeys = $derived(keyHint("doc.wrapLines"));
</script>

<SettingRow label={t("settings.language")} labelFor="set-language">
  <select
    id="set-language"
    class="field-select"
    value={language.preference}
    onchange={(e) =>
      void store.changeSetting({ key: "language", value: parseLanguage(e.currentTarget.value) })}
  >
    <option value="system">{t("settings.languageSystem")}</option>
    {#each LOCALES as locale (locale)}
      <option value={locale} lang={locale}>{LOCALE_NAMES[locale]}</option>
    {/each}
  </select>
</SettingRow>

<SettingRow label={t("settings.theme")} labelId="set-theme">
  <Segments
    labelledby="set-theme"
    value={look.preference}
    options={THEMES.map((o) => ({ value: o.value, label: t(o.label) }))}
    onchange={(value) => void store.changeSetting({ key: "theme", value })}
  />
</SettingRow>

<SettingRow label={t("settings.tone")} labelId="set-tone" hint={t("settings.toneHint")}>
  <Segments
    labelledby="set-tone"
    value={look.tone}
    disabled={look.theme === "dark"}
    options={TONES.map((o) => ({ value: o.value, label: t(o.label) }))}
    onchange={(value) => void store.changeSetting({ key: "tone", value })}
  />
</SettingRow>

<SettingRow
  label={t("settings.wrap")}
  labelFor="set-wrap"
  hint={wrapKeys === null ? t("settings.wrapHint") : t("settings.wrapHintKeys", { keys: wrapKeys })}
>
  <input
    id="set-wrap"
    type="checkbox"
    role="switch"
    checked={look.wrap}
    onchange={(e) => void setWrap(e.currentTarget.checked, false)}
  />
</SettingRow>
