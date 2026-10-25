// The stores behind the Search everywhere box and the Search panel, against a fake backend.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  Api,
  FileHit,
  IndexProgress,
  PathResults,
  Recent,
  Result,
  SectionHit,
  SectionResults,
} from "../../api";
import { NO_REQUIREMENT } from "../conditions/available";
import type { CommandEntry } from "../commands/commands";
import { DEBOUNCE_MS, FinderStore, type FinderHost } from "./finderStore.svelte";
import { PANEL_DEBOUNCE_MS, SearchPanelStore } from "./panel.svelte";

const ok = <T>(value: T): Result<T> => ({ ok: true, value });
const hit = (path: string): FileHit => ({
  path,
  category: "markdown",
  positions: [0],
  hidden: path.startsWith("."),
});
const section = (path: string, line: number): SectionHit => ({
  path,
  headingPath: ["H"],
  anchor: "h",
  line,
  snippet: [{ text: "t", mark: false }],
  score: 1,
});

type Fake = {
  searchFiles: ReturnType<typeof vi.fn>;
  searchSections: ReturnType<typeof vi.fn>;
  searchRecent: ReturnType<typeof vi.fn>;
  searchStatus: ReturnType<typeof vi.fn>;
  on: ReturnType<typeof vi.fn>;
};

function fakeApi(): { api: Api; fake: Fake; emit(p: IndexProgress): void } {
  let listener: ((p: IndexProgress) => void) | null = null;
  const fake: Fake = {
    searchFiles: vi.fn(async (): Promise<Result<PathResults>> =>
      ok({ files: [hit("docs/a.md")], folders: [], partial: false }),
    ),
    searchSections: vi.fn(async (): Promise<Result<SectionResults>> =>
      ok({ hits: [section("a.md", 3), section("a.md", 9), section("b.md", 1)], partial: false }),
    ),
    searchRecent: vi.fn(async (): Promise<Result<Recent>> =>
      ok({ opened: [hit("o.md")], changed: [hit("o.md"), hit("c.md")] }),
    ),
    searchStatus: vi.fn(async () =>
      ok({ phase: "building" as const, done: 5, total: 50, files: 60, sections: 0 }),
    ),
    on: vi.fn(async (_event: string, handler: (p: IndexProgress) => void) => {
      listener = handler;
      return () => {
        listener = null;
      };
    }),
  };
  return { api: fake as unknown as Api, fake, emit: (p) => listener?.(p) };
}

function command(id: string, available: boolean): CommandEntry {
  return {
    command: {
      kind: "command",
      owner: "core",
      id,
      label: "cmd.save",
      run: vi.fn(),
      requires: NO_REQUIREMENT,
    },
    combos: [],
    availability: available
      ? { status: "available", degraded: [] }
      : {
          status: "unavailable",
          reason: { need: "permark", remedy: "project.init" },
        },
  };
}

function host(
  api: Api,
  extra: Partial<FinderHost> = {},
): FinderHost & Record<string, ReturnType<typeof vi.fn>> {
  return {
    api,
    singleFile: false,
    commands: () => [command("edit.save", true), command("git.log", false)],
    label: () => "Save",
    outline: () => [],
    opened: () => ["o.md"],
    places: () => [],
    goToPlace: vi.fn(),
    showsHidden: () => false,
    openFile: vi.fn(),
    reveal: vi.fn(),
    openSection: vi.fn(),
    runCommand: vi.fn(),
    close: vi.fn(),
    ...extra,
  } as FinderHost & Record<string, ReturnType<typeof vi.fn>>;
}

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

async function settle(): Promise<void> {
  await vi.advanceTimersByTimeAsync(0);
}

describe("the Search everywhere box", () => {
  it("asks for the recent files at once when it opens", async () => {
    const { api, fake } = fakeApi();
    const finder = new FinderStore("quick", host(api));
    await settle();
    expect(fake.searchRecent).toHaveBeenCalledWith(["o.md"], 16, {
      includeSilenced: false,
      tags: [],
      hiddenLast: false,
    });
    expect(finder.view.groups.map((g) => g.title)).toEqual(["opened", "changed"]);
    expect(finder.load).toEqual({ status: "ready", partial: false });
  });

  it("asks for hidden files after the others unless the tree shows hidden files", async () => {
    const { api, fake } = fakeApi();
    const quiet = new FinderStore("quick", host(api));
    await settle();
    quiet.setQuery("env");
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    expect(fake.searchFiles.mock.calls.at(-1)?.[3]).toMatchObject({ hiddenLast: true });
    const shown = new FinderStore("quick", host(api, { showsHidden: () => true }));
    await settle();
    shown.setQuery("env");
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    expect(fake.searchFiles.mock.calls.at(-1)?.[3]).toMatchObject({ hiddenLast: false });
  });

  it("waits for a pause in the typing and asks once", async () => {
    const { api, fake } = fakeApi();
    const finder = new FinderStore("quick", host(api));
    await settle();
    finder.setQuery("p");
    finder.setQuery("pl");
    finder.setQuery("pla");
    expect(fake.searchFiles).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    expect(fake.searchFiles).toHaveBeenCalledTimes(1);
    expect(fake.searchFiles).toHaveBeenCalledWith("pla", 50, 0, {
      includeSilenced: true,
      tags: [],
      hiddenLast: true,
    });
    expect(finder.view.rows).toHaveLength(1);
  });

  it("drops an answer that arrives after a newer question", async () => {
    const { api, fake } = fakeApi();
    const finder = new FinderStore("everywhere", host(api));
    await settle();
    let release: (r: Result<PathResults>) => void = () => {};
    fake.searchFiles.mockImplementationOnce(
      () =>
        new Promise<Result<PathResults>>((resolve) => {
          release = resolve;
        }),
    );
    finder.setQuery("slow");
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    finder.setQuery("fast");
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    release(ok({ files: [hit("stale.md")], folders: [], partial: false }));
    await settle();
    const paths = finder.view.rows.map((r) => (r.type === "file" ? r.path : ""));
    expect(paths).toContain("docs/a.md");
    expect(paths).not.toContain("stale.md");
  });

  it("a `>` turns it into the palette without asking the backend", async () => {
    const { api, fake } = fakeApi();
    const finder = new FinderStore("everywhere", host(api));
    await settle();
    fake.searchRecent.mockClear();
    finder.setQuery(">sav");
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS * 2);
    expect(finder.scope).toBe("commands");
    expect(finder.query).toBe("sav");
    expect(fake.searchFiles).not.toHaveBeenCalled();
    expect(fake.searchSections).not.toHaveBeenCalled();
    expect(finder.view.rows.length).toBeGreaterThan(0);
  });

  it("Tab moves to the next scope and loads what it needs", async () => {
    const { api, fake } = fakeApi();
    const finder = new FinderStore("everywhere", host(api));
    await settle();
    finder.setQuery("plan");
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    expect(fake.searchSections).toHaveBeenCalledTimes(1);
    finder.cycle(1);
    expect(finder.scope).toBe("files");
    finder.cycle(1);
    await settle();
    expect(finder.scope).toBe("sections");
    expect(finder.view.rows.every((r) => r.type === "section")).toBe(true);
  });

  it("a single-file window asks the backend for nothing", async () => {
    const { api, fake } = fakeApi();
    const finder = new FinderStore("everywhere", host(api, { singleFile: true }));
    finder.setQuery("x");
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS * 2);
    expect(fake.searchFiles).not.toHaveBeenCalled();
    expect(fake.searchSections).not.toHaveBeenCalled();
    expect(finder.scopes).toEqual(["sections", "commands"]);
  });

  it("opens, reveals, jumps and runs from the row, and closes", async () => {
    const { api } = fakeApi();
    const h = host(api);
    const finder = new FinderStore("everywhere", h);
    finder.setQuery("a");
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    const rows = finder.view.rows;
    const fileRow = rows.find((r) => r.type === "file");
    const sectionRow = rows.find((r) => r.type === "section");
    const commandRow = rows.find((r) => r.type === "command" && r.entry.command.id === "edit.save");
    finder.activate(fileRow ?? null);
    expect(h["openFile"]).toHaveBeenCalledWith("docs/a.md");
    expect(h["close"]).toHaveBeenCalledTimes(1);
    finder.activate(fileRow ?? null, true);
    expect(h["reveal"]).toHaveBeenCalledWith("docs/a.md");
    finder.activate(sectionRow ?? null);
    expect(h["openSection"]).toHaveBeenCalledTimes(1);
    finder.activate(commandRow ?? null);
    expect(h["runCommand"]).toHaveBeenCalledTimes(1);
    expect(h["close"]).toHaveBeenCalledTimes(4);
  });

  it("does nothing on a command that cannot run, and `Show all` switches the scope", async () => {
    const { api } = fakeApi();
    const h = host(api);
    const finder = new FinderStore("commands", h);
    await settle();
    const blocked = finder.view.rows.find(
      (r) => r.type === "command" && r.entry.command.id === "git.log",
    );
    finder.activate(blocked ?? null);
    expect(h["runCommand"]).not.toHaveBeenCalled();
    expect(h["close"]).not.toHaveBeenCalled();
    finder.activate(null);
    const wide = new FinderStore("everywhere", h);
    wide.activate({ type: "more", key: "more:files", scope: "files", count: 4 });
    expect(wide.scope).toBe("files");
  });

  it("says when the backend fails and tries again on request", async () => {
    const { api, fake } = fakeApi();
    fake.searchRecent.mockResolvedValueOnce({
      ok: false,
      error: { kind: "window", message: "down" },
    });
    const finder = new FinderStore("quick", host(api));
    await settle();
    expect(finder.load.status).toBe("failed");
    expect(finder.view.rows).toEqual([]);
    finder.retry();
    await settle();
    expect(finder.load.status).toBe("ready");
  });

  it("flags partial answers while the index builds, and the keyboard wraps", async () => {
    const { api, fake } = fakeApi();
    fake.searchFiles.mockResolvedValue(
      ok({ files: [hit("a.md"), hit("b.md")], folders: [], partial: true }),
    );
    const finder = new FinderStore("quick", host(api));
    await settle();
    finder.setQuery("a");
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    expect(finder.load).toEqual({ status: "ready", partial: true });
    finder.move(-1);
    expect(finder.active).toBe(1);
    finder.move(1);
    expect(finder.active).toBe(0);
    finder.dispose();
  });
});

describe("the Search panel", () => {
  it("asks nothing for an empty box and once for a typed one after the pause", async () => {
    const { api, fake } = fakeApi();
    const panel = new SearchPanelStore(api);
    panel.setQuery("   ");
    await vi.advanceTimersByTimeAsync(PANEL_DEBOUNCE_MS);
    expect(fake.searchSections).not.toHaveBeenCalled();
    expect(panel.answer.status).toBe("idle");
    panel.setQuery("ret");
    panel.setQuery("retry");
    await vi.advanceTimersByTimeAsync(PANEL_DEBOUNCE_MS);
    expect(fake.searchSections).toHaveBeenCalledTimes(1);
    expect(fake.searchSections).toHaveBeenCalledWith("retry", 100, {
      includeSilenced: false,
      tags: [],
      hiddenLast: false,
    });
    expect(panel.groups.map((g) => g.path)).toEqual(["a.md", "b.md"]);
    expect(panel.rows.map((r) => r.type)).toEqual(["file", "hit", "hit", "file", "hit"]);
  });

  it("folds a document, walks the rows round the ends and clears", async () => {
    const { api } = fakeApi();
    const panel = new SearchPanelStore(api);
    panel.setQuery("x");
    await vi.advanceTimersByTimeAsync(PANEL_DEBOUNCE_MS);
    panel.toggle("a.md");
    expect(panel.rows.map((r) => r.type)).toEqual(["file", "file", "hit"]);
    panel.toggle("a.md");
    panel.move(-1);
    expect(panel.active).toBe(panel.rows.length - 1);
    panel.move(1);
    expect(panel.active).toBe(0);
    panel.clear();
    expect(panel.query).toBe("");
    expect(panel.answer.status).toBe("idle");
    expect(panel.rows).toEqual([]);
  });

  it("follows the index of its own project and asks again when a partial answer can be complete", async () => {
    const { api, fake, emit } = fakeApi();
    fake.searchSections.mockResolvedValueOnce(ok({ hits: [section("a.md", 1)], partial: true }));
    const panel = new SearchPanelStore(api);
    const stop = await panel.start("C:/proj");
    expect(panel.progress).toEqual({ phase: "building", done: 5, total: 50 });
    emit({ root: "C:/other", phase: "building", done: 9, total: 10 });
    expect(panel.progress?.done).toBe(5);
    emit({ root: "C:/proj", phase: "building", done: 20, total: 50 });
    expect(panel.progress?.done).toBe(20);
    panel.setQuery("x");
    await vi.advanceTimersByTimeAsync(PANEL_DEBOUNCE_MS);
    expect(panel.answer).toMatchObject({ status: "ready", partial: true });
    emit({ root: "C:/proj", phase: "ready", done: 50, total: 50 });
    await settle();
    expect(fake.searchSections).toHaveBeenCalledTimes(2);
    expect(panel.answer).toMatchObject({ status: "ready", partial: false });
    stop();
    emit({ root: "C:/proj", phase: "ready", done: 1, total: 1 });
    expect(panel.progress?.done).toBe(50);
  });

  it("shows a failure and tries again; the silenced switch rides along", async () => {
    const { api, fake } = fakeApi();
    fake.searchSections.mockResolvedValueOnce({
      ok: false,
      error: { kind: "window", message: "down" },
    });
    const panel = new SearchPanelStore(api);
    panel.setQuery("x");
    await vi.advanceTimersByTimeAsync(PANEL_DEBOUNCE_MS);
    expect(panel.answer.status).toBe("failed");
    panel.retry();
    await settle();
    expect(panel.answer.status).toBe("ready");
    panel.setIncludeSilenced(true);
    await settle();
    expect(fake.searchSections).toHaveBeenLastCalledWith("x", 100, {
      includeSilenced: true,
      tags: [],
      hiddenLast: false,
    });
  });

  it("an answer for words that were replaced is dropped", async () => {
    const { api, fake } = fakeApi();
    const panel = new SearchPanelStore(api);
    let release: (r: Result<SectionResults>) => void = () => {};
    fake.searchSections.mockImplementationOnce(
      () =>
        new Promise<Result<SectionResults>>((resolve) => {
          release = resolve;
        }),
    );
    panel.setQuery("old");
    await vi.advanceTimersByTimeAsync(PANEL_DEBOUNCE_MS);
    panel.setQuery("new");
    release(ok({ hits: [section("stale.md", 1)], partial: false }));
    await settle();
    expect(panel.groups.map((g) => g.path)).not.toContain("stale.md");
    panel.dispose();
  });
});

describe("narrowing the search to tags", () => {
  it("passes the selected tags to every question and keeps the box open when a tag is chosen", async () => {
    const { api, fake } = fakeApi();
    let selected: readonly string[] = ["spec"];
    const chosen: string[] = [];
    const finder = new FinderStore(
      "everywhere",
      host(api, {
        tags: {
          hits: () => [{ name: "design", count: 3 }],
          selected: () => selected,
          select: (name) => {
            chosen.push(name);
            selected = [...selected, name];
          },
        },
      }),
    );
    expect(finder.scopes).toContain("tags");
    await settle();
    expect(fake.searchRecent).toHaveBeenCalledWith(["o.md"], 16, {
      includeSilenced: false,
      tags: ["spec"],
      hiddenLast: false,
    });
    finder.setScope("all");
    finder.setQuery("plan");
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    expect(fake.searchSections).toHaveBeenLastCalledWith("plan", 30, {
      includeSilenced: false,
      tags: ["spec"],
      hiddenLast: false,
    });
    expect(fake.searchFiles).toHaveBeenLastCalledWith("plan", 6, 3, {
      includeSilenced: true,
      tags: ["spec"],
      hiddenLast: true,
    });
    // the Tags scope asks nothing of the backend; a chosen tag narrows the search and goes back to All
    finder.setScope("tags");
    fake.searchFiles.mockClear();
    expect(finder.view.rows).toMatchObject([{ type: "tag", name: "design", count: 3 }]);
    finder.activate(finder.current);
    expect(chosen).toEqual(["design"]);
    expect(finder.scope).toBe("all");
    expect(finder.query).toBe("");
    await settle();
    expect(fake.searchRecent).toHaveBeenLastCalledWith(["o.md"], 16, {
      includeSilenced: false,
      tags: ["spec", "design"],
      hiddenLast: false,
    });
    expect(host(api).close).not.toHaveBeenCalled();
  });

  it("offers no Tags scope without the plugin", () => {
    const { api } = fakeApi();
    expect(new FinderStore("everywhere", host(api)).scopes).not.toContain("tags");
  });
});
