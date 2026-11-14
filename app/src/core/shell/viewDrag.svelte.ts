// A view dragged by its tab, or by its header while it is alone in its group, with the mouse | a pen (a finger
// never drags: the menu moves views there). A press becomes a drag once the pointer has traveled `DRAG_START_PX`;
// until then it is the click it always was. While the drag runs the boxes of the window are read at each move and
// the pure `dragTarget` names the ONE place a drop would put the view; a drop there makes the move the views' menu
// makes (the same arrangement, the same words, the keyboard on the view's options). Escape, a drop anywhere else,
// a pointer the system takes back and a window that loses the focus change nothing. The window is listened to only
// between the press and the end; `ViewDragLayer` draws what a drag shows.

import { REGIONS } from "./arrangement";
import { arrangement } from "./arrangement.svelte";
import {
  dragTarget,
  dropOn,
  saidFor,
  type Box,
  type DragScene,
  type DragTarget,
  type GroupBox,
  type Point,
  type RegionBox,
} from "./dragTarget";
import { placeView } from "./sides";
import { viewMenu } from "./viewMenu.svelte";
import { viewSlots } from "./viewSlots.svelte";

/** A pressed pointer must travel this far before the press is a drag. */
export const DRAG_START_PX = 6;

/** What a press holds until it ends: the view, the element pressed, the pointer, what had the keyboard before. */
type Press = {
  readonly id: string;
  readonly handle: HTMLElement;
  readonly pointer: number;
  readonly focus: Element | null;
};

export type DragState =
  | { readonly status: "idle" }
  | ({ readonly status: "pending"; readonly from: Point } & Press)
  | ({
      readonly status: "dragging";
      readonly at: Point;
      readonly target: DragTarget | null;
    } & Press);

/** Things in a handle that take a press for themselves: its buttons (the options button), fields, links. */
const CONTROLS =
  'button, input, select, textarea, a[href], label, summary, [role="button"], [role="radio"], [contenteditable="true"]';

function boxOf(el: Element): Box {
  const { left, top, right, bottom } = el.getBoundingClientRect();
  return { left, top, right, bottom };
}

/**
 * The boxes of what the window draws now: per region on screen its groups (strip, tabs, the body of the view shown),
 * and the frame. A region the person hid has no room on screen: like one without a view, its edge stands for it.
 */
function scene(): DragScene {
  const regions = REGIONS.flatMap((region): RegionBox[] => {
    const groups = arrangement.drawn(region).flatMap((group): GroupBox[] => {
      const slot = viewSlots.of(arrangement.selectedOf(group) ?? "");
      const body = slot === undefined ? null : boxOf(slot);
      if (body === null || body.right <= body.left || body.bottom <= body.top) return [];
      const tabs = group.views.flatMap((id) => {
        const tab = document.querySelector(`[role="tab"][data-view="${id}"]`);
        return tab === null ? [] : [{ id, tab }];
      });
      const bar = tabs[0]?.tab.closest("[data-view-strip]") ?? null;
      const strip =
        bar === null
          ? null
          : { box: boxOf(bar), tabs: tabs.map(({ id, tab }) => ({ id, box: boxOf(tab) })) };
      return [{ at: group.at, strip, body }];
    });
    return groups.length === 0 ? [] : [{ region, folded: arrangement.short(region), groups }];
  });
  const frame = (part: string): Box | null => {
    const el = document.querySelector(`[data-frame="${part}"]`);
    return el === null ? null : boxOf(el);
  };
  return { regions, body: frame("body"), center: frame("center") };
}

/**
 * The release of a drag is not a click: the one the webview makes of it (on the handle, which held the pointer) goes
 * nowhere. `released`: the pointer is up already, the click comes within this turn; else (the drag ended while the
 * button is still down) whenever the button is let go. A new press is never swallowed. Nor is a click a key makes
 * (Enter | Space on a button: `detail` 0), and a key pressed ends the wait: a release this window never sees (the
 * drag ended because another window took the focus) must not cost the keyboard its next press.
 */
function swallowClick(released: boolean): void {
  const stop = (event: MouseEvent): void => {
    if (event.detail === 0) return;
    event.preventDefault();
    event.stopImmediatePropagation();
  };
  const off = (): void => {
    window.removeEventListener("click", stop, true);
    window.removeEventListener("pointerdown", off, true);
    window.removeEventListener("keydown", off, true);
    window.removeEventListener("pointerup", soon, true);
  };
  const soon = (): void => {
    setTimeout(off, 0);
  };
  window.addEventListener("click", stop, true);
  window.addEventListener("pointerdown", off, true);
  window.addEventListener("keydown", off, true);
  if (released) soon();
  else window.addEventListener("pointerup", soon, true);
}

class ViewDrag {
  state = $state.raw<DragState>({ status: "idle" });
  /** Stops listening to the window (`null`: no press runs). */
  #off: (() => void) | null = null;

  /** The view being dragged (`null`: none). */
  get dragged(): string | null {
    return this.state.status === "dragging" ? this.state.id : null;
  }

  /**
   * A pointer went down on a handle of the view `id` (its tab, its header): the press may become a drag. Only the
   * main button of the mouse | a pen, on the handle itself (never on a control inside it).
   */
  press(id: string, handle: HTMLElement, event: PointerEvent): void {
    if (this.state.status !== "idle" || event.button !== 0 || !event.isPrimary) return;
    if (event.pointerType !== "mouse" && event.pointerType !== "pen") return;
    if (arrangement.viewOf(id) === undefined || !(event.target instanceof Element)) return;
    const control = event.target.closest(CONTROLS);
    if (control !== null && control !== handle && handle.contains(control)) return;
    this.state = {
      status: "pending",
      id,
      handle,
      pointer: event.pointerId,
      from: { x: event.clientX, y: event.clientY },
      focus: document.activeElement,
    };
    this.#listen(handle);
  }

  /** Ends a drag | a press without a drop: nothing changed, the keyboard back where it was. */
  cancel(): void {
    const press = this.state;
    if (press.status === "idle") return;
    this.#end(false);
    if (press.status === "dragging") giveBack(press.focus);
  }

  #listen(handle: HTMLElement): void {
    const move = (event: PointerEvent): void => this.#move(event);
    const up = (event: PointerEvent): void => this.#up(event);
    const cancel = (): void => this.cancel();
    const key = (event: KeyboardEvent): void => this.#key(event);
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("blur", cancel);
    // before anything else hears it: Escape ends the drag and nothing more
    window.addEventListener("keydown", key, true);
    handle.addEventListener("lostpointercapture", cancel);
    this.#off = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", cancel);
      window.removeEventListener("blur", cancel);
      window.removeEventListener("keydown", key, true);
      handle.removeEventListener("lostpointercapture", cancel);
    };
  }

  /** The press is over: the window is no longer listened to; a drag lets its pointer go and takes its click. */
  #end(released: boolean): void {
    const press = this.state;
    this.#off?.();
    this.#off = null;
    this.state = { status: "idle" };
    if (press.status !== "dragging") return;
    if (press.handle.hasPointerCapture(press.pointer)) {
      press.handle.releasePointerCapture(press.pointer);
    }
    swallowClick(released);
  }

  #targetAt(id: string, at: Point): DragTarget | null {
    return dragTarget(at, scene(), id, arrangement.layout);
  }

  #move(event: PointerEvent): void {
    const press = this.state;
    if (press.status === "idle" || event.pointerId !== press.pointer) return;
    // no button is down: the release happened where this window could not see it; the press is over
    if (event.buttons === 0) {
      this.cancel();
      return;
    }
    const at = { x: event.clientX, y: event.clientY };
    if (press.status === "pending") {
      if (Math.hypot(at.x - press.from.x, at.y - press.from.y) < DRAG_START_PX) return;
      // every move and the release come here, whatever the pointer is over
      press.handle.setPointerCapture(press.pointer);
    }
    const { id, handle, pointer, focus } = press;
    this.state = {
      status: "dragging",
      id,
      handle,
      pointer,
      focus,
      at,
      target: this.#targetAt(id, at),
    };
  }

  #up(event: PointerEvent): void {
    const press = this.state;
    if (press.status === "idle" || event.pointerId !== press.pointer) return;
    this.#end(true);
    if (press.status !== "dragging") return;
    const target = this.#targetAt(press.id, { x: event.clientX, y: event.clientY });
    if (target === null) {
      giveBack(press.focus);
      return;
    }
    const layout = arrangement.layout;
    const said = saidFor(layout, press.id, target);
    placeView(press.id, dropOn(layout, press.id, target));
    viewMenu.moved(press.id, said);
  }

  #key(event: KeyboardEvent): void {
    if (this.state.status !== "dragging" || event.key !== "Escape" || event.isComposing) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    this.cancel();
  }
}

/** The keyboard goes back to what had it before the press (a press on a tab gives it to the tab). */
function giveBack(focus: Element | null): void {
  if (focus instanceof HTMLElement && focus !== document.body && focus.isConnected) {
    focus.focus({ preventScroll: true });
  }
}

export const viewDrag = new ViewDrag();
