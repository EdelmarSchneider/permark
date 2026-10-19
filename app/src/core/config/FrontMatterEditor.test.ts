// @vitest-environment jsdom
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { joinFrontMatter, splitFrontMatter } from "./frontMatter";

// the components talk to the mock backend (one per test file)
vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { default: FrontMatterEditor } = await import("./FrontMatterEditor.svelte");

let component: Record<string, unknown> | null = null;

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
});

const SOURCE = "---\ntitle: A plan\nowner: ana\nversion: 3\n---";

describe("the front matter fences", () => {
  it("cuts a block at its fences and puts it back byte for byte", () => {
    const parts = splitFrontMatter(SOURCE);
    expect(parts).toEqual({
      syntax: "yaml",
      open: "---",
      body: "title: A plan\nowner: ana\nversion: 3",
      close: "---",
    });
    if (parts) expect(joinFrontMatter(parts, parts.body)).toBe(SOURCE);
    expect(splitFrontMatter("+++\na = 1\n+++")?.syntax).toBe("toml");
    expect(splitFrontMatter("---\nno close")).toBeNull();
    expect(splitFrontMatter("# not front matter")).toBeNull();
    const empty = splitFrontMatter("---\n---");
    if (empty) expect(joinFrontMatter(empty, "")).toBe("---\n---");
  });
});

describe("editing the properties of a front matter block", () => {
  it("shows the properties and puts the block back with only that value changed", async () => {
    const target = document.createElement("div");
    document.body.append(target);
    const replace = vi.fn();
    component = mount(FrontMatterEditor, {
      target,
      props: { source: SOURCE, syntax: "yaml", replace },
    });
    await vi.waitFor(() => expect(target.querySelectorAll(".row")).toHaveLength(3));
    const owner = Array.from(target.querySelectorAll<HTMLElement>(".row")).find(
      (row) => row.querySelector(".key")?.textContent === "owner",
    );
    owner?.querySelector<HTMLButtonElement>(".hit")?.click();
    flushSync();
    const input = owner?.querySelector<HTMLInputElement>("input");
    if (!input) throw new Error("no input");
    input.value = "bea";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await vi.waitFor(() => expect(replace).toHaveBeenCalledTimes(1));
    expect(replace).toHaveBeenCalledWith(SOURCE.replace("ana", "bea"));
  });

  it("shows the source as it is when the properties cannot be read", async () => {
    const target = document.createElement("div");
    document.body.append(target);
    component = mount(FrontMatterEditor, {
      target,
      props: { source: "---\n- just\n- a list\n---", syntax: "yaml", replace: vi.fn() },
    });
    await vi.waitFor(() => expect(target.querySelector("pre.raw")).not.toBeNull());
    expect(target.querySelector("pre.raw")?.textContent).toBe("---\n- just\n- a list\n---");
  });
});
