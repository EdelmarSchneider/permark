// Key combos: the text form (`Mod+Shift+P`), the event -> combo mapping and how a combo is written for the person.
// Pure. `Mod` = Ctrl on Windows | Linux, Cmd on macOS; written once for every platform.

export type Combo = { mod: boolean; alt: boolean; shift: boolean; key: string };

/** The key as the event gives it, reduced to what the grammar knows. */
export type KeyEventLike = {
  key: string;
  code?: string;
  keyCode?: number;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  isComposing?: boolean;
  getModifierState?: (name: string) => boolean;
};

const NAMED_KEYS: ReadonlySet<string> = new Set([
  "ArrowLeft",
  "ArrowRight",
  "ArrowUp",
  "ArrowDown",
  "Enter",
  "Escape",
  "Tab",
  "Space",
  "Backspace",
  "Delete",
  "Home",
  "End",
  "PageUp",
  "PageDown",
  ...Array.from({ length: 24 }, (_, i) => `F${i + 1}`),
]);

/** One printable ASCII character that is not a space (lowercase letters only). */
const SINGLE = /^[a-z0-9!-/:-@[-`{-~]$/;

export function isKnownKey(key: string): boolean {
  return NAMED_KEYS.has(key) || SINGLE.test(key);
}

/** Parses `Mod+Alt+Shift+Key` (modifiers in any order, each at most once). `null` when it is not a combo. */
export function parseCombo(text: string): Combo | null {
  if (text === "" || text.length > 40) return null;
  // the key may itself be `+`: `Mod++` is not valid (`=` is the key of zoom in)
  const parts = text.split("+");
  const key = parts.pop();
  if (key === undefined || !isKnownKey(key)) return null;
  const combo: Combo = { mod: false, alt: false, shift: false, key };
  for (const part of parts) {
    if (part === "Mod" && !combo.mod) combo.mod = true;
    else if (part === "Alt" && !combo.alt) combo.alt = true;
    else if (part === "Shift" && !combo.shift) combo.shift = true;
    else return null;
  }
  return combo;
}

/** The combo written the one way the map stores it. */
export function writeCombo(combo: Combo): string {
  return `${combo.mod ? "Mod+" : ""}${combo.alt ? "Alt+" : ""}${combo.shift ? "Shift+" : ""}${combo.key}`;
}

/** The stored spelling of a combo, or `null` for text that is not one. */
export function normalizeCombo(text: string): string | null {
  const combo = parseCombo(text);
  return combo === null ? null : writeCombo(combo);
}

/** Whether an event is part of a text composition or a dead key: it never runs a shortcut. */
export function isComposition(event: KeyEventLike): boolean {
  return (
    event.isComposing === true ||
    event.keyCode === 229 ||
    event.key === "Dead" ||
    event.key === "Process" ||
    event.key === "Unidentified"
  );
}

/**
 * The combo of a key event, or `null` when it names none: a modifier alone, a composition, AltGr (it types
 * characters), the other platform's modifier, a key outside the grammar.
 */
export function comboFromEvent(event: KeyEventLike, mac: boolean): string | null {
  if (isComposition(event)) return null;
  if (event.getModifierState?.("AltGraph") === true) return null;
  const mod = mac ? event.metaKey : event.ctrlKey;
  const foreign = mac ? event.ctrlKey : event.metaKey;
  if (foreign) return null;
  let shift = event.shiftKey;
  let key = event.key === " " ? "Space" : event.key;
  if (key.length === 1) {
    const code = event.code ?? "";
    const digit = /^Digit(\d)$/.exec(code);
    const letter = /^Key([A-Z])$/.exec(code);
    if (digit?.[1] !== undefined) key = digit[1];
    else if (/^[A-Za-z]$/.test(key)) key = key.toLowerCase();
    else if (!/^[\x21-\x7e]$/.test(key)) {
      // another layout (Cyrillic, Greek) or a mac Option character: the physical letter
      if (letter?.[1] === undefined) return null;
      key = letter[1].toLowerCase();
    } else {
      // `+` is the shifted `=`, `_` the shifted `-`; a character the Shift key made carries no Shift of its own
      if (key === "+") key = "=";
      else if (key === "_") key = "-";
      shift = false;
    }
  } else if (!NAMED_KEYS.has(key)) return null;
  return writeCombo({ mod, alt: event.altKey, shift, key });
}

/** Alt | Shift alone or with a plain key: types text, so it yields to a focused field. */
export function isSingleKey(combo: string): boolean {
  const parsed = parseCombo(combo);
  return parsed !== null && !parsed.mod && !parsed.alt;
}

const WORDS: Readonly<Record<string, string>> = {
  ArrowLeft: "Left",
  ArrowRight: "Right",
  ArrowUp: "Up",
  ArrowDown: "Down",
  Escape: "Esc",
  Backspace: "Backspace",
};
const SYMBOLS: Readonly<Record<string, string>> = {
  ArrowLeft: "←",
  ArrowRight: "→",
  ArrowUp: "↑",
  ArrowDown: "↓",
  Escape: "⎋",
  Backspace: "⌫",
  Delete: "⌦",
  Enter: "↩",
  Tab: "⇥",
  Space: "␣",
};

function keyText(key: string, mac: boolean): string {
  if (mac) return SYMBOLS[key] ?? (key.length === 1 ? key.toUpperCase() : key);
  return WORDS[key] ?? (key.length === 1 ? key.toUpperCase() : key);
}

/** How the platform writes a combo: `Ctrl+Shift+P`, or `⇧⌘P` on macOS (Option, Shift, Command, then the key). */
export function comboLabel(combo: string, mac: boolean): string {
  const parsed = parseCombo(combo);
  if (parsed === null) return combo;
  if (mac) {
    return `${parsed.alt ? "⌥" : ""}${parsed.shift ? "⇧" : ""}${parsed.mod ? "⌘" : ""}${keyText(parsed.key, true)}`;
  }
  const mods = [parsed.mod ? "Ctrl" : "", parsed.alt ? "Alt" : "", parsed.shift ? "Shift" : ""];
  return [...mods.filter((m) => m !== ""), keyText(parsed.key, false)].join("+");
}
