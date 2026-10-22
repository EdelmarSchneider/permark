<script lang="ts">
  // Reading: the language, the look (theme + light tone), how documents are read (width, zoom) in this window, and
  // whether long lines of text wrap (everywhere).
  import type { LanguagePref, ThemePref, Tone } from "../../../api";
  import { keyHint } from "../../commands/hint";
  import {
    WIDTHS,
    ZOOM_DEFAULT,
    ZOOM_MAX,
    ZOOM_MIN,
    reading,
    type Width,
  } from "../../doc/reading.svelte";
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
  const WIDTH_LABELS: Record<Width, MessageKey> = {
    comfortable: "width.comfortable",
    wide: "width.wide",
    full: "width.full",
  };

  function parseLanguage(value: string): LanguagePref {
    if (value === "system") return "system";
    return LOCALES.find((locale) => locale === value) ?? "system";
  }

  const wrapKeys = $derived(keyHint("doc.wrapLines"));

  /** A document can be read here (a project, a file or the Notes is shown): width and zoom have a page to act on. */
  const reads = $derived(store.view.state === "showing");
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
  label={t("settings.width")}
  labelId="set-width"
  hint={reads ? t("settings.readingHint") : t("settings.readingNone")}
>
  <Segments
    labelledby="set-width"
    value={reading.width}
    disabled={!reads}
    options={WIDTHS.map((w) => ({ value: w, label: t(WIDTH_LABELS[w]) }))}
    onchange={(value) => reading.setWidth(value)}
  />
</SettingRow>

<SettingRow label={t("settings.zoom")} labelId="set-zoom">
  <div class="zoom" role="group" aria-labelledby="set-zoom">
    <button
      type="button"
      class="btn btn--sm"
      aria-label={t("media.zoomOut")}
      disabled={!reads || reading.zoom <= ZOOM_MIN}
      onclick={() => reading.zoomBy(-1)}>−</button
    >
    <output class="zoom-value" aria-live="polite">{reading.zoom}%</output>
    <button
      type="button"
      class="btn btn--sm"
      aria-label={t("media.zoomIn")}
      disabled={!reads || reading.zoom >= ZOOM_MAX}
      onclick={() => reading.zoomBy(1)}>+</button
    >
    <button
      type="button"
      class="btn btn--ghost btn--sm"
      disabled={!reads || reading.zoom === ZOOM_DEFAULT}
      onclick={() => reading.setZoom(ZOOM_DEFAULT)}
    >
      {t("cmd.zoomReset")}
    </button>
  </div>
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
    onchange={(e) => void store.changeSetting({ key: "wrapLines", value: e.currentTarget.checked })}
  />
</SettingRow>

<style>
  .zoom {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
  }
  .zoom-value {
    min-width: 44px;
    text-align: center;
    font-variant-numeric: tabular-nums;
  }
</style>
