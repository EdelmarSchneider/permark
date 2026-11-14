<script lang="ts">
  // The Layouts dialog: the arrangements of the window's views the person saved under a name. A row per layout (Use |
  // Rename | Delete), then the arrangement shown now saved under a name (a name already kept: the button says
  // Replace before the press), then the whole arrangement back to the default. Use and the reset change the window:
  // the dialog closes and a notice offers Undo (the store's, as for the reset of a view's menu). A delete stays here
  // with an Undo of its own. A write that fails changes nothing and is said in the dialog. The line that says what
  // an action did goes with the next action.
  import { tick } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import type { ApiError } from "../../api";
  import { isComposition } from "../commands/combo";
  import { apiErrorText } from "../errors";
  import { t } from "../i18n";
  import { arrangement } from "./arrangement.svelte";
  import { dialogFocus } from "./dialogFocus";
  import {
    findLayout,
    foldName,
    layoutName,
    MAX_LAYOUT_NAME,
    MAX_LAYOUTS,
    type Layout,
  } from "./layouts";
  import { focusView } from "./sides";
  import { ui } from "./ui.svelte";

  /** The rows: all read, or the one of the layout `name` being renamed (`text`: what its field holds). */
  type Rows = { status: "viewing" } | { status: "renaming"; name: string; text: string };
  /** What the last action that worked said: a layout saved, one deleted (it can come back), a delete undone. */
  type Said =
    | { kind: "nothing" }
    | { kind: "saved"; name: string }
    | { kind: "deleted"; layout: Layout; at: number }
    | { kind: "undone" };
  /** A name field, judged: the name it would keep (`null`: it cannot act), whether that replaces a layout, the line
   *  under the field. */
  type Verdict = { name: string | null; replaces: boolean; hint: string };
  const NO_NAME: Verdict = { name: null, replaces: false, hint: "" };
  type RowButton = "use" | "rename" | "delete";

  let rows = $state.raw<Rows>({ status: "viewing" });
  let said = $state.raw<Said>({ kind: "nothing" });
  /** Why the last write failed, in words ("": it did not). */
  let problem = $state("");
  /** What the "Layout name" field holds. */
  let typed = $state("");
  let box: HTMLElement | undefined = $state();
  let field: HTMLInputElement | undefined = $state();

  /** The view whose options opened the dialog (`null`: opened from elsewhere). */
  const openedFor =
    document.activeElement?.closest("[data-view-menu]")?.getAttribute("data-view-menu") ?? null;

  const layouts = $derived(arrangement.layouts);
  const full = $derived(layouts.length >= MAX_LAYOUTS);
  const draft = $derived(judge(typed, null));
  const renamed = $derived(rows.status === "renaming" ? judge(rows.text, rows.name) : NO_NAME);
  const saidText = $derived.by(() => {
    switch (said.kind) {
      case "nothing":
        return "";
      case "saved":
        return t("layouts.saved", { name: said.name });
      case "deleted":
        return t("layouts.deleted", { name: said.layout.name });
      case "undone":
        return t("fileOps.undone");
      default:
        return said satisfies never;
    }
  });

  /**
   * What a name field holds. `own`: the layout being renamed (another spelling of its own name is free, and a rename
   * needs no room in the list); `null`: the name the arrangement shown is saved under (a kept name replaces).
   */
  function judge(text: string, own: string | null): Verdict {
    const no = (hint: string): Verdict => ({ ...NO_NAME, hint });
    const noRoom = own === null && full;
    if (text.trim() === "") return no(noRoom ? t("layouts.full", { max: MAX_LAYOUTS }) : "");
    const name = layoutName(text);
    if (name === null) return no(t("layouts.nameRule", { max: MAX_LAYOUT_NAME }));
    const taken = findLayout(layouts, name);
    const exists = taken === undefined ? "" : t("layouts.exists", { name: taken.name });
    if (own !== null) {
      const free = taken === undefined || foldName(taken.name) === foldName(own);
      return free ? { name, replaces: false, hint: "" } : no(exists);
    }
    if (taken !== undefined) return { name, replaces: true, hint: exists };
    return noRoom
      ? no(t("layouts.full", { max: MAX_LAYOUTS }))
      : { name, replaces: false, hint: "" };
  }

  /** A write the settings refused | that failed: said in words (the list is already as it was). */
  function fail(error: ApiError): void {
    problem = apiErrorText(error);
  }

  /** An action starts: the lines the one before left (what it did, why it failed) go. */
  function begin(): void {
    said = { kind: "nothing" };
    problem = "";
  }

  /** Once drawn, the keyboard goes to a button of the row `at`; no such row: the name field. */
  async function focusRow(at: number, button: RowButton): Promise<void> {
    await tick();
    const row = box?.querySelectorAll<HTMLElement>("[data-layout]")[at];
    (row?.querySelector<HTMLElement>(`[data-do="${button}"]`) ?? field)?.focus();
  }

  async function save(): Promise<void> {
    const { name } = draft;
    if (name === null) return;
    rows = { status: "viewing" };
    begin();
    const error = await arrangement.saveLayoutAs(name);
    if (error !== null) return fail(error);
    typed = "";
    said = { kind: "saved", name };
    // the button cannot act on the empty field: the keyboard is not left on it
    field?.focus();
  }

  function startRename(layout: Layout): void {
    begin();
    rows = { status: "renaming", name: layout.name, text: layout.name };
  }

  /** The row is read again, the keyboard back on its Rename button. Answers the row's place. */
  function endRename(): number {
    const name = rows.status === "renaming" ? rows.name : null;
    const at = layouts.findIndex((layout) => layout.name === name);
    rows = { status: "viewing" };
    void focusRow(at, "rename");
    return at;
  }

  async function rename(): Promise<void> {
    const { name } = renamed;
    if (rows.status !== "renaming" || name === null) return;
    const from = rows.name;
    const at = endRename();
    if (name === from) return;
    problem = "";
    const error = await arrangement.renameLayout(from, name);
    if (error === null) said = { kind: "saved", name };
    else fail(error);
    await focusRow(at, "rename");
  }

  async function remove(layout: Layout): Promise<void> {
    const at = layouts.indexOf(layout);
    rows = { status: "viewing" };
    begin();
    const error = await arrangement.deleteLayout(layout.name);
    if (error === null) said = { kind: "deleted", layout, at };
    else fail(error);
    // deleted: the row that took its place; not deleted: its own
    await focusRow(at, "delete");
  }

  async function undoDelete(): Promise<void> {
    if (said.kind !== "deleted") return;
    const { layout, at } = said;
    problem = "";
    const error = await arrangement.restoreLayout(layout, at);
    if (error !== null) return fail(error);
    said = { kind: "undone" };
    await focusRow(at, "delete");
  }

  /**
   * The window's arrangement changed (the store said it and left the notice that offers the way back): the dialog
   * closes. What opened the dialog may be gone with the old arrangement (the options button of a strip): the
   * keyboard then goes to the view it was for, else the first one present, never the body.
   */
  async function changed(): Promise<void> {
    ui.closeLayouts();
    await tick();
    const at = document.activeElement;
    if (at instanceof HTMLElement && at !== document.body) return;
    const view = openedFor ?? arrangement.present[0]?.id;
    if (view !== undefined) focusView(view);
  }

  function use(layout: Layout): void {
    if (arrangement.useLayout(layout.name)) void changed();
  }

  function reset(): void {
    arrangement.reset();
    void changed();
  }

  // Escape leaves a rename first: the dialog closes only when no row is being renamed
  function onEscape(): void {
    if (rows.status === "renaming") endRename();
    else ui.closeLayouts();
  }

  /** Enter in a name field is its button (never a key of a text being composed). */
  function onEnter(event: KeyboardEvent, act: () => Promise<void>): void {
    if (event.key !== "Enter" || isComposition(event)) return;
    event.preventDefault();
    void act();
  }

  /** The rename field takes the keyboard with the name selected: typing replaces it. */
  const selected: Attachment<HTMLInputElement> = (node) => {
    node.focus();
    node.select();
  };
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="backdrop"
  onclick={(e) => {
    if (e.target === e.currentTarget) ui.closeLayouts();
  }}
>
  <div
    bind:this={box}
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="layouts-title"
    tabindex="-1"
    use:dialogFocus={{ initial: '[data-do="use"], [data-do="name"]', onEscape }}
  >
    <h2 id="layouts-title">{t("layouts.title")}</h2>

    {#if layouts.length === 0}
      <p class="empty">{t("layouts.empty")}</p>
    {:else}
      <ul class="rows">
        {#each layouts as layout (foldName(layout.name))}
          {@const name = layout.name}
          <li class="row" data-layout={name}>
            {#if rows.status === "renaming" && rows.name === name}
              <input
                {@attach selected}
                class="field-input"
                type="text"
                data-do="rename-to"
                autocomplete="off"
                spellcheck="false"
                aria-label={t("layouts.renameNamed", { name })}
                aria-describedby="layouts-rename-hint"
                aria-invalid={renamed.hint !== ""}
                value={rows.text}
                oninput={(e) => (rows = { status: "renaming", name, text: e.currentTarget.value })}
                onkeydown={(e) => onEnter(e, rename)}
              />
              <span class="actions">
                <button
                  type="button"
                  class="btn btn--sm btn--primary"
                  data-do="rename-save"
                  disabled={renamed.name === null}
                  onclick={() => void rename()}>{t("edit.save")}</button
                >
                <button
                  type="button"
                  class="btn btn--sm"
                  data-do="rename-cancel"
                  onclick={() => endRename()}>{t("common.cancel")}</button
                >
              </span>
              <p class="hint" id="layouts-rename-hint" aria-live="polite">{renamed.hint}</p>
            {:else}
              <span class="name" title={name}>{name}</span>
              <span class="actions">
                <button
                  type="button"
                  class="btn btn--sm"
                  data-do="use"
                  aria-label={t("layouts.useNamed", { name })}
                  onclick={() => use(layout)}>{t("layouts.use")}</button
                >
                <button
                  type="button"
                  class="btn btn--sm btn--ghost"
                  data-do="rename"
                  aria-label={t("layouts.renameNamed", { name })}
                  onclick={() => startRename(layout)}>{t("fileOps.rename")}</button
                >
                <button
                  type="button"
                  class="btn btn--sm btn--ghost"
                  data-do="delete"
                  aria-label={t("layouts.deleteNamed", { name })}
                  onclick={() => void remove(layout)}>{t("fileOps.delete")}</button
                >
              </span>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}

    <div class="said">
      <p role="status" data-layouts-status>{saidText}</p>
      {#if said.kind === "deleted"}
        <button type="button" class="btn btn--sm" data-do="undo" onclick={() => void undoDelete()}
          >{t("fileOps.undo")}</button
        >
      {/if}
    </div>
    {#if problem !== ""}
      <p class="problem" role="alert">{problem}</p>
    {/if}

    <section class="save" aria-labelledby="layouts-save-title">
      <h3 id="layouts-save-title">{t("layouts.saveAs")}</h3>
      <label for="layouts-name">{t("layouts.name")}</label>
      <div class="save-row">
        <input
          bind:this={field}
          bind:value={typed}
          id="layouts-name"
          class="field-input"
          type="text"
          data-do="name"
          autocomplete="off"
          spellcheck="false"
          aria-describedby="layouts-name-hint"
          aria-invalid={draft.name === null && typed.trim() !== ""}
          onkeydown={(e) => onEnter(e, save)}
        />
        <button
          type="button"
          class="btn btn--primary"
          data-do="save"
          disabled={draft.name === null}
          onclick={() => void save()}
          >{draft.replaces ? t("source.replace") : t("edit.save")}</button
        >
      </div>
      <p class="hint" id="layouts-name-hint" aria-live="polite">{draft.hint}</p>
    </section>

    <!-- a plain row: a `footer` here would be a second page footer among the window's landmarks -->
    <div class="foot">
      <button
        type="button"
        class="btn btn--ghost"
        data-do="reset"
        disabled={arrangement.atDefault}
        onclick={reset}>{t("layouts.resetDefault")}</button
      >
      <span class="spacer"></span>
      <button type="button" class="btn" data-do="close" onclick={() => ui.closeLayouts()}
        >{t("common.close")}</button
      >
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 56;
    display: grid;
    place-items: center;
    padding: var(--sp-5);
    background: var(--backdrop);
    animation: pm-fade var(--dur) var(--ease);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    width: min(520px, 100%);
    /* against the window, not the backdrop's row: a percentage would grow with a long list */
    max-height: min(640px, calc(100vh - 2 * var(--sp-5)));
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    box-shadow: var(--shadow-pop);
    outline: none;
  }
  h2 {
    margin: 0;
    padding: var(--sp-4) var(--sp-4) var(--sp-2);
    font-size: 15px;
    font-weight: 600;
  }
  /* the list scrolls inside the dialog: the name field and the buttons under it stay in reach */
  .rows {
    flex: 1 1 auto;
    min-height: 34px;
    margin: 0;
    padding: 0 var(--sp-2);
    overflow-y: auto;
    list-style: none;
  }
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--sp-1) var(--sp-2);
    padding: 5px var(--sp-2);
    border-radius: var(--radius-control);
  }
  .row:hover {
    background: var(--hover);
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .actions {
    display: flex;
    gap: var(--sp-1);
  }
  .row .field-input {
    min-width: 0;
    height: 24px;
  }
  .empty,
  .said,
  .problem {
    margin: 0;
    padding: 0 var(--sp-4);
    font-size: var(--fs-ui-sm);
  }
  .empty {
    padding-block: var(--sp-2);
    color: var(--muted);
  }
  /* the line has its room whether it says something or not: the field under it never jumps */
  .said {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    min-height: 28px;
    color: var(--muted);
  }
  .said p {
    margin: 0;
  }
  .problem {
    padding-block: var(--sp-2) 0;
    color: var(--git-deleted-text);
  }
  .save {
    display: grid;
    gap: var(--sp-1);
    padding: var(--sp-3) var(--sp-4);
  }
  h3 {
    margin: 0 0 var(--sp-1);
    font-size: var(--fs-ui);
    font-weight: 600;
  }
  label {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .save-row {
    display: flex;
    gap: var(--sp-2);
  }
  .save-row .field-input {
    flex: 1;
    min-width: 0;
  }
  .hint {
    grid-column: 1 / -1;
    margin: 0;
    color: var(--git-modified-text);
    font-size: var(--fs-ui-sm);
  }
  .foot {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-3) var(--sp-4);
    border-top: 1px solid var(--border);
  }
</style>
