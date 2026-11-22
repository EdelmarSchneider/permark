// @vitest-environment jsdom
// The saved time beside the document's name: a fresh save never reads as "0m ago".
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});

const DocHeader = (await import("./DocHeader.svelte")).default;

let component: Record<string, unknown> | null = null;

/** The header's saved words for a file saved `ago` ms before the header shows. */
function savedText(ago: number): string | null {
  const target = document.createElement("div");
  document.body.append(target);
  component = mount(DocHeader, {
    target,
    props: {
      path: "docs/guide.md",
      savedMs: Date.now() - ago,
      role: null,
      template: "",
      hasOutline: false,
      onFind: () => {},
      canOpenSystem: false,
      opensOutside: false,
    },
  });
  flushSync();
  return target.querySelector(".saved")?.textContent ?? null;
}

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
});

describe("the saved time", () => {
  it("says just now for the whole first minute", () => {
    for (const ago of [0, 25_000, 45_000, 59_000]) {
      expect(savedText(ago), `${ago} ms`).toBe("saved just now");
      if (component) void unmount(component);
      component = null;
    }
  });

  it("counts minutes after the first one", () => {
    expect(savedText(3 * 60_000)).toBe("saved 3m ago");
  });
});
