// The commands of the app, for the Commands scope of Search everywhere (Ctrl+Shift+P): every action the keyboard
// reaches, plus the actions that only live in a menu. Pure: the registry says what each command is called, which
// key runs it and when it applies; `app.run` does the work.
//
// To add a command: give it an id here (and a case in `app.run` when it is not a shortcut action), an English label
// `cmd.<id>` in every language table, and an `available` check when it does not apply everywhere. The test of this
// file fails for a shortcut action that has no entry.

import type { MessageKey } from "./i18n";
import { fuzzyMatch } from "./fuzzy";
import { SHORTCUT_ACTIONS, type ShortcutAction } from "./shortcuts";
import { foldChars } from "./text";

/** Actions that have no key: they live in a menu, the status bar or the settings. */
export type MenuAction =
  | "initProject"
  | "checkUpdates"
  | "newFolder"
  | "collapseAll"
  | "markAllRead"
  | "showOnlyUnread"
  | "markFolderRead"
  | "markUnread"
  | "showComments"
  | "revealInTree"
  | "baselineLastVisit"
  | "baselineCommit"
  | "openContainingFolder"
  | "agentOn"
  | "agentOff"
  | "agentCopySetup";

/** What `app.run` can do. */
export type CommandAction = ShortcutAction | MenuAction;

/** The state a command's `available` check reads. */
export type CommandContext = {
  /** A folder is open (IDE mode); false in Simple mode, where a single file is open. */
  ide: boolean;
  /** A document is open in the reading view or the editor. */
  hasDoc: boolean;
  editing: boolean;
  canBack: boolean;
  canForward: boolean;
  /** The open document has blocks marked as changed or removed. */
  hasChanges: boolean;
  /** The project's local state is kept in the project folder already. */
  stateInProject: boolean;
  /** The project is a git repository that can be read. */
  hasRepo: boolean;
  /** Some document is unread. */
  hasUnread: boolean;
  /** The agent connection is on for this project. */
  agentOn: boolean;
};

export type Command = {
  id: CommandAction;
  /** The English label is the key's value in `en.ts`; the other languages translate it. */
  label: MessageKey;
  /** The key that runs it, written with Windows key names ("Ctrl+Alt+O"); shown on the right of the row. */
  shortcut?: string;
  /** Whether the command applies right now. A command with no check applies everywhere. */
  available?: (ctx: CommandContext) => boolean;
};

const inFolder = (ctx: CommandContext): boolean => ctx.ide;
const withDoc = (ctx: CommandContext): boolean => ctx.hasDoc;
const reading = (ctx: CommandContext): boolean => ctx.hasDoc && !ctx.editing;
const canEdit = (ctx: CommandContext): boolean => ctx.hasDoc || ctx.editing;

/**
 * In the order a person would look for them when nothing is typed: opening and finding, views, editing, the open
 * document, then the project.
 */
export const COMMANDS: readonly Command[] = [
  { id: "openFolder", label: "cmd.openFolder", shortcut: "Ctrl+O" },
  { id: "openFile", label: "cmd.openFile", shortcut: "Ctrl+Shift+O" },
  { id: "quickOpen", label: "cmd.quickOpen", shortcut: "Ctrl+P", available: inFolder },
  { id: "searchAll", label: "cmd.searchAll", shortcut: "Ctrl+Shift+F", available: inFolder },
  { id: "nextUnread", label: "cmd.nextUnread", shortcut: "Alt+U", available: inFolder },
  { id: "back", label: "cmd.back", shortcut: "Alt+←", available: (ctx) => ctx.canBack },
  { id: "forward", label: "cmd.forward", shortcut: "Alt+→", available: (ctx) => ctx.canForward },

  { id: "explorer", label: "cmd.explorer", shortcut: "Ctrl+Shift+X", available: inFolder },
  { id: "tagsView", label: "cmd.tagsView", shortcut: "Ctrl+Shift+T", available: inFolder },
  { id: "toggleLeft", label: "cmd.toggleLeft", shortcut: "Ctrl+B", available: inFolder },
  { id: "toggleRight", label: "cmd.toggleRight", shortcut: "Ctrl+Alt+B", available: inFolder },
  { id: "toggleOutline", label: "cmd.toggleOutline", shortcut: "Ctrl+Alt+O" },
  { id: "showComments", label: "cmd.showComments", available: inFolder },
  { id: "toggleFollow", label: "cmd.toggleFollow", shortcut: "Ctrl+Shift+L", available: inFolder },
  { id: "zoomIn", label: "cmd.zoomIn", shortcut: "Ctrl+=" },
  { id: "zoomOut", label: "cmd.zoomOut", shortcut: "Ctrl+-" },
  { id: "zoomReset", label: "cmd.zoomReset", shortcut: "Ctrl+0" },
  { id: "cycleWidth", label: "cmd.cycleWidth", shortcut: "Ctrl+Alt+W" },
  { id: "settings", label: "cmd.settings", shortcut: "Ctrl+," },

  { id: "newFile", label: "cmd.newFile", shortcut: "Ctrl+N", available: inFolder },
  { id: "edit", label: "cmd.edit", shortcut: "E", available: reading },
  { id: "toggleEdit", label: "cmd.toggleEdit", shortcut: "Ctrl+E", available: canEdit },
  { id: "toggleEditMode", label: "cmd.toggleEditMode", shortcut: "Ctrl+Alt+E", available: canEdit },
  { id: "save", label: "cmd.save", shortcut: "Ctrl+S", available: (ctx) => ctx.editing },
  {
    id: "editTags",
    label: "cmd.editTags",
    shortcut: "T",
    available: (ctx) => ctx.ide && reading(ctx),
  },
  {
    id: "addComment",
    label: "cmd.addComment",
    shortcut: "N",
    available: (ctx) => ctx.ide && reading(ctx),
  },

  { id: "find", label: "cmd.find", shortcut: "Ctrl+F", available: reading },
  {
    id: "nextChange",
    label: "cmd.nextChange",
    shortcut: "F7",
    available: (ctx) => reading(ctx) && ctx.hasChanges,
  },
  {
    id: "prevChange",
    label: "cmd.prevChange",
    shortcut: "Shift+F7",
    available: (ctx) => reading(ctx) && ctx.hasChanges,
  },
  { id: "copyReference", label: "cmd.copyReference", shortcut: "Ctrl+Alt+C", available: withDoc },
  { id: "copyPath", label: "cmd.copyPath", shortcut: "Shift+Alt+C", available: withDoc },
  { id: "openExternal", label: "cmd.openExternal", shortcut: "Ctrl+Shift+E", available: withDoc },
  { id: "revealInTree", label: "cmd.revealInTree", available: (ctx) => ctx.ide && ctx.hasDoc },
  { id: "markUnread", label: "cmd.markUnread", available: (ctx) => ctx.ide && ctx.hasDoc },

  { id: "newFolder", label: "cmd.newFolder", available: inFolder },
  { id: "collapseAll", label: "cmd.collapseAll", available: inFolder },
  { id: "markAllRead", label: "cmd.markAllRead", available: (ctx) => ctx.ide && ctx.hasUnread },
  {
    id: "markFolderRead",
    label: "cmd.markFolderRead",
    available: (ctx) => ctx.ide && ctx.hasUnread,
  },
  { id: "showOnlyUnread", label: "cmd.showOnlyUnread", available: inFolder },
  {
    id: "baselineLastVisit",
    label: "cmd.baselineLastVisit",
    available: (ctx) => ctx.ide && ctx.hasRepo,
  },
  { id: "baselineCommit", label: "cmd.baselineCommit", available: (ctx) => ctx.ide && ctx.hasRepo },
  {
    id: "initProject",
    label: "cmd.initProject",
    available: (ctx) => ctx.ide && !ctx.stateInProject,
  },
  { id: "agentOn", label: "cmd.agentOn", available: (ctx) => ctx.ide && !ctx.agentOn },
  { id: "agentOff", label: "cmd.agentOff", available: (ctx) => ctx.ide && ctx.agentOn },
  { id: "agentCopySetup", label: "cmd.agentCopySetup", available: inFolder },
  { id: "openContainingFolder", label: "cmd.openContainingFolder", available: (ctx) => !ctx.ide },
  { id: "checkUpdates", label: "cmd.checkUpdates" },
];

/**
 * Shortcut actions that are not commands: the two that open Search everywhere itself. Running them from inside the
 * box would only open it again.
 */
export const NOT_COMMANDS: readonly ShortcutAction[] = ["searchEverywhere", "commands"];

/** Shortcut actions with no entry in the registry (empty when every one is accounted for). */
export function unregisteredActions(): ShortcutAction[] {
  const known = new Set<CommandAction>([...COMMANDS.map((c) => c.id), ...NOT_COMMANDS]);
  return SHORTCUT_ACTIONS.filter((action) => !known.has(action));
}

/** The commands that apply in this state, in registry order. */
export function availableCommands(ctx: CommandContext): Command[] {
  return COMMANDS.filter((command) => command.available?.(ctx) ?? true);
}

// ---- matching ----------------------------------------------------------------------------------------

export type CommandMatch = {
  command: Command;
  /** The label in the current language: what the row shows. */
  label: string;
  /** Positions of the matched characters of `label`; empty when the English label is what matched. */
  positions: number[];
  /** The English label, when it is why the command is listed and the label shown is in another language. */
  alias: string | null;
};

export type CommandLabels = {
  /** The label in the language of the interface. */
  current: (command: Command) => string;
  /** The English label. */
  english: (command: Command) => string;
};

type Scored = { score: number; positions: number[] };

const WORD_START = /[\s\-_.(]/;

/** Every word of the query inside the label: words that start a word of the label rank higher. */
function matchWords(words: readonly string[], label: string): Scored | null {
  const folded = foldChars(label);
  const positions = new Set<number>();
  let atStarts = true;
  let first = -1;
  for (const word of words) {
    let at = -1;
    for (let from = folded.indexOf(word); from >= 0; from = folded.indexOf(word, from + 1)) {
      if (from === 0 || WORD_START.test(folded[from - 1])) {
        at = from;
        break;
      }
    }
    if (at < 0) {
      atStarts = false;
      at = folded.indexOf(word);
    }
    if (at < 0) return null;
    if (first < 0) first = at;
    for (let i = 0; i < word.length; i++) positions.add(at + i);
  }
  const score = (atStarts ? 1000 : 800) - first * 2 - folded.length * 0.2;
  return { score, positions: [...positions].sort((a, b) => a - b) };
}

/** The label has the words of the query. */
function scoreWords(query: string, label: string): Scored | null {
  const words = foldChars(query.trim()).split(/\s+/).filter(Boolean);
  return words.length === 0 ? null : matchWords(words, label);
}

/** The label has the letters of the query in order (two or more of them: one letter would match nearly everything). */
function scoreLetters(query: string, label: string): Scored | null {
  const letters = foldChars(query.trim());
  if (letters.replace(/\s/g, "").length < 2) return null;
  const loose = fuzzyMatch(letters, label);
  return loose ? { score: Math.min(loose.score, 600), positions: loose.positions } : null;
}

/**
 * The commands whose label has the typed words (else the typed letters in order), best first; equal matches keep the
 * registry order. A label matches in the current language or in English, so the English names keep working after a
 * language switch. An empty query lists every command.
 */
export function matchCommands(
  query: string,
  commands: readonly Command[],
  labels: CommandLabels,
): CommandMatch[] {
  if (query.trim() === "") {
    return commands.map((command) => ({
      command,
      label: labels.current(command),
      positions: [],
      alias: null,
    }));
  }
  const search = (score: (query: string, label: string) => Scored | null) => {
    const found: (CommandMatch & { score: number })[] = [];
    for (const command of commands) {
      const label = labels.current(command);
      const english = labels.english(command);
      const own = score(query, label);
      const other = english === label ? null : score(query, english);
      if (!own && !other) continue;
      // the better of the two ranks it; the label shown is the current language's, with the English one beside it
      // only when that is the one that matched
      found.push({
        command,
        label,
        positions: own?.positions ?? [],
        alias: own ? null : english,
        score: Math.max(own?.score ?? -Infinity, other?.score ?? -Infinity),
      });
    }
    return found.sort((x, y) => y.score - x.score);
  };
  // the letters in order are only a way out when no label has the words
  const byWords = search(scoreWords);
  return byWords.length > 0 ? byWords : search(scoreLetters);
}
