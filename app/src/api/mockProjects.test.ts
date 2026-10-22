import { describe, expect, it, vi } from "vitest";
import type { Result } from "./types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const { createMockApi, MOCK_FOLDER } = await import("./mock");
const { tauriApi } = await import("./tauri");
const { MOCK_PROJECTS_FRESH, MOCK_PROJECTS_LISTED, MOCK_PROJECTS_BROKEN, MOCK_PROJECTS_VOCAB } =
  await import("./mockProjects");
const core = await import("@tauri-apps/api/core");

async function opened(path: string) {
  const api = createMockApi();
  await api.openPath(path, "tab");
  return api;
}

function value<T>(r: { ok: true; value: T } | { ok: false }): T {
  if (!r.ok) throw new Error("not ok");
  return r.value;
}

describe("mock projects", () => {
  it("a plain folder has nothing new", async () => {
    const api = await opened(MOCK_FOLDER);
    const view = value(await api.projectsView());
    expect(view.present).toBe(false);
    expect(view.rows).toEqual([]);
  });

  it("include all, then undo, with projects-changed each time", async () => {
    const api = await opened(MOCK_PROJECTS_FRESH);
    const seen: number[] = [];
    await api.on("projects-changed", (e) => seen.push(e.seq));
    const first = value(await api.projectsView());
    expect(first.answer).toBe("unanswered");
    expect(first.rows.filter((r) => r.state === "offered")).toHaveLength(4);
    // the fixture's folders join the Explorer
    expect(api.probe.project.files.has("hubs/hub-b/README.md")).toBe(true);
    const outcome = value(await api.projectsInclude({ kind: "all" }));
    expect(outcome.included).toBe(4);
    const after = value(await api.projectsView());
    expect(after.answer).toBe("answered");
    expect(after.rows.every((r) => r.state === "included" && r.uid !== null)).toBe(true);
    const undone = value(await api.projectsUndo(outcome.undo ?? -1));
    expect(undone.undone).toBe(true);
    expect(value(await api.projectsView()).answer).toBe("unanswered");
    expect(seen.length).toBe(2);
  });

  it("no thanks on the first question declines for the folder", async () => {
    const api = await opened(MOCK_PROJECTS_FRESH);
    await api.projectsDecline({ kind: "all" });
    const view = value(await api.projectsView());
    expect(view.answer).toBe("declinedAll");
    expect(view.offered.count).toBe(0);
  });

  it("a listed folder: rules, agent, card, locate, remove", async () => {
    const api = await opened(MOCK_PROJECTS_LISTED);
    const view = value(await api.projectsView());
    expect(view.offered.names).toEqual(["new-app"]);
    const hub = view.rows.find((r) => r.path === "hubs/hub-b");
    expect(hub?.name).toBe("PerMark software");
    const uid = hub?.uid ?? "";
    await api.projectsSetHidden(uid, "own+root");
    await api.projectsSetAgent(uid, "off");
    const card = value(
      await api.projectsCardSet({ kind: "member", uid }, { field: "short", value: "hb" }),
    );
    expect(card.short).toBe("hb");
    const again = value(await api.projectsView()).rows.find((r) => r.uid === uid);
    expect(again).toMatchObject({ hidden: "own+root", agent: "off", short: "hb" });
    expect(value(await api.projectsLocate("hubx2hubx2", "D:/elsewhere"))).toBe("outside");
    expect(value(await api.projectsLocate("hubx2hubx2", `${MOCK_PROJECTS_LISTED}/docs`))).toBe(
      "notProject",
    );
    await api.projectsRemove(["hubx2hubx2"]);
    expect(value(await api.projectsView()).rows.some((r) => r.state === "missing")).toBe(false);
  });

  it("an unreadable list refuses changes", async () => {
    const api = await opened(MOCK_PROJECTS_BROKEN);
    const result = await api.projectsInclude({ kind: "all" });
    expect(result.ok).toBe(false);
  });

  it("the real api sends the commands with their argument names", async () => {
    const invoke = vi.mocked(core.invoke);
    invoke.mockResolvedValue(null);
    await tauriApi.projectsInclude({ kind: "paths", paths: ["a"] });
    expect(invoke).toHaveBeenCalledWith("projects_include", {
      pick: { kind: "paths", paths: ["a"] },
    });
    await tauriApi.projectsAnswerMove("k7m2q7xd2a", "x/a", true);
    expect(invoke).toHaveBeenCalledWith("projects_answer_move", {
      uid: "k7m2q7xd2a",
      path: "x/a",
      same: true,
    });
    await tauriApi.projectsUndo(3);
    expect(invoke).toHaveBeenCalledWith("projects_undo", { token: 3 });
    await tauriApi.vocabularyPromote("category", "lab");
    expect(invoke).toHaveBeenCalledWith("vocabulary_promote", { kind: "category", name: "lab" });
    await tauriApi.vocabularyRename("tag", "spec", "specs");
    expect(invoke).toHaveBeenCalledWith("vocabulary_rename", {
      kind: "tag",
      name: "spec",
      newName: "specs",
    });
    await tauriApi.vocabularyRemove("category", "lab");
    expect(invoke).toHaveBeenCalledWith("vocabulary_remove", { kind: "category", name: "lab" });
    await tauriApi.vocabularySetStyle("tag", "spec", "blue", null);
    expect(invoke).toHaveBeenCalledWith("vocabulary_set_style", {
      kind: "tag",
      name: "spec",
      color: "blue",
      icon: null,
    });
  });

  it("official lists: status marks on cards and tags, promote (round 4)", async () => {
    const plain = value(await (await opened(MOCK_PROJECTS_LISTED)).projectsView());
    expect(plain.vocabulary.active).toBe(false);
    expect(plain.rows.every((r) => r.categoryStatus === null)).toBe(true);

    const api = await opened(MOCK_PROJECTS_VOCAB);
    const view = value(await api.projectsView());
    expect(view.vocabulary).toMatchObject({ active: true, file: { status: "ok" } });
    const byPath = new Map(view.rows.map((r) => [r.path, r]));
    expect(byPath.get("hub-b")?.categoryStatus).toBe("official");
    expect(byPath.get("site")?.categoryStatus).toBe("local");
    expect(byPath.get("tools/scratch")?.categoryStatus).toBe("unlisted");
    expect(byPath.get("hub-b")?.localTags).toBe(1);
    const overview = value(await api.pluginCall("tags.overview", {}));
    expect(overview.marks).toBe(true);
    const tag = (name: string) => overview.tags.find((t) => t.name === name);
    expect(tag("spec")).toMatchObject({ status: "official", owner: null });
    expect(tag("draft")).toMatchObject({ status: "local", owner: "vocabhubb2" });
    expect(tag("roadmap")).toMatchObject({ status: "unlisted", owner: null });
    const meta = value(await api.pluginCall("tags.fileMeta", { path: "site/index.md" }));
    expect(meta.tags[0]).toMatchObject({ name: "draft", status: "unlisted" });

    let tagsChanged = 0;
    await api.onPlugin("tags:changed", () => (tagsChanged += 1));
    expect(value(await api.vocabularyPromote("tag", "Draft"))).toBeNull();
    const after = value(await api.projectsView());
    expect(after.vocabulary.tags.map((t) => t.name)).toEqual(["draft", "needs-review", "spec"]);
    expect(after.rows.find((r) => r.path === "hub-b")?.localTags).toBe(0);
    expect(tag("draft")).toBeDefined();
    const again = value(await api.pluginCall("tags.overview", {}));
    expect(again.tags.find((t) => t.name === "draft")?.status).toBe("official");
    expect(tagsChanged).toBe(1);
    // a folder without a project list has no .permark/ to write in
    const fresh = await opened(MOCK_PROJECTS_FRESH);
    expect((await fresh.vocabularyPromote("tag", "x")).ok).toBe(false);
  });

  it("official lists: rename, remove, look (D46)", async () => {
    const api = await opened(MOCK_PROJECTS_VOCAB);
    let tagsChanged = 0;
    await api.onPlugin("tags:changed", () => (tagsChanged += 1));
    const refusal = async (r: Promise<Result<null>>) => {
      const res = await r;
      if (res.ok || res.error.kind !== "backend") throw new Error("expected a backend refusal");
      return [res.error.error.kind, res.error.error.subject];
    };
    expect(await refusal(api.vocabularyRename("tag", "spec", "needs-review"))).toEqual([
      "exists",
      "newName",
    ]);
    expect(await refusal(api.vocabularyRename("tag", "nope", "x"))).toEqual(["notFound", "name"]);
    expect(await refusal(api.vocabularyRename("tag", "spec", " "))).toEqual([
      "invalidData",
      "newName",
    ]);
    expect(await refusal(api.vocabularySetStyle("tag", "spec", "ultraviolet", null))).toEqual([
      "invalidData",
      "color",
    ]);
    expect(await refusal(api.vocabularyRemove("category", "nope"))).toEqual(["notFound", "name"]);
    expect(tagsChanged).toBe(0);

    // same name | same look: nothing to do
    expect(value(await api.vocabularyRename("tag", "spec", "Spec"))).toBeNull();
    expect(value(await api.vocabularySetStyle("tag", "spec", "blue", "book-open"))).toBeNull();
    expect(tagsChanged).toBe(0);

    expect(value(await api.vocabularyRename("tag", "Spec", "Specs"))).toBeNull();
    let view = value(await api.projectsView());
    expect(view.vocabulary.tags).toEqual([
      { name: "needs-review", color: null, icon: null },
      { name: "specs", color: "blue", icon: "book-open" },
    ]);
    // documents untouched: the old name in files is now unlisted
    const overview = value(await api.pluginCall("tags.overview", {}));
    expect(overview.tags.find((t) => t.name === "spec")?.status).toBe("local");
    expect(value(await api.vocabularySetStyle("tag", "specs", null, "star"))).toBeNull();
    expect(value(await api.vocabularySetStyle("category", "software", "red", null))).toBeNull();
    view = value(await api.projectsView());
    expect(view.vocabulary.tags[1]).toEqual({ name: "specs", color: null, icon: "star" });
    expect(view.vocabulary.categories).toEqual([{ name: "software", color: "red", icon: null }]);
    expect(value(await api.vocabularyRemove("category", "software"))).toBeNull();
    view = value(await api.projectsView());
    expect(view.rows.find((r) => r.path === "hub-b")?.categoryStatus).toBe("unlisted");
    for (const name of ["needs-review", "specs"]) {
      expect(value(await api.vocabularyRemove("tag", name))).toBeNull();
    }
    view = value(await api.projectsView());
    expect(view.vocabulary.active).toBe(false);
    expect(view.rows.every((r) => r.categoryStatus === null)).toBe(true);
    expect(tagsChanged).toBe(6);
    // a folder without a project list has no .permark/ to write in
    const fresh = await opened(MOCK_PROJECTS_FRESH);
    expect(await refusal(fresh.vocabularyRemove("tag", "x"))).toEqual([
      "unavailable",
      "vocabulary",
    ]);
  });
});
