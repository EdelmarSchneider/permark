<script lang="ts">
  // The one box behind Quick open (Ctrl+P), Search everywhere (Ctrl+T) and the command palette (Ctrl+Shift+P): a
  // field, optional scopes, and the rows the model builds. All state is in the store; this draws it.
  import { api } from "../../api";
  import { conditions } from "../conditions/context.svelte";
  import { reasonKey, remedyKey } from "../conditions/guidance";
  import { listCommands } from "../commands/commands";
  import { comboLabel } from "../commands/combo";
  import { remedyAction } from "../commands/remedy";
  import { keys } from "../commands/state.svelte";
  import { documents } from "../doc/document.svelte";
  import { confirmLeave } from "../editing/guard";
  import { revealInTree } from "../explorer/current";
  import { iconOfCategory } from "../explorer/icons";
  import Folder from "@lucide/svelte/icons/folder";
  import Hash from "@lucide/svelte/icons/hash";
  import TagIcon from "@lucide/svelte/icons/tag";
  import Terminal from "@lucide/svelte/icons/terminal";
  import { t, tn, type MessageKey } from "../i18n";
  import { contributions } from "../plugins/instance.svelte";
  import { ui, type FinderPreset } from "../shell/ui.svelte";
  import { windowStore as store } from "../window.svelte";
  import { FinderStore, type FinderHost } from "./finderStore.svelte";
  import { outlineRows, type GroupTitle, type Row, type Scope } from "./finderModel";
  import { markRuns } from "./fuzzy";
  import Marked from "./Marked.svelte";
  import PathText from "./PathText.svelte";
  import { recentFiles } from "./recent.svelte";
  import SnippetText from "./SnippetText.svelte";
  import { searchTags } from "./tags.svelte";

  type Props = { preset: FinderPreset };
  let { preset }: Props = $props();

  const mac = $derived(conditions.current.platform === "mac");

  const host: FinderHost = {
    api,
    get tags() {
      return searchTags.available
        ? {
            hits: (query: string) => searchTags.hits(query),
            selected: () => searchTags.selected,
            select: (name: string) => searchTags.add(name),
          }
        : undefined;
    },
    get singleFile() {
      return documents.singleFile;
    },
    commands: () =>
      listCommands(contributions.items, conditions.current, keys.map, { palette: true }),
    label: (key) => t(key),
    outline: () => {
      const view = documents.view;
      return view?.body.kind === "markdown" ? outlineRows(view.path, view.body.outline) : [];
    },
    opened: () => recentFiles.list,
    openFile: (path) => {
      void (async () => {
        if (!(await confirmLeave({ kind: "document", path }))) return;
        const result = await api.showDocument(path);
        if (!result.ok) store.notifyFileOpFailed(result.error);
      })();
    },
    reveal: (path) => {
      ui.showSidebar("files");
      revealInTree(path);
    },
    openSection: (row) => documents.navigate(row.path, row.anchor, row.line),
    runCommand: (entry) => void entry.command.run(),
    close: () => ui.closeFinder(),
  };

  // one store per open (the parent re-creates this component for every open)
  // svelte-ignore state_referenced_locally
  const finder = new FinderStore(preset, host);

  let input: HTMLInputElement | undefined = $state();
  let list: HTMLElement | undefined = $state();

  const SCOPE_LABEL: Record<Scope, MessageKey> = {
    all: "finder.scope.all",
    files: "finder.scope.files",
    sections: "finder.scope.sections",
    tags: "finder.scope.tags",
    commands: "finder.scope.commands",
  };
  const GROUP_LABEL: Record<GroupTitle, MessageKey> = {
    opened: "finder.group.opened",
    changed: "finder.group.changed",
    files: "finder.group.files",
    folders: "finder.group.folders",
    sections: "finder.group.sections",
    tags: "finder.group.tags",
    commands: "finder.group.commands",
  };
  const filters = $derived(contributions.visible("searchFilter", conditions.current));
  const label = $derived(
    preset === "quick"
      ? t("finder.quickLabel")
      : preset === "commands"
        ? t("finder.commandsLabel")
        : t("finder.label"),
  );
  const placeholder = $derived.by(() => {
    switch (finder.scope) {
      case "files":
        return t("finder.placeholder.files");
      case "sections":
        return t("finder.placeholder.sections");
      case "tags":
        return t("finder.placeholder.tags");
      case "commands":
        return t("finder.placeholder.commands");
      case "all":
        return t("finder.placeholder.all");
      default:
        return finder.scope satisfies never;
    }
  });
  const activeKey = $derived(finder.current?.key ?? null);

  function onkeydown(e: KeyboardEvent): void {
    if (e.isComposing || e.keyCode === 229) return;
    switch (e.key) {
      case "Escape":
        e.preventDefault();
        ui.closeFinder();
        break;
      case "ArrowDown":
        e.preventDefault();
        finder.move(1);
        break;
      case "ArrowUp":
        e.preventDefault();
        finder.move(-1);
        break;
      case "Enter":
        e.preventDefault();
        finder.activate(finder.current, e.ctrlKey || e.metaKey);
        break;
      case "Tab":
        e.preventDefault();
        finder.cycle(e.shiftKey ? -1 : 1);
        break;
      default:
        break;
    }
  }

  function rowText(row: Row): string {
    switch (row.type) {
      case "file":
      case "folder":
        return row.path;
      case "section":
        return row.headingPath.join(" / ");
      case "tag":
        return row.name;
      case "command":
        return t(row.entry.command.label);
      case "more":
        return t("finder.showAll", { n: row.count });
      default:
        return row satisfies never;
    }
  }

  // the tags the search is narrowed to changed (a chip was added | removed): ask again
  let seenTags = searchTags.selected;
  $effect(() => {
    const now = searchTags.selected;
    if (now === seenTags) return;
    seenTags = now;
    finder.retry();
  });

  // the field has the focus while the box lives; the focus goes back to where it was
  $effect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    input?.focus();
    return () => {
      finder.dispose();
      before?.focus();
    };
  });

  // the row the keyboard is on stays in view
  $effect(() => {
    const key = activeKey;
    if (key === null || !list) return;
    for (const el of list.querySelectorAll<HTMLElement>("[data-key]")) {
      if (el.dataset["key"] === key) el.scrollIntoView({ block: "nearest" });
    }
  });

  function remedyFor(row: Row & { type: "command" }): { label: string; run: () => void } | null {
    const availability = row.entry.availability;
    if (availability.status !== "unavailable") return null;
    const run = remedyAction(availability.reason.remedy);
    const key = remedyKey(availability.reason.remedy);
    return run && key ? { label: t(key), run } : null;
  }

  function hintKey(): MessageKey {
    return finder.scope === "sections" ? "finder.sectionsHint" : "finder.filesHint";
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="backdrop"
  onclick={(e) => {
    if (e.target === e.currentTarget) ui.closeFinder();
  }}
>
  <div class="box" role="dialog" aria-modal="true" aria-label={label} tabindex="-1" {onkeydown}>
    {#if finder.scopes.length > 1}
      <div class="scopes" role="tablist" aria-label={t("finder.scopes")}>
        {#each finder.scopes as scope (scope)}
          <button
            type="button"
            role="tab"
            class="scope"
            aria-selected={finder.scope === scope}
            tabindex="-1"
            onclick={() => {
              finder.setScope(scope);
              input?.focus();
            }}>{t(SCOPE_LABEL[scope])}</button
          >
        {/each}
      </div>
    {/if}
    <input
      bind:this={input}
      class="field"
      type="text"
      role="combobox"
      aria-expanded="true"
      aria-controls="finder-list"
      aria-activedescendant={activeKey ? `finder-${activeKey}` : undefined}
      autocomplete="off"
      autocapitalize="off"
      spellcheck="false"
      {placeholder}
      value={finder.query}
      oninput={(e) => finder.setQuery(e.currentTarget.value)}
    />
    {#each filters as filter (filter.owner + filter.id)}
      {#await filter.view() then loaded}
        {@const Filter = loaded.default}
        <Filter />
      {/await}
    {/each}

    <div class="list" id="finder-list" role="listbox" aria-label={label} bind:this={list}>
      {#each finder.view.groups as group (group.title)}
        <div class="group" role="presentation">{t(GROUP_LABEL[group.title])}</div>
        {#each group.rows as row (row.key)}
          {@const remedy = row.type === "command" ? remedyFor(row) : null}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            id={`finder-${row.key}`}
            data-key={row.key}
            class="row"
            class:active={activeKey === row.key}
            class:dim={row.type === "command" && row.entry.availability.status === "unavailable"}
            role="option"
            tabindex="-1"
            aria-selected={activeKey === row.key}
            aria-disabled={row.type === "command" &&
              row.entry.availability.status === "unavailable"}
            title={row.type === "file" || row.type === "folder" ? row.path : undefined}
            onpointermove={() => {
              const at = finder.view.rows.findIndex((r) => r.key === row.key);
              if (at >= 0 && at !== finder.active) finder.active = at;
            }}
            onclick={() => finder.activate(row)}
          >
            {#if row.type === "file"}
              {@const Icon = iconOfCategory(row.category)}
              <Icon size={15} aria-hidden="true" />
              <span class="text"><PathText path={row.path} positions={row.positions} /></span>
            {:else if row.type === "folder"}
              <Folder size={15} aria-hidden="true" />
              <span class="text"
                ><PathText path={row.path} positions={row.positions} folderOnly={true} /></span
              >
              <span class="aside">{t("finder.revealFolder")}</span>
            {:else if row.type === "section"}
              <Hash size={15} aria-hidden="true" />
              <span class="text">
                <span class="crumbs">
                  <span class="strong">{row.headingPath.at(-1) ?? row.path}</span>
                  {#if row.headingPath.length > 1}
                    <span class="faint">{row.headingPath.slice(0, -1).join(" › ")}</span>
                  {/if}
                  <span class="faint">{row.path}</span>
                </span>
                {#if row.snippet.length > 0}
                  <span class="snippet"><SnippetText parts={row.snippet} /></span>
                {/if}
              </span>
            {:else if row.type === "tag"}
              <TagIcon size={15} aria-hidden="true" />
              <span class="text"><span class="strong">{row.name}</span></span>
              <span class="aside">{tn("finder.tagFiles", row.count)}</span>
            {:else if row.type === "command"}
              <Terminal size={15} aria-hidden="true" />
              <span class="text">
                <span class="strong">
                  <Marked runs={markRuns(t(row.entry.command.label), row.positions)} />
                </span>
                {#if row.entry.availability.status === "unavailable"}
                  <span class="reason">
                    {t(reasonKey(row.entry.availability.reason))}
                    {#if remedy}
                      <button
                        type="button"
                        class="remedy"
                        tabindex="-1"
                        onclick={(e) => {
                          e.stopPropagation();
                          remedy.run();
                          ui.closeFinder();
                        }}>{remedy.label}</button
                      >
                    {/if}
                  </span>
                {/if}
              </span>
              {#if row.entry.combos[0]}
                <kbd>{comboLabel(row.entry.combos[0], mac)}</kbd>
              {/if}
            {:else}
              <span class="text more">{t("finder.showAll", { n: row.count })}</span>
            {/if}
          </div>
        {/each}
      {/each}

      {#if finder.view.rows.length === 0}
        <p class="note" role="status">
          {#if finder.load.status === "failed"}
            {t("finder.failed")}
            <button type="button" class="remedy" tabindex="-1" onclick={() => finder.retry()}
              >{t("finder.retry")}</button
            >
          {:else if finder.view.empty === "noMatch"}
            {finder.scope === "sections"
              ? t("finder.noSections", { query: finder.query })
              : t("finder.none", { query: finder.query })}
          {:else if finder.load.status === "loading"}
            {t("finder.searching")}
          {:else if finder.scope !== "commands"}
            {t(hintKey())}
          {/if}
        </p>
      {/if}
    </div>

    <footer class="foot">
      <span>{tn("finder.results", finder.view.rows.length)}</span>
      {#if finder.load.status === "ready" && finder.load.partial}
        <span class="partial" role="status">{t("finder.partial")}</span>
      {/if}
      <span class="spacer"></span>
      <span>{t("finder.footMove")}</span>
      {#if finder.scopes.length > 1}<span>{t("finder.footScope")}</span>{/if}
      <span>{t("finder.footOpen")}</span>
      <span>{t("finder.footClose")}</span>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 55;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding: 10vh var(--sp-4) var(--sp-4);
    background: var(--backdrop);
    animation: pm-fade var(--dur) var(--ease);
  }
  .box {
    display: flex;
    flex-direction: column;
    width: min(680px, 100%);
    max-height: min(560px, 78vh);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    box-shadow: var(--shadow-pop);
    overflow: hidden;
    outline: none;
  }
  .scopes {
    display: flex;
    gap: 2px;
    padding: var(--sp-2) var(--sp-2) 0;
  }
  .scope {
    padding: 3px var(--sp-3);
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-ui-sm);
    cursor: pointer;
  }
  .scope:hover {
    background: var(--hover);
  }
  .scope[aria-selected="true"] {
    background: var(--accent-soft);
    color: var(--accent-text);
    font-weight: 600;
  }
  .field {
    margin: var(--sp-2);
    padding: 8px var(--sp-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 14px;
    outline: none;
  }
  .field:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-soft);
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 var(--sp-2) var(--sp-2);
  }
  .group {
    padding: var(--sp-2) var(--sp-2) 2px;
    color: var(--faint);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .row {
    display: flex;
    align-items: flex-start;
    gap: var(--sp-2);
    padding: 5px var(--sp-2);
    border-radius: var(--radius-control);
    color: var(--text);
    cursor: pointer;
  }
  .row :global(svg) {
    flex: none;
    margin-top: 2px;
    color: var(--muted);
  }
  .row.active {
    background: var(--accent-soft);
  }
  .row.dim {
    color: var(--muted);
    cursor: default;
  }
  .text {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .text.more {
    color: var(--accent-text);
  }
  .crumbs {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 2px var(--sp-2);
  }
  .strong {
    font-weight: 500;
  }
  .faint {
    color: var(--faint);
    font-size: var(--fs-ui-sm);
  }
  .snippet {
    display: block;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .aside {
    flex: none;
    color: var(--faint);
    font-size: var(--fs-ui-xs);
  }
  .reason {
    display: block;
    color: var(--faint);
    font-size: var(--fs-ui-sm);
  }
  .remedy {
    margin-left: var(--sp-2);
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent-text);
    font: inherit;
    cursor: pointer;
    text-decoration: underline;
  }
  kbd {
    flex: none;
    padding: 1px 6px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface);
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-ui-xs);
  }
  .note {
    margin: var(--sp-4) var(--sp-2);
    color: var(--muted);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: 6px var(--sp-3);
    border-top: 1px solid var(--border);
    background: var(--surface);
    color: var(--faint);
    font-size: var(--fs-ui-xs);
  }
  .partial {
    color: var(--git-modified-text);
  }
</style>
