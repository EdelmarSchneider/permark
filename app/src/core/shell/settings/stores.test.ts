// The folder's shared settings are read again when they change outside the page (another window, a git pull, a hand
// edit): the page never keeps showing what the file no longer says.
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Api, ProjectSettings } from "../../../api";
import { createMockApi } from "../../../api/mock";
import { projectSettings } from "./stores.svelte";

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
});
