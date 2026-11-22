// The folder's shared settings are read again when they change outside the page (another window, a git pull, a hand
// edit): the page never keeps showing what the file no longer says. They can't be changed while the backend would
// refuse every change (no `.permark/`, a file that does not read).
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Api, ProjectSettings } from "../../../api";
import { createMockApi } from "../../../api/mock";
import { projectSettings, sharedLockedOf } from "./stores.svelte";

let stop: (() => void) | null = null;

afterEach(() => {
  stop?.();
  stop = null;
});

describe("the folder's shared settings", () => {
  it("are read again when the folder's file changes, never for another folder, and not after the page goes", async () => {
    const mock = createMockApi();
    const first = await mock.projectConfigGet();
    if (!first.ok) throw new Error("no mock project settings");
    let answer: ProjectSettings = { ...first.value, initialized: true, ignore: [] };
    let reads = 0;
    const backend: Api = {
      ...mock,
      projectConfigGet: () => {
        reads += 1;
        return Promise.resolve({ ok: true, value: answer });
      },
    };
    stop = projectSettings.follow("/folder", backend);
    await vi.waitFor(() => expect(projectSettings.state.status).toBe("ready"));
    answer = { ...answer, ignore: ["drafts/"], unreadableLine: 3 };

    mock.emit("project-config-changed", { root: "/elsewhere" });
    await Promise.resolve();
    expect(reads).toBe(1);

    mock.emit("project-config-changed", { root: "/folder" });
    await vi.waitFor(() => {
      const state = projectSettings.state;
      expect(state.status === "ready" ? state.settings.ignore : null).toEqual(["drafts/"]);
    });
    expect(reads).toBe(2);

    stop();
    stop = null;
    mock.emit("project-config-changed", { root: "/folder" });
    await Promise.resolve();
    expect(reads).toBe(2);
  });

  it("listen before the first read: a change between the two is never lost", async () => {
    const mock = createMockApi();
    const first = await mock.projectConfigGet();
    if (!first.ok) throw new Error("no mock project settings");
    let reads = 0;
    let listening = false;
    let registered: () => void = () => undefined;
    const backend: Api = {
      ...mock,
      projectConfigGet: () => {
        reads += 1;
        expect(listening).toBe(true);
        return Promise.resolve({ ok: true, value: first.value });
      },
      on: (event, handler) =>
        new Promise((resolve) => {
          registered = () => {
            listening = true;
            void mock.on(event, handler).then(resolve);
          };
        }),
    };
    stop = projectSettings.follow("/folder", backend);
    await Promise.resolve();
    expect(reads).toBe(0);
    expect(projectSettings.state.status).toBe("loading");
    registered();
    await vi.waitFor(() => expect(projectSettings.state.status).toBe("ready"));
    expect(reads).toBe(1);
  });

  it("can't be changed before they are read, without .permark/, or while the file does not read", async () => {
    const mock = createMockApi();
    const first = await mock.projectConfigGet();
    if (!first.ok) throw new Error("no mock project settings");
    const fine: ProjectSettings = { ...first.value, initialized: true };
    const lockedWith = async (settings: ProjectSettings): Promise<boolean> => {
      await projectSettings.load({
        ...mock,
        projectConfigGet: () => Promise.resolve({ ok: true, value: settings }),
      });
      return projectSettings.sharedLocked;
    };
    projectSettings.state = { status: "loading" };
    expect(projectSettings.sharedLocked).toBe(true);
    expect(await lockedWith({ ...fine, initialized: false })).toBe(true);
    expect(await lockedWith({ ...fine, unreadableLine: 3 })).toBe(true);
    expect(await lockedWith({ ...fine, unreadableWhy: "tooLarge" })).toBe(true);
    expect(await lockedWith(fine)).toBe(false);
    expect(sharedLockedOf(null)).toBe(true);
    expect(sharedLockedOf({ ...fine, unreadableWhy: "notText" })).toBe(true);
    expect(sharedLockedOf(fine)).toBe(false);
  });
});
