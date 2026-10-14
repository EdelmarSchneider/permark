import { describe, expect, it, vi } from "vitest";
import { DescriptionDraft } from "./descriptionDraft";

describe("DescriptionDraft", () => {
  it("saves to the document the editor was opened on, never to another one", () => {
    const write = vi.fn();
    const draft = new DescriptionDraft("a.md", "old", write);
    // the reader navigates to b.md meanwhile: the draft does not know or care
    expect(draft.flush("typed for a", true)).toBe(true);
    expect(write).toHaveBeenCalledExactlyOnceWith("a.md", "typed for a");
    expect(draft.path).toBe("a.md");
  });

  it("does not save what is already saved, or when the tags cannot be changed", () => {
    const write = vi.fn();
    const draft = new DescriptionDraft("a.md", "same", write);
    expect(draft.flush("same", true)).toBe(false);
    expect(draft.flush("changed", false)).toBe(false);
    expect(write).not.toHaveBeenCalled();
    expect(draft.flush("changed", true)).toBe(true);
    expect(draft.flush("changed", true)).toBe(false);
    expect(write).toHaveBeenCalledTimes(1);
  });

  it("takes in a description that changed elsewhere", () => {
    const write = vi.fn();
    const draft = new DescriptionDraft("a.md", "old", write);
    draft.adopt("from the agent");
    expect(draft.flush("from the agent", true)).toBe(false);
    expect(draft.flush("old", true)).toBe(true);
    expect(write).toHaveBeenCalledExactlyOnceWith("a.md", "old");
  });
});
