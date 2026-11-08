// How a modal dialog owns the keyboard: the focus enters it when it opens, Tab | Shift+Tab cycle through its
// controls and never reach the page behind, Escape is the dialog's cancel, and the focus goes back to what had it
// once the dialog goes. The key handler runs after the handlers of the controls inside (Svelte's delegated
// `onkeydown` included, via `on`): a control that already used the key (`preventDefault`) keeps it.
import type { ActionReturn } from "svelte/action";
import { on } from "svelte/events";
import { isComposition } from "../commands/combo";

export type DialogFocusOptions = {
  /** What takes the focus on open: a selector inside the dialog, or a function given the dialog (return the
   *  dialog itself for a reading surface). Absent, not found or not focusable: the first control, else the dialog. */
  initial?: string | ((dialog: HTMLElement) => HTMLElement | null | undefined);
  /** Escape: the dialog's cancel; the key goes no further. Absent: Escape passes through. */
  onEscape?: () => void;
  /** The focus goes back to what had it before the dialog opened (default true). */
  restoreFocus?: boolean;
  /** Tab | Shift+Tab cycle the dialog's controls (default true). False: the dialog gives Tab a meaning of its own
   *  (and so never lets it move the focus out). */
  trapTab?: boolean;
};

const CONTROLS = [
  "a[href]",
  "area[href]",
  "button",
  "input:not([type=hidden])",
  "select",
  "textarea",
  "summary",
  "iframe",
  "[contenteditable]:not([contenteditable=false])",
  "[tabindex]",
].join(", ");

/** The places Tab stops at inside `dialog`, in order: not disabled, not hidden | inert, not taken out of the Tab
 *  order (`tabindex=-1`: the other rows of a list with one stop). */
function stops(dialog: HTMLElement): HTMLElement[] {
  return Array.from(dialog.querySelectorAll<HTMLElement>(CONTROLS)).filter(
    (el) =>
      !el.matches(":disabled") &&
      Number(el.getAttribute("tabindex") ?? "0") >= 0 &&
      el.closest("[hidden], [inert]") === null &&
      // not every web view has it (an older WebKit): there the attributes above decide
      (typeof el.checkVisibility !== "function" || el.checkVisibility()),
  );
}

function pick(dialog: HTMLElement, initial: DialogFocusOptions["initial"]): HTMLElement | null {
  if (initial === undefined) return null;
  if (typeof initial === "string") return dialog.querySelector<HTMLElement>(initial);
  return initial(dialog) ?? null;
}

function enter(dialog: HTMLElement, initial: DialogFocusOptions["initial"]): void {
  // a disabled default answer cannot take the focus: the next candidate does
  for (const el of [pick(dialog, initial), stops(dialog)[0], dialog]) {
    el?.focus();
    if (el && document.activeElement === el) return;
  }
}

function trap(dialog: HTMLElement, event: KeyboardEvent): void {
  const all = stops(dialog);
  const first = all[0];
  const last = all.at(-1);
  if (!first || !last) {
    event.preventDefault();
    dialog.focus();
    return;
  }
  const at = document.activeElement;
  // on the dialog itself | outside it, the browser would step out of it (Shift+Tab) or past it
  const inside = at instanceof Node && at !== dialog && dialog.contains(at);
  if (event.shiftKey ? !inside || at === first : !inside || at === last) {
    event.preventDefault();
    (event.shiftKey ? last : first).focus();
  }
}

/** Svelte action for a modal dialog (`role=dialog|alertdialog`, `aria-modal=true`, `tabindex=-1`). */
export function dialogFocus(
  node: HTMLElement,
  options: DialogFocusOptions = {},
): ActionReturn<DialogFocusOptions> {
  let current = options;
  const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  enter(node, current.initial);
  const off = on(node, "keydown", (event) => {
    if (event.defaultPrevented || isComposition(event)) return;
    if (event.key === "Escape" && current.onEscape) {
      event.preventDefault();
      event.stopPropagation();
      current.onEscape();
    } else if (event.key === "Tab" && current.trapTab !== false) {
      trap(node, event);
    }
  });
  return {
    update(next) {
      current = next;
    },
    destroy() {
      off();
      if (current.restoreFocus !== false && opener?.isConnected && !node.contains(opener)) {
        opener.focus();
      }
    },
  };
}
