<script lang="ts">
  // Settings popover (skeleton): language, theme, light tone. The full settings page comes with the shell features.
  import type { LanguagePref, LinkUpdate, ThemePref, Tone } from "../../api";
  import { language, LOCALE_NAMES, LOCALES, t, type MessageKey } from "../i18n";
  import { look } from "../look.svelte";
  import ReadingSettings from "./ReadingSettings.svelte";
  import type { WindowStore } from "../window.svelte";
  import { ui } from "./ui.svelte";

  type Props = { store: WindowStore; close: () => void };
  let { store, close }: Props = $props();

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

  const LINK_UPDATES: { value: LinkUpdate; label: MessageKey }[] = [
    { value: "ask", label: "settings.linkAsk" },
    { value: "always", label: "settings.linkAlways" },
    { value: "never", label: "settings.linkNever" },
  ];

  function parseLanguage(value: string): LanguagePref {
    if (value === "system") return "system";
    return LOCALES.find((locale) => locale === value) ?? "system";
  }

  let panel: HTMLElement | undefined = $state();

  // closes on Escape | click outside (imperative listeners, torn down on close)
  $effect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") close();
    };
    const onPointer = (e: PointerEvent): void => {
      const target = e.target;
      if (
        panel &&
        target instanceof Node &&
        !panel.contains(target) &&
        !(target instanceof Element && target.closest("[data-settings-trigger]"))
      )
        close();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onPointer);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onPointer);
    };
  });
</script>

<div class="popover" role="dialog" aria-label={t("settings.title")} bind:this={panel}>
  <div class="menu-label">{t("settings.title")}</div>

  <label class="row">
    <span>{t("settings.language")}</span>
    <select
      value={language.preference}
      onchange={(e) =>
        void store.changeSetting({ key: "language", value: parseLanguage(e.currentTarget.value) })}
    >
      <option value="system">{t("settings.languageSystem")}</option>
      {#each LOCALES as locale (locale)}
        <option value={locale} lang={locale}>{LOCALE_NAMES[locale]}</option>
      {/each}
    </select>
  </label>

  <div class="row">
    <span id="theme-label">{t("settings.theme")}</span>
    <div class="segments" role="radiogroup" aria-labelledby="theme-label">
      {#each THEMES as option (option.value)}
        <button
          type="button"
          role="radio"
          aria-checked={look.preference === option.value}
          class={["segment", look.preference === option.value && "is-on"]}
          onclick={() => void store.changeSetting({ key: "theme", value: option.value })}
          >{t(option.label)}</button
        >
      {/each}
    </div>
  </div>

  <div class="row">
    <span id="tone-label">{t("settings.tone")}</span>
    <div class="segments" role="radiogroup" aria-labelledby="tone-label">
      {#each TONES as option (option.value)}
        <button
          type="button"
          role="radio"
          aria-checked={look.tone === option.value}
          disabled={look.theme === "dark"}
          class={["segment", look.tone === option.value && "is-on"]}
          onclick={() => void store.changeSetting({ key: "tone", value: option.value })}
          >{t(option.label)}</button
        >
      {/each}
    </div>
  </div>

  <div class="row">
    <span id="link-update-label">{t("settings.linkUpdate")}</span>
    <div class="segments" role="radiogroup" aria-labelledby="link-update-label">
      {#each LINK_UPDATES as option (option.value)}
        <button
          type="button"
          role="radio"
          aria-checked={store.linkUpdate === option.value}
          class={["segment", store.linkUpdate === option.value && "is-on"]}
          onclick={() => void store.changeSetting({ key: "linkUpdate", value: option.value })}
          >{t(option.label)}</button
        >
      {/each}
    </div>
  </div>

  <ReadingSettings {store} />

  <div class="row">
    <span title={t("settings.keyboardHint")}>{t("settings.keyboard")}</span>
    <button type="button" class="btn btn--sm" onclick={() => ui.openKeyboard()}
      >{t("keymap.change")}</button
    >
  </div>
</div>

<style>
  .popover {
    position: fixed;
    top: calc(var(--h-header) - 4px);
    right: var(--sp-3);
    z-index: 40;
    width: 340px;
    max-height: calc(100vh - var(--h-header) - var(--sp-4));
    overflow-y: auto;
    padding: var(--sp-2) var(--sp-2) var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    box-shadow: var(--shadow-pop);
    animation: pm-fade var(--dur-fast) var(--ease);
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
    padding: 6px 10px;
  }
  select {
    min-width: 160px;
    height: 28px;
    padding: 0 6px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
  }
  .segments {
    display: inline-flex;
    padding: 2px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--surface);
  }
  .segment {
    height: 24px;
    padding: 0 8px;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    cursor: pointer;
  }
  .segment:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .segment.is-on {
    background: var(--bg);
    color: var(--text);
    box-shadow: 0 1px 2px var(--hover);
  }
</style>
