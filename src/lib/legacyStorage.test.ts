import { describe, expect, it } from "vitest";
import { migrateLegacyStorage } from "./legacyStorage";

function memory(initial: Record<string, string>): Storage {
  const map = new Map(Object.entries(initial));
  return {
    get length() {
      return map.size;
    },
    key: (i: number) => [...map.keys()][i] ?? null,
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => void map.set(k, v),
    removeItem: (k: string) => void map.delete(k),
    clear: () => map.clear(),
  };
}

describe("migrateLegacyStorage", () => {
  it("copies trailmark.* values to permark.* and keeps the old keys", () => {
    const s = memory({ "trailmark.tone": "sage", "trailmark.hint.firstSave": "1", other: "x" });
    migrateLegacyStorage(s);
    expect(s.getItem("permark.tone")).toBe("sage");
    expect(s.getItem("permark.hint.firstSave")).toBe("1");
    expect(s.getItem("trailmark.tone")).toBe("sage");
    expect(s.getItem("other")).toBe("x");
    expect(s.getItem("permark.other")).toBeNull();
  });

  it("does not overwrite a permark.* key that is already set", () => {
    const s = memory({ "trailmark.tone": "sage", "permark.tone": "paper" });
    migrateLegacyStorage(s);
    expect(s.getItem("permark.tone")).toBe("paper");
  });

  it("does not throw when the storage is blocked", () => {
    const blocked = {
      get length(): number {
        throw new Error("blocked");
      },
    } as unknown as Storage;
    expect(() => migrateLegacyStorage(blocked)).not.toThrow();
  });
});
