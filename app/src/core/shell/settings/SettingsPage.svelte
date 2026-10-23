<script lang="ts">
  // The full settings: a left list of sections (Reading, Editing, Integration, Plugins, Projects inside, This folder,
  // and the plugins' own, such as AI agents, by order) and the shown one at the right, each a card of rows; Projects
  // inside only where the folder holds projects (`projects.md` §12.2; loaded when shown). Covers the window below the
  // top bar; Escape or Close goes back to where the person was.
  import X from "@lucide/svelte/icons/x";
  import { conditions } from "../../conditions/context.svelte";
  import { t, type MessageKey } from "../../i18n";
  import { contributions } from "../../plugins/instance.svelte";
  import { projects } from "../../projects/instance.svelte";
  import type { WindowStore } from "../../window.svelte";
  import {
    CORE_SECTION_ORDER,
    SETTINGS_SECTIONS,
    ui,
    type CoreSettingsSection,
    type SettingsSection,
  } from "../ui.svelte";
  import Card from "./Card.svelte";
  import EditingSection from "./EditingSection.svelte";
  import IntegrationSection from "./IntegrationSection.svelte";
  import PluginsSection from "./PluginsSection.svelte";
  import ProjectSection from "./ProjectSection.svelte";
  import ReadingSection from "./ReadingSection.svelte";

  type Props = { store: WindowStore };
  let { store }: Props = $props();

  const LABELS: Record<CoreSettingsSection, MessageKey> = {
    reading: "settings.section.reading",
    editing: "settings.section.editing",
    integration: "settings.section.integration",
    plugins: "settings.section.plugins",
    projects: "settings.section.projects",
    project: "settings.section.project",
  };

  const inProject = $derived(conditions.current.workspace === "project");
  /** The plugins' own sections that may show here (plugin on, requirement met). */
  const pluginPages = $derived(contributions.visible("settingsPage", conditions.current));
  type NavItem = { id: SettingsSection; label: MessageKey; order: number };
  /** Sections the person can reach here: Project only while a project is shown, Projects only where it holds some;
   *  the plugins' sections among them by order. */
  const nav = $derived(
    [
      ...SETTINGS_SECTIONS.filter(
        (s) =>
          (s !== "project" || inProject) && (s !== "projects" || (inProject && projects.present)),
      ).map((s): NavItem => ({ id: s, label: LABELS[s], order: CORE_SECTION_ORDER[s] })),
      ...pluginPages.map((p): NavItem => ({ id: p.id, label: p.label, order: p.order })),
    ].sort((a, b) => a.order - b.order),
  );
  const sections = $derived(nav.map((item) => item.id));
  const wanted = $derived(ui.settingsPage.status === "open" ? ui.settingsPage.section : "reading");
  /** Settings > Projects inside: its code loads the first time it shows (a plain folder never needs it). */
  const projectsSection = (): Promise<typeof import("../../projects/ProjectsSection.svelte")> =>
    import("../../projects/ProjectsSection.svelte");
  const section = $derived(sections.includes(wanted) ? wanted : "reading");
  const shown = $derived(nav.find((item) => item.id === section));
  const pluginPage = $derived(pluginPages.find((p) => p.id === section) ?? null);
  /** Its code, loaded once per page shown (the same promise while it shows: the section is never drawn twice). */
  const pluginView = $derived(pluginPage?.view() ?? null);

  let page: HTMLElement | undefined = $state();

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "Escape" && !e.defaultPrevented) {
      e.preventDefault();
      // consent mode: Esc = Cancel (back to the Explorer, the strip still asks)
      if (projects.choosing !== null) projects.cancelChoose();
      else ui.closeSettingsPage();
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

  // focus enters the page on the section list (or on what the section asks for: the Choose heading); it goes back
  // where it was when the page closes
  $effect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    (
      page?.querySelector<HTMLElement>("[data-autofocus]") ??
      page?.querySelector<HTMLElement>('[aria-current="page"]')
    )?.focus();
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
        {#each nav as item (item.id)}
          <button
            type="button"
            class="nav-item"
            aria-current={item.id === section ? "page" : undefined}
            tabindex={item.id === section ? 0 : -1}
            onclick={() => ui.showSettingsSection(item.id)}
            onkeydown={onnavkey}
          >
            {t(item.label)}
          </button>
        {/each}
      </nav>
      <div class="content">
        <div class="inner">
          <h3 class="title">{t(shown?.label ?? LABELS.reading)}</h3>
          {#if pluginView !== null}
            {#await pluginView then loaded}
              <loaded.default />
            {/await}
          {:else if section === "reading"}
            <Card><ReadingSection {store} /></Card>
          {:else if section === "editing"}
            <Card><EditingSection {store} /></Card>
          {:else if section === "integration"}
            <IntegrationSection />
          {:else if section === "plugins"}
            <PluginsSection {store} />
          {:else if section === "projects"}
            {#await projectsSection() then loaded}
              <loaded.default />
            {/await}
          {:else}
            <ProjectSection />
          {/if}
        </div>
      </div>
    </div>
    <!-- what a projects action did, for screen readers (inside the dialog: outside it is not heard while it shows) -->
    <p class="sr-only" aria-live="polite">{projects.announcement}</p>
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
    /* room under the last control for the notices at the bottom (they never sit on a control for good) */
    padding: var(--sp-5) var(--sp-5) max(var(--sp-7), var(--notices-h, 0px));
    scroll-padding-bottom: var(--notices-h, 0px);
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
      padding: var(--sp-4) var(--sp-4) max(var(--sp-6), var(--notices-h, 0px));
    }
  }
</style>
