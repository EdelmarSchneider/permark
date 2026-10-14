// Imperative hooks that DocView registers for code that lives outside it (global shortcuts, the find
// bar, the selection popover). Components that need DOM access to the rendered document go through here.

import type { FindMode } from "./find";

export type DocSelection = { text: string; section: string | null };

export type FindApi = {
  /** Runs a search. `near` positions the first match at or after that element (a located section). */
  run(
    query: string,
    mode: FindMode,
    opts?: { keepIndex?: boolean; near?: HTMLElement | null },
  ): void;
  step(direction: 1 | -1): void;
  clear(): void;
};

export type DocBridge = {
  /** F7 / Shift+F7: move to the next or previous changed block or removed marker. */
  jumpChange?: (direction: 1 | -1) => void;
  find?: FindApi;
  /** The current text selection inside the document, with its nearest heading. */
  getSelection?: () => DocSelection | null;
  /** The element for a block or heading, used to position the first find match. */
  locatedElement?: () => HTMLElement | null;
};

export const bridge: DocBridge = {};
