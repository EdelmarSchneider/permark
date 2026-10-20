// What the notes plugin answers in the mock backend (`dev:mock`, tests): a few notes in memory, the loose files, pins,
// versions, the journal. The same shapes the real plugin serves; titles are made the simple way (no Markdown parser).

import { MockPluginError } from "./mockOrganizing";
import type {
  Deleted,
  FolderInfo,
  LooseState,
  NoteEntry,
  NoteList,
  NotesIntent,
  PluginMethod,
  PluginMethods,
  Version,
} from "./types";

type NotesMethod = Extract<PluginMethod, `notes.${string}`>;
export type NotesHandlers = {
  [M in NotesMethod]: (args: PluginMethods[M]["args"]) => PluginMethods[M]["result"];
};

export const MOCK_NOTES_FOLDER = "C:/Users/demo/Documents/PerMark Notes";

type MockNote = { name: string; text: string; modifiedMs: number };
type MockLoose = {
  id: string;
  name: string;
  folder: string;
  text: string;
  modifiedMs: number;
  state: LooseState;
};

export type NotesProbe = {
  /** The notes as the backend holds them. */
  notes: Map<string, MockNote>;
  /** What the journal holds (path -> text). */
  journal: Map<string, string>;
  /** Folder status the next list reports. */
  folder(status: FolderInfo["status"]): void;
  /** A loose file in a state. */
  loose(entry: { name: string; folder: string; text: string; state: LooseState }): string;
  /** Marks a note as recovered after a crash, with the typed text. */
  recover(path: string, text: string): void;
  /** Calls the notes got, `notes.<method>`. */
  calls: string[];
};

function titleOf(text: string): string {
  for (const raw of text.split("\n")) {
    const line = raw
      .replace(/^[#>\-*\s]+/, "")
      .replace(/[*_`]/g, "")
      .trim();
    if (line !== "") return line.slice(0, 120);
  }
  return "";
}

function snippetOf(text: string): string {
  const lines = text
    .split("\n")
    .map((l) =>
      l
        .replace(/^[#>\-*\s]+/, "")
        .replace(/[*_`]/g, "")
        .trim(),
    )
    .filter((l) => l !== "");
  return lines.slice(1).join(" ").slice(0, 160);
}

export function createMockNotes(
  files: Map<string, string>,
  announceIntent: () => void = () => {},
): {
  handlers: NotesHandlers;
  probe: NotesProbe;
} {
  const now = Date.now();
  const hour = 3_600_000;
  const day = 24 * hour;
  const notes = new Map<string, MockNote>();
  /** What the Notes tab shows next (the plugin's one slot). */
  let intent: NotesIntent | null = null;
  const seed = (name: string, text: string, ago: number): void => {
    notes.set(name, { name, text, modifiedMs: now - ago });
    files.set(name, text);
  };
  seed(
    "2026-10-19 0907.md",
    "# Trip to São Paulo\n\nFlight at 7:40.\nHotel near Paulista.\n",
    20 * 60_000,
  );
  seed("2026-10-19 0815.md", "Call the bank about the card\n\nAsk for the new limit.\n", 2 * hour);
  seed(
    "2026-10-18 1855.md",
    "# Server IPs\n\nstaging 10.0.0.12\nproduction 10.0.0.40\n",
    day + hour,
  );
  seed(
    "2026-10-16 1031.md",
    "# Reading list\n\n- Designing Data-Intensive Applications\n- The Pragmatic Programmer\n",
    3 * day,
  );
  seed("2026-09-02 1700.md", "# Recipe: lentil soup\n\nOnion, carrot, lentils.\n", 47 * day);
  let pins = ["2026-10-18 1855.md"];
  const shared = new Set(["2026-10-16 1031.md"]);
  const copied = new Set(["2026-09-02 1700.md"]);
  const recovered = new Map<string, string>();
  const loose = new Map<string, MockLoose>();
  const versions = new Map<string, (Version & { text: string })[]>();
  const journal = new Map<string, string>();
  const deleted = new Map<number, { name: string; note: MockNote; pinned: boolean }[]>();
  const calls: string[] = [];
  let nextUndo = 1;
  let folderStatus: FolderInfo["status"] = { status: "ready" };
  let hex = 0x0a1b2c00;

  const folder = (): FolderInfo => ({
    path: MOCK_NOTES_FOLDER,
    status: folderStatus,
    isDefault: true,
  });
  const noteEntry = (n: MockNote): NoteEntry => ({
    path: n.name,
    kind: "note",
    name: n.name,
    title: titleOf(n.text),
    snippet: snippetOf(n.text),
    modifiedMs: n.modifiedMs,
    size: n.text.length,
    pinned: pins.includes(n.name),
    shared: shared.has(n.name),
    copied: copied.has(n.name),
    recovered: recovered.has(n.name),
    folder: null,
    state: null,
    pending: false,
  });
  const looseEntry = (l: MockLoose): NoteEntry => ({
    path: `@file/${l.id}/${l.name}`,
    kind: "file",
    name: l.name,
    title: l.name,
    snippet: "",
    modifiedMs: l.modifiedMs,
    size: l.text.length,
    pinned: pins.includes(`@file/${l.id}`),
    shared: false,
    copied: false,
    recovered: false,
    folder: l.folder,
    state: l.state,
    pending: l.state === "unsaved" || journal.has(`@file/${l.id}/${l.name}`),
  });
  const list = (): NoteList => {
    const all = [...notes.values()].map(noteEntry);
    const pinned = pins.flatMap((key) => {
      const note = all.find((e) => e.path === key);
      if (note) return [note];
      const file = [...loose.values()].find((l) => `@file/${l.id}` === key);
      return file ? [looseEntry(file)] : [];
    });
    const rest = all.filter((e) => !e.pinned).sort((a, b) => b.modifiedMs - a.modifiedMs);
    const files = [...loose.values()].map(looseEntry).filter((e) => !e.pinned);
    return { folder: folder(), entries: [...pinned, ...rest, ...files], open: null };
  };
  const need = (path: string): MockNote => {
    const note = notes.get(path);
    if (!note) throw new MockPluginError("notFound", path);
    return note;
  };
  const textOf = (path: string): string => files.get(path) ?? notes.get(path)?.text ?? "";
  const snapshot = (name: string, text: string): void => {
    const kept = versions.get(name) ?? [];
    if (kept[0]?.text === text) return;
    kept.unshift({
      id: String(Date.now() + kept.length),
      atMs: Date.now(),
      size: text.length,
      text,
    });
    versions.set(name, kept);
  };
  const remove = (names: string[]): Deleted => {
    const gone = names.map((name) => ({ name, note: need(name), pinned: pins.includes(name) }));
    for (const { name } of gone) {
      notes.delete(name);
      files.delete(name);
    }
    pins = pins.filter((p) => !names.includes(p));
    const undoId = nextUndo++;
    deleted.set(undoId, gone);
    return { undoId, names };
  };

  const handlers: NotesHandlers = {
    "notes.folder": () => folder(),
    "notes.createFolder": () => {
      folderStatus = { status: "ready" };
      return folder();
    },
    "notes.list": () => {
      // the notes follow what the editor saved
      for (const [name, note] of notes) {
        const text = files.get(name);
        if (text !== undefined && text !== note.text)
          notes.set(name, { ...note, text, modifiedMs: Date.now() });
      }
      return list();
    },
    "notes.summary": () => {
      const all = list().entries.filter((e) => e.kind === "note");
      return {
        pinned: all.filter((e) => e.pinned).slice(0, 5),
        latest: all.filter((e) => !e.pinned).slice(0, 5),
        count: all.length,
      };
    },
    "notes.create": () => {
      if (folderStatus.status === "missing")
        throw new MockPluginError("notFound", MOCK_NOTES_FOLDER);
      let name = "2026-10-19 1500.md";
      for (let n = 2; notes.has(name); n++) name = `2026-10-19 1500-${n}.md`;
      notes.set(name, { name, text: "", modifiedMs: Date.now() });
      files.set(name, "");
      return name;
    },
    "notes.delete": ({ paths }) => remove(paths),
    "notes.undoDelete": ({ undoId }) => {
      const gone = deleted.get(undoId);
      if (!gone) throw new MockPluginError("notFound", "undo");
      deleted.delete(undoId);
      return gone.map(({ name, note, pinned }) => {
        notes.set(name, note);
        files.set(name, note.text);
        if (pinned) pins.push(name);
        return name;
      });
    },
    "notes.rename": ({ path, name }) => {
      const note = need(path);
      const next = /\.[a-z]+$/i.test(name) ? name : `${name}.md`;
      if (/[\\/:*?"<>|]/.test(next)) throw new MockPluginError("invalidName", "chars");
      if (next !== path && notes.has(next)) throw new MockPluginError("exists", next);
      notes.delete(path);
      notes.set(next, { ...note, name: next });
      files.set(next, textOf(path));
      files.delete(path);
      pins = pins.map((p) => (p === path ? next : p));
      return next;
    },
    "notes.setPinned": ({ path, pinned }) => {
      const key = path.startsWith("@file/")
        ? path.split("/").slice(0, 2).join("/")
        : need(path).name;
      pins = pins.filter((p) => p !== key);
      if (pinned) pins.push(key);
      return null;
    },
    "notes.setPinOrder": ({ order }) => {
      const keys = order.filter((p) => pins.includes(p));
      pins = [...keys, ...pins.filter((p) => !keys.includes(p))];
      return null;
    },
    "notes.setShared": ({ path, shared: on }) => {
      need(path);
      if (on) shared.add(path);
      else shared.delete(path);
      return null;
    },
    "notes.markCopied": ({ path }) => {
      need(path);
      copied.add(path);
      return null;
    },
    "notes.deleteCopied": () =>
      remove([...copied].filter((n) => notes.has(n) && !pins.includes(n))),
    "notes.opened": () => null,
    "notes.versions": ({ path }) =>
      (versions.get(path) ?? []).map(({ id, atMs, size }) => ({ id, atMs, size })),
    "notes.versionText": ({ path, id }) => {
      const found = versions.get(path)?.find((v) => v.id === id);
      if (!found) throw new MockPluginError("notFound", id);
      return found.text;
    },
    "notes.restoreVersion": ({ path, id }) => {
      const found = versions.get(path)?.find((v) => v.id === id);
      if (!found) throw new MockPluginError("notFound", id);
      snapshot(path, textOf(path));
      files.set(path, found.text);
      notes.set(path, { ...need(path), text: found.text, modifiedMs: Date.now() });
      return null;
    },
    "notes.buffer": ({ path, text }) => {
      if (text === null) journal.delete(path);
      else journal.set(path, text);
      return null;
    },
    "notes.recoveredText": ({ path }) => recovered.get(path) ?? journal.get(path) ?? null,
    "notes.keepRecovered": ({ path }) => {
      const text = recovered.get(path);
      if (text === undefined) throw new MockPluginError("notFound", "recovered");
      snapshot(path, textOf(path));
      files.set(path, text);
      notes.set(path, { ...need(path), text, modifiedMs: Date.now() });
      recovered.delete(path);
      return null;
    },
    "notes.discardRecovered": ({ path }) => {
      recovered.delete(path);
      journal.delete(path);
      return null;
    },
    "notes.openFile": ({ path }) => {
      const name = path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
      const id = (hex++).toString(16).padStart(8, "0");
      const folderOf = path.replace(/[\\/][^\\/]*$/, "");
      loose.set(id, {
        id,
        name,
        folder: folderOf,
        text: "",
        modifiedMs: Date.now(),
        state: "clean",
      });
      files.set(`@file/${id}/${name}`, "# From a file\n");
      return `@file/${id}/${name}`;
    },
    "notes.removeFile": ({ path }) => {
      loose.delete(path.split("/")[1] ?? "");
      return null;
    },
    "notes.locateFile": ({ path, newPath }) => {
      const id = path.split("/")[1] ?? "";
      const entry = loose.get(id);
      if (!entry) throw new MockPluginError("notFound", path);
      const name = newPath.split(/[\\/]/).filter(Boolean).at(-1) ?? newPath;
      loose.set(id, { ...entry, name, state: "clean" });
      return `@file/${id}/${name}`;
    },
    "notes.reloadFile": ({ path }) => {
      const entry = loose.get(path.split("/")[1] ?? "");
      if (entry) loose.set(entry.id, { ...entry, state: "clean" });
      journal.delete(path);
      return null;
    },
    "notes.keepMine": ({ path }) => {
      const entry = loose.get(path.split("/")[1] ?? "");
      if (entry) loose.set(entry.id, { ...entry, state: "clean" });
      journal.delete(path);
      return null;
    },
    "notes.saveAsNote": ({ path }) => {
      const text = journal.get(path) ?? "";
      const name = `${(loose.get(path.split("/")[1] ?? "")?.name ?? "file").replace(/\.[^.]*$/, "")} (my edits).md`;
      notes.set(name, { name, text, modifiedMs: Date.now() });
      files.set(name, text);
      journal.delete(path);
      return name;
    },
    "notes.suggestName": ({ path }) =>
      `${
        titleOf(textOf(path))
          .replace(/[\\/:*?"<>|]/g, " ")
          .trim() || "note"
      }.md`,
    "notes.moveToProject": ({ path, folder: dest, name }) => {
      const note = need(path);
      if (dest === MOCK_NOTES_FOLDER) throw new MockPluginError("intoItself", dest);
      if (/[\\/:*?"<>|]/.test(name)) throw new MockPluginError("invalidName", "chars");
      const gone = remove([path]);
      void note;
      return { destination: `${dest}/${name}`, undoId: gone.undoId, kept: false };
    },
    "notes.want": ({ intent: wish }) => {
      intent = wish;
      announceIntent();
      return null;
    },
    "notes.takeIntent": () => {
      const taken = intent;
      intent = null;
      return taken;
    },
    "notes.undoMove": ({ undoId }) => {
      const gone = deleted.get(undoId);
      if (!gone) throw new MockPluginError("notFound", "undo");
      const back = handlers["notes.undoDelete"]({ undoId });
      return { note: back[0] ?? "", removed: true };
    },
  };

  return {
    handlers: new Proxy(handlers, {
      get: (target, key: string) => {
        const fn = target[key as NotesMethod];
        if (typeof fn !== "function") return fn;
        return (args: never) => {
          calls.push(key);
          return fn(args);
        };
      },
    }),
    probe: {
      notes,
      journal,
      calls,
      folder: (status) => {
        folderStatus = status;
      },
      loose: ({ name, folder: place, text, state }) => {
        const id = (hex++).toString(16).padStart(8, "0");
        loose.set(id, { id, name, folder: place, text, modifiedMs: now - hour, state });
        files.set(`@file/${id}/${name}`, text);
        return `@file/${id}/${name}`;
      },
      recover: (path, text) => {
        recovered.set(path, text);
      },
    },
  };
}
