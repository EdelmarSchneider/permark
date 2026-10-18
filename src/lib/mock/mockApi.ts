// The mock backend behind `mockApi` (VITE_MOCK=1). Loaded lazily by api.ts.

import type { Api } from "../api";
import { parsePreference, type LanguagePreference } from "../i18n";
import { normalizeTone } from "../tone";
import type { SessionState, ShellState } from "../types";
import {
  cleanUp,
  fileMeta,
  filesWithTags,
  listTags,
  metaIndex,
  setDescription,
  setTags,
  setTagStyle,
  tagStyles,
} from "./mockMeta";
import {
  MOCK_ROOT,
  getDocument,
  mockGit,
  mockIdeVault,
  mockRecents,
  mockSimpleVault,
  mockSince,
  mockSnapshot,
  mockState,
  mockTree,
  resolveMockLink,
  searchDocs,
} from "./fixtures";
import { applyMove, deleteEntries, newFolder, previewMove, undoFileOp } from "./mockFileOps";
import { agentStatus, publishAgentContext, setAgentConnection, takeAgentOpen } from "./mockAgent";
import { addComment, deleteComment, listComments, setCommentDone } from "./mockComments";
import * as mockNotes from "./mockNotes";
import { reviewGet, reviewList } from "./mockReview";
import { listUnread, markAllRead, markFolderRead, markRead, markUnread } from "./mockUnread";
import {
  createDocument,
  documentSource,
  parseBlocks,
  renderFragments,
  saveDocument,
  writeAsset,
} from "./mockSource";

let savedSession: SessionState | null = {
  lastDoc: "docs/specs/live-feed.md",
  scrollTop: 0,
  zoom: 1,
  width: "comfortable",
  refTemplate: "@{path}",
  leftPanel: { open: true, width: 260 },
  rightPanel: { open: true, width: 280 },
  theme: "system",
  follow: false,
  treeFilter: null,
  autoReveal: true,
  rightTab: "changes",
  linkUpdate: "ask",
  editMode: "visual",
  initHintNever: false,
};
let stateLocation: "appData" | "project" = "appData";
let mockEditorCommand = "";
/** The file manager entries of the mock "system": on, so the Settings section shows in the browser. */
const mockShell: ShellState = {
  supported: true,
  contextMenu: true,
  association: false,
  isDefault: false,
};
const LANGUAGE_KEY = "permark.mock.language";
const TONE_KEY = "permark.mock.tone";

/** `?lang=de` picks the language for a run; otherwise the one chosen in Settings last time. */
function mockLanguage(): LanguagePreference {
  let stored: string | null = null;
  try {
    stored = localStorage.getItem(LANGUAGE_KEY);
  } catch {
    stored = null;
  }
  const asked =
    typeof location === "undefined" ? null : new URLSearchParams(location.search).get("lang");
  return parsePreference(asked ?? stored);
}

const wait = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

export const mockImpl: Api = {
  openVault: async (path) => {
    await wait(250);
    if (/missing|old\/notes/i.test(path)) {
      throw new Error(`the folder was not found`);
    }
    const simple = /\.(md|markdown|txt|json)$/i.test(path);
    mockState.vault = simple ? "simple" : "ide";
    return simple ? mockSimpleVault : mockIdeVault;
  },
  closeVault: async () => {},
  initialPath: async () =>
    typeof location === "undefined" ? null : new URLSearchParams(location.search).get("open"),
  recentVaults: async () => mockRecents,
  appLanguage: async () => mockLanguage(),
  // `?store` shows the Microsoft Store build of the interface in the browser mock.
  appDistribution: async () =>
    typeof location !== "undefined" && new URLSearchParams(location.search).has("store")
      ? "store"
      : "standard",
  // `?tone=paper` picks the tone for a run; otherwise the one chosen in Settings last time
  appTone: async () => {
    let stored: string | null = null;
    try {
      stored = localStorage.getItem(TONE_KEY);
    } catch {
      stored = null;
    }
    const asked =
      typeof location === "undefined" ? null : new URLSearchParams(location.search).get("tone");
    return normalizeTone(asked ?? stored);
  },
  setAppTone: async (tone) => {
    try {
      localStorage.setItem(TONE_KEY, tone);
    } catch {
      // the browser mock forgets it on reload
    }
  },
  setAppLanguage: async (language) => {
    try {
      localStorage.setItem(LANGUAGE_KEY, language);
    } catch {
      // the browser mock forgets it on reload
    }
  },
  shellIntegrationState: async () => ({ ...mockShell }),
  setShellContextMenu: async (enabled, locale) => {
    console.info("[mock] right-click menu entry:", enabled, locale);
    mockShell.contextMenu = enabled;
    return { ...mockShell };
  },
  setShellFileAssociation: async (enabled, locale) => {
    console.info("[mock] file association:", enabled, locale);
    mockShell.association = enabled;
    if (!enabled) mockShell.isDefault = false;
    return { ...mockShell };
  },
  refreshShellLabels: async (locale) => {
    console.info("[mock] reword the file manager entries:", locale);
  },
  openDefaultApps: async () => {
    console.info("[mock] open the default apps settings");
  },
  listTree: async () => mockTree(),
  readDocument: async (path) => {
    await wait(40);
    const view = getDocument(path);
    mockNotes.seenLoose(path);
    return view;
  },
  resolveLink: async (from, href) => resolveMockLink(from, href),
  gitSnapshot: async () => mockSnapshot(),
  setGitBaseline: async (kind) => {
    mockGit.baseline = kind;
    mockGit.generation += 1;
    return mockSnapshot();
  },
  sinceLastVisit: async () => mockSince(),
  saveSession: async (s) => {
    savedSession = s;
  },
  loadSession: async () => (mockState.vault === "simple" ? null : savedSession),
  absolutePath: async (path) => `${MOCK_ROOT}/${path}`,
  openExternal: async (path) => {
    console.info("[mock] open externally:", path);
  },
  // Never starts anything: it says what the real command would do. The programs and scripts it refuses are the ones
  // the backend refuses (`media.rs`), so the message can be seen here.
  openWithDefaultApp: async (path) => {
    const ext = path.split(".").pop()?.toLowerCase() ?? "";
    if (["exe", "bat", "cmd", "ps1", "sh", "msi", "js", "vbs", "lnk", "jar"].includes(ext)) {
      throw {
        kind: "blockedType",
        subject: `.${ext}`,
        message: `.${ext} files can run code (programs, scripts, installers, shortcuts, documents with macros) and are not opened from here`,
      };
    }
    console.info("[mock] open with the default app:", path);
  },
  openInBrowser: async (path) => {
    console.info("[mock] open in the browser:", path);
  },
  openInEditor: async (path, line, col) => {
    const program = mockEditorCommand.trim().split(/\s+/)[0] ?? "";
    // a program that is not "installed" shows how the UI words the failure
    if (program === "missing-editor") {
      throw {
        kind: "editor",
        reason: "notFound",
        subject: program,
        message: `can't run '${program}': not found`,
      };
    }
    console.info(
      "[mock] open in editor:",
      mockEditorCommand || "(system default)",
      path,
      line,
      col,
    );
  },
  editorCommand: async () => mockEditorCommand,
  setEditorCommand: async (command) => {
    // the backend refuses a command it can't read; the same two mistakes are caught here
    const unknown = /\{(\w+)\}/g;
    for (const match of command.matchAll(unknown)) {
      if (!["path", "line", "col"].includes(match[1])) {
        throw {
          kind: "editor",
          reason: "unknownPlaceholder",
          subject: match[1],
          message: `the editor command is not valid: unknown placeholder {${match[1]}}`,
        };
      }
    }
    if ((command.match(/"/g)?.length ?? 0) % 2 === 1) {
      throw {
        kind: "editor",
        reason: "unclosedQuote",
        message: "the editor command is not valid: a quote is not closed",
      };
    }
    mockEditorCommand = command.trim();
  },
  revealInFolder: async (path) => {
    console.info("[mock] reveal in folder:", path);
  },
  search: async (query, limit = 50) => {
    await wait(60);
    return searchDocs(query, limit);
  },
  // `docs/design/wireframes.md` embeds it from another folder
  referencedImages: async () => ["docs/images/pipeline.svg"],
  seedProject: async () => true,
  openProjectWindow: async () => {},
  initProjectFolder: async () => {
    await wait(400);
    stateLocation = "project";
  },
  stateLocation: async () => stateLocation,
  fileMeta: async (path) => fileMeta(path),
  setFileTags: async (path, tags) => setTags(path, tags),
  setFileDescription: async (path, description) => setDescription(path, description),
  listTags: async () => listTags(),
  filesWithTags: async (tags, untagged) => filesWithTags(tags, untagged),
  metaIndex: async () => metaIndex(),
  tagStyles: async () => tagStyles(),
  setTagStyle: async (tag, color, icon) => setTagStyle(tag, color, icon),
  cleanUpMeta: async () => cleanUp(),
  documentSource: async (path) => {
    const source = await documentSource(path);
    mockNotes.seenLoose(path);
    return source;
  },
  parseBlocks: (text) => parseBlocks(text),
  renderFragments: (texts) => renderFragments(texts),
  saveDocument: async (path, text, expectedHash) => {
    const outcome = await saveDocument(path, text, expectedHash);
    if (outcome.result === "saved") mockNotes.savedLoose(path);
    return outcome;
  },
  createDocument: async (path, text) =>
    path.startsWith("@file/") ? mockNotes.recreateLoose(path, text) : createDocument(path, text),
  writeAsset: (docPath, nameHint, data) => writeAsset(docPath, nameHint, data),
  previewMove: (moves, scanLinks, heldBack) => previewMove(moves, scanLinks, heldBack),
  brokenLinks: async () => ({ links: [], total: 0, truncated: false, skipped: 0 }),
  repairLink: async () => ({ status: "stale", opId: null, buffer: null }),
  applyMove: (planId, updateLinks, buffers) => applyMove(planId, updateLinks, buffers),
  deleteEntries: (paths) => deleteEntries(paths),
  newFolder: (path) => newFolder(path),
  undoFileOp: (opId) => undoFileOp(opId),
  getUnread: async () => listUnread(),
  markRead: async (path) => markRead(path),
  markUnread: async (path) => markUnread(path),
  markAllRead: async () => markAllRead(),
  markFolderRead: async (path) => markFolderRead(path),
  reviewList: async () => reviewList(),
  reviewGet: async (path) => reviewGet(path),
  reviewKeep: async (path) => reviewGet(path),
  reviewRevert: async (path) => reviewGet(path),
  reviewKeepAll: async (path) => reviewGet(path),
  reviewRevertAll: async (path) => reviewGet(path),
  reviewRestoreDeleted: async (path) => reviewGet(path),
  reviewUndo: async (path) => reviewGet(path),
  reviewRelease: async () => {},
  setAgentConnection: async (enabled) => setAgentConnection(enabled),
  agentStatus: async () => agentStatus(),
  publishAgentContext: async (context) => publishAgentContext(context),
  takeAgentOpen: async () => takeAgentOpen(),
  addComment: async (comment) => addComment(comment),
  listComments: async () => listComments(),
  setCommentDone: async (id, done) => setCommentDone(id, done),
  deleteComment: async (id) => deleteComment(id),
  launchMode: async () => mockNotes.launchMode(),
  notesFolder: async () => mockNotes.notesFolder(),
  setNotesFolder: async (path) => mockNotes.setNotesFolder(path),
  createNotesFolder: async () => mockNotes.createNotesFolder(),
  notesStartIn: async () => mockNotes.notesStartIn(),
  setNotesStartIn: async (startIn) => mockNotes.setNotesStartIn(startIn),
  notesOverview: async () => mockNotes.notesOverview(),
  openNotes: async () => {
    await wait(120);
    return mockNotes.openNotes();
  },
  listNotes: async () => mockNotes.listNotes(),
  newNote: async () => mockNotes.newNote(),
  saveNote: async (path, text, expectedHash) => {
    await wait(80);
    return mockNotes.saveNote(path, text, expectedHash);
  },
  noteOpened: async (path) => mockNotes.noteOpened(path),
  closeNote: async () => mockNotes.closeNote(),
  setNoteView: async (path, view) => mockNotes.setNoteView(path, view),
  setNotePinned: async (path, pinned) => mockNotes.setNotePinned(path, pinned),
  setPinOrder: async (order) => mockNotes.setPinOrder(order),
  renameNote: async (path, name) => mockNotes.renameNote(path, name),
  deleteNotes: async (paths) => mockNotes.deleteNotes(paths),
  undoDeleteNotes: async (undoId) => mockNotes.undoDeleteNotes(undoId),
  deleteCopiedNotes: async () => mockNotes.deleteCopiedNotes(),
  markNoteCopied: async (path) => mockNotes.markNoteCopied(path),
  noteVersions: async (path) => mockNotes.noteVersions(path),
  noteVersionText: async (path, id) => mockNotes.noteVersionText(path, id),
  restoreNoteVersion: async (path, id) => mockNotes.restoreNoteVersion(path, id),
  openNotesFolder: async () => {
    console.info("[mock] open the notes folder");
  },
  setNoteShared: async (path, shared) => mockNotes.setNoteShared(path, shared),
  moveSuggestion: async (path) => mockNotes.moveSuggestion(path),
  moveNoteToProject: async (path, destination) => mockNotes.moveNoteToProject(path, destination),
  undoMoveNote: async (undoId) => mockNotes.undoMoveNote(undoId),
  openInProject: async (path) => mockNotes.openInProject(path),
  launchShow: async () => mockNotes.launchShow(),
  classifyPath: async (path) => mockNotes.classifyPath(path),
  openLooseFile: async (path) => mockNotes.openLooseFile(path),
  looseStatus: async () => mockNotes.looseStatus(),
  removeLooseFile: async (path) => mockNotes.removeLooseFile(path),
  locateLooseFile: async (path, newPath) => mockNotes.locateLooseFile(path, newPath),
  savePending: async (path, text, baseHash) => mockNotes.savePending(path, text, baseHash),
  loadPending: async (path) => mockNotes.loadPending(path),
  clearPending: async (path) => mockNotes.clearPending(path),
  keepEditsAsNote: async (path, text) => mockNotes.keepEditsAsNote(path, text),
  showNotes: async (newNote, open, openFile = null) => mockNotes.showNotes(newNote, open, openFile),
  becomeNotesWindow: async (newNote, open, openFile = null) =>
    mockNotes.becomeNotesWindow(newNote, open, openFile),
  takeNotesRequest: async () => mockNotes.takeNotesRequest(),
  on: async () => () => {},
};
