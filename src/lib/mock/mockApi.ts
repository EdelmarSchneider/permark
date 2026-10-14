// The mock backend behind `mockApi` (VITE_MOCK=1). Loaded lazily by api.ts.

import type { Api } from "../api";
import type { SessionState } from "../types";
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

let savedSession: SessionState | null = {
  lastDoc: "docs/specs/live-feed.md",
  scrollTop: 0,
  zoom: 1,
  wide: false,
  refTemplate: "@{path}",
  leftPanel: { open: true, width: 260 },
  rightPanel: { open: true, width: 280 },
  theme: "system",
  follow: false,
};
let stateLocation: "appData" | "project" = "appData";
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
  revealInFolder: async (path) => {
    console.info("[mock] reveal in folder:", path);
  },
  search: async (query, limit = 50) => {
    await wait(60);
    return searchDocs(query, limit);
  },
  initProjectFolder: async () => {
    await wait(400);
    stateLocation = "project";
  },
  stateLocation: async () => stateLocation,
  on: async () => () => {},
};
