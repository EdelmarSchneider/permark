<script lang="ts">
  // The header of the document area: where the document is, how to go back, and what can be done with it.
  import AlignLeft from "@lucide/svelte/icons/align-left";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import ArrowRight from "@lucide/svelte/icons/arrow-right";
  import Copy from "@lucide/svelte/icons/copy";
  import Crosshair from "@lucide/svelte/icons/crosshair";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import History from "@lucide/svelte/icons/history";
  import Search from "@lucide/svelte/icons/search";
  import SquareArrowOutUpRight from "@lucide/svelte/icons/square-arrow-out-up-right";
  import Type from "@lucide/svelte/icons/a-large-small";
  import type { Snippet } from "svelte";
  import { revealInTree } from "../explorer/current";
  import ContextMenu from "../explorer/ContextMenu.svelte";
  import type { MenuEntry } from "../explorer/menu";
  import { language, t, type MessageKey } from "../i18n";
  import { documents } from "./document.svelte";
  import ReadingMenu from "./ReadingMenu.svelte";
  import { reading } from "./reading.svelte";

  type Props = {
    path: string;
    /** Last modification of the file (ms since the epoch), when it has one. */
    savedMs: number | null;
    /** The document's role (`agents`, ...). */
    role: string | null;
    template: string;
    hasOutline: boolean;
    onFind: () => void;
    /** Opens the system program: only for files of a project. */
    canOpenSystem: boolean;
    /** The mode switch (View | Edit | Source) and Save, when the document can be edited. */
    modes?: Snippet;
  };
  let { path, savedMs, role, template, hasOutline, onFind, canOpenSystem, modes }: Props = $props();

  const ROLES: Readonly<Record<string, MessageKey>> = {
    agents: "role.agents",
    claude: "role.claude",
    skill: "role.skill",
    "cursor-rule": "role.cursor-rule",
    llms: "role.llms",
    prompt: "role.prompt",
    copilot: "role.copilot",
  };

  /** The badge words for a role; an id the interface does not know shows as it is. */
  function roleLabel(role: string): string {
    const key = ROLES[role];
    return key === undefined ? role : t(key);
  }

  type Popup = { x: number; y: number; entries: readonly MenuEntry[] };
  let popup = $state.raw<Popup | null>(null);
  let viewMenu = $state(false);
  let now = $state(Date.now());

  // "saved 3 min ago" stays true while the window stays open
  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), 30_000);
    return () => clearInterval(timer);
  });

  const parts = $derived(path.split("/"));
  const name = $derived(parts.at(-1) ?? path);
  const folders = $derived(parts.slice(0, -1));

  const saved = $derived.by((): string | null => {
    if (savedMs === null) return null;
    const seconds = Math.max(0, Math.round((now - savedMs) / 1000));
    if (seconds < 20) return t("doc.savedNow");
    const rtf = new Intl.RelativeTimeFormat(language.locale, {
      numeric: "always",
      style: "narrow",
    });
    const [value, unit]: [number, Intl.RelativeTimeFormatUnit] =
      seconds < 3600
        ? [Math.round(seconds / 60), "minute"]
        : seconds < 86_400
          ? [Math.round(seconds / 3600), "hour"]
          : [Math.round(seconds / 86_400), "day"];
    return t("doc.saved", { time: rtf.format(-value, unit) });
  });

  function below(button: Element, entries: readonly MenuEntry[]): Popup {
    const box = button.getBoundingClientRect();
    return { x: box.left, y: box.bottom + 4, entries };
  }

  function trailMenu(event: MouseEvent): void {
    const entries: MenuEntry[] = documents.trail.recent().map(({ index, place, current }) => ({
      kind: "item",
      label: `${current ? "• " : ""}${place.path.split("/").at(-1) ?? place.path}${place.anchor ? `#${place.anchor}` : ""}`,
      hint: place.path,
      run: () => void documents.jump(index),
    }));
    if (entries.length === 0 || !(event.currentTarget instanceof Element)) return;
    popup = below(event.currentTarget, entries);
  }

  function moreMenu(event: MouseEvent): void {
    if (!(event.currentTarget instanceof Element)) return;
    const entries: MenuEntry[] = [
      { kind: "item", label: t("cmd.copyDocument"), run: () => void documents.copyDocument() },
    ];
    if (canOpenSystem) {
      entries.push(
        { kind: "separator" },
        {
          kind: "item",
          label: t("menu.openDefaultApp"),
          run: () => void documents.openExternal(path, "defaultApp"),
        },
        {
          kind: "item",
          label: t("menu.showInFileManager"),
          run: () => void documents.openExternal(path, "reveal"),
        },
      );
    }
    popup = below(event.currentTarget, entries);
  }
</script>

<div class="doc-header" role="toolbar" aria-label={t("header.path")}>
  <button
    type="button"
    class="icon-btn"
    title={t("header.backHint")}
    aria-label={t("header.back")}
    disabled={!documents.trail.canBack}
    onclick={() => void documents.back()}
    oncontextmenu={(e) => {
      e.preventDefault();
      trailMenu(e);
    }}
  >
    <ArrowLeft size={16} />
  </button>
  <button
    type="button"
    class="icon-btn"
    title={t("header.forwardHint")}
    aria-label={t("header.forward")}
    disabled={!documents.trail.canForward}
    onclick={() => void documents.forward()}
    oncontextmenu={(e) => {
      e.preventDefault();
      trailMenu(e);
    }}
  >
    <ArrowRight size={16} />
  </button>
  <button
    type="button"
    class="icon-btn"
    title={t("trail.all")}
    aria-label={t("trail.all")}
    disabled={!documents.trail.canBack && !documents.trail.canForward}
    onclick={trailMenu}
  >
    <History size={15} />
  </button>

  <nav class="crumbs" aria-label={t("header.path")} title={path}>
    {#each folders as folder, at (at)}
      <span class="crumb">{folder}</span><span class="sep" aria-hidden="true">/</span>
    {/each}
    <span class="crumb current">{name}</span>
  </nav>
  {#if role}
    <span class="role" title={t("header.roleHint")}>{roleLabel(role)}</span>
  {/if}
  <span class="spacer"></span>
  {#if modes}{@render modes()}{/if}
  {#if saved}
    <span class="saved">{saved}</span>
  {/if}

  <button
    type="button"
    class="icon-btn"
    title={t("header.reveal")}
    aria-label={t("header.reveal")}
    onclick={() => revealInTree(path)}
  >
    <Crosshair size={16} />
  </button>
  <button
    type="button"
    class="icon-btn"
    title={t("header.copyReference")}
    aria-label={t("header.copyReference")}
    onclick={() => void documents.copyReference(template)}
  >
    <Copy size={15} />
  </button>
  <button
    type="button"
    class="icon-btn"
    title={t("header.openExternal")}
    aria-label={t("header.openExternal")}
    onclick={() => void documents.openInEditor()}
  >
    <SquareArrowOutUpRight size={15} />
  </button>
  <button
    type="button"
    class="icon-btn"
    title={t("find.label")}
    aria-label={t("find.label")}
    onclick={onFind}
  >
    <Search size={16} />
  </button>
  <button
    type="button"
    class="icon-btn"
    data-reading-trigger
    title={t("header.readingHint", { zoom: reading.zoom })}
    aria-label={t("reading.menu")}
    aria-expanded={viewMenu}
    onclick={() => (viewMenu = !viewMenu)}
  >
    <Type size={17} />
  </button>
  {#if hasOutline}
    <button
      type="button"
      class="icon-btn"
      title={t("outline.label")}
      aria-label={t("outline.label")}
      aria-pressed={reading.outline}
      onclick={() => reading.toggleOutline()}
    >
      <AlignLeft size={16} />
    </button>
  {/if}
  <button
    type="button"
    class="icon-btn"
    title={t("header.more")}
    aria-label={t("header.more")}
    onclick={moreMenu}
  >
    <Ellipsis size={16} />
  </button>

  {#if viewMenu}
    <ReadingMenu close={() => (viewMenu = false)} />
  {/if}
</div>

{#if popup}
  <ContextMenu x={popup.x} y={popup.y} entries={popup.entries} close={() => (popup = null)} />
{/if}

<style>
  .doc-header {
    position: relative;
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
    height: 40px;
    padding: 0 var(--sp-2);
    border-bottom: 1px solid var(--border);
    background: var(--bg);
  }
  .crumbs {
    display: flex;
    align-items: baseline;
    min-width: 0;
    margin-left: var(--sp-2);
    overflow: hidden;
    white-space: nowrap;
    color: var(--muted);
    font-size: var(--fs-ui);
  }
  .crumb {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .crumb.current {
    flex: none;
    max-width: 100%;
    color: var(--text);
    font-weight: 600;
  }
  .sep {
    flex: none;
    margin: 0 4px;
    color: var(--faint);
  }
  .role {
    flex: none;
    margin-left: var(--sp-2);
    padding: 1px 8px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface);
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
  }
  .saved {
    flex: none;
    margin-right: var(--sp-2);
    color: var(--faint);
    font-size: var(--fs-ui-xs);
    white-space: nowrap;
  }
</style>
