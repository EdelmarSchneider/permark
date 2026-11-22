// @vitest-environment jsdom
// This folder's overview says when its shared settings file can't be read (as TOML: where; as text: why): the person
// knows why shared settings are not changed until the file is fixed. It also says when agents get none of the folder's
// comments or activity because git tracks PerMark's private folder.
import "../../testing/media";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { Api, ProjectSettings } from "../../../api";
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

async function show(
  unreadableLine: number | null,
  more: Partial<ProjectSettings> = {},
): Promise<void> {
  const mock = createMockApi();
  const settings = await mock.projectConfigGet();
  if (!settings.ok) throw new Error("no mock project settings");
  const backend: Api = {
    ...mock,
    projectConfigGet: () =>
      Promise.resolve({
        ok: true,
        value: { ...settings.value, initialized: true, unreadableLine, ...more },
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

  it("says why the shared file can't be read as text", async () => {
    const why = (key: "tooLarge" | "notText" | "locked"): string =>
      en[`error.settingsUnreadable.${key}`].replace("{subject}", ".permark/config.toml");
    for (const reason of ["tooLarge", "notText", "locked"] as const) {
      await show(null, { unreadableWhy: reason });
      const said = root.querySelector('[role="alert"]');
      expect(said?.textContent).toContain(why(reason));
      if (component) await unmount(component);
      component = null;
      root.replaceChildren();
    }
  });

  it("says why agents get none of the folder's comments or activity while git tracks its private folder", async () => {
    await show(null, { privateTracked: true });
    expect(root.textContent).toContain(en["project.privateTracked"]);
  });

  it("says nothing of agents while git leaves the private folder alone", async () => {
    await show(null, { privateTracked: false });
    expect(root.textContent).not.toContain(en["project.privateTracked"].slice(0, 20));
  });

  // Spacing and wrapping come from the shared row and card styles only: what git sees stays a plain row right after
  // the shared file's row in the same card (same padding, same divider, its paragraph is the row's hint), never a
  // wrapper or a wide row with a look of its own.
  it("draws what git sees as a plain row of the shared file's card", async () => {
    await show(null, { sharing: "shared" });
    const card = root.querySelector(".overview > .card");
    const rows = [...(card?.children ?? [])];
    expect(rows.map((row) => row.querySelector(".name")?.textContent?.trim() ?? "")).toEqual([
      ".permark/config.toml",
      expect.stringContaining(en["project.shareGit"]),
    ]);
    const [config, git] = rows;
    expect(git?.className).toBe(config?.className);
    expect(git?.querySelector(":scope > .text > .hint")?.textContent).toBe(
      en["project.shareGitHint"],
    );
  });
});
