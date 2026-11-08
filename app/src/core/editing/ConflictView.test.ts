// @vitest-environment jsdom
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import ConflictView from "./ConflictView.svelte";

let component: Record<string, unknown> | null = null;

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  document.body.replaceChildren();
});

function shown(deleted = false) {
  const target = document.createElement("div");
  document.body.append(target);
  const calls = { reload: vi.fn(), keepMine: vi.fn(), saveAsNew: vi.fn() };
  component = mount(ConflictView, {
    target,
    props: { theirs: "a\nb\n", deleted, mine: "a\nc\n", ...calls },
  });
  flushSync();
  const buttons = (): HTMLButtonElement[] => Array.from(target.querySelectorAll("button"));
  const named = (text: string): HTMLButtonElement | undefined =>
    buttons().find((b) => b.textContent?.trim() === text);
  return { target, calls, buttons, named };
}

describe("the conflict bar", () => {
  it("leads with Show differences; neither way that drops a version is the primary button", () => {
    const { buttons } = shown();
    const labels = buttons().map((b) => b.textContent?.trim());
    expect(labels[0]).toBe("Show differences");
    expect(buttons()[0]?.classList.contains("btn--primary")).toBe(true);
    const primaries = buttons().filter((b) => b.classList.contains("btn--primary"));
    expect(primaries).toHaveLength(1);
    expect(labels).toContain("Keep my version (replace the file)");
    expect(labels).toContain("Use the file's version (drop my edits)");
  });

  it("each way out says what it drops and runs at once", () => {
    const { named, calls } = shown();
    named("Use the file's version (drop my edits)")?.click();
    expect(calls.reload).toHaveBeenCalledTimes(1);
    named("Keep my version (replace the file)")?.click();
    expect(calls.keepMine).toHaveBeenCalledTimes(1);
  });

  it("Show differences unfolds them below", () => {
    const { named, target } = shown();
    named("Show differences")?.click();
    flushSync();
    expect(target.querySelector(".diff")).not.toBeNull();
    expect(named("Hide differences")?.getAttribute("aria-expanded")).toBe("true");
  });

  it("a deleted file: writing it back loses nothing, so it may lead", () => {
    const { buttons } = shown(true);
    const labels = buttons().map((b) => b.textContent?.trim());
    expect(labels).toEqual(["Keep my version (write it back)", "Save as new file"]);
  });
});
