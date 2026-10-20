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
      /** `shift`: picked with Shift held (Shift+click | Shift+Enter). */
      run: (shift: boolean) => void;
    }
  | { kind: "heading"; label: string }
  | { kind: "separator" };
