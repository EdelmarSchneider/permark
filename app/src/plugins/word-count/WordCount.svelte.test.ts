// @vitest-environment jsdom
// The status item's numbers for long texts and long selections: counted by the backend after a pause (counting here
// would hold the window), never shown for a text they were not made for.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Measure, Result, TextKind } from "../../api";
import type { AnalysisText } from "../../core/doc/analysisText.svelte";

class Source implements AnalysisText {
  countable = true;
  markdown = false;
  text = $state("");
  selection = $state("");
  selectionMarkdown = false;
  path = $state<string | null>("a.md");
  editing = false;
}
const source = new Source();

type Asked = { text: string; kind: TextKind; answer: (words: number) => void };
const asked: Asked[] = [];

vi.mock("../../api", () => ({
  api: {
    analyze: (_path: string | null, text: string, kind: TextKind) =>
      new Promise<Result<Measure[]>>((resolve) => {
        asked.push({
          text,
          kind,
          answer: (words) =>
            resolve({
              ok: true,
              value: [
                { id: "word-count.words", value: words, unit: "words" },
                { id: "word-count.characters", value: text.length, unit: "characters" },
              ],
            }),
        });
      }),
  },
}));
vi.mock("../../core/doc/analysisText.svelte", () => ({ createAnalysisText: () => source }));
const measured: number[] = [];
vi.mock("./count", async (original) => {
  const real = await original<typeof import("./count")>();
  return {
    ...real,
    measure: (text: string, kind: TextKind) => {
      measured.push(text.length);
      return real.measure(text, kind);
    },
  };
});

const { default: WordCount } = await import("./WordCount.svelte");
const { LOCAL_LIMIT } = await import("./count");

let component: Record<string, unknown> | null = null;
let target: HTMLElement;
const label = (): string => (target.querySelector(".count")?.textContent ?? "").trim();

beforeEach(() => {
  vi.useFakeTimers();
  target = document.createElement("div");
  document.body.replaceChildren(target);
  source.text = "one two three";
  source.selection = "";
  source.path = "a.md";
  asked.length = 0;
  measured.length = 0;
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  vi.useRealTimers();
});

it("a long selection is never measured here: the backend counts it once, after a pause", async () => {
  component = mount(WordCount, { target });
  flushSync();
  const big = "word ".repeat(60_000);
  expect(big.length).toBeGreaterThan(LOCAL_LIMIT);
  source.selection = big;
  flushSync();
  expect(label()).toBe("3 words");
  expect(measured.filter((n) => n > LOCAL_LIMIT)).toEqual([]);
  // the selection still growing: no ask before the pause
  source.selection = `${big}word `;
  flushSync();
  await vi.advanceTimersByTimeAsync(599);
  expect(asked).toEqual([]);
  await vi.advanceTimersByTimeAsync(1);
  expect(asked.map((a) => [a.text.length, a.kind])).toEqual([[big.length + 5, "plain"]]);
  asked[0]?.answer(60_001);
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
  expect(label()).toBe("60,001 of 3 words");
  expect(measured.filter((n) => n > LOCAL_LIMIT)).toEqual([]);
});

it("an answer for an older long text never shows for the text after it", async () => {
  const first = "alpha ".repeat(40_000);
  const second = "beta ".repeat(50_000);
  source.text = first;
  component = mount(WordCount, { target });
  flushSync();
  await vi.advanceTimersByTimeAsync(600);
  // another document before the answer came
  source.path = "b.md";
  source.text = second;
  flushSync();
  await vi.advanceTimersByTimeAsync(600);
  expect(asked.map((a) => a.text.length)).toEqual([first.length, second.length]);
  asked[1]?.answer(50_000);
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
  expect(label()).toBe("50,000 words");
  // the late answer about the first text changes nothing
  asked[0]?.answer(40_000);
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
  expect(label()).toBe("50,000 words");
});

it("the count of one long document is not shown for the next one", async () => {
  source.text = "alpha ".repeat(40_000);
  component = mount(WordCount, { target });
  flushSync();
  await vi.advanceTimersByTimeAsync(600);
  asked[0]?.answer(40_000);
  await vi.advanceTimersByTimeAsync(0);
  flushSync();
  expect(label()).toBe("40,000 words");
  source.path = "b.md";
  source.text = "beta ".repeat(50_000);
  flushSync();
  expect(label()).toBe("0 words");
});
