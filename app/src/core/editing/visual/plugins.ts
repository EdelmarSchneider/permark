// The ProseMirror plugins of Edit mode that are the app's own: change and selection reports, keys (save, mode,
// AltGr-safe headings, lists), the slash menu's state, pasted | dropped pictures, links that never navigate.

import { liftListItem } from "@milkdown/kit/prose/schema-list";
import { Plugin, PluginKey, type EditorState } from "@milkdown/kit/prose/state";
import type { EditorView } from "@milkdown/kit/prose/view";
import { $prose } from "@milkdown/kit/utils";
import { run, type Command } from "./commands";
import type { Core } from "./core";

/** What the plugins ask of the editor that owns them. */
export type PluginHost = {
  core(): Core | null;
  changed(): void;
  selected(): void;
  save(): void;
  /** Ctrl+E (`source`: with Alt). */
  modeKey(source: boolean): void;
  /** Ctrl+K: the link popover. */
  linkKey(): void;
  slash(state: SlashState | null): void;
  /** Keys while the slash menu is open; true when used. */
  slashKey(key: "up" | "down" | "enter" | "escape"): boolean;
  pictures(files: File[], at: number): void;
  followLink(href: string): void;
};

/** `/` typed alone at the start of an empty paragraph: what follows it filters the menu. */
export type SlashState = { query: string; from: number; to: number };

export const slashKey = new PluginKey<{ dismissed: boolean }>("PM_SLASH");

/** The slash text around the caret, if any. */
export function slashAt(state: EditorState): SlashState | null {
  const { $from, empty } = state.selection;
  if (!empty || $from.parent.type.name !== "paragraph") return null;
  if ($from.depth !== 1) return null;
  const text = $from.parent.textContent;
  const match = /^\/([\p{L}\p{N}-]*)$/u.exec(text);
  if (!match || $from.parentOffset !== text.length) return null;
  return { query: match[1] ?? "", from: $from.start(), to: $from.end() };
}

const DIGITS: Record<string, number> = {
  Digit0: 0,
  Digit1: 1,
  Digit2: 2,
  Digit3: 3,
  Digit4: 4,
  Digit5: 5,
  Digit6: 6,
};

function keys(host: PluginHost, view: EditorView, event: KeyboardEvent): boolean {
  if (event.isComposing) return false;
  const core = host.core();
  const mod = event.ctrlKey || event.metaKey;
  const key = event.key.toLowerCase();
  const slash = slashKey.getState(view.state);
  if (slash && !slash.dismissed && slashAt(view.state)) {
    const named =
      event.key === "ArrowUp"
        ? "up"
        : event.key === "ArrowDown"
          ? "down"
          : event.key === "Enter"
            ? "enter"
            : event.key === "Escape"
              ? "escape"
              : null;
    if (named !== null && host.slashKey(named)) {
      if (named === "escape") view.dispatch(view.state.tr.setMeta(slashKey, { dismissed: true }));
      return true;
    }
  }
  if (mod && !event.shiftKey && !event.altKey && key === "s") {
    host.save();
    return true;
  }
  if (mod && !event.shiftKey && key === "e") {
    host.modeKey(event.altKey);
    return true;
  }
  if (mod && !event.shiftKey && !event.altKey && key === "k") {
    host.linkKey();
    return true;
  }
  if (!core) return false;
  const go = (command: Command): boolean => run(core, command);
  if (mod && event.shiftKey && !event.altKey && key === "c") return go({ kind: "code" });
  if (mod && event.shiftKey && !event.altKey && key === "x") return go({ kind: "strike" });
  // Ctrl+Alt+digit: headings; AltGr (Ctrl+Alt on Windows) types characters and is left alone
  const level = DIGITS[event.code];
  if (event.ctrlKey && event.altKey && level !== undefined && !event.getModifierState("AltGraph")) {
    return go(level === 0 ? { kind: "block", to: "paragraph" } : headingOf(level));
  }
  // Enter in an empty nested list item: one level out
  if (event.key === "Enter" && !mod && !event.shiftKey) {
    const { $from, empty } = view.state.selection;
    const itemType = view.state.schema.nodes.list_item;
    if (
      empty &&
      itemType &&
      $from.parent.content.size === 0 &&
      $from.depth >= 4 &&
      $from.node($from.depth - 1).type === itemType &&
      $from.node($from.depth - 3).type === itemType
    ) {
      return liftListItem(itemType)(view.state, view.dispatch);
    }
  }
  return false;
}

function headingOf(level: number): Command {
  const to = (["h1", "h2", "h3", "h4", "h5", "h6"] as const)[level - 1] ?? "h1";
  return { kind: "block", to };
}

function imagesOf(data: DataTransfer | null): File[] {
  return Array.from(data?.files ?? []).filter((file) => file.type.startsWith("image/"));
}

export function appPlugins(host: PluginHost) {
  return [
    $prose(
      () =>
        new Plugin({
          key: slashKey,
          state: {
            init: () => ({ dismissed: false }),
            apply: (tr, value, _old, next) => {
              const meta: unknown = tr.getMeta(slashKey);
              if (typeof meta === "object" && meta !== null && "dismissed" in meta) {
                return { dismissed: meta.dismissed === true };
              }
              // the slash is gone: the menu may open again
              return value.dismissed && !slashAt(next) ? { dismissed: false } : value;
            },
          },
          props: {
            handleKeyDown: (view, event) => keys(host, view, event),
            handlePaste: (view, event) => {
              const files = imagesOf(event.clipboardData);
              if (files.length === 0) return false;
              host.pictures(files, view.state.selection.from);
              return true;
            },
            handleDrop: (view, event) => {
              const files = imagesOf(event.dataTransfer);
              if (files.length === 0) return false;
              const at = view.posAtCoords({ left: event.clientX, top: event.clientY })?.pos;
              host.pictures(files, at ?? view.state.selection.from);
              return true;
            },
            handleDOMEvents: {
              click: (_view, event) => {
                const anchor =
                  event.target instanceof Element ? event.target.closest("a[href]") : null;
                if (!anchor) return false;
                event.preventDefault();
                if (event.ctrlKey || event.metaKey)
                  host.followLink(anchor.getAttribute("href") ?? "");
                return true;
              },
            },
          },
          view: () => ({
            update: (view, previous) => {
              const docChanged = view.state.doc !== previous.doc;
              if (docChanged) host.changed();
              if (docChanged || !view.state.selection.eq(previous.selection)) {
                host.selected();
                const slash = slashKey.getState(view.state);
                host.slash(slash && !slash.dismissed ? slashAt(view.state) : null);
              }
            },
          }),
        }),
    ),
  ];
}
