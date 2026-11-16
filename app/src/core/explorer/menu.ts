// Entries of a context menu: data the component draws.

export type MenuEntry =
  | {
      kind: "item";
      label: string;
      hint?: string;
      /** A second, quieter line (a path). */
      detail?: string;
      danger?: boolean;
      /** Opens a submenu in place (drawn with ▸; ArrowRight runs it too). */
      opens?: boolean;
      /** Can't act now: still reached by the arrows and announced (`aria-disabled`), does nothing. */
      disabled?: boolean;
      /** One of a set where one is chosen (`menuitemradio`): its state, announced as such and drawn as a check mark
       *  before the label (never a mark in the label, read aloud as a word); the labels of the set stay aligned. */
      checked?: boolean;
      /** `shift`: picked with Shift held (Shift+click | Shift+Enter). */
      run: (shift: boolean) => void;
    }
  | { kind: "heading"; label: string }
  | { kind: "separator" };
