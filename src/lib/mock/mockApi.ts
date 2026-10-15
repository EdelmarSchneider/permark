// The mock backend behind `mockApi` (VITE_MOCK=1). Loaded lazily by api.ts.

import type { Api } from "../api";
import { parsePreference, type LanguagePreference } from "../i18n";
import type { SessionState, ShellState } from "../types";
import {
  cleanUp,
  fileMeta,
  filesWithTags,
  listTags,
  metaIndex,
  setDescription,
  setTags,
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
const LANGUAGE_KEY = "trailmark.mock.language";

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
  initialPath: async () =>
    typeof location === "undefined" ? null : new URLSearchParams(location.search).get("open"),
  recentVaults: async () => mockRecents,
  appLanguage: async () => mockLanguage(),
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
    return getDocument(path);
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
          reason: "template",
          detail: `unknown placeholder {${match[1]}}`,
          message: `the editor command is not valid: unknown placeholder {${match[1]}}`,
        };
      }
    }
    if ((command.match(/"/g)?.length ?? 0) % 2 === 1) {
      throw {
        kind: "editor",
        reason: "template",
        detail: "a quote is not closed",
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
  cleanUpMeta: async () => cleanUp(),
  documentSource: (path) => documentSource(path),
  parseBlocks: (text) => parseBlocks(text),
  renderFragments: (texts) => renderFragments(texts),
  saveDocument: (path, text, expectedHash) => saveDocument(path, text, expectedHash),
  createDocument: (path, text) => createDocument(path, text),
  writeAsset: (docPath, nameHint, data) => writeAsset(docPath, nameHint, data),
  previewMove: (moves, scanLinks, heldBack) => previewMove(moves, scanLinks, heldBack),
  applyMove: (planId, updateLinks, buffers) => applyMove(planId, updateLinks, buffers),
  deleteEntries: (paths) => deleteEntries(paths),
  newFolder: (path) => newFolder(path),
  undoFileOp: (opId) => undoFileOp(opId),
  getUnread: async () => listUnread(),
  markRead: async (path) => markRead(path),
  markUnread: async (path) => markUnread(path),
  markAllRead: async () => markAllRead(),
  markFolderRead: async (path) => markFolderRead(path),
  on: async () => () => {},
};
