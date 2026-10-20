<script lang="ts">
  // The full settings: a left list of sections (Reading, Editing, Integration, Plugins, Project) and the shown one at
  // the right, each a card of rows. Covers the window below the top bar; Escape or Close goes back to where the
  // person was.
  import X from "@lucide/svelte/icons/x";
  import { conditions } from "../../conditions/context.svelte";
  import { t, type MessageKey } from "../../i18n";
  import type { WindowStore } from "../../window.svelte";
  import { SETTINGS_SECTIONS, ui, type SettingsSection } from "../ui.svelte";
  import Card from "./Card.svelte";
  import EditingSection from "./EditingSection.svelte";
  import IntegrationSection from "./IntegrationSection.svelte";
  import PluginsSection from "./PluginsSection.svelte";
  import ProjectSection from "./ProjectSection.svelte";
  import ReadingSection from "./ReadingSection.svelte";

  type Props = { store: WindowStore };
  let { store }: Props = $props();

  const LABELS: Record<SettingsSection, MessageKey> = {
    reading: "settings.section.reading",
    editing: "settings.section.editing",
    integration: "settings.section.integration",
    plugins: "settings.section.plugins",
    project: "settings.section.project",
  };

  const inProject = $derived(conditions.current.workspace === "project");
  /** Sections the person can reach here: Project only while a project is shown. */
  const sections = $derived(SETTINGS_SECTIONS.filter((s) => s !== "project" || inProject));
  const wanted = $derived(ui.settingsPage.status === "open" ? ui.settingsPage.section : "reading");
  const section = $derived(sections.includes(wanted) ? wanted : "reading");

  let page: HTMLElement | undefined = $state();

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "Escape" && !e.defaultPrevented) {
      e.preventDefault();
      ui.closeSettingsPage();
      return;
    }
    if (e.key !== "Tab" || !page) return;
    const focusable = Array.from(
      page.querySelectorAll<HTMLElement>(
        'button:not(:disabled), input:not(:disabled), select:not(:disabled), [tabindex="0"]',
      ),
    ).filter((el) => el.offsetParent !== null);
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last?.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first?.focus();
    }
  }

  function onnavkey(e: KeyboardEvent): void {
    const step =
      e.key === "ArrowDown" || e.key === "ArrowRight"
        ? 1
        : e.key === "ArrowUp" || e.key === "ArrowLeft"
          ? -1
          : 0;
    if (step === 0) return;
    e.preventDefault();
    const at = sections.indexOf(section);
    const next = sections[(at + step + sections.length) % sections.length];
    if (next) {
      ui.showSettingsSection(next);
      queueMicrotask(() => page?.querySelector<HTMLElement>('[aria-current="page"]')?.focus());
    }
  }

  // focus enters the page on the section list; it goes back where it was when the page closes
  $effect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    page?.querySelector<HTMLElement>('[aria-current="page"]')?.focus();
    return () => before?.focus();
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="overlay">
  <div
    class="page"
    role="dialog"
    aria-modal="true"
    aria-label={t("settings.title")}
    tabindex="-1"
    bind:this={page}
    {onkeydown}
  >
    <header class="bar">
      <h2>{t("settings.title")}</h2>
      <span class="spacer"></span>
      <button type="button" class="btn btn--sm" onclick={() => ui.closeSettingsPage()}>
        <X size={13} aria-hidden="true" />
        {t("common.close")}
      </button>
    </header>
    <div class="layout">
      <nav class="nav" aria-label={t("settings.sections")}>
        {#each sections as name (name)}
          <button
            type="button"
            class="nav-item"
            aria-current={name === section ? "page" : undefined}
            tabindex={name === section ? 0 : -1}
            onclick={() => ui.showSettingsSection(name)}
            onkeydown={onnavkey}
          >
            {t(LABELS[name])}
          </button>
        {/each}
      </nav>
      <div class="content">
        <div class="inner">
          <h3 class="title">{t(LABELS[section])}</h3>
          {#if section === "reading"}
            <Card><ReadingSection {store} /></Card>
          {:else if section === "editing"}
            <Card><EditingSection {store} /></Card>
          {:else if section === "integration"}
            <IntegrationSection />
          {:else if section === "plugins"}
            <PluginsSection {store} />
          {:else}
            <ProjectSection />
          {/if}
        </div>
      </div>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: var(--h-top) 0 0;
    z-index: 45;
    display: flex;
    background: var(--bg);
    animation: pm-fade var(--dur-fast) var(--ease);
  }
  .page {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    outline: none;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    height: var(--h-header);
    flex: none;
    padding: 0 var(--sp-3);
    background: var(--surface);
    border-bottom: 1px solid var(--border);
  }
  .bar h2 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
  }
  .layout {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .nav {
    display: flex;
    flex: none;
    flex-direction: column;
    gap: 2px;
    width: 200px;
    padding: var(--sp-3) var(--sp-2);
    background: var(--surface);
    border-right: 1px solid var(--border);
    overflow-y: auto;
  }
  .nav-item {
    padding: 6px var(--sp-3);
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--muted);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition:
      background-color var(--dur-fast) var(--ease),
      color var(--dur-fast) var(--ease);
  }
  .nav-item:hover {
    background: var(--hover);
    color: var(--text);
  }
  .nav-item[aria-current="page"] {
    background: var(--accent-soft);
    color: var(--accent-text);
    font-weight: 600;
  }
  .content {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    padding: var(--sp-5) var(--sp-5) var(--sp-7);
  }
  .inner {
    max-width: 720px;
    margin: 0 auto;
  }
  .title {
    margin: 0 0 var(--sp-4);
    font-size: 18px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  @media (max-width: 640px) {
    .layout {
      flex-direction: column;
    }
    .nav {
      flex-direction: row;
      width: auto;
      padding: var(--sp-2);
      border-right: 0;
      border-bottom: 1px solid var(--border);
      overflow-x: auto;
    }
    .nav-item {
      white-space: nowrap;
    }
    .content {
      padding: var(--sp-4) var(--sp-4) var(--sp-6);
    }
  }
</style>
