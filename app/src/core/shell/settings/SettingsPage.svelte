<script lang="ts">
  // The full settings: a left list of sections in two groups - the app's (Reading, Editing, Integration, Plugins and
  // the app plugins' own, such as Local AI) and, only while a folder is shown, the folder's (Overview, Files, Projects
  // inside, and its plugins' own, such as AI agents), each by order - and the shown one at the right, each a card of
  // rows; Projects inside only where the folder holds projects (`projects.md` §12.2; loaded when shown). One list for
  // the keyboard across both groups. Covers the window below the top bar; Escape or Close goes back to where the
  // person was.
  import BookOpen from "@lucide/svelte/icons/book-open";
  import Files from "@lucide/svelte/icons/files";
  import FolderCog from "@lucide/svelte/icons/folder-cog";
  import PenLine from "@lucide/svelte/icons/pen-line";
  import Plug from "@lucide/svelte/icons/plug";
  import Puzzle from "@lucide/svelte/icons/puzzle";
  import X from "@lucide/svelte/icons/x";
  import { conditions } from "../../conditions/context.svelte";
  import { t, type MessageKey } from "../../i18n";
  import { projectIcon } from "../../explorer/icons";
  import type { MenuIcon } from "../../explorer/menu";
  import { contributions } from "../../plugins/instance.svelte";
  import { projects } from "../../projects/instance.svelte";
  import type { WindowStore } from "../../window.svelte";
  import {
    CORE_SECTION_GROUP,
    CORE_SECTION_ORDER,
    SETTINGS_GROUPS,
    SETTINGS_SECTIONS,
    ui,
    type CoreSettingsSection,
    type SettingsGroup,
    type SettingsSection,
  } from "../ui.svelte";
  import { dialogFocus } from "../dialogFocus";
  import Card from "./Card.svelte";
  import EditingSection from "./EditingSection.svelte";
  import IntegrationSection from "./IntegrationSection.svelte";
  import PluginsSection from "./PluginsSection.svelte";
  import FilesSection from "./FilesSection.svelte";
  import FolderSection from "./FolderSection.svelte";
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
    files: "settings.section.files",
  };
  /** A small icon per section (decoration: the words name it); Projects inside wears the included project's. */
  const ICONS: Record<CoreSettingsSection, MenuIcon> = {
    reading: BookOpen,
    editing: PenLine,
    integration: Plug,
    plugins: Puzzle,
    projects: projectIcon("permark"),
    project: FolderCog,
    files: Files,
  };

  const GROUP_LABELS: Record<SettingsGroup, MessageKey> = {
    app: "settings.group.app",
    folder: "settings.group.folder",
  };

  const inProject = $derived(conditions.current.workspace === "project");
  /** The folder shown (its name in the title), `null` in a window with none (a single file, the Notes, the Welcome). */
  const folder = $derived(
    store.view.state === "showing" && store.view.workspace.kind === "project"
      ? store.view.workspace.name
      : null,
  );
  const title = $derived(
    folder === null ? t("settings.title") : t("settings.titleFolder", { name: folder }),
  );
  /** The plugins' own sections that may show here (plugin on, requirement met). */
  const pluginPages = $derived(contributions.visible("settingsPage", conditions.current));
  type NavItem = {
    id: SettingsSection;
    label: MessageKey;
    group: SettingsGroup;
    order: number;
    icon?: MenuIcon;
  };
  /** Sections the person can reach here, group by group: the folder's only while a project is shown, Projects only
   *  where it holds some; the plugins' sections among the core ones by order. */
  const nav = $derived(
    [
      ...SETTINGS_SECTIONS.filter((s) => s !== "projects" || projects.present).map(
        (s): NavItem => ({
          id: s,
          label: LABELS[s],
          group: CORE_SECTION_GROUP[s],
          order: CORE_SECTION_ORDER[s],
          icon: ICONS[s],
        }),
      ),
      ...pluginPages.map((p): NavItem => ({
        id: p.id,
        label: p.label,
        group: p.group ?? "app",
        order: p.order,
        icon: p.icon,
      })),
    ]
      .filter((item) => item.group === "app" || inProject)
      .sort(
        (a, b) =>
          SETTINGS_GROUPS.indexOf(a.group) - SETTINGS_GROUPS.indexOf(b.group) || a.order - b.order,
      ),
  );
  const navGroups = $derived(
    SETTINGS_GROUPS.map((group) => ({
      group,
      items: nav.filter((item) => item.group === group),
    })).filter((g) => g.items.length > 0),
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

  function onEscape(): void {
    // consent mode: Esc = Cancel (back to the Explorer, the strip still asks)
    if (projects.choosing !== null) projects.cancelChoose();
    else ui.closeSettingsPage();
  }

  /** Focus enters the page on what the section asks for (the Choose heading), else on the section list. */
  function entry(dialog: HTMLElement): HTMLElement | null {
    return (
      dialog.querySelector<HTMLElement>("[data-autofocus]") ??
      dialog.querySelector<HTMLElement>('[aria-current="page"]')
    );
  }

  /** "All settings" sits in the quick settings, which close as the page opens: the gear is the way back. */
  function gear(): HTMLElement | null {
    return document.querySelector<HTMLElement>("[data-settings-trigger]");
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
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="overlay">
  <div
    class="page"
    role="dialog"
    aria-modal="true"
    aria-label={title}
    tabindex="-1"
    bind:this={page}
    use:dialogFocus={{ initial: entry, onEscape, fallback: gear }}
  >
    <header class="bar">
      <h2>{title}</h2>
      <span class="spacer"></span>
      <button type="button" class="btn btn--sm" onclick={() => ui.closeSettingsPage()}>
        <X size={13} aria-hidden="true" />
        {t("common.close")}
      </button>
    </header>
    <div class="layout">
      <nav class="nav" aria-label={t("settings.sections")}>
        {#each navGroups as g (g.group)}
          <!-- the group's name is said once, from the group (the label on screen repeats it) -->
          <div class="nav-group" role="group" aria-label={t(GROUP_LABELS[g.group])}>
            <span class="menu-label nav-label" aria-hidden="true">{t(GROUP_LABELS[g.group])}</span>
            {#each g.items as item (item.id)}
              <button
                type="button"
                class="nav-item"
                aria-current={item.id === section ? "page" : undefined}
                tabindex={item.id === section ? 0 : -1}
                onclick={() => ui.showSettingsSection(item.id)}
                onkeydown={onnavkey}
              >
                {#if item.icon}
                  {@const Icon = item.icon}
                  <Icon size={15} strokeWidth={item.id === section ? 2 : 1.8} />
                {/if}
                {t(item.label)}
              </button>
            {/each}
          </div>
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
          {:else if section === "files"}
            <FilesSection />
          {:else}
            <FolderSection />
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
  .nav-group {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .nav-group + .nav-group {
    margin-top: var(--sp-3);
  }
  .nav-label {
    padding-left: var(--sp-3);
  }
  .nav-item {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
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
    .nav-group {
      flex-direction: row;
      align-items: center;
    }
    .nav-group + .nav-group {
      margin-top: 0;
      margin-left: var(--sp-3);
    }
    .nav-label,
    .nav-item {
      white-space: nowrap;
    }
    .content {
      padding: var(--sp-4) var(--sp-4) max(var(--sp-6), var(--notices-h, 0px));
    }
  }
</style>
