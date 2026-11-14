// Where each present view is drawn: the element its region made for it (`ViewRegion`), by view id. The views
// themselves are created once for the window (`ViewHosts`) and moved into these elements; a region never draws a
// view's body.

import type { Attachment } from "svelte/attachments";
import { SvelteMap } from "svelte/reactivity";

class ViewSlots {
  readonly #slots = new SvelteMap<string, HTMLElement>();
  readonly #attachments = new Map<string, Attachment<HTMLElement>>();

  /** The slot of the view `id` now (reactive; undefined: no region draws it). */
  of(id: string): HTMLElement | undefined {
    return this.#slots.get(id);
  }

  /**
   * For the slot element of `id` (`{@attach}`): listed while it lives. One function per id: a region that draws
   * again never takes a slot off the list and puts it back (the view would leave it for a moment).
   */
  slot(id: string): Attachment<HTMLElement> {
    let attachment = this.#attachments.get(id);
    if (attachment === undefined) {
      attachment = (el) => {
        this.#slots.set(id, el);
        return () => {
          // a view that changed region: the new region's slot may be listed already
          if (this.#slots.get(id) === el) this.#slots.delete(id);
        };
      };
      this.#attachments.set(id, attachment);
    }
    return attachment;
  }
}

export const viewSlots = new ViewSlots();
