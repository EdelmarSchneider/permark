// @vitest-environment jsdom
// The figures of a selection longer than what is counted here: no number from this window, the backend counts it.
import { flushSync } from "svelte";
import { afterEach, expect, it, vi } from "vitest";
import type { AnalysisText } from "../../core/doc/analysisText.svelte";
import type { TokenFigures } from "./figures.svelte";
import { infoOf, type TokenizerInfo } from "./tokenizers";

const calls: { tokenizers: string[]; text: string }[] = [];
vi.mock("../../api", () => ({
  api: {
    // the backend never answers in these tests: what shows is what this window has before an answer
    pluginCall: (_method: string, args: { tokenizers: string[]; text: string }) => {
      calls.push(args);
      return new Promise(() => {});
    },
  },
}));

const estimates: number[] = [];
vi.mock("./estimate", async (original) => {
  const real = await original<typeof import("./estimate")>();
  return {
    ...real,
    estimate: (id: Parameters<typeof real.estimate>[0], text: string) => {
      estimates.push(text.length);
      return real.estimate(id, text);
    },
  };
});

const claude = infoOf("claude");
vi.mock("./store.svelte", () => ({
  tokens: { popover: false, chosen: [infoOf("claude")], ready: () => true },
}));

const { createFigures, LOCAL_LIMIT } = await import("./figures.svelte");

const stops: (() => void)[] = [];

afterEach(() => {
  for (const stop of stops.splice(0)) stop();
  calls.length = 0;
  estimates.length = 0;
  vi.useRealTimers();
});

function figures(text: string, selection: string): TokenFigures {
  const source: AnalysisText = {
    countable: true,
    markdown: false,
    text,
    selection,
    selectionMarkdown: false,
    path: "a.md",
    editing: false,
  };
  let made: TokenFigures | null = null;
  stops.push(
    $effect.root(() => {
      made = createFigures(source);
    }),
  );
  flushSync();
  if (made === null) throw new Error("no figures");
  return made;
}

it("a selection longer than the local limit shows no number until the backend counts it", async () => {
  vi.useFakeTimers();
  if (claude === null) throw new Error("no tokenizer");
  const info: TokenizerInfo = claude;
  const big = "word ".repeat(60_000);
  expect(big.length).toBeGreaterThan(LOCAL_LIMIT);
  const shown = figures("short text", big);
  expect(shown.selection(info)).toBeNull();
  expect(estimates.filter((n) => n > LOCAL_LIMIT)).toEqual([]);
  // the estimator tokenizer is asked of the backend for that selection
  await vi.advanceTimersByTimeAsync(10);
  expect(calls.some((c) => c.text === big && c.tokenizers.includes("claude"))).toBe(true);
});

it("a short selection keeps its estimate at once", () => {
  if (claude === null) throw new Error("no tokenizer");
  const shown = figures("short text and more", "short");
  expect(shown.selection(claude)).toEqual({ value: expect.any(Number), approx: true });
});
