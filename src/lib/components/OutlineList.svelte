<script lang="ts">
  import { tick, untrack } from "svelte";
  import { bridge } from "../bridge";
  import { formatNumber, t } from "../i18n";
  import { navigate, visibleAncestor, visibleRows } from "../outline";
  import { nearestScrollTop } from "../reveal";
  import { docs } from "../stores/docs.svelte";
  import { outline } from "../stores/outline.svelte";
  import { ui } from "../stores/ui.svelte";
  import Icon from "./Icon.svelte";

  type Props = {
    /** The list is on screen: the section being read is kept in view. False while another tab covers it. */
    active?: boolean;
    /** Prefix of the row ids (the panel and the popover never exist together, but ids stay unique anyway). */
    idPrefix?: string;
    /** A heading was chosen (the popover closes). */
    onpick?: () => void;
  };
  let { active = true, idPrefix = "outline", onpick }: Props = $props();

  let list: HTMLElement | undefined = $state();
  let hasFocus = $state(false);
  /** The row the keyboard is on, as an index into the outline's headings. */
  let cursor = $state(0);

  const items = $derived(outline.model.items);
  const collapsed = $derived(outline.collapsed);
  const rows = $derived(visibleRows(items, collapsed));
  /** The row that shows the section being read (a collapsed parent stands in for a hidden one). */
  const marked = $derived(visibleAncestor(items, collapsed, outline.current));
  const cursorRow = $derived(visibleAncestor(items, collapsed, cursor) ?? rows[0] ?? 0);
  const view = $derived(docs.current);
  const status = $derived(
    !view
      ? "noDoc"
      : view.kind !== "markdown"
        ? "notMarkdown"
        : outline.path !== view.path
          ? "pending"
          : items.length === 0
            ? "empty"
            : "list",
  );
  const cut = $derived(outline.model.total > items.length);

  export function focus(): void {
    list?.focus({ preventScroll: true });
  }

  function rowEl(index: number): HTMLElement | null {
    return list?.querySelector<HTMLElement>(`[data-index="${index}"]`) ?? null;
  }

  /** Brings a row into view by the least movement; only this list scrolls. */
  function showRow(index: number): void {
    const row = rowEl(index);
    if (!row || !list) return;
    const box = list.getBoundingClientRect();
    const at = row.getBoundingClientRect();
    const top = nearestScrollTop(
      { top: at.top - box.top + list.scrollTop, bottom: at.bottom - box.top + list.scrollTop },
      { scrollTop: list.scrollTop, height: list.clientHeight },
      8,
    );
    if (top !== null) list.scrollTo({ top, behavior: "instant" });
  }

  function pick(index: number): void {
    bridge.outline?.goTo(index);
    onpick?.();
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const result = navigate(items, rows, cursor, collapsed, e.key);
    if (!result) return;
    e.preventDefault();
    if (result.toggle) outline.toggle(result.toggle.index, result.toggle.expand);
    cursor = result.cursor;
    if (result.open !== undefined) pick(result.open);
    else void tick().then(() => showRow(result.cursor));
  }

  function onrowclick(e: MouseEvent, index: number): void {
    cursor = index;
    if (e.target instanceof Element && e.target.closest(".outline-chev")) outline.toggle(index);
    else pick(index);
  }

  /** A click on a row keeps the focus on the list itself, where the keyboard and the cursor live. */
  function onmousedown(e: MouseEvent): void {
    if (e.target === list) return; // the scrollbar
    e.preventDefault();
    focus();
  }

  function onfocus(): void {
    hasFocus = true;
    // start from where the reader is
    cursor = marked ?? rows[0] ?? 0;
  }

  // keep the section being read in view, and show it when the tab comes back
  $effect(() => {
    const target = marked;
    if (!active || target === null) return;
    untrack(() => void tick().then(() => showRow(target)));
  });

  // Ctrl+Alt+O moves the focus into the list
  let handledFocus = untrack(() => ui.outlineFocusTick);
  $effect(() => {
    const requested = ui.outlineFocusTick;
    if (requested === handledFocus || !active) return;
    untrack(() => {
      handledFocus = requested;
      void tick().then(focus);
    });
  });
</script>

{#if status === "list"}
  <!-- the tree is the scroll container: arrow keys move the cursor row, Enter scrolls the document -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    bind:this={list}
    class="outline"
    role="tree"
    aria-label={t("outline.label")}
    tabindex="0"
    aria-activedescendant={hasFocus ? `${idPrefix}-row-${cursorRow}` : undefined}
    {onfocus}
    onblur={() => (hasFocus = false)}
    {onkeydown}
    {onmousedown}
  >
    {#each rows as i (i)}
      {@const item = items[i]}
      {@const isClosed = item.hasChildren && collapsed.has(item.key)}
      {@const dot = item.changed || (isClosed && item.subtreeChanged)}
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        id="{idPrefix}-row-{i}"
        class={[
          "outline-row",
          marked === i && "is-current",
          hasFocus && cursorRow === i && "is-focused",
        ]}
        role="treeitem"
        tabindex="-1"
        aria-level={item.depth + 1}
        aria-selected={marked === i}
        aria-current={marked === i ? "location" : undefined}
        aria-expanded={item.hasChildren ? !isClosed : undefined}
        style:--depth={item.depth}
        data-index={i}
        title={item.text}
        onclick={(e) => onrowclick(e, i)}
      >
        {#if item.hasChildren}
          <Icon
            name="chevron-down"
            size="sm"
            class={isClosed ? "outline-chev is-closed" : "outline-chev"}
          />
        {:else}
          <span class="outline-chev-space"></span>
        {/if}
        <span class="outline-text">{item.text || t("outline.untitled")}</span>
        {#if dot}
          <span class="outline-dot" title={t("outline.changed")}></span>
          <span class="sr-only">{t("outline.changed")}</span>
        {/if}
      </div>
    {/each}
  </div>
  {#if cut}
    <div class="outline-note">
      {t("outline.truncated", {
        shown: formatNumber(items.length),
        total: formatNumber(outline.model.total),
      })}
    </div>
  {/if}
{:else if status === "noDoc"}
  <p class="panel-note">{t("outline.noDoc")}</p>
{:else if status === "notMarkdown"}
  <p class="panel-note">{t("outline.notMarkdown")}</p>
{:else if status === "empty"}
  <p class="panel-note">{t("outline.empty")}</p>
{/if}
