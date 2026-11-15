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
});
