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
      /** Why it can't act now: its tooltip, which assistive tech reads as its description. */
      why?: string;
      /** One of a set where one is chosen (`menuitemradio`): its state, announced as such and drawn as a check mark
       *  before the label (never a mark in the label, read aloud as a word); the labels of the set stay aligned. */
      checked?: boolean;
      /** With `checked`: on | off on its own, whatever the other entries are (`menuitemcheckbox`), not the one
       *  chosen of a set. */
      checkbox?: boolean;
      /** `shift`: picked with Shift held (Shift+click | Shift+Enter). */
      run: (shift: boolean) => void;
    }
  | { kind: "heading"; label: string }
  /** A quiet line that does nothing (a group with nothing in it): passed over by the keyboard. */
  | { kind: "note"; label: string }
  | { kind: "separator" };
