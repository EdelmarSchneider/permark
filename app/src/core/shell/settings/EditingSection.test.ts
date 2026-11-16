// @vitest-environment jsdom
// The groups file row of the Editing settings: its status line, the Create button (only while no file is there under
// a name), what a failed Create says, the Created news settling to the steady status, and Show in folder.
import "../../testing/media";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api, type ApiError, type CharacterGroup } from "../../../api";
import { windowStore } from "../../window.svelte";
import EditingSection from "./EditingSection.svelte";

const GROUPS: CharacterGroup[] = [
  { name: "greek", entries: [{ char: "α", names: ["alpha"] }] },
  { name: "arrows", entries: [{ char: "→", names: ["rightwards arrow"] }] },
];

const FAILURE: ApiError = {
  kind: "backend",
  command: "characters_create",
  error: { kind: "io", subject: "characters.toml", detail: "denied" },
};

let component: Record<string, unknown> | null = null;
let root: HTMLElement;
let refresh: ReturnType<typeof vi.spyOn>;

beforeEach(() => {
  refresh = vi.spyOn(windowStore, "refreshCharacters").mockImplementation(() => {});
  root = document.body.appendChild(document.createElement("div"));
});

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
  windowStore.charactersFile = "";
  windowStore.charactersStatus = "builtin";
  windowStore.characters = [];
  vi.useRealTimers();
  vi.restoreAllMocks();
});

function show(status: "builtin" | "read" | "unreadable", file: string): void {
  windowStore.charactersStatus = status;
  windowStore.charactersFile = file;
  windowStore.characters = GROUPS;
  component = mount(EditingSection, { target: root, props: { store: windowStore } });
  flushSync();
}

const status = (): string => root.querySelector(".status")?.textContent?.trim() ?? "";
const button = (name: string): HTMLButtonElement | null =>
  [...root.querySelectorAll<HTMLButtonElement>("button")].find(
    (b) => b.textContent?.trim() === name,
  ) ?? null;
const CREATE = "Create the file with the built-in groups";

/** Lets the button's awaited backend answer land and the page redraw. */
async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await tick();
  flushSync();
}

describe("the groups file row", () => {
  it("reads the groups file again once when it opens", () => {
    show("builtin", "characters.toml");
    expect(refresh).toHaveBeenCalledTimes(1);
  });

  it("says where the groups come from", () => {
    show("builtin", "characters.toml");
    expect(status()).toBe("Using the built-in groups. There is no file yet.");
    windowStore.charactersFile = "";
    flushSync();
    // no file name: the built-ins by choice, no file is missing
    expect(status()).toBe("Using the built-in groups.");
    windowStore.charactersFile = "mine.toml";
    windowStore.charactersStatus = "read";
    flushSync();
    expect(status()).toBe("Using 2 groups from mine.toml.");
    windowStore.charactersStatus = "unreadable";
    flushSync();
    expect(status()).toBe("mine.toml could not be read, so the built-in groups are shown.");
  });

  it("offers Create only while the built-ins stand in for a named file", () => {
    show("builtin", "characters.toml");
    expect(button(CREATE)).not.toBeNull();
    windowStore.charactersFile = "  ";
    flushSync();
    expect(button(CREATE)).toBeNull();
    windowStore.charactersFile = "characters.toml";
    windowStore.charactersStatus = "read";
    flushSync();
    expect(button(CREATE)).toBeNull();
    windowStore.charactersStatus = "unreadable";
    flushSync();
    expect(button(CREATE)).toBeNull();
  });

  it("a failed Create says so", async () => {
    vi.spyOn(api, "charactersCreate").mockResolvedValue({ ok: false, error: FAILURE });
    show("builtin", "characters.toml");
    button(CREATE)?.click();
    await settle();
    expect(status()).toBe("Could not create characters.toml.");
  });

  it("Created shows once, then the line settles to the steady status", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    vi.spyOn(api, "charactersCreate").mockImplementation(() => {
      // the backend's `settings-changed` lands before its answer
      windowStore.charactersStatus = "read";
      return Promise.resolve({ ok: true, value: true });
    });
    show("builtin", "characters.toml");
    button(CREATE)?.click();
    await settle();
    expect(status()).toBe("Created characters.toml.");
    vi.advanceTimersByTime(10_000);
    flushSync();
    expect(status()).toBe("Using 2 groups from characters.toml.");
  });

  it("Show in folder asks the backend to show the file", async () => {
    const reveal = vi.spyOn(api, "charactersReveal").mockResolvedValue({ ok: true, value: null });
    show("read", "characters.toml");
    button("Show in folder")?.click();
    await settle();
    expect(reveal).toHaveBeenCalledTimes(1);
    expect(status()).toBe("Using 2 groups from characters.toml.");
  });
});
