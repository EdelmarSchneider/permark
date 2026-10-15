// The keyboard map (docs/design/wireframes.md, section 13) as a pure function, so it can be tested.
// Ctrl+K stays unbound (in the editor it is the link).

/**
 * Every action the keyboard map reaches. The list is the source of the type, so a test can walk it: the command
 * registry (`commands.ts`) has to account for each one.
 */
export const SHORTCUT_ACTIONS = [
  "openFolder",
  "openFile",
  "searchEverywhere",
  "commands",
  "quickOpen",
  "searchAll",
  "explorer",
  "tagsView",
  "editTags",
  "edit",
  "toggleEdit",
  "toggleEditMode",
  "save",
  "newFile",
  "find",
  "back",
  "forward",
  "nextChange",
  "prevChange",
  "toggleFollow",
  "copyReference",
  "copyPath",
  "openExternal",
  "toggleLeft",
  "toggleRight",
  "zoomIn",
  "zoomOut",
  "zoomReset",
  "cycleWidth",
  "toggleOutline",
  "settings",
] as const;

export type ShortcutAction = (typeof SHORTCUT_ACTIONS)[number];

export type KeyInfo = {
  key: string;
  code?: string;
  ctrlKey: boolean;
  metaKey?: boolean;
  altKey: boolean;
  shiftKey: boolean;
};

/** Layout-independent name of the pressed key: "c", "0", "=", "-", ",", "ArrowLeft", "F7". */
function keyName(e: KeyInfo): string {
  const code = e.code ?? "";
  if (/^Key[A-Z]$/.test(code)) return code.slice(3).toLowerCase();
  if (/^(Digit|Numpad)[0-9]$/.test(code)) return code.slice(-1);
  switch (code) {
    case "Equal":
    case "NumpadAdd":
      return "=";
    case "Minus":
    case "NumpadSubtract":
      return "-";
    case "Comma":
      return ",";
  }
  if (e.key === "+") return "=";
  if (e.key === "_") return "-";
  return e.key.length === 1 ? e.key.toLowerCase() : e.key;
}

export function matchShortcut(e: KeyInfo): ShortcutAction | null {
  const mod = e.ctrlKey || e.metaKey === true;
  const { altKey: alt, shiftKey: shift } = e;
  const key = keyName(e);

  if (!mod && !shift && alt) {
    if (key === "ArrowLeft") return "back";
    if (key === "ArrowRight") return "forward";
  }
  if (!mod && !alt && key === "F7") return shift ? "prevChange" : "nextChange";
  // A plain letter: the caller leaves it alone while the user is typing in a field or an overlay is open.
  if (!mod && !alt && !shift && key === "t") return "editTags";
  if (!mod && !alt && !shift && key === "e") return "edit";
  if (!mod && alt && shift && key === "c") return "copyPath";
  if (!mod) return null;

  if (alt && !shift) {
    switch (key) {
      case "c":
        return "copyReference";
      case "b":
        return "toggleRight";
      case "w":
        return "cycleWidth";
      case "o":
        return "toggleOutline";
      case "e":
        return "toggleEditMode";
    }
    return null;
  }
  if (alt) return null;

  if (shift) {
    switch (key) {
      case "o":
        return "openFile";
      case "f":
        return "searchAll";
      case "x":
        return "explorer";
      case "t":
        return "tagsView";
      case "p":
        return "commands";
      case "l":
        return "toggleFollow";
      case "e":
        return "openExternal";
      case "=": // Ctrl+Shift+= is Ctrl++ on most layouts
        return "zoomIn";
    }
    return null;
  }
  switch (key) {
    case "o":
      return "openFolder";
    case "p":
      return "quickOpen";
    case "t":
      return "searchEverywhere";
    case "f":
      return "find";
    case "e":
      return "toggleEdit";
    case "s":
      return "save";
    case "n":
      return "newFile";
    case "b":
      return "toggleLeft";
    case "=":
      return "zoomIn";
    case "-":
      return "zoomOut";
    case "0":
      return "zoomReset";
    case ",":
      return "settings";
  }
  return null;
}

// ---- how a shortcut is written ---------------------------------------------------------------------

export type Combo = { ctrl: boolean; alt: boolean; shift: boolean; key: string };

/** Reads a combo as the menus write it: "Ctrl+Shift+P", "Alt+Left", "F7", "T". */
export function parseCombo(combo: string): Combo {
  const out: Combo = { ctrl: false, alt: false, shift: false, key: combo };
  for (;;) {
    const match = /^(Ctrl|Alt|Shift)\+(.+)$/.exec(out.key);
    if (!match) return out;
    if (match[1] === "Ctrl") out.ctrl = true;
    else if (match[1] === "Alt") out.alt = true;
    else out.shift = true;
    out.key = match[2];
  }
}

/** Whether the webview runs on macOS, from its user agent. */
export function isMacPlatform(userAgent: string): boolean {
  return /Mac/.test(userAgent);
}

/**
 * A combo written the way the platform prints it. Combos are written with Windows names, where "Ctrl" is the main
 * modifier; on macOS that key is Command, and "Ctrl+Shift+P" reads "⇧⌘P" (Option, Shift, Command, then the key).
 * Elsewhere `localize` writes the key names of the interface's language.
 */
export function shortcutLabel(
  combo: string,
  mac: boolean,
  localize: (combo: string) => string,
): string {
  if (!mac) return localize(combo);
  const { ctrl, alt, shift, key } = parseCombo(combo);
  return `${alt ? "⌥" : ""}${shift ? "⇧" : ""}${ctrl ? "⌘" : ""}${key}`;
}
