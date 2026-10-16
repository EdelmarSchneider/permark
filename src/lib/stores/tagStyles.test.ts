import { beforeEach, describe, expect, it, vi } from "vitest";

// The backend is replaced: these tests are about what the store shows and asks for.
const fake = vi.hoisted(() => ({
  tagStyles: vi.fn(),
  setTagStyle: vi.fn(),
}));
vi.mock("../api", () => ({ api: { ...fake, on: async () => () => {} } }));

import { readStyles } from "../tagStyle";
import { tags } from "./tags.svelte";
import { toast } from "./toast.svelte";

beforeEach(() => {
  fake.tagStyles.mockReset().mockResolvedValue(readStyles({}));
  fake.setTagStyle.mockReset();
  toast.clear();
  tags.reset();
});

describe("the looks of tags", () => {
  it("are what the backend last said, and start empty", async () => {
    expect(tags.styleOf("spec")).toBeUndefined();
    fake.tagStyles.mockResolvedValue(readStyles({ spec: { color: "blue", icon: "book-open" } }));
    await tags.refreshStyles();
    expect(tags.styleOf("spec")).toEqual({ color: "blue", icon: "book-open" });
    expect(tags.styleOf("other")).toBeUndefined();
  });

  it("keep what is shown when the backend cannot answer", async () => {
    fake.tagStyles.mockResolvedValue(readStyles({ spec: { color: "red", icon: null } }));
    await tags.refreshStyles();
    fake.tagStyles.mockRejectedValue(new Error("gone"));
    await tags.refreshStyles();
    expect(tags.styleOf("spec")?.color).toBe("red");
  });

  it("are forgotten when the folder changes", async () => {
    fake.tagStyles.mockResolvedValue(readStyles({ spec: { color: "red", icon: null } }));
    await tags.refreshStyles();
    tags.reset();
    expect(tags.styleOf("spec")).toBeUndefined();
  });
});

describe("giving a tag a look", () => {
  it("shows it at once and then takes the backend's answer", async () => {
    let answer: (v: unknown) => void = () => {};
    fake.setTagStyle.mockReturnValue(new Promise((resolve) => (answer = resolve)));
    const done = tags.setStyle("spec", "teal", "flag");
    expect(tags.styleOf("spec")).toEqual({ color: "teal", icon: "flag" });
    expect(fake.setTagStyle).toHaveBeenCalledWith("spec", "teal", "flag");
    answer(
      readStyles({ spec: { color: "teal", icon: "flag" }, idea: { color: "red", icon: null } }),
    );
    await expect(done).resolves.toBe(true);
    expect(tags.styleOf("idea")?.color).toBe("red");
  });

  it("removes the look when both parts are none", async () => {
    fake.setTagStyle.mockResolvedValueOnce(readStyles({ spec: { color: "blue", icon: null } }));
    await tags.setStyle("spec", "blue", null);
    fake.setTagStyle.mockResolvedValueOnce(readStyles({}));
    await tags.setStyle("spec", null, null);
    expect(tags.styleOf("spec")).toBeUndefined();
  });

  it("builds a second pick on the first, even before the backend has answered", async () => {
    fake.setTagStyle.mockReturnValue(new Promise(() => {}));
    void tags.setStyle("spec", "blue", null);
    void tags.setStyle("spec", "blue", "star");
    expect(tags.styleOf("spec")).toEqual({ color: "blue", icon: "star" });
  });

  it("says so and goes back to what the backend has when the change is refused", async () => {
    fake.tagStyles.mockResolvedValue(readStyles({ spec: { color: "red", icon: null } }));
    await tags.refreshStyles();
    fake.setTagStyle.mockRejectedValue({
      kind: "metaUnreadable",
      message: "meta.toml can't be read",
    });
    await expect(tags.setStyle("spec", "green", null)).resolves.toBe(false);
    expect(tags.styleOf("spec")?.color).toBe("red");
    expect(toast.current).toMatchObject({ type: "message", tone: "warn" });
  });
});
