// "Convert…" on a table of the page: one small button over the table under the pointer (no element per table, none in
// the page itself, nothing when the pointer is elsewhere). The table goes to the converter as CSV.

import { t } from "../../core/i18n";
import { cellsOf, tableAsCsv } from "./model";

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
  const onOver = (event: PointerEvent): void => {
    const target = event.target;
    if (!(target instanceof Element)) return;
    const table = target.closest("table");
    // the tables of the document, not the ones a plugin draws inside it (a TOON block)
    if (table instanceof HTMLTableElement && !table.closest(".sheet")) {
      if (table !== current) place(table);
    }
  };
  const onLeave = (event: PointerEvent): void => {
    if (event.relatedTarget !== button) hide();
  };
  const onClick = (): void => {
    const table = current;
    hide();
    if (table === null) return;
    const rows = cellsOf(table);
    if (rows.length > 1) open(tableAsCsv(rows));
  };
  button.addEventListener("click", onClick);
  root.addEventListener("pointerover", onOver);
  root.addEventListener("pointerleave", onLeave);
  window.addEventListener("scroll", hide, true);
  document.body.append(button);
  return () => {
    root.removeEventListener("pointerover", onOver);
    root.removeEventListener("pointerleave", onLeave);
    window.removeEventListener("scroll", hide, true);
    button.removeEventListener("click", onClick);
    button.remove();
  };
}
