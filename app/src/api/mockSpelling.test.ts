import { afterEach, describe, expect, it, vi } from "vitest";
import { createMockSpelling } from "./mockSpelling";
import type { AppPluginEventName, AppPluginEvents } from "./types";

function setup() {
  const heard: { event: AppPluginEventName; payload: unknown }[] = [];
  const spelling = createMockSpelling(
    <E extends AppPluginEventName>(event: E, payload: AppPluginEvents[E]) => {
      heard.push({ event, payload });
    },
  );
  return { ...spelling, heard };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("the mock spelling plugin", () => {
  it("knows English words at once, in their capitalized and upper forms", () => {
    const { handlers } = setup();
    expect(
      handlers["spelling.check"]({ langs: ["en"], words: ["hello", "Hello", "WORLD", "helo"] }),
    ).toEqual([true, true, true, false]);
    expect(handlers["spelling.suggest"]({ langs: ["en"], word: "helo" })).toContain("hello");
  });

  it("refuses languages that are not ready and checks a downloaded one", () => {
    vi.useFakeTimers();
    const { handlers, heard } = setup();
    expect(() => handlers["spelling.check"]({ langs: ["pt-BR"], words: ["casa"] })).toThrow();
    handlers["spelling.download"]({ lang: "pt-BR" });
    vi.advanceTimersByTime(1000);
    expect(heard.at(-1)).toEqual({
      event: "spelling:download",
      payload: { lang: "pt-BR", state: { kind: "ready" } },
    });
    expect(handlers["spelling.check"]({ langs: ["pt-BR", "de"], words: ["casa"] })).toEqual([true]);
  });

  it("accepts personal words and tells the windows", () => {
    const { handlers, heard } = setup();
    expect(handlers["spelling.personal.add"]({ word: "permark" })).toEqual(["permark"]);
    expect(heard.at(-1)).toEqual({ event: "spelling:personal", payload: ["permark"] });
    expect(handlers["spelling.check"]({ langs: ["en"], words: ["Permark"] })).toEqual([true]);
    expect(() => handlers["spelling.personal.add"]({ word: "two words" })).toThrow();
  });

  it("detects a document's language from its words, else answers the default", () => {
    const { handlers } = setup();
    const language = (sample: string, uiLocale = "en") =>
      handlers["spelling.language"]({ path: "a.md", sample, uiLocale });
    expect(
      language(
        "Eu fui para a casa de um amigo que não via há muito tempo e foi uma tarde muito boa.",
      ),
    ).toEqual({ langs: ["pt-BR"], source: "detected", detected: "pt-BR" });
    expect(language("Hello", "pt-PT")).toEqual({ langs: ["pt-PT"], source: "default" });
    expect(language("Hello", "ja")).toEqual({ langs: ["en"], source: "default" });
    expect(language("오늘은 날씨가 정말 좋네요")).toEqual({
      langs: [],
      source: "unsupported",
      script: "hangul",
    });
  });

  it("keeps a declared language over the text until it is cleared", () => {
    const { handlers, probe } = setup();
    const english = "The house of the friend is on the hill and it is a very nice place to be in.";
    expect(handlers["spelling.declare"]({ path: "a.md", lang: "fr" })).toEqual({
      langs: ["fr"],
      source: "declared",
    });
    expect(
      handlers["spelling.language"]({ path: "a.md", sample: english, uiLocale: "en" }),
    ).toEqual({ langs: ["fr"], source: "declared" });
    expect(probe.declared.get("a.md")).toBe("fr");
    expect(
      handlers["spelling.declare"]({ path: "a.md", lang: null, sample: english, uiLocale: "en" }),
    ).toEqual({ langs: ["en"], source: "detected", detected: "en" });
    expect(() => handlers["spelling.declare"]({ path: "a.md", lang: "xx" })).toThrow();
  });

  it("follows the window's language until a default is picked", () => {
    const { handlers } = setup();
    expect(handlers["spelling.defaults.get"]({ uiLocale: "pt-PT" })).toEqual({
      lang: "pt-PT",
      chosen: false,
    });
    expect(handlers["spelling.defaults.get"]({ uiLocale: "zh-CN" })).toEqual({
      lang: "en",
      chosen: false,
    });
    expect(handlers["spelling.defaults.set"]({ lang: "de" })).toEqual({ lang: "de", chosen: true });
    expect(handlers["spelling.defaults.get"]({ uiLocale: "en" })).toEqual({
      lang: "de",
      chosen: true,
    });
    expect(() => handlers["spelling.defaults.set"]({ lang: "ja" })).toThrow();
  });
});
