import { readFileSync } from "node:fs";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => null) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { api, tauriApi, type Api } from "./api";
import type { SessionState } from "./types";

// The boundary contract is shared with the Rust test in src-tauri/src/commands.rs.
const contract = JSON.parse(readFileSync("src-tauri/ipc-contract.json", "utf8")) as {
  commands: Record<string, string[]>;
  events: string[];
};

const camel = (snake: string): string => snake.replace(/_(\w)/g, (_, c: string) => c.toUpperCase());

/** One call per `Api` method; the type makes a new method fail to compile until it is listed here. */
const calls: Record<Exclude<keyof Api, "on">, unknown[]> = {
  openVault: ["/project"],
  initialPath: [],
  recentVaults: [],
  appLanguage: [],
  setAppLanguage: ["de"],
  shellIntegrationState: [],
  setShellContextMenu: [true, "de"],
  setShellFileAssociation: [false, "pt-BR"],
  refreshShellLabels: ["es"],
  openDefaultApps: [],
  listTree: [true, false],
  readDocument: ["docs/a.md"],
  resolveLink: ["docs/a.md", "b.md"],
  gitSnapshot: [],
  setGitBaseline: ["head"],
  sinceLastVisit: [],
  saveSession: [{} as SessionState],
  loadSession: [],
  absolutePath: ["docs/a.md"],
  openExternal: ["docs/a.md"],
  openInEditor: ["src/main.rs", 42, 7],
  editorCommand: [],
  setEditorCommand: ["code -g {path}:{line}:{col}"],
  revealInFolder: ["docs"],
  search: ["query", 5],
  referencedImages: [],
  initProjectFolder: [],
  stateLocation: [],
  fileMeta: ["docs/a.md"],
  setFileTags: ["docs/a.md", ["spec"]],
  setFileDescription: ["docs/a.md", "A description"],
  listTags: [],
  filesWithTags: [["spec"], false],
  metaIndex: [],
  cleanUpMeta: [],
  documentSource: ["docs/a.md"],
  parseBlocks: ["# Title"],
  renderFragments: [["# Title"]],
  saveDocument: ["docs/a.md", "# Title", "00ff00ff00ff00ff"],
  createDocument: ["docs/new.md", "# New"],
  writeAsset: ["docs/a.md", "shot.png", "aGVsbG8="],
  previewMove: [[{ from: "docs/a.md", to: "docs/b.md" }], true, ["docs/c.md"]],
  applyMove: [7, true, [{ path: "docs/a.md", text: "# A" }]],
  deleteEntries: [["docs/a.md"]],
  newFolder: ["docs/new"],
  undoFileOp: [3],
  getUnread: [],
  markRead: ["docs/a.md"],
  markUnread: ["docs/a.md"],
  markAllRead: [],
  markFolderRead: ["docs"],
  setAgentConnection: [true],
  agentStatus: [],
  publishAgentContext: [
    {
      document: "docs/a.md",
      mode: "reading",
      section: { headingPath: ["A"], startLine: 1, endLine: 4 },
      selection: null,
      unsaved: false,
    },
  ],
  takeAgentOpen: [],
  addComment: [
    {
      path: "docs/a.md",
      quote: "a passage",
      text: "Too vague.",
      headingPath: ["A"],
      line: 3,
      endLine: 3,
    },
  ],
  listComments: [],
  setCommentDone: ["c1a2b3c4d", true],
  deleteComment: ["c1a2b3c4d"],
};

describe("tauriApi", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockClear();
    vi.mocked(listen).mockClear();
  });

  it("is the API the app uses unless built with VITE_MOCK=1", () => {
    expect(api).toBe(tauriApi);
  });

  it("invokes exactly the commands of the contract, with the argument names Rust expects", async () => {
    const sent = new Map<string, string[]>();
    for (const [method, args] of Object.entries(calls)) {
      vi.mocked(invoke).mockClear();
      await (tauriApi[method as keyof typeof calls] as (...a: unknown[]) => Promise<unknown>)(
        ...args,
      );
      expect(invoke, method).toHaveBeenCalledTimes(1);
      const [command, payload] = vi.mocked(invoke).mock.calls[0];
      sent.set(command, Object.keys(payload ?? {}).sort());
    }

    const expected = new Map(
      Object.entries(contract.commands).map(([command, rustArgs]) => [
        command,
        rustArgs.map(camel).sort(),
      ]),
    );
    expect(Object.fromEntries([...sent].sort())).toEqual(Object.fromEntries([...expected].sort()));
  });

  it("subscribes to the event name it is given and hands over the payload", async () => {
    const handler = vi.fn();
    await tauriApi.on("fs-change", handler);
    expect(listen).toHaveBeenCalledWith("fs-change", expect.any(Function));
    const forward = vi.mocked(listen).mock.calls[0][1] as (e: { payload: unknown }) => void;
    forward({ payload: { changes: [], bulk: null, rescan: false } });
    expect(handler).toHaveBeenCalledWith({ changes: [], bulk: null, rescan: false });
  });

  it("declares the events of the contract, and only those", () => {
    const source = readFileSync("src/lib/api.ts", "utf8");
    const block = /export type EventMap = \{(.*?)\n\};/s.exec(source)?.[1] ?? "";
    const declared = [...block.matchAll(/"([a-z-]+)":/g)].map((m) => m[1]);
    expect(declared.sort()).toEqual([...contract.events].sort());
  });
});
