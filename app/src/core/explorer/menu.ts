// Entries of a context menu: data the component draws.

import type { Component } from "svelte";

/** A small icon before an entry's label (a `@lucide/svelte` icon). */
export type MenuIcon = Component<{ size?: number; strokeWidth?: number }>;

export type MenuEntry =
  | {
      kind: "item";
      label: string;
      /** Drawn in a leading column; the menu's other items keep that column (empty) so the labels line up. */
      icon?: MenuIcon;
      hint?: string;
      /** A second, quieter line (a path). */
      detail?: string;
      danger?: boolean;
      /** Opens a submenu in place (drawn with ▸; ArrowRight runs it too). */
      opens?: boolean;
      /** Can't act now: still reached by the arrows and announced (`aria-disabled`), does nothing. */
      disabled?: boolean;
      /** `shift`: picked with Shift held (Shift+click | Shift+Enter). */
      run: (shift: boolean) => void;
    }
  | { kind: "heading"; label: string }
  | { kind: "separator" };
