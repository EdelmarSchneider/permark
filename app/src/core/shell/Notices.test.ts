// @vitest-environment jsdom
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import { createMockApi } from "../../api/mock";
import { WindowStore } from "../window.svelte";
import Notices from "./Notices.svelte";

let component: Record<string, unknown> | null = null;

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  document.body.replaceChildren();
});

function shown(rightInset: number): { store: WindowStore; target: HTMLElement } {
  const target = document.createElement("div");
  document.body.append(target);
  const store = new WindowStore(createMockApi());
  component = mount(Notices, { target, props: { store, rightInset } });
  store.notifyNote("Saved, 1 change.");
  flushSync();
  return { store, target };
}

describe("the notices", () => {
  it("stand clear of the right panel (Live changes): they keep to the left of its width", () => {
    const { target } = shown(300);
    const region = target.querySelector<HTMLElement>(".notices");
    expect(region?.style.getPropertyValue("--notices-right")).toBe("300px");
  });

  it("are told politely to screen readers, and their buttons take the keyboard", () => {
    const { store, target } = shown(0);
    store.notifyAction("Deleted a.md", { label: "Undo", run: () => {} });
    flushSync();
    const region = target.querySelector(".notices");
    expect(region?.getAttribute("aria-live")).toBe("polite");
    const undo = Array.from(target.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === "Undo",
    );
    expect(undo?.tabIndex).toBe(0);
  });
});
