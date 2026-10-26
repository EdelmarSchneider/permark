// @vitest-environment jsdom
// TOON in the UI: the plugin's parts, a toon block of a page as a sheet (the backend's sheet), the code block it stays
// when it is not valid, and Copy as JSON.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});

const { api } = await import("../../api");
const { mockView } = await import("../../api/mockDocs");
const { ContributionRegistry, PluginHost, pluginLoaders } =
  await import("../../core/plugins/registry.svelte");
const { default: ToonEmbed } = await import("./ToonEmbed.svelte");

let component: Record<string, unknown> | null = null;

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function show(source: string): HTMLElement {
  const target = document.createElement("div");
  document.body.append(target);
  component = mount(ToonEmbed, { target, props: { source, info: "toon", display: false } });
  return target;
}

describe("the plugin", () => {
  it("claims .toon files and toon blocks, and leaves nothing when off", async () => {
    const registry = new ContributionRegistry();
    const host = new PluginHost(registry, pluginLoaders());
    await host.enable("fmt-toon");
    const viewer = registry.of("viewer").find((v) => v.id === "toon");
    expect(viewer?.claims("data/team.toon")).toBe(true);
    expect(viewer?.claims("data/team.json")).toBe(false);
    expect(registry.of("embed").map((e) => e.language)).toEqual(["toon"]);
    expect(host.disable("fmt-toon")).toBe(0);
    expect(registry.items).toHaveLength(0);
  });
});

describe("a toon block in a page", () => {
  it("is drawn as the sheet the backend reads, values as written, read only", async () => {
    const body = mockView("data/team.toon", [], true)?.body;
    if (body?.kind !== "sheet") throw new Error("no fixture sheet");
    const sheetCall = vi.spyOn(api, "configSheet").mockResolvedValue({ ok: true, value: body });
    const target = show("team[1]{id}:\n  1");
    await vi.waitFor(() => expect(target.querySelector("table")).not.toBeNull());
    expect(sheetCall).toHaveBeenCalledWith("embed.toon", "team[1]{id}:\n  1");
    expect(target.textContent).toContain("sk-ci-5f2a91");
    expect(target.querySelector(".grip")).toBeNull();
    expect(target.querySelector(".copy-json")).not.toBeNull();
  });

  it("stays a code block with the line and the reason when it is not valid", async () => {
    vi.spyOn(api, "configSheet").mockResolvedValue({
      ok: true,
      value: {
        kind: "text",
        text: "a[3]: x,y",
        language: "toon",
        problem: { line: 1, detail: "Array length mismatch: expected 3, found 2" },
      },
    });
    const target = show("a[3]: x,y");
    await vi.waitFor(() => expect(target.querySelector(".reason")).not.toBeNull());
    expect(target.querySelector("pre")?.textContent).toBe("a[3]: x,y");
    expect(target.querySelector(".reason")?.textContent).toMatch(
      /Not valid TOON 4\.4\s*·\s*line 1:\s*Array length mismatch/,
    );
    expect(target.querySelector("table")).toBeNull();
  });

  it("copies the TOON as written, and the data as JSON", async () => {
    const body = mockView("data/team.toon", [], true)?.body;
    if (body?.kind !== "sheet") throw new Error("no fixture sheet");
    vi.spyOn(api, "configSheet").mockResolvedValue({ ok: true, value: body });
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    const target = show("a: 1\nb: 2");
    await vi.waitFor(() => expect(target.querySelector(".copy-json")).not.toBeNull());
    target.querySelector<HTMLButtonElement>("button.copy")?.click();
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith("a: 1\nb: 2"));
    target.querySelector<HTMLButtonElement>(".copy-json")?.click();
    flushSync();
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledTimes(2));
    expect(JSON.parse(writeText.mock.calls[1]?.[0] as string)).toEqual({ toon: 2 });
  });
});
