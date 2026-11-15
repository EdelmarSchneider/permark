// @vitest-environment jsdom
// The export flow against the mock backend: refusal of unsaved changes, HTML straight to the Save dialog, the page
// setup offered and remembered, Cancel of a run, the Save dialog closed, the notices and their buttons, the dialog.
import "../../core/testing/media";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { api } = await import("../../api");
const { PluginClient } = await import("../../core/plugins/client");
const { MockPluginError } = await import("../../api/mockOrganizing");
const { ExportStore, exporter } = await import("./store.svelte");
const { default: SetupDialog } = await import("./SetupDialog.svelte");

import type { NoticeAction } from "../../core/window.svelte";
import type { ExportOptions, ExportOutcome } from "./builder";
import type { ExportDeps } from "./store.svelte";

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;
const probe = mock.probe.export;
const DOC = "docs/guide.md";

type Shown = { text: string; actions: readonly NoticeAction[]; lasting: boolean; id: number };

/** Notices as the window would show them (the last one shown is the last of `shown`). */
function notices() {
  const shown: Shown[] = [];
  let next = 1;
  return {
    shown,
    notifyKeyed: (_key: string, text: string, ...actions: NoticeAction[]) => {
      shown.push({ text, actions, lasting: false, id: next++ });
    },
    notifyLasting: (body: { text: string; actions: readonly NoticeAction[] }) => {
      const id = next++;
      shown.push({ ...body, lasting: true, id });
      return id;
    },
    dismiss: (id: number) => {
      const at = shown.findIndex((n) => n.id === id);
      if (at >= 0) shown.splice(at, 1);
    },
    dismissKey: () => {},
  };
}

const done = (notes: number = 0): ExportOutcome => ({
  status: "done",
  html: "<!doctype html><p>page</p>",
  notes: Array.from({ length: notes }, (_, i) => ({ kind: "picture" as const, name: `p${i}.png` })),
});

function setup(over: Partial<ExportDeps> = {}) {
  const notes = notices();
  const built: ExportOptions[] = [];
  const store = new ExportStore({
    client: new PluginClient(mock, () => null),
    build: (_path, options) => {
      built.push(options);
      return Promise.resolve(done());
    },
    notices: notes,
    unsaved: () => false,
    env: () => ({ assetUrl: (p) => p }),
    region: () => "de-DE",
    progressAfterMs: 60_000,
    ...over,
  });
  return { store, notes: notes.shown, built };
}

const last = (shown: Shown[]): Shown | undefined => shown.at(-1);

beforeEach(() => {
  probe.calls.length = 0;
  probe.stored = null;
  probe.next = null;
  probe.fail = null;
  probe.saved.clear();
  probe.capabilities = { pdf: true, print: true, pageNumbers: true };
});

describe("the export flow", () => {
  it("refuses a document with unsaved changes and builds nothing", async () => {
    const { store, notes: shown, built } = setup({ unsaved: (path) => path === DOC });
    await store.start("pdf", DOC);
    await store.start("html", DOC);
    expect(store.state.status).toBe("closed");
    expect(built).toEqual([]);
    expect(probe.calls).toEqual([]);
    expect(last(shown)?.text).toBe(
      "Save your changes first: PDF, HTML and printing use the saved file.",
    );
  });

  it("HTML skips the page setup: straight to the Save dialog, then Saved with Open | Show in folder", async () => {
    const { store, notes: shown, built } = setup();
    await store.start("html", DOC);
    expect(probe.calls).toEqual(["saveHtml"]);
    expect(built[0]?.target).toBe("html");
    expect(built[0]?.pageNumbers).toBe(false);
    expect(store.state.status).toBe("closed");
    const notice = last(shown);
    expect(notice?.text).toBe("Saved guide.html.");
    expect(notice?.actions.map((a) => a.label)).toEqual(["Open", "Show in folder"]);
    await notice?.actions[0]?.run();
    await notice?.actions[1]?.run();
    expect(probe.calls).toEqual(["saveHtml", "open", "reveal"]);
  });

  it("first run offers the paper of the person's region; a stored choice wins", async () => {
    const { store } = setup({ region: () => "en-US" });
    await store.start("pdf", DOC);
    expect(store.state.status === "setup" && store.state.setup.size).toBe("letter");
    store.close();

    probe.stored = { size: "a3", orientation: "landscape", margins: "wide", pageNumbers: false };
    await store.start("print", DOC);
    expect(store.state.status === "setup" && store.state.setup).toEqual(probe.stored);
  });

  it("remembers the setup when a button runs and builds with its page box", async () => {
    const { store, built, notes: shown } = setup();
    await store.start("pdf", DOC);
    store.change({ size: "a5", orientation: "landscape" });
    await store.go("pdf");
    expect(probe.stored).toEqual({
      size: "a5",
      orientation: "landscape",
      margins: "normal",
      pageNumbers: true,
    });
    expect(built[0]?.target).toBe("paper");
    expect(built[0]?.page).toEqual({ widthMm: 210, heightMm: 148, marginMm: 20 });
    expect(built[0]?.pageNumbers).toBe(true);
    expect(probe.calls).toEqual(["setup.get", "setup.set", "savePdf"]);
    expect(last(shown)?.text).toBe("Saved guide.pdf.");
  });

  it("no page numbers where the web view cannot draw them, whatever the setup says", async () => {
    probe.capabilities = { pdf: true, print: true, pageNumbers: false };
    const { store, built } = setup();
    await store.start("print", DOC);
    await store.go("print");
    expect(built[0]?.pageNumbers).toBe(false);
    expect(probe.calls).toContain("print");
    expect(store.state.status).toBe("closed");
  });

  it("closing the Save dialog goes back to the page setup", async () => {
    const { store, notes: shown } = setup();
    await store.start("pdf", DOC);
    store.change({ margins: "narrow" });
    probe.next = { outcome: "cancelled" };
    await store.go("pdf");
    expect(store.state.status).toBe("setup");
    expect(store.state.status === "setup" && store.state.setup.margins).toBe("narrow");
    expect(store.state.status === "setup" && store.state.busy).toBe(false);
    expect(shown).toEqual([]);
  });

  it("a slow run shows progress with Cancel; Cancel stops the build and tells the backend", async () => {
    const { store, notes: shown } = setup({
      progressAfterMs: 0,
      build: (_path, options) =>
        new Promise((resolve) => {
          options.signal?.addEventListener("abort", () => resolve({ status: "cancelled" }));
        }),
    });
    const run = store.start("html", DOC);
    await new Promise((resolve) => setTimeout(resolve, 5));
    const progress = last(shown);
    expect(progress?.lasting).toBe(true);
    expect(progress?.text).toBe("Making the HTML page…");
    await progress?.actions[0]?.run();
    await run;
    expect(probe.calls).toEqual(["cancel"]);
    expect(shown.map((n) => n.text)).toEqual(["Canceled."]);
    expect(store.state.status).toBe("closed");
  });

  it("left-out items are counted in the Saved notice", async () => {
    const { store, notes: shown } = setup({ build: () => Promise.resolve(done(2)) });
    await store.start("html", DOC);
    expect(last(shown)?.text).toBe("Saved guide.html. 2 items couldn't be included.");
  });

  it("a refusal says what to do", async () => {
    const cases: [InstanceType<typeof MockPluginError>, string][] = [
      [
        new MockPluginError("denied", "C:/x/guide.html"),
        "Couldn't save guide.html. The folder may be read-only or full.",
      ],
      [
        new MockPluginError("conflict", null, "source"),
        "The document itself can't be replaced. Choose another name.",
      ],
      [
        new MockPluginError("unsupported", "print"),
        "PDF and printing are not available on this system.",
      ],
    ];
    for (const [fail, text] of cases) {
      const { store, notes: shown } = setup();
      probe.fail = fail;
      await store.start("html", DOC);
      expect(last(shown)?.text).toBe(text);
      expect(store.state.status).toBe("closed");
    }
  });
});

describe("the page setup dialog", () => {
  let component: Record<string, unknown> | null = null;
  afterEach(() => {
    exporter.close();
    if (component) void unmount(component);
    component = null;
  });

  it("the entry the person came from is the primary button; page numbers say when unavailable", async () => {
    probe.capabilities = { pdf: true, print: true, pageNumbers: false };
    component = mount(SetupDialog, { target: document.body });
    await exporter.start("print", DOC);
    flushSync();
    await tick();
    const primary = document.querySelector(".dialog .btn--primary");
    expect(primary?.textContent?.trim()).toBe("Print…");
    expect(document.activeElement).toBe(primary);
    const box = document.querySelector<HTMLInputElement>(".dialog input[type=checkbox]");
    expect(box?.disabled).toBe(true);
    expect(document.body.textContent).toContain("Page numbers are not available on this system.");
    expect(document.body.textContent).toContain("Prints in light colors, whatever your theme.");
    const sizes = Array.from(document.querySelectorAll(".dialog option")).map((o) => o.textContent);
    expect(sizes).toContain("US Letter (8.5 × 11 in)");
  });

  it("Enter on a choice runs the primary button; Escape closes", async () => {
    component = mount(SetupDialog, { target: document.body });
    await exporter.start("pdf", DOC);
    flushSync();
    expect(document.querySelector(".dialog .btn--primary")?.textContent?.trim()).toBe("Save PDF…");
    const radio = document.querySelector<HTMLElement>('.dialog [role="radio"]');
    radio?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }),
    );
    await tick();
    expect(probe.calls).toContain("setup.set");
    exporter.close();
    await exporter.start("pdf", DOC);
    flushSync();
    document
      .querySelector(".dialog")
      ?.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
      );
    flushSync();
    expect(exporter.state.status).toBe("closed");
    expect(document.querySelector(".dialog")).toBeNull();
  });
});
