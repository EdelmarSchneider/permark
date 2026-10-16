// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { applyTone, cacheTone, cachedTone, DEFAULT_TONE, normalizeTone, TONES } from "./tone";

describe("normalizeTone", () => {
  it("keeps the tones it knows", () => {
    for (const tone of TONES) expect(normalizeTone(tone)).toBe(tone);
  });

  it("reads anything else as the default", () => {
    for (const value of ["sepia", "Paper", "", null, undefined, 7, {}, ["sage"]]) {
      expect(normalizeTone(value)).toBe(DEFAULT_TONE);
    }
    expect(DEFAULT_TONE).toBe("white");
  });
});

describe("applyTone", () => {
  it("puts the tone on the page next to the theme", () => {
    const root = document.createElement("html");
    root.dataset.theme = "light";
    for (const tone of TONES) {
      applyTone(root, tone);
      expect(root.dataset.tone).toBe(tone);
      expect(root.dataset.theme).toBe("light");
    }
  });
});

describe("the remembered tone", () => {
  const memory = (): Storage => {
    const map = new Map<string, string>();
    return {
      getItem: (key) => map.get(key) ?? null,
      setItem: (key, value) => void map.set(key, value),
    } as Storage;
  };

  it("is read back, and is the default before anything was kept", () => {
    const storage = memory();
    expect(cachedTone(storage)).toBe("white");
    cacheTone(storage, "sage");
    expect(cachedTone(storage)).toBe("sage");
  });

  it("ignores a value it does not know", () => {
    const storage = memory();
    storage.setItem("trailmark.tone", "purple");
    expect(cachedTone(storage)).toBe("white");
  });

  it("survives a storage that is missing or refuses", () => {
    const refusing = {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
    };
    expect(cachedTone(undefined)).toBe("white");
    expect(cachedTone(refusing)).toBe("white");
    expect(() => cacheTone(refusing, "paper")).not.toThrow();
    expect(() => cacheTone(undefined, "paper")).not.toThrow();
  });
});
