// Entries of a context menu: data the component draws.

export type MenuEntry =
  | {
      kind: "item";
      label: string;
      hint?: string;
      /** A second, quieter line (a path). */
      detail?: string;
      danger?: boolean;
      run: () => void;
    }
  | { kind: "heading"; label: string }
  | { kind: "separator" };
