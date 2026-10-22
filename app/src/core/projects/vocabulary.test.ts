// Official lists as the UI treats them (pure): status words, when a promote is possible, where it writes, refusals.
import { describe, expect, it } from "vitest";
import type { ApiError, ProjectsView } from "../../api";
import { normalizeTag } from "../../plugins/tags/model";
import {
  officialEntry,
  ownerShort,
  promoteBlocked,
  statusWords,
  vocabularyErrorText,
  vocabularyName,
  vocabularyPlace,
} from "./vocabulary";

function view(fields: Partial<ProjectsView> = {}): ProjectsView {
  return {
    seq: 1,
    present: true,
    ready: true,
    answer: "answered",
    rows: [],
    offered: { count: 0, inner: [] },
    record: { status: "ok" },
    skipped: [],
    ask: null,
    rootCard: {
      name: "root",
      short: "root",
      description: null,
      category: null,
      place: "project",
      invalid: [],
      warning: null,
    },
    rootCategoryStatus: null,
    vocabulary: {
      file: { status: "ok" },
      active: true,
      tags: [{ name: "spec", color: "blue", icon: null }],
      categories: [],
    },
    ...fields,
  } as ProjectsView;
}

const backend = (kind: string, subject: string | null): ApiError =>
  ({
    kind: "backend",
    command: "vocabulary_promote",
    error: { kind, subject, detail: null },
  }) as ApiError;

describe("status words", () => {
  it("official | local (by short name, else any project) | unlisted per kind", () => {
    expect(statusWords("official", "tag", null)).toBe("Official");
    expect(statusWords("local", "tag", "hub-b")).toBe("Local to hub-b");
    expect(statusWords("local", "category", null)).toBe("Local to a project");
    expect(statusWords("unlisted", "tag", null)).toBe("Unlisted (found in files, in no list)");
    expect(statusWords("unlisted", "category", null)).toBe("Unlisted (in no list)");
  });

  it("the owner of a local name by its uid; unknown -> null", () => {
    const v = view({ rows: [{ uid: "abc", short: "hub-b" } as ProjectsView["rows"][number]] });
    expect(ownerShort(v, "abc")).toBe("hub-b");
    expect(ownerShort(v, "zzz")).toBeNull();
    expect(ownerShort(v, null)).toBeNull();
    expect(officialEntry(v, "tag", "spec")?.color).toBe("blue");
    expect(officialEntry(v, "category", "spec")).toBeNull();
  });
});

describe("promote: possible, where, refused", () => {
  it("a readable list | a folder with a project list can take names; it says whether it creates the file", () => {
    expect(promoteBlocked(view())).toBeNull();
    expect(vocabularyPlace(view())).toBe("projects.vocabulary.saves");
    const fresh = view({
      vocabulary: { file: { status: "absent" }, active: false, tags: [], categories: [] },
    });
    expect(promoteBlocked(fresh)).toBeNull();
    expect(vocabularyPlace(fresh)).toBe("projects.vocabulary.creates");
  });

  it("never over an unreadable | newer list; not without a .permark folder; not in a plain folder", () => {
    expect(
      promoteBlocked(
        view({
          vocabulary: {
            file: { status: "unreadable", line: 4, message: "x" },
            active: false,
            tags: [],
            categories: [],
          },
        }),
      ),
    ).toContain("(line 4)");
    expect(
      promoteBlocked(
        view({
          vocabulary: { file: { status: "newer" }, active: false, tags: [], categories: [] },
        }),
      ),
    ).toContain("newer version");
    expect(promoteBlocked(view({ record: { status: "newer" } }))).not.toBeNull();
    expect(
      promoteBlocked(
        view({
          record: { status: "absent" },
          vocabulary: { file: { status: "absent" }, active: false, tags: [], categories: [] },
        }),
      ),
    ).not.toBeNull();
    expect(promoteBlocked(view({ present: false }))).not.toBeNull();
    expect(promoteBlocked(null)).not.toBeNull();
  });

  it("refusals in plain words: changed while saving | not now | the system's reason", () => {
    expect(vocabularyErrorText(backend("conflict", "vocabulary"))).toBe(
      "The official list changed while PerMark was saving it. Try again.",
    );
    expect(vocabularyErrorText(backend("unavailable", "vocabulary"))).toBe(
      "The official list of this folder can't be changed now.",
    );
    expect(vocabularyErrorText(backend("io", ".permark/vocabulary.toml"))).not.toBe("");
  });
});

describe("typed names", () => {
  it("kept as the backend keeps tags (same rules as the tag editor), Unicode and long names included", () => {
    for (const raw of [
      "  Roadmap Items ",
      "ÜBER  größe",
      "标签 数据",
      "x".repeat(60),
      "",
      "   ",
      "a\tb",
    ]) {
      expect(vocabularyName(raw)).toBe(normalizeTag(raw));
    }
  });
});
