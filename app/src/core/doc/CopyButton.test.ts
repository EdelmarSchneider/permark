// @vitest-environment jsdom
// The one copy button of code blocks and data: idle, done or failed for a moment, the result said in a live line that
// exists before the first press (a live region added with its text is not announced).
import { flushSync, mount, unmount, type ComponentProps } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { t } from "../i18n";
import CopyButton from "./CopyButton.svelte";

let component: Record<string, unknown> | null = null;

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

type Shown = Omit<ComponentProps<typeof CopyButton>, "label"> & { label?: string };

function show(props: Shown): HTMLElement {
  const target = document.createElement("div");
  document.body.append(target);
  component = mount(CopyButton, { target, props: { label: "Copy code", ...props } });
  flushSync();
  return target;
}

function live(target: HTMLElement): HTMLElement {
  const el = target.querySelector<HTMLElement>(".sr-only[aria-live]");
  if (el === null) throw new Error("no live line");
  return el;
}

function button(target: HTMLElement): HTMLButtonElement {
  const el = target.querySelector("button");
  if (!(el instanceof HTMLButtonElement)) throw new Error("no button");
  return el;
}

describe("the copy button", () => {
  it("has its live line, empty, before the first press, and its name as label and tooltip", () => {
    const target = show({ copy: "x" });
    expect(live(target).textContent).toBe("");
    expect(live(target).getAttribute("aria-live")).toBe("polite");
    expect(button(target).getAttribute("aria-label")).toBe("Copy code");
    expect(button(target).title).toBe("Copy code");
    // the live line is beside the button, not in it (a button's content is not read as a region)
    expect(button(target).contains(live(target))).toBe(false);
  });

  it("copies a text and says so, then goes back to idle", async () => {
    vi.useFakeTimers();
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    const target = show({ copy: "fn main() {}", class: "copy" });
    button(target).click();
    await vi.waitFor(() => expect(live(target).textContent).toBe(t("edit.copied")));
    expect(writeText).toHaveBeenCalledWith("fn main() {}");
    expect(button(target).classList.contains("is-done")).toBe(true);
    expect(button(target).classList.contains("copy")).toBe(true);
    vi.advanceTimersByTime(1600);
    flushSync();
    expect(live(target).textContent).toBe("");
    expect(button(target).classList.contains("is-done")).toBe(false);
  });

  it("says a refused clipboard write instead of staying silent", async () => {
    const writeText = vi.fn().mockRejectedValue(new Error("denied"));
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    const target = show({ copy: "secret" });
    button(target).click();
    await vi.waitFor(() => expect(live(target).textContent).toBe(t("doc.copyFailed")));
    expect(button(target).classList.contains("is-failed")).toBe(true);
    expect(button(target).classList.contains("is-done")).toBe(false);
  });

  it("runs a copy step and reads its answer; a failure shows its own words when not compact", async () => {
    let worked = false;
    const target = show({
      copy: () => Promise.resolve(worked),
      label: "Copy as JSON",
      failedLabel: "Could not copy",
      compact: false,
    });
    expect(button(target).textContent?.trim()).toBe("Copy as JSON");
    button(target).click();
    await vi.waitFor(() => expect(live(target).textContent).toBe(t("doc.copyFailed")));
    expect(button(target).textContent?.trim()).toBe("Could not copy");
    worked = true;
    button(target).click();
    await vi.waitFor(() => expect(live(target).textContent).toBe(t("edit.copied")));
    expect(button(target).textContent?.trim()).toBe("Copy as JSON");
  });

  it("counts a copy step that throws as failed", async () => {
    const target = show({ copy: () => Promise.reject(new Error("no")) });
    button(target).click();
    await vi.waitFor(() => expect(live(target).textContent).toBe(t("doc.copyFailed")));
  });
});
