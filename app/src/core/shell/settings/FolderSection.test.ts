// @vitest-environment jsdom
// This folder's overview says when its shared settings file can't be read as TOML, and where: the person knows why
// shared settings are not changed until the file is fixed.
import "../../testing/media";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { Api } from "../../../api";
import { createMockApi } from "../../../api/mock";
import { conditions, neutralContext } from "../../conditions/context.svelte";
import en from "../../i18n/locales/en";
import FolderSection from "./FolderSection.svelte";
import { projectSettings } from "./stores.svelte";

let component: Record<string, unknown> | null = null;
let root: HTMLElement;

beforeEach(() => {
  root = document.body.appendChild(document.createElement("div"));
  conditions.set({ ...neutralContext(), workspace: "project", permark: "initialized" });
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  document.body.replaceChildren();
  conditions.set(neutralContext());
});

async function show(unreadableLine: number | null): Promise<void> {
  const mock = createMockApi();
  const settings = await mock.projectConfigGet();
  if (!settings.ok) throw new Error("no mock project settings");
  const backend: Api = {
    ...mock,
    projectConfigGet: () =>
      Promise.resolve({
        ok: true,
        value: { ...settings.value, initialized: true, unreadableLine },
      }),
  };
  await projectSettings.load(backend);
  component = mount(FolderSection, { target: root, props: {} });
  flushSync();
  await tick();
}

const line = (n: number): string => en["project.sharedFileUnreadable"].replace("{line}", String(n));

describe("the folder's overview", () => {
  it("says the shared file can't be read and at which line", async () => {
    await show(7);
    const said = root.querySelector('[role="alert"]');
    expect(said?.textContent).toContain(line(7));
  });

  it("says nothing of the kind while the file reads", async () => {
    await show(null);
    expect(root.textContent).not.toContain(line(7).slice(0, 20));
    expect(root.querySelector('[role="alert"]')).toBeNull();
  });
});
