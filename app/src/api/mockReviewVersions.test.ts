import { describe, expect, it } from "vitest";
import { MOCK_FOLDER, createMockApi } from "./mock";
import { MOCK_HEAD, MOCK_LONG_HISTORY } from "./mockReviewVersions";
import type { PluginMethod, PluginMethods, ReviewVersions } from "./types";

const GUIDE = "docs/guide.md";
const README = "README.md";

async function opened() {
  const api = createMockApi();
  await api.openPath(MOCK_FOLDER, "thisTab");
  /** What the plugin answers; an error is a failed test. */
  const call = async <M extends PluginMethod>(
    method: M,
    args: PluginMethods[M]["args"],
  ): Promise<PluginMethods[M]["result"]> => {
    const result = await api.pluginCall(method, args);
    if (!result.ok) throw new Error(`${method} failed`);
    return result.value;
  };
  return { api, call };
}

/** The list a file has (a file with none is a failed test). */
function listed(answer: ReviewVersions): Extract<ReviewVersions, { state: "list" }> {
  if (answer.state !== "list") throw new Error(`no list: ${answer.reason}`);
  return answer;
}

describe("mock earlier versions", () => {
  it("a committed file has a short history: several authors, a co-author, a long subject", async () => {
    const { call } = await opened();
    const list = listed(await call("review.versions", { path: README }));
    expect(list).toMatchObject({ next: null, renamedFrom: null, cut: false });
    expect(list.rows).toHaveLength(4);
    expect(list.rows[0]?.commit).toBe(MOCK_HEAD);
    for (const row of list.rows) expect(row.commit).toMatch(/^[0-9a-f]{40}$/);
    expect(new Set(list.rows.map((row) => row.author)).size).toBeGreaterThan(2);
    expect(list.rows.filter((row) => row.coAuthors.length > 0)).toHaveLength(1);
    expect(list.rows.some((row) => row.subject.length > 100)).toBe(true);
    const times = list.rows.map((row) => row.atMs);
    expect(times).toEqual([...times].sort((a, b) => b - a));
    // the file is what the last commit holds: its newest version is the text now
    expect(list.rows.map((row) => row.same)).toEqual([true, false, false, false]);
  });

  it("a file changed since the last commit has no version with the text now, and names its old path", async () => {
    const { call } = await opened();
    const list = listed(await call("review.versions", { path: GUIDE }));
    expect(list.rows.map((row) => row.same)).toEqual([false, false, false, false]);
    expect(list.renamedFrom).toBe("docs/manual.md");
  });

  it("says why a file has no list: never committed, not text", async () => {
    const { call } = await opened();
    expect(await call("review.versions", { path: "notes/ideas.md" })).toEqual({
      state: "unavailable",
      reason: "notCommitted",
    });
    expect(await call("review.versions", { path: "docs/img/logo.png" })).toEqual({
      state: "unavailable",
      reason: "unsupported",
    });
  });

  it("a long history comes in pages of 50, from a copy without its whole history", async () => {
    const { api, call } = await opened();
    const first = listed(await call("review.versions", { path: MOCK_LONG_HISTORY }));
    expect(first.rows).toHaveLength(50);
    expect(first.cut).toBe(true);
    const second = listed(
      await call("review.versions", { path: MOCK_LONG_HISTORY, after: first.next ?? -1 }),
    );
    const last = listed(
      await call("review.versions", { path: MOCK_LONG_HISTORY, after: second.next ?? -1 }),
    );
    expect([second.rows.length, last.rows.length, last.next]).toEqual([50, 20, null]);
    const all = [...first.rows, ...second.rows, ...last.rows];
    expect(new Set(all.map((row) => row.commit)).size).toBe(120);
    // its oldest version is not in the copy
    const oldest = all.at(-1)?.commit ?? "";
    expect(
      await call("review.status", { path: MOCK_LONG_HISTORY, basis: { version: oldest } }),
    ).toEqual({ state: { state: "noBasis" }, basis: { version: oldest }, gap: "missing" });
    const open = await api.pluginCall("review.open", {
      path: MOCK_LONG_HISTORY,
      basis: { version: oldest },
    });
    expect(open).toMatchObject({
      ok: false,
      error: { kind: "backend", error: { kind: "unavailable", subject: "noBasis" } },
    });
  });

  it("a version is compared, reverted and undone like the last commit, and is never remembered", async () => {
    const { call } = await opened();
    const rows = listed(await call("review.versions", { path: README })).rows;
    const newest = { version: rows[0]?.commit ?? "" };
    const older = { version: rows[2]?.commit ?? "" };
    expect(await call("review.status", { path: README, basis: newest })).toEqual({
      state: { state: "same" },
      basis: newest,
    });
    expect(await call("review.status", { path: README, basis: older })).toEqual({
      state: { state: "changes", count: 2 },
      basis: older,
    });
    const view = await call("review.open", { path: README, basis: older });
    expect(view).toMatchObject({
      path: README,
      basis: older,
      asked: older,
      unit: "blocks",
      count: 2,
      canKeep: false,
      uncommitted: false,
    });
    expect(view.changes.map((change) => change.index)).toEqual([0, 1]);
    const reverted = await call("review.revert", {
      path: README,
      hash: view.hash,
      index: 0,
      basis: older,
    });
    expect(reverted).toMatchObject({ basis: older, count: 1 });
    expect(await call("review.undo", { path: README, basis: older })).toMatchObject({
      basis: older,
      count: 2,
    });
    expect(
      await call("review.revertAll", { path: README, hash: view.hash, basis: older }),
    ).toMatchObject({ basis: older, count: 0 });
    await call("review.setBasis", { basis: older });
    expect(await call("review.basis", {})).toBe("read");
  });

  it("a version of a file with changes that are in no commit says so, and draws lines for code", async () => {
    const { call } = await opened();
    const guide = listed(await call("review.versions", { path: GUIDE })).rows;
    const head = await call("review.open", {
      path: GUIDE,
      basis: { version: guide[0]?.commit ?? "" },
    });
    expect(head).toMatchObject({ count: 2, uncommitted: true, unit: "blocks" });
    const code = listed(await call("review.versions", { path: "package.json" })).rows;
    const lines = await call("review.open", {
      path: "package.json",
      basis: { version: code[1]?.commit ?? "" },
    });
    expect(lines).toMatchObject({ count: 1, unit: "lines", uncommitted: false });
    expect(lines.changes[0]?.old.length).toBeGreaterThan(0);
  });
});
