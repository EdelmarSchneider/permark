// "Convert…" on a table of the page: one small button over the table under the pointer or holding the focus (no
// element per table, none in the page itself, nothing when neither is on a table). The table goes to the converter as
// CSV. The palette's "Convert table…" takes the table holding the selection | the focus.

import { t } from "../../core/i18n";
import { cellsOf, tableAsCsv } from "./model";

/** The table of a decorated page holding the selection | the focus, as last seen inside the page. */
let held: HTMLTableElement | null = null;

/** The document's table at `node`: not one a plugin draws inside the page (a TOON block's sheet). */
function tableAt(node: Node | null): HTMLTableElement | null {
  const element = node instanceof Element ? node : (node?.parentElement ?? null);
  const table = element?.closest("table") ?? null;
  return table instanceof HTMLTableElement && table.closest(".sheet") === null ? table : null;
}

/** The table holding the selection | the focus in the page on screen (`null`: none; the page hidden behind the
 *  editor offers none). */
export function heldTable(): HTMLTableElement | null {
  return held?.isConnected === true && held.closest("[hidden]") === null ? held : null;
}

/** Sends `table` to the converter as CSV (a table of one row has nothing to convert). */
export function convertTable(table: HTMLTableElement, open: (csv: string) => void): void {
  const rows = cellsOf(table);
  if (rows.length > 1) open(tableAsCsv(rows));
}

/** Puts the button on the tables of `root`; returns what takes it away. */
export function decorateTables(root: HTMLElement, open: (csv: string) => void): () => void {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "btn btn--sm pm-convert-table";
  button.textContent = t("convert.table");
  button.hidden = true;
  Object.assign(button.style, { position: "fixed", zIndex: "30" });
  let current: HTMLTableElement | null = null;

  const hide = (): void => {
    button.hidden = true;
    current = null;
  };
  const place = (table: HTMLTableElement): void => {
    current = table;
    button.hidden = false;
    const box = table.getBoundingClientRect();
    const width = button.offsetWidth || 90;
    button.style.top = `${Math.max(4, box.top - 30)}px`;
    button.style.left = `${Math.max(4, box.right - width)}px`;
  };
  /** The pointer | the focus came to `node` of the page: the button goes to its table, or away. */
  const reach = (node: EventTarget | null): void => {
    const table = node instanceof Node ? tableAt(node) : null;
    if (table !== null) {
      if (table !== current) place(table);
    } else if (current !== null) {
      // on the page but off the table: the button goes
      hide();
    }
  };
  const onOver = (event: PointerEvent): void => reach(event.target);
  /** A press | the focus inside the page: the selection | the focus is there now (a press is seen at once, its
   *  `selectionchange` only a task later: a shortcut right after the click must not miss the table). */
  const hold = (event: Event): void => {
    held = event.target instanceof Node ? tableAt(event.target) : null;
  };
  const onFocusIn = (event: FocusEvent): void => {
    hold(event);
    reach(event.target);
  };
  /** Focus off the table: the button stays only for its table or itself. */
  const onFocusOut = (event: FocusEvent): void => {
    const to = event.relatedTarget;
    if (current === null || to === button) return;
    if (!(to instanceof Node) || !current.contains(to)) hide();
  };
  /** A selection outside the page (the palette's field) leaves the table that was meant. */
  const onSelection = (): void => {
    const node = document.getSelection()?.anchorNode ?? null;
    if (node !== null && root.contains(node)) held = tableAt(node);
  };
  const onLeave = (event: PointerEvent): void => {
    if (event.relatedTarget !== button) hide();
  };
  /** Off the button: it stays only when the pointer goes back to its table. */
  const onButtonLeave = (event: PointerEvent): void => {
    const to = event.relatedTarget;
    if (!(to instanceof Node) || current === null || !current.contains(to)) hide();
  };
  const onClick = (): void => {
    const table = current;
    hide();
    if (table !== null) convertTable(table, open);
  };
  button.addEventListener("click", onClick);
  button.addEventListener("pointerleave", onButtonLeave);
  root.addEventListener("pointerover", onOver);
  root.addEventListener("pointerleave", onLeave);
  root.addEventListener("pointerdown", hold);
  root.addEventListener("focusin", onFocusIn);
  root.addEventListener("focusout", onFocusOut);
  document.addEventListener("selectionchange", onSelection);
  window.addEventListener("scroll", hide, true);
  document.body.append(button);
  return () => {
    root.removeEventListener("pointerover", onOver);
    root.removeEventListener("pointerleave", onLeave);
    root.removeEventListener("pointerdown", hold);
    root.removeEventListener("focusin", onFocusIn);
    root.removeEventListener("focusout", onFocusOut);
    document.removeEventListener("selectionchange", onSelection);
    window.removeEventListener("scroll", hide, true);
    button.removeEventListener("click", onClick);
    button.removeEventListener("pointerleave", onButtonLeave);
    button.remove();
    if (held !== null && root.contains(held)) held = null;
  };
}
