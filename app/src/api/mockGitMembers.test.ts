import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const { createMockApi, MOCK_FOLDER } = await import("./mock");
const { MOCK_PROJECTS_LISTED, MOCK_PROJECTS_MANY } = await import("./mockProjects");

async function opened(path: string) {
  const api = createMockApi();
  await api.openPath(path, "tab");
  // the projects view makes the fixture (as the window does at start)
  await api.projectsView();
  return api;
}

function value<T>(r: { ok: true; value: T } | { ok: false }): T {
  if (!r.ok) throw new Error("not ok");
  return r.value;
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 40));

describe("mock repositories of included projects", () => {
  it("a plain folder has none", async () => {
    const api = await opened(MOCK_FOLDER);
    expect(value(await api.pluginCall("git.repos", {}))).toEqual([]);
  });

  it("included git projects have cheap facts at once, details after interest", async () => {
    const api = await opened(MOCK_PROJECTS_LISTED);
    const view = value(await api.projectsView());
    const included = view.rows.filter(
      (r) => r.state === "included" && (r.marker === "git" || r.marker === "both"),
    );
    const repos = value(await api.pluginCall("git.repos", {}));
    expect(repos.map((r) => r.id)).toEqual(included.map((r) => r.uid));
    expect(repos[0]?.branch).toBe("main");
    expect(repos.every((r) => r.lastCommit === null)).toBe(true);
    const id = repos[0]?.id ?? "";
    expect(value(await api.pluginCall("git.snapshot", { project: id }))).toBeNull();
    const heard: string[] = [];
    await api.onPlugin("git:repos", (e) => heard.push(...e.data.upsert.map((s) => s.id)));
    await api.onPlugin("git:projectSnapshot", (e) => heard.push(`list:${e.data.project}`));
    await api.pluginCall("git.interest", { ids: [id], level: "detail" });
    await tick();
    expect(heard).toContain(id);
    expect(heard).toContain(`list:${id}`);
    const after = value(await api.pluginCall("git.repos", {}));
    expect(after[0]?.lastCommit?.subject).toMatch(/^Update the plan of /);
    const list = value(await api.pluginCall("git.snapshot", { project: id }));
    expect(list?.files.every((f) => f.path.startsWith(`${after[0]?.path}/`))).toBe(true);
    // the root repository answers as before
    expect(value(await api.pluginCall("git.snapshot", {}))?.head.branch).toBe("main");
  });

  it("a large folder shows every case; git missing and pending are switchable", async () => {
    const api = await opened(MOCK_PROJECTS_MANY);
    const repos = value(await api.pluginCall("git.repos", {}));
    expect(repos.some((r) => r.detached)).toBe(true);
    expect(repos.some((r) => r.state === "merging")).toBe(true);
    expect(repos.some((r) => r.error?.kind === "failed")).toBe(true);
    expect(repos.some((r) => r.unborn)).toBe(true);
    const probe = api.probe.plugins.gitMembers;
    probe.hold(true);
    expect(probe.summaries().every((s) => s.pending && s.changed === null)).toBe(true);
    probe.hold(false);
    probe.unavailable(true);
    expect(
      probe.summaries().every((s) => s.error?.kind === "unavailable" && s.branch === null),
    ).toBe(true);
  });
});
