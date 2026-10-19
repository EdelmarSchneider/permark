// Entries of a context menu: data the component draws.

export type MenuEntry =
  | { kind: "item"; label: string; hint?: string; danger?: boolean; run: () => void }
  | { kind: "separator" };
