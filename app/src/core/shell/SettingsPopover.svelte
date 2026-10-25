<script lang="ts">
  // Quick settings under the header button: language, theme, light tone, the rows plugins put here (the agent
  // connection's state + its way in), and the way to the keyboard map and to all the settings. Focus moves in on open
  // (first control) and Tab stays inside; Escape closes and puts focus back on the gear; a click outside closes.
  import Settings from "@lucide/svelte/icons/settings";
  import type { LanguagePref, ThemePref, Tone } from "../../api";
  import { conditions } from "../conditions/context.svelte";
  import { language, LOCALE_NAMES, LOCALES, t, type MessageKey } from "../i18n";
  import { look } from "../look.svelte";
  import { contributions } from "../plugins/instance.svelte";
  import type { WindowStore } from "../window.svelte";
  import Segments from "./settings/Segments.svelte";
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

  function parseLanguage(value: string): LanguagePref {
    if (value === "system") return "system";
    return LOCALES.find((locale) => locale === value) ?? "system";
  }

  let panel: HTMLElement | undefined = $state();
  /** The quick rows of the plugins' own settings sections (order of their sections). */
  const quick = $derived(
    contributions
      .visible("settingsPage", conditions.current)
      .filter((p) => p.quick !== undefined)
      .sort((a, b) => a.order - b.order),
  );

  const FOCUSABLE =
    'button:not(:disabled), input:not(:disabled), select:not(:disabled), [tabindex="0"]';

  function focusables(): HTMLElement[] {
    // Tab stops only (a segment group's other options are reached with the arrows)
    return panel
      ? Array.from(panel.querySelectorAll<HTMLElement>(FOCUSABLE)).filter((el) => el.tabIndex >= 0)
      : [];
  }

  // focus enters the popover on its first control (Ctrl+, | Enter on the gear | a click)
  $effect(() => {
    focusables()[0]?.focus();
  });

  /** Tab | Shift+Tab cycle inside while it is open. */
  function onkeydown(e: KeyboardEvent): void {
    if (e.key !== "Tab") return;
    const all = focusables();
    const first = all[0];
    const last = all[all.length - 1];
    if (first === undefined || last === undefined) return;
    const at = document.activeElement;
    if (e.shiftKey && (at === first || !panel?.contains(at))) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && (at === last || !panel?.contains(at))) {
      e.preventDefault();
      first.focus();
    }
  }

  // closes on Escape (focus back on the gear) | click outside (imperative listeners, torn down on close)
  $effect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key !== "Escape") return;
      close();
      document.querySelector<HTMLElement>("[data-settings-trigger]")?.focus();
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

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="popover"
  role="dialog"
  tabindex="-1"
  aria-label={t("settings.title")}
  bind:this={panel}
  {onkeydown}
>
  <div class="menu-label">{t("settings.title")}</div>

  <label class="row">
    <span>{t("settings.language")}</span>
    <select
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
  </label>

  <div class="row">
    <span id="pop-theme">{t("settings.theme")}</span>
    <Segments
      labelledby="pop-theme"
      value={look.preference}
      options={THEMES.map((o) => ({ value: o.value, label: t(o.label) }))}
      onchange={(value) => void store.changeSetting({ key: "theme", value })}
    />
  </div>

  <div class="row">
    <span id="pop-tone">{t("settings.tone")}</span>
    <Segments
      labelledby="pop-tone"
      value={look.tone}
      disabled={look.theme === "dark"}
      options={TONES.map((o) => ({ value: o.value, label: t(o.label) }))}
      onchange={(value) => void store.changeSetting({ key: "tone", value })}
    />
  </div>

  {#each quick as page (page.owner + page.id)}
    {#await page.quick?.() then loaded}
      {#if loaded}<loaded.default />{/if}
    {/await}
  {/each}

  <div class="foot">
    <button type="button" class="btn btn--sm" onclick={() => ui.openKeyboard()}>
      {t("settings.keyboard")}
    </button>
    <button type="button" class="btn btn--primary btn--sm" onclick={() => ui.openSettingsPage()}>
      <Settings size={13} aria-hidden="true" />
      {t("settings.all")}
    </button>
  </div>
</div>

<style>
  .popover {
    position: fixed;
    top: calc(var(--h-top) - 2px);
    right: var(--sp-3);
    z-index: 40;
    width: min(380px, calc(100vw - var(--sp-4)));
    max-height: calc(100vh - var(--h-top) - var(--sp-4));
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
    flex-wrap: wrap;
    gap: var(--sp-1) var(--sp-3);
    padding: 6px 10px;
  }
  .row select {
    min-width: 150px;
  }
  .foot {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    gap: var(--sp-2);
    margin-top: var(--sp-2);
    padding: var(--sp-3) 10px 0;
    border-top: 1px solid var(--border);
  }
</style>
