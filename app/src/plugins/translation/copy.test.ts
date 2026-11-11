// @vitest-environment jsdom
// A document translated into a new file: the name the file gets, a whole run against the mock backend (what is
// written, where, what the person is told), the runs that write nothing (stopped, failed, nothing translated) and
// the status bar item that shows a run with its Cancel.
import "../../core/testing/media";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi } from "../../api/mock";
import { MOCK_CRLF_BOM_DOC } from "../../api/mockFs";
import { mockTranslate } from "../../api/mockTranslation";
import type { PieceRunPhase } from "../../api/generated/PieceRunPhase";
import { ui } from "../../core/shell/ui.svelte";
import type { NoticeAction } from "../../core/window.svelte";
import { copyName, TranslationCopy, type CopySession, type CopyWindow } from "./copy.svelte";
import CopyStatus from "./CopyStatus.svelte";
import { Languages } from "./store.svelte";

describe("the name of a translated copy", () => {
  it("carries the language before the extension", () => {
    expect(copyName("readme.md", "pt-BR")).toBe("readme.pt-BR.md");
    expect(copyName("Guide.markdown", "de")).toBe("Guide.de.markdown");
  });

  it("replaces the language the name already carries, in any letter case", () => {
    expect(copyName("readme.en.md", "pt-BR")).toBe("readme.pt-BR.md");
    expect(copyName("README.PT-br.md", "de")).toBe("README.de.md");
    expect(copyName("guide.zh-Hans.md", "ja")).toBe("guide.ja.md");
    // a region the list does not hold, of a language it does
    expect(copyName("README.zh-CN.md", "en-US")).toBe("README.en-US.md");
    expect(copyName("readme.de.md", "de")).toBe("readme.de.md");
  });

  it("a name without extension gets the language at its end", () => {
    expect(copyName("LICENSE", "de")).toBe("LICENSE.de");
    expect(copyName(".notes", "de")).toBe(".notes.de");
  });

  it("keeps the other dotted parts of a name", () => {
    expect(copyName("notes.v2.md", "de")).toBe("notes.v2.de.md");
    expect(copyName("notes.v2.en.md", "de")).toBe("notes.v2.de.md");
  });

  it("a part that only looks like a language stays", () => {
    expect(copyName("data.xx.md", "de")).toBe("data.xx.de.md");
    expect(copyName("todo.tmp.md", "de")).toBe("todo.tmp.de.md");
    expect(copyName("plan.is-final.md", "de")).toBe("plan.is-final.de.md");
    // the name itself, not a part after it
    expect(copyName("en.md", "de")).toBe("en.de.md");
    expect(copyName(".en.md", "de")).toBe(".en.de.md");
  });
});

/** What the window was asked: documents opened, quiet notes, lines that stay (with their buttons); `folder`: the
 *  workspace it shows. */
type Asked = CopyWindow & {
  folder: number | null;
  opened: string[];
  notes: string[];
  stays: { text: string; actions: NoticeAction[] }[];
};

function windowProbe(): Asked {
  const asked: Asked = {
    folder: 7,
    opened: [],
    notes: [],
    stays: [],
    workspace: () => asked.folder,
    open: (path) => asked.opened.push(path),
    note: (text) => asked.notes.push(text),
    lasting: (text, ...actions) => asked.stays.push({ text, actions }),
  };
  return asked;
}

const GUIDE = "docs/guide.md";
const DOC = [
  "---",
  "title: Guide",
  "---",
  "",
  "# Getting started",
  "",
  "Open the folder and read.",
  "",
  "| Name | Use |",
  "| ---- | --- |",
  "| Tree | Files |",
  "",
  "```sh",
  "npm run build",
  "```",
  "",
  "> [!NOTE]",
  "> Keep `code` as it is.",
  "",
].join("\n");
/** The words of `DOC` a translation changes, in document order. */
const PROSE = [
  "Getting started",
  "Open the folder and read.",
  "Name",
  "Use",
  "Tree",
  "Files",
  "Keep ",
  " as it is.",
];
const THREE = "First paragraph here.\n\nSecond paragraph here.\n\nThird paragraph here.\n";

/** `text` as the mock model writes it in German. */
const german = (text: string): string => mockTranslate(text, "de");

let mock: ReturnType<typeof createMockApi>;
let session: { path: string | null; text: string } & CopySession;
let asked: Asked;
let languages: Languages;
let copy: TranslationCopy;
let stop: () => void;

beforeEach(async () => {
  mock = createMockApi();
  mock.probe.translation.stepMs = 1e9;
  session = { path: GUIDE, text: DOC, current: () => session.text };
  asked = windowProbe();
  languages = new Languages(mock);
  await languages.choose("de");
  copy = new TranslationCopy(languages, mock, session, asked);
  stop = copy.listen();
});

afterEach(() => {
  stop();
  mock.probe.translation.stopAll();
  ui.closeSettingsPage();
  vi.restoreAllMocks();
});

const sent = () => mock.probe.translation.runs;
const files = () => mock.probe.project.files;
/** Files of the mock project whose name says German. */
const germanFiles = (): string[] => [...files().keys()].filter((path) => path.includes(".de"));

function news(phase: PieceRunPhase): void {
  const run = sent().at(-1)?.run;
  if (run === undefined) throw new Error("no run sent");
  copy.news({ run, phase });
}

function answer(id: number, text: string): void {
  news({ kind: "piece", id, result: { kind: "output", text } });
}

/** The run's end told, and what follows it (the file written, the person told) settled. */
async function finished(): Promise<void> {
  news({ kind: "done" });
  await vi.waitFor(() => expect(copy.running).toBe(false));
}

describe("a document translated into a new file", () => {
  it("writes the translated text once, beside the original, opens it and says so", async () => {
    mock.probe.translation.stepMs = 1;
    const calls = vi.spyOn(mock, "pluginCall");
    await copy.run();
    expect(copy.state).toMatchObject({ kind: "running", file: "guide.md" });
    await vi.waitFor(() => expect(asked.opened).toEqual(["docs/guide.de.md"]));
    const expected = PROSE.reduce((text, part) => text.replace(part, german(part)), DOC);
    expect(expected).not.toBe(DOC);
    expect(files().get("docs/guide.de.md")).toBe(expected);
    // what is not prose is the original's, byte for byte
    for (const kept of [
      "---\ntitle: Guide\n---\n",
      "| ---- | --- |",
      "```sh\nnpm run build\n```",
      "> [!NOTE]\n",
      "`code`",
    ])
      expect(files().get("docs/guide.de.md")).toContain(kept);
    const saves = calls.mock.calls.filter(([method]) => method === "translation.save");
    expect(saves).toEqual([
      ["translation.save", { beside: GUIDE, name: "guide.de.md", text: expected }],
    ]);
    expect(files().get(GUIDE)).not.toBe(expected);
    expect(copy.state.kind).toBe("idle");
    expect(asked.notes).toEqual(["Translated to German: guide.de.md."]);
    expect(asked.stays).toEqual([]);
  });

  it("sends every prose part of the whole document, table cells and headings too, with the languages chosen", async () => {
    languages.from("en-US");
    await copy.run();
    expect(sent()).toHaveLength(1);
    expect(sent()[0]).toMatchObject({
      target: { tag: "de", name: "German" },
      source: { tag: "en-US", name: "American English" },
    });
    expect(sent()[0]?.pieces.map((piece) => piece.text)).toEqual([
      "Getting started",
      "Open the folder and read.",
      "Name",
      "Use",
      "Tree",
      "Files",
      "Keep ⟦1⟧ as it is.",
    ]);
  });

  it("a part that couldn't be translated stays in the original language and is counted in a note that stays", async () => {
    session.text = THREE;
    await copy.run();
    answer(0, "Erster Absatz hier.");
    news({ kind: "piece", id: 1, result: { kind: "discarded", reason: "tooLong" } });
    // an answer that is more than the translation
    answer(2, "Dritter Absatz hier.\n\nNote: I kept the tone.");
    await finished();
    expect(files().get("docs/guide.de.md")).toBe(
      "Erster Absatz hier.\n\nSecond paragraph here.\n\nThird paragraph here.\n",
    );
    expect(asked.opened).toEqual(["docs/guide.de.md"]);
    expect(asked.notes).toEqual([]);
    expect(asked.stays).toEqual([
      {
        text: "Translated to German: guide.de.md. 2 parts couldn't be translated and stay in the original language.",
        actions: [],
      },
    ]);
  });

  it("a part already in the language is left as it is, without a word", async () => {
    session.text = THREE;
    await copy.run();
    answer(0, "First paragraph here.");
    answer(1, "Zweiter Absatz hier.");
    answer(2, "Third paragraph here.");
    await finished();
    expect(files().get("docs/guide.de.md")).toBe(
      "First paragraph here.\n\nZweiter Absatz hier.\n\nThird paragraph here.\n",
    );
    expect(asked.notes).toEqual(["Translated to German: guide.de.md."]);
  });

  it("nothing translated because the text is already in the language: no file, and a note that says so", async () => {
    session.text = THREE;
    await copy.run();
    for (const { id, text } of sent()[0]?.pieces ?? []) answer(id, text);
    await finished();
    expect(germanFiles()).toEqual([]);
    expect(asked.opened).toEqual([]);
    expect(asked.stays).toEqual([
      {
        text: "Nothing was translated: the text is already in German. No file was created.",
        actions: [],
      },
    ]);
  });

  it("nothing translated because no part could be: no file, and a note that says so", async () => {
    // the mock model cannot write Thai: every answer is in the wrong script
    await languages.choose("th");
    mock.probe.translation.stepMs = 1;
    await copy.run();
    await vi.waitFor(() => expect(copy.running).toBe(false));
    expect([...files().keys()].filter((path) => path.includes(".th"))).toEqual([]);
    expect(asked.opened).toEqual([]);
    expect(asked.stays).toEqual([
      { text: "The text couldn't be translated. No file was created.", actions: [] },
    ]);
  });

  it("a taken name: the note names the file that was written, and that one opens", async () => {
    session.text = THREE;
    files().set("docs/guide.de.md", "taken\n");
    await copy.run();
    answer(0, "Erster Absatz hier.");
    answer(1, "Zweiter Absatz hier.");
    answer(2, "Dritter Absatz hier.");
    await finished();
    expect(files().get("docs/guide.de.md")).toBe("taken\n");
    expect(files().get("docs/guide.de (2).md")).toBe(
      "Erster Absatz hier.\n\nZweiter Absatz hier.\n\nDritter Absatz hier.\n",
    );
    expect(asked.opened).toEqual(["docs/guide.de (2).md"]);
    expect(asked.notes).toEqual(["Translated to German: guide.de (2).md."]);
  });

  it("replaces the language the original's name carries", async () => {
    session.path = "docs/guide.en.md";
    session.text = THREE;
    await copy.run();
    answer(0, "Erster Absatz hier.");
    await finished();
    expect(asked.opened).toEqual(["docs/guide.de.md"]);
  });

  it("writes the copy with the line ending of the original", async () => {
    session.path = MOCK_CRLF_BOM_DOC;
    session.text = THREE;
    await copy.run();
    answer(0, "Erster Absatz hier.");
    await finished();
    expect(files().get("docs/windows.de.md")).toBe(
      "Erster Absatz hier.\r\n\r\nSecond paragraph here.\r\n\r\nThird paragraph here.\r\n",
    );
  });

  it("goes on with the text and the place it started from, whatever the editor shows since", async () => {
    session.text = THREE;
    await copy.run();
    session.path = "notes/ideas.md";
    session.text = "Another document.\n";
    answer(0, "Erster Absatz hier.");
    await finished();
    expect(files().get("docs/guide.de.md")).toBe(
      "Erster Absatz hier.\n\nSecond paragraph here.\n\nThird paragraph here.\n",
    );
    expect(germanFiles()).toEqual(["docs/guide.de.md"]);
  });

  it("shows where the run stands: the engine, the model, then part by part", async () => {
    session.text = THREE;
    await copy.run();
    expect(copy.state).toMatchObject({ status: { kind: "loading", progress: 0 } });
    news({ kind: "engine", receivedBytes: 10, totalBytes: 40 });
    expect(copy.state).toMatchObject({
      status: { kind: "engine", receivedBytes: 10, totalBytes: 40 },
    });
    news({ kind: "loading", progress: 0.5 });
    expect(copy.state).toMatchObject({ status: { kind: "loading", progress: 0.5 } });
    answer(0, "Erster Absatz hier.");
    expect(copy.state).toMatchObject({ status: { kind: "checking", part: 2, parts: 3 } });
    answer(1, "Zweiter Absatz hier.");
    answer(2, "Dritter Absatz hier.");
    expect(copy.state).toMatchObject({ status: { kind: "checking", part: 3, parts: 3 } });
  });
});

describe("a copy that writes nothing", () => {
  it("cancelled: the run is stopped, no file, and its late news change nothing", async () => {
    session.text = THREE;
    await copy.run();
    const run = sent()[0]?.run ?? "";
    answer(0, "Erster Absatz hier.");
    copy.cancel();
    expect(copy.state.kind).toBe("idle");
    expect(mock.probe.translation.calls).toContain(`cancel ${run}`);
    copy.news({ run, phase: { kind: "done" } });
    await Promise.resolve();
    expect(germanFiles()).toEqual([]);
    expect(asked.opened).toEqual([]);
    expect(asked.notes).toEqual(["Translation stopped. No file was created."]);
    expect(asked.stays).toEqual([]);
  });

  it("stopped by the backend: no file, and a note that stays", async () => {
    session.text = THREE;
    await copy.run();
    answer(0, "Erster Absatz hier.");
    news({ kind: "cancelled" });
    expect(copy.state.kind).toBe("idle");
    expect(germanFiles()).toEqual([]);
    expect(asked.stays).toEqual([
      { text: "Translation stopped. No file was created.", actions: [] },
    ]);
  });

  it("the window shows another folder meanwhile: the run is stopped, nothing is written there", async () => {
    session.text = THREE;
    await copy.run();
    const run = sent()[0]?.run ?? "";
    answer(0, "Erster Absatz hier.");
    asked.folder = 8;
    answer(1, "Zweiter Absatz hier.");
    expect(copy.state.kind).toBe("idle");
    expect(mock.probe.translation.calls).toContain(`cancel ${run}`);
    expect(germanFiles()).toEqual([]);
    expect(asked.opened).toEqual([]);
    expect(asked.stays).toEqual([
      { text: "Translation stopped. No file was created.", actions: [] },
    ]);
  });

  it("the window shows another folder when the run is done: nothing is written there", async () => {
    session.text = THREE;
    await copy.run();
    answer(0, "Erster Absatz hier.");
    answer(1, "Zweiter Absatz hier.");
    answer(2, "Dritter Absatz hier.");
    asked.folder = null;
    await finished();
    expect(germanFiles()).toEqual([]);
    expect(asked.opened).toEqual([]);
    expect(asked.stays).toEqual([
      { text: "Translation stopped. No file was created.", actions: [] },
    ]);
  });

  it("failed: no file, why in the words of the Local AI page, and the way to its settings", async () => {
    session.text = THREE;
    await copy.run();
    answer(0, "Erster Absatz hier.");
    news({
      kind: "failed",
      error: { kind: "unavailable", subject: "engine", detail: "outOfMemory" },
    });
    expect(copy.state.kind).toBe("idle");
    expect(germanFiles()).toEqual([]);
    expect(asked.opened).toEqual([]);
    expect(asked.stays.map((stay) => stay.text)).toEqual([
      "The text couldn't be translated. Not enough free memory right now. Close other apps and try again, or choose a smaller model.",
    ]);
    const [action] = asked.stays[0]?.actions ?? [];
    expect(action?.label).toBe("Open Local AI settings");
    action?.run();
    expect(ui.settingsPage).toEqual({ status: "open", section: "local-ai.page" });
  });

  it.each([
    [
      { kind: "unavailable", subject: "model", detail: "noModel" },
      "The model chosen for translation isn't available. Choose one on the Local AI page.",
    ],
    [
      { kind: "unavailable", subject: "engine", detail: "notAvailable" },
      "The AI engine isn't available for this computer yet.",
    ],
    [
      { kind: "unavailable", subject: "engine", detail: "engineTooOld" },
      "This model needs a newer version of the AI engine.",
    ],
  ] as const)("failed (%o): says why", async (error, why) => {
    await copy.run();
    news({ kind: "failed", error });
    expect(asked.stays.map((stay) => stay.text)).toEqual([
      `The text couldn't be translated. ${why}`,
    ]);
  });

  it("a run the backend refuses to start fails the same way", async () => {
    mock.probe.translation.failRun = null;
    vi.spyOn(mock, "pluginCall").mockResolvedValueOnce({
      ok: false,
      error: {
        kind: "backend",
        command: "plugin_call",
        error: { kind: "unavailable", subject: "model", detail: "noModel" },
      },
    });
    await copy.run();
    expect(copy.state.kind).toBe("idle");
    expect(asked.stays).toHaveLength(1);
    expect(asked.stays[0]?.text).toContain("Choose one on the Local AI page.");
  });

  it("a file that cannot be written: said, nothing opens", async () => {
    session.text = THREE;
    await copy.run();
    answer(0, "Erster Absatz hier.");
    const call = mock.pluginCall.bind(mock);
    vi.spyOn(mock, "pluginCall").mockImplementation((method, args) =>
      method === "translation.save"
        ? Promise.resolve({
            ok: false,
            error: {
              kind: "backend",
              command: "plugin_call",
              error: { kind: "exists", subject: "guide.de.md", detail: null },
            },
          })
        : call(method, args),
    );
    await finished();
    expect(asked.opened).toEqual([]);
    expect(asked.stays).toHaveLength(1);
    expect(asked.stays[0]?.text).toMatch(/^The translated file couldn't be saved\. \S/);
  });

  it("a document with no text to translate: no run, and a note", async () => {
    session.text = "```\ncode only\n```\n";
    await copy.run();
    expect(sent()).toEqual([]);
    expect(copy.state.kind).toBe("idle");
    expect(asked.notes).toEqual(["There's no text to translate here."]);
  });

  it("no second run while one goes on, none without a language or a document", async () => {
    await copy.run();
    await copy.run();
    expect(sent()).toHaveLength(1);
    copy.cancel();
    languages.reset();
    await copy.run();
    session.path = null;
    await languages.choose("de");
    await copy.run();
    expect(sent()).toHaveLength(1);
  });

  it("let go mid-run (the plugin goes off): the run is stopped, nothing is written or said", async () => {
    session.text = THREE;
    await copy.run();
    const run = sent()[0]?.run ?? "";
    answer(0, "Erster Absatz hier.");
    stop();
    expect(copy.state.kind).toBe("idle");
    expect(mock.probe.translation.calls).toContain(`cancel ${run}`);
    expect(germanFiles()).toEqual([]);
    expect(asked).toMatchObject({ opened: [], notes: [], stays: [] });
  });
});

describe("the status bar item of a copy", () => {
  let target: HTMLElement;
  let component: Record<string, unknown>;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.append(target);
    component = mount(CopyStatus, { target, props: { copy } });
    flushSync();
  });

  afterEach(async () => {
    await unmount(component);
    target.remove();
  });

  const said = (): string => (target.textContent ?? "").replace(/\s+/g, " ").trim();
  const label = (): string =>
    (target.querySelector(".label")?.textContent ?? "").replace(/\s+/g, " ").trim();
  const cancel = (): HTMLButtonElement | null => target.querySelector("button");

  it("draws nothing until a copy runs, and nothing after it", async () => {
    expect(said()).toBe("");
    expect(target.querySelector("button")).toBeNull();
    session.text = THREE;
    await copy.run();
    flushSync();
    expect(said()).not.toBe("");
    answer(0, "Erster Absatz hier.");
    await finished();
    flushSync();
    expect(said()).toBe("");
  });

  it("says where the run stands, in the words of the review bar while the model gets ready", async () => {
    session.text = THREE;
    await copy.run();
    flushSync();
    expect(label()).toBe("Getting the model ready… 0%");
    news({ kind: "engine", receivedBytes: 10, totalBytes: 40 });
    flushSync();
    expect(label()).toBe("Getting the AI engine (first time only) 25%");
    news({ kind: "loading", progress: 0.5 });
    flushSync();
    expect(label()).toBe("Getting the model ready… 50%");
    answer(0, "Erster Absatz hier.");
    flushSync();
    expect(label()).toBe("Translating 2 / 3");
  });

  it("its Cancel names the file and stops the run", async () => {
    session.text = THREE;
    await copy.run();
    flushSync();
    const button = cancel();
    expect(button?.textContent?.trim()).toBe("Cancel");
    expect(button?.getAttribute("aria-label")).toBe("Cancel translation of guide.md");
    button?.click();
    flushSync();
    expect(copy.state.kind).toBe("idle");
    expect(mock.probe.translation.calls).toContain(`cancel ${sent()[0]?.run ?? ""}`);
    expect(said()).toBe("");
    expect(germanFiles()).toEqual([]);
  });
});
