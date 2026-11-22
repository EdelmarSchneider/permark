// @vitest-environment jsdom
// While the folder's shared file does not read, the quiet line above the locked rows of Files, Token count, Plugins
// here and agent limits says why (the same reasons as the Overview), and the rows dim only their controls: labels and
// hints stay readable.
import "../../testing/media";
import { readFileSync } from "node:fs";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { Api, ProjectSettings } from "../../../api";
import { createMockApi } from "../../../api/mock";
import en from "../../i18n/locales/en";
import { declarations, rulesOf, styleOf } from "../../testing/cssRules";
import ShareInit from "./ShareInit.svelte";
import { projectSettings } from "./stores.svelte";

let component: Record<string, unknown> | null = null;
let root: HTMLElement;

beforeEach(() => {
  root = document.body.appendChild(document.createElement("div"));
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  document.body.replaceChildren();
});

async function show(more: Partial<ProjectSettings>): Promise<string> {
  if (component) await unmount(component);
  root.replaceChildren();
  const mock = createMockApi();
  const settings = await mock.projectConfigGet();
  if (!settings.ok) throw new Error("no mock project settings");
  const backend: Api = {
    ...mock,
    projectConfigGet: () =>
      Promise.resolve({ ok: true, value: { ...settings.value, initialized: true, ...more } }),
  };
  await projectSettings.load(backend);
  component = mount(ShareInit, { target: root, props: { compact: true } });
  flushSync();
  await tick();
  return root.textContent ?? "";
}

describe("the line above locked shared rows", () => {
  it("says at which line the shared file stops reading", async () => {
    const said = await show({ unreadableLine: 3, unreadableWhy: null });
    expect(said).toContain(en["project.sharedLocked.line"].replace("{line}", "3"));
  });

  it("says why the shared file can't be read as text", async () => {
    for (const why of ["tooLarge", "notText", "locked"] as const) {
      const said = await show({ unreadableLine: null, unreadableWhy: why });
      expect(said, why).toContain(en[`project.sharedLocked.${why}`]);
    }
  });

  it("still leads to the Overview", async () => {
    await show({ unreadableLine: 3, unreadableWhy: null });
    expect(root.querySelector("button")?.textContent).toContain(en["settings.section.project"]);
  });
});

describe("the locked shared rows", () => {
  const LOCKED = [
    { file: "./FilesSection.svelte", box: ".shared.off" },
    { file: "../../../plugins/token-count/TokenProject.svelte", box: ".shared.off" },
    { file: "../../../plugins/agent/AgentNarrowing.svelte", box: ".limits-rows.off" },
  ];

  it.each(LOCKED)("$file dims only its controls, never the labels and hints", ({ file, box }) => {
    const rules = rulesOf(styleOf(readFileSync(new URL(file, import.meta.url), "utf8")));
    const dims = (body: string): boolean =>
      declarations(body).some((d) => d.startsWith("opacity:"));
    // the whole box dimmed takes row labels and hints below readable contrast
    expect(rules.filter((r) => r.selectors.includes(box) && dims(r.body))).toEqual([]);
    const dimmed = rules.filter((r) => dims(r.body)).flatMap((r) => r.selectors);
    expect(dimmed).toEqual(
      expect.arrayContaining([`${box} :global(input)`, `${box} :global(select)`]),
    );
  });
});
