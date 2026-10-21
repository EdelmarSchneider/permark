// @vitest-environment jsdom
// Verifier (adversarial): the question at the drop point, edge cases.
import "../testing/media";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ImportPlan } from "../../api";
import ImportAsk from "./ImportAsk.svelte";

let target: HTMLElement;
let component: Record<string, unknown> | null = null;

beforeEach(() => {
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
});

const item = (name: string) => ({
  source: `C:/out/${name}`,
  name,
  kind: "file" as const,
  inProject: false,
  conflict: false,
});

function open(plan: ImportPlan | null) {
  const cancel = vi.fn<() => void>();
  component = mount(ImportAsk, {
    target,
    props: {
      at: { x: 100, y: 120 },
      plan,
      where: "docs",
      conflicts: "keepBoth",
      setConflicts: vi.fn(),
      confirm: vi.fn(),
      cancel,
    },
  });
  flushSync();
  return cancel;
}

describe("verifier: the question at the drop point", () => {
  it("the refused heading ends with one colon, not two", () => {
    open({
      planId: 1,
      target: "docs",
      items: [item("a.md")],
      refused: [{ source: "C:/out/x.lnk", name: "x.lnk", reason: "link" }],
    });
    const text = target.textContent ?? "";
    expect(text).toContain("1 item can't be added:");
    expect(text).not.toContain("::");
  });

  it("Esc closes it while it is still checking names (the keyboard is not inside it yet)", () => {
    const cancel = open(null);
    // focus stays where it was (the page): nothing inside is focusable while the buttons are off
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    flushSync();
    expect(cancel).toHaveBeenCalledTimes(1);
  });
});
