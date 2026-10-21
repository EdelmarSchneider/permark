import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const { createMockApi, MOCK_FOLDER } = await import("./mock");
const { tauriApi } = await import("./tauri");
const { MOCK_PROJECTS_FRESH, MOCK_PROJECTS_LISTED, MOCK_PROJECTS_BROKEN } =
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
  });
});
