<script lang="ts">
  // Rows of uniform objects (a TOON table): the fields once as a header, one row per element, the entry key first in a
  // keyed table. Long tables draw only the rows near the screen (fixed row height). In Edit mode cells take typing,
  // fields and entry keys take new names, and rows can be added, duplicated, moved (drag or the row menu) and
  // deleted: each change is one edit the backend makes in place.
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import GripVertical from "@lucide/svelte/icons/grip-vertical";
  import Plus from "@lucide/svelte/icons/plus";
  import type { ConfigEdit, NewValue, Table } from "../../api";
  import ContextMenu from "../explorer/ContextMenu.svelte";
  import type { MenuEntry } from "../explorer/menu";
  import { t, tn } from "../i18n";
  import { refusalKey } from "./reasons";
  import SheetValue from "./SheetValue.svelte";
  import { windowOf, VIRTUAL_FROM, ROW_HEIGHT } from "./tableWindow";

  type Props = {
    table: Table;
    editable: boolean;
    revealed: ReadonlySet<string>;
    reveal: (path: string) => void;
    commit: (path: string, value: NewValue) => Promise<string | null>;
    edit: (edit: ConfigEdit) => Promise<string | null>;
  };
  let { table, editable, revealed, reveal, commit, edit }: Props = $props();

  let wrap: HTMLElement | undefined = $state();
  /** The rows drawn (long tables): first index and how many. */
  let shown = $state({ first: 0, count: 60 });
  let refusal = $state<string | null>(null);
  /** A name being typed: a field (`index`) or an entry key (`path`). */
  let naming = $state<{ kind: "field"; index: number } | { kind: "key"; path: string } | null>(
    null,
  );
  let nameText = $state("");
  let menu = $state<{ x: number; y: number; index: number; anchor: HTMLElement } | null>(null);
  let dragFrom = $state<number | null>(null);
  let dropAt = $state<number | null>(null);

  const virtual = $derived(table.rows.length >= VIRTUAL_FROM);
  const range = $derived(
    virtual
      ? {
          first: Math.min(shown.first, table.rows.length),
          last: Math.min(shown.first + shown.count, table.rows.length),
        }
      : { first: 0, last: table.rows.length },
  );
  const visible = $derived(table.rows.slice(range.first, range.last));
  const delimiter = $derived(
    table.delimiter === "\t"
      ? t("config.table.tab")
      : table.delimiter === "|"
        ? t("config.table.pipe")
        : null,
  );

  // a long table follows the scroll of the page it sits in: only the rows near the screen are drawn
  $effect(() => {
    if (!virtual || !wrap) return;
    const node = wrap;
    let scroller: HTMLElement | null = node.parentElement;
    while (scroller && !/(auto|scroll)/.test(getComputedStyle(scroller).overflowY)) {
      scroller = scroller.parentElement;
    }
    const target: HTMLElement | Window = scroller ?? window;
    const update = (): void => {
      const body = node.querySelector("tbody");
      if (!body) return;
      const top = body.getBoundingClientRect().top;
      const view = scroller
        ? scroller.getBoundingClientRect()
        : { top: 0, height: window.innerHeight };
      shown = windowOf(view.top - top, view.height, table.rows.length);
    };
    update();
    target.addEventListener("scroll", update, { passive: true });
    window.addEventListener("resize", update);
    return () => {
      target.removeEventListener("scroll", update);
      window.removeEventListener("resize", update);
    };
  });

  async function run(change: ConfigEdit): Promise<void> {
    refusal = await edit(change);
  }

  function rowMenu(index: number, anchor: HTMLElement): void {
    const box = anchor.getBoundingClientRect();
    menu = { x: box.left, y: box.bottom + 2, index, anchor };
  }

  function entries(index: number): MenuEntry[] {
    const row = table.rows[index];
    if (row === undefined) return [];
    const last = table.rows.length - 1;
    const items: MenuEntry[] = [
      {
        kind: "item",
        label: t("config.table.addAbove"),
        run: () => void run({ op: "addRow", table: table.path, at: index }),
      },
      {
        kind: "item",
        label: t("config.table.addBelow"),
        run: () => void run({ op: "addRow", table: table.path, at: index + 1 }),
      },
      {
        kind: "item",
        label: t("config.table.duplicate"),
        run: () => void run({ op: "duplicateRow", path: row.path }),
      },
    ];
    if (index > 0) {
      items.push({
        kind: "item",
        label: t("config.table.moveUp"),
        hint: "Alt+↑",
        run: () => void run({ op: "moveRow", path: row.path, to: index - 1 }),
      });
    }
    if (index < last) {
      items.push({
        kind: "item",
        label: t("config.table.moveDown"),
        hint: "Alt+↓",
        run: () => void run({ op: "moveRow", path: row.path, to: index + 1 }),
      });
    }
    items.push({ kind: "separator" });
    items.push({
      kind: "item",
      label: t("config.table.delete"),
      danger: true,
      run: () => void run({ op: "removeRow", path: row.path }),
    });
    return items;
  }

  /** Alt+Up | Alt+Down on a row moves it (the keyboard way of dragging). */
  function onRowKey(event: KeyboardEvent, index: number): void {
    if (!editable || event.isComposing || !event.altKey) return;
    const row = table.rows[index];
    if (row === undefined) return;
    const to = event.key === "ArrowUp" ? index - 1 : event.key === "ArrowDown" ? index + 1 : null;
    if (to === null || to < 0 || to >= table.rows.length) return;
    event.preventDefault();
    void run({ op: "moveRow", path: row.path, to });
  }

  function onDrop(index: number): void {
    const from = dragFrom;
    dragFrom = null;
    dropAt = null;
    const row = from === null ? undefined : table.rows[from];
    if (row === undefined || from === index) return;
    void run({ op: "moveRow", path: row.path, to: index });
  }

  async function finishName(): Promise<void> {
    const current = naming;
    if (current === null) return;
    const text = nameText.trim();
    naming = null;
    if (current.kind === "field") {
      if (text === "" || text === table.fields[current.index]?.split(".").at(-1)) return;
      await run({ op: "renameField", table: table.path, index: current.index, name: text });
    } else {
      const row = table.rows.find((r) => r.path === current.path);
      if (text === "" || text === row?.key) return;
      await run({ op: "rename", path: current.path, key: text });
    }
  }

  function onNameKey(event: KeyboardEvent): void {
    if (event.isComposing) return;
    if (event.key === "Enter") {
      event.preventDefault();
      void finishName();
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      naming = null;
    }
  }

  function focusOnMount(node: HTMLInputElement): void {
    node.focus();
    node.select();
  }
</script>

{#snippet nameField(label: string)}
  {#if naming}
    <input
      class="field name"
      aria-label={label}
      spellcheck="false"
      bind:value={nameText}
      onkeydown={onNameKey}
      onblur={() => void finishName()}
      use:focusOnMount
    />
  {/if}
{/snippet}

<div class="table-wrap" bind:this={wrap}>
  <p class="meta">
    {tn("config.table.rows", table.rows.length)}{#if delimiter}<span class="delim">
        · {delimiter}</span
      >{/if}
  </p>
  <div class="scroll-x">
    <table class:virtual style:--row-h="{ROW_HEIGHT}px">
      <thead>
        <tr>
          {#if editable}<th class="grip" aria-hidden="true"></th>{/if}
          {#if table.keyed}<th class="entry-head">{t("config.table.key")}</th>{/if}
          {#each table.fields as field, index (index)}
            <th>
              {#if naming?.kind === "field" && naming.index === index}
                {@render nameField(t("config.table.renameField", { name: field }))}
              {:else if editable}
                <button
                  type="button"
                  class="name-btn"
                  title={t("config.table.renameField", { name: field })}
                  onclick={() => {
                    naming = { kind: "field", index };
                    nameText = field.split(".").at(-1) ?? field;
                  }}
                >
                  {field}
                </button>
              {:else}
                {field}
              {/if}
            </th>
          {/each}
          {#if editable}<th class="tools" aria-hidden="true"></th>{/if}
        </tr>
      </thead>
      <tbody>
        {#if virtual && range.first > 0}
          <tr class="spacer" aria-hidden="true" style:height="{range.first * ROW_HEIGHT}px"></tr>
        {/if}
        {#each visible as row, k (row.path)}
          {@const index = range.first + k}
          <tr
            class:drop={dropAt === index && dragFrom !== null && dragFrom !== index}
            class:dragging={dragFrom === index}
            draggable={editable}
            ondragstart={(e) => {
              dragFrom = index;
              e.dataTransfer?.setData("text/plain", row.path);
            }}
            ondragover={(e) => {
              if (dragFrom === null) return;
              e.preventDefault();
              dropAt = index;
            }}
            ondragend={() => {
              dragFrom = null;
              dropAt = null;
            }}
            ondrop={(e) => {
              e.preventDefault();
              onDrop(index);
            }}
            onkeydown={(e) => onRowKey(e, index)}
          >
            {#if editable}
              <td class="grip" title={t("config.table.drag")}><GripVertical size={13} /></td>
            {/if}
            {#if table.keyed}
              <td class="entry">
                {#if naming?.kind === "key" && naming.path === row.path}
                  {@render nameField(t("config.table.renameKey", { name: row.key ?? "" }))}
                {:else if editable}
                  <button
                    type="button"
                    class="name-btn"
                    title={t("config.table.renameKey", { name: row.key ?? "" })}
                    onclick={() => {
                      naming = { kind: "key", path: row.path };
                      nameText = row.key ?? "";
                    }}
                  >
                    {row.key}
                  </button>
                {:else}
                  {row.key}
                {/if}
              </td>
            {/if}
            {#each row.cells as cell, c (cell.path)}
              <td>
                <SheetValue
                  scalar={cell}
                  label={`${table.fields[c] ?? ""} ${index + 1}`}
                  {editable}
                  revealed={revealed.has(cell.path)}
                  {reveal}
                  {commit}
                />
              </td>
            {/each}
            {#if editable}
              <td class="tools">
                <button
                  type="button"
                  class="icon-btn icon-btn--sm"
                  aria-haspopup="menu"
                  aria-label={t("config.table.rowMenu", { n: index + 1 })}
                  title={t("config.table.rowMenu", { n: index + 1 })}
                  onclick={(e) => rowMenu(index, e.currentTarget)}
                >
                  <Ellipsis size={14} />
                </button>
              </td>
            {/if}
          </tr>
        {/each}
        {#if virtual && range.last < table.rows.length}
          <tr
            class="spacer"
            aria-hidden="true"
            style:height="{(table.rows.length - range.last) * ROW_HEIGHT}px"
          ></tr>
        {/if}
      </tbody>
    </table>
  </div>
  {#if editable}
    <button
      type="button"
      class="btn btn--sm add"
      onclick={() => void run({ op: "addRow", table: table.path, at: table.rows.length })}
    >
      <Plus size={14} />
      {t("config.table.addRow")}
    </button>
  {/if}
  {#if refusal}<p class="refusal" role="alert">{t(refusalKey(refusal))}</p>{/if}
</div>

{#if menu}
  <ContextMenu
    x={menu.x}
    y={menu.y}
    entries={entries(menu.index)}
    label={t("config.table.rowMenu", { n: menu.index + 1 })}
    anchor={menu.anchor}
    close={() => (menu = null)}
  />
{/if}

<style>
  .table-wrap {
    padding: var(--sp-2) var(--sp-3) var(--sp-3);
  }
  .meta {
    margin: 0 0 var(--sp-2);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .scroll-x {
    overflow-x: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--fs-ui);
  }
  th,
  td {
    padding: 4px 10px;
    border-bottom: 1px solid var(--border);
    text-align: left;
    vertical-align: middle;
  }
  th {
    color: var(--muted);
    font: 600 var(--fs-ui-sm) / 1.4 var(--font-code);
    white-space: nowrap;
    border-bottom-color: var(--border-strong);
  }
  tbody tr:hover {
    background: var(--hover);
  }
  table.virtual tbody tr:not(.spacer) {
    height: var(--row-h);
  }
  table.virtual td {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 28em;
  }
  .spacer,
  .spacer:hover {
    background: none;
  }
  .entry,
  .entry-head {
    font-family: var(--font-code);
    color: var(--muted);
  }
  .grip {
    width: 18px;
    padding: 0 2px;
    color: var(--faint);
    cursor: grab;
  }
  .tools {
    width: 30px;
    padding: 0 4px;
  }
  .dragging {
    opacity: 0.5;
  }
  .drop {
    box-shadow: inset 0 2px 0 var(--accent);
  }
  .name-btn {
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    cursor: text;
    border-radius: 3px;
  }
  .name-btn:hover {
    background: var(--hover);
  }
  .name-btn:focus-visible {
    outline: 2px solid var(--accent);
  }
  .name {
    min-width: 8em;
    font-family: var(--font-code);
  }
  .add {
    margin-top: var(--sp-2);
  }
  .refusal {
    margin: var(--sp-1) 0 0;
    color: var(--git-deleted-text);
    font-size: var(--fs-ui-sm);
  }
</style>
