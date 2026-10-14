<script lang="ts">
  // The toolbar of Visual mode (docs/design/v0.2.md section 3): block type, marks, link, lists, quote, table, code block, image and
  // rule. Sticky under the header. When the pane is narrow the last buttons fold into a `[...]` menu. It only draws; what a
  // button does is decided by the callbacks, and what is pressed comes from the selection (`format`).
  import { t } from "../i18n";
  import type { CommandId, FormatState } from "../editor/visual/format";
  import {
    BLOCK_TYPES,
    CODE_LANGUAGES,
    LANGUAGE_WIDTH,
    TOOLBAR_ITEMS,
    blockLabel,
    visibleCount,
    type ToolbarItem,
  } from "../editor/visual/toolbarModel";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";
  import Menu, { type MenuEntry } from "./Menu.svelte";
  import type { Anchor } from "./Popover.svelte";

  type Props = {
    format: FormatState;
    /** The editor is loaded; until then the buttons are off. */
    ready: boolean;
    run(id: CommandId): void;
    link(): void;
    image(): void;
    /** Puts the focus back on the text. */
    focus(): void;
    setLanguage(language: string): void;
  };
  let { format, ready, run, link, image, focus, setLanguage }: Props = $props();

  let width = $state(1000);
  let bar: HTMLElement | undefined = $state();
  let menu = $state<{ kind: "type" | "table" | "more" | "language"; anchor: Anchor } | null>(null);

  const off = $derived(!ready || format.atom);
  const shown = $derived(
    visibleCount(
      TOOLBAR_ITEMS.map((i) => i.width),
      width - (format.block === "code" ? LANGUAGE_WIDTH : 0),
    ),
  );
  const visible = $derived(TOOLBAR_ITEMS.slice(0, shown));
  const folded = $derived(TOOLBAR_ITEMS.slice(shown));

  function hintOf(entry: ToolbarItem): string {
    return entry.hint ? `${t(entry.label)} (${entry.hint})` : t(entry.label);
  }

  function open(kind: NonNullable<typeof menu>["kind"], e: MouseEvent): void {
    const el = e.currentTarget as HTMLElement;
    menu = menu?.kind === kind ? null : { kind, anchor: el.getBoundingClientRect() };
  }

  function press(entry: ToolbarItem, e: MouseEvent): void {
    if (entry.id === "table") open("table", e);
    else activate(entry);
  }

  function activate(entry: ToolbarItem): void {
    if (entry.id === "link") link();
    else if (entry.id === "image") image();
    else run(entry.id);
  }

  const typeEntries = $derived<MenuEntry[]>(
    BLOCK_TYPES.map((b) => ({
      id: b.id,
      label: t(b.label),
      hint: b.hint,
      checked: format.block === b.block,
      onselect: () => run(b.id),
    })),
  );

  const tableEntries = $derived<MenuEntry[]>([
    { id: "insert", label: t("toolbar.insertTable"), icon: "table", onselect: () => run("table") },
    { type: "separator" },
    {
      id: "rowBefore",
      label: t("toolbar.rowAbove"),
      disabled: !format.table || format.tableHeader,
      onselect: () => run("rowBefore"),
    },
    {
      id: "rowAfter",
      label: t("toolbar.rowBelow"),
      disabled: !format.table,
      onselect: () => run("rowAfter"),
    },
    {
      id: "colBefore",
      label: t("toolbar.colLeft"),
      disabled: !format.table,
      onselect: () => run("colBefore"),
    },
    {
      id: "colAfter",
      label: t("toolbar.colRight"),
      disabled: !format.table,
      onselect: () => run("colAfter"),
    },
    { type: "separator" },
    {
      id: "deleteRow",
      label: t("toolbar.deleteRow"),
      disabled: !format.table || format.tableHeader,
      onselect: () => run("deleteRow"),
    },
    {
      id: "deleteCol",
      label: t("toolbar.deleteCol"),
      disabled: !format.table,
      onselect: () => run("deleteCol"),
    },
    {
      id: "deleteTable",
      label: t("toolbar.deleteTable"),
      icon: "trash",
      disabled: !format.table,
      onselect: () => run("deleteTable"),
    },
  ]);

  const languageEntries = $derived<MenuEntry[]>([
    {
      id: "plain",
      label: t("toolbar.plainText"),
      checked: format.language === "",
      onselect: () => setLanguage(""),
    },
    ...CODE_LANGUAGES.map((language): MenuEntry => ({
      id: language,
      label: language,
      checked: format.language === language,
      onselect: () => setLanguage(language),
    })),
  ]);

  const moreEntries = $derived<MenuEntry[]>(
    folded.map((entry) => ({
      id: entry.id,
      label: t(entry.label),
      icon: entry.icon,
      hint: entry.hint,
      checked:
        entry.id === "table" || entry.id === "link" || entry.id === "image"
          ? undefined
          : entry.active(format),
      disabled: off,
      onselect: () => (entry.id === "table" ? run("table") : activate(entry)),
    })),
  );

  // Left and Right walk the buttons (a toolbar is one tab stop's worth of work); Esc gives the focus back to the text
  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.stopPropagation();
      e.preventDefault();
      focus();
      return;
    }
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    const buttons = Array.from(
      bar?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [],
    );
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (at < 0) return;
    e.preventDefault();
    const next = (at + (e.key === "ArrowRight" ? 1 : -1) + buttons.length) % buttons.length;
    buttons[next].focus();
  }

  // a click on a button must not take the caret out of the text
  function keepCaret(e: MouseEvent): void {
    if ((e.target as HTMLElement).closest("button")) e.preventDefault();
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  bind:this={bar}
  bind:clientWidth={width}
  class="vtoolbar"
  role="toolbar"
  tabindex="-1"
  aria-label={t("toolbar.label")}
  aria-orientation="horizontal"
  {onkeydown}
  onmousedown={keepCaret}
>
  <button
    type="button"
    class="vtool vtool--type"
    aria-haspopup="menu"
    aria-label={t("toolbar.blockType")}
    title={t("toolbar.blockType")}
    disabled={off}
    data-popover-trigger
    onclick={(e) => open("type", e)}
  >
    <span>{t(blockLabel(format.block))}</span>
    <Icon name="chevron-down" size="xs" />
  </button>
  {#if format.block === "code"}
    <button
      type="button"
      class="vtool vtool--language"
      aria-haspopup="menu"
      aria-label={t("toolbar.language")}
      title={t("toolbar.language")}
      data-popover-trigger
      onclick={(e) => open("language", e)}
    >
      <span>{format.language || t("toolbar.plainText")}</span>
      <Icon name="chevron-down" size="xs" />
    </button>
  {/if}
  {#each visible as entry (entry.id)}
    {@const on = entry.active(format)}
    <span class={["vtool-slot", entry.group && "vtool-slot--group"]}>
      <button
        type="button"
        class={["vtool", on && "is-on"]}
        aria-label={t(entry.label)}
        title={hintOf(entry)}
        aria-pressed={entry.id === "table" || entry.id === "image" || entry.id === "rule"
          ? undefined
          : on}
        aria-haspopup={entry.id === "table" ? "menu" : undefined}
        disabled={off}
        data-popover-trigger={entry.id === "table" ? true : undefined}
        onclick={(e) => press(entry, e)}
      >
        <Icon name={entry.icon} size="sm" />
        {#if entry.id === "table"}<Icon name="chevron-down" size="xs" />{/if}
      </button>
    </span>
  {/each}
  {#if folded.length > 0}
    <span class="vtool-slot vtool-slot--group">
      <IconButton
        small
        icon="more"
        label={t("toolbar.more")}
        data-popover-trigger
        aria-haspopup="menu"
        onclick={(e: MouseEvent) => open("more", e)}
      />
    </span>
  {/if}
</div>

{#if menu}
  <Menu
    label={menu.kind === "type"
      ? t("toolbar.blockType")
      : menu.kind === "table"
        ? t("toolbar.table")
        : menu.kind === "language"
          ? t("toolbar.language")
          : t("toolbar.more")}
    anchor={menu.anchor}
    entries={menu.kind === "type"
      ? typeEntries
      : menu.kind === "table"
        ? tableEntries
        : menu.kind === "language"
          ? languageEntries
          : moreEntries}
    onclose={() => (menu = null)}
  />
{/if}
