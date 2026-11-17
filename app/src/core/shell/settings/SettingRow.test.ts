// @vitest-environment jsdom
// A row of the settings page may say who its value is for: everyone sharing the folder (kept in its config file) or
// only this person on this computer. Short words on screen, the whole sentence in the control's name.
import { createRawSnippet, flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import SettingRow from "./SettingRow.svelte";

let component: ReturnType<typeof mount> | null = null;
const target = document.body.appendChild(document.createElement("div"));

afterEach(() => {
  if (component) unmount(component);
  component = null;
  target.innerHTML = "";
});

const control = createRawSnippet(() => ({ render: () => '<input id="row-control" />' }));

function show(scope?: "everyone" | "you", ignored?: boolean): void {
  component = mount(SettingRow, {
    target,
    props: { label: "Files to hide", labelFor: "row-control", scope, ignored, children: control },
  });
  flushSync();
}

/** What a screen reader says for the control: its label's words, without the parts hidden from it. */
function nameOf(input: HTMLElement): string {
  const label = target.querySelector<HTMLElement>(`label[for="${input.id}"]`);
  const copy = label?.cloneNode(true) as HTMLElement | undefined;
  copy?.querySelectorAll('[aria-hidden="true"]').forEach((el) => el.remove());
  return (copy?.textContent ?? "").replace(/\s+/g, " ").trim();
}

/** The words on screen: hidden-from-sight text left out. */
function seen(): string {
  const copy = target.cloneNode(true) as HTMLElement;
  copy.querySelectorAll(".sr-only").forEach((el) => el.remove());
  return (copy.textContent ?? "").replace(/\s+/g, " ").trim();
}

describe("the scope chip of a setting row", () => {
  it("everyone: short on screen, the file in the control's name", () => {
    show("everyone");
    const input = target.querySelector<HTMLElement>("#row-control")!;
    expect(seen()).toBe("Files to hide Everyone");
    expect(nameOf(input)).toBe("Files to hide Everyone, kept in .permark/config.toml");
  });

  it("only you: short on screen, this computer in the control's name", () => {
    show("you");
    const input = target.querySelector<HTMLElement>("#row-control")!;
    expect(seen()).toBe("Files to hide Only you");
    expect(nameOf(input)).toBe("Files to hide Only you, on this computer");
  });

  it("no scope: no chip (app rows)", () => {
    show();
    expect(target.querySelector(".scope-chip")).toBeNull();
    expect(seen()).toBe("Files to hide");
  });

  it("ignored: a quiet line says the folder's file value is not used (the caller decides)", () => {
    show(undefined, true);
    expect(seen()).toContain("Set in .permark/config.toml; not used here");
  });

  it("not ignored: no line", () => {
    show(undefined, false);
    expect(seen()).toBe("Files to hide");
  });
});
