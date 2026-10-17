import { afterEach, describe, expect, it } from "vitest";
import { language } from "./i18n";
import { windowTitle, type TitleState } from "./windowTitle";

const project = (extra: Partial<Extract<TitleState, { screen: "project" }>> = {}): TitleState => ({
  screen: "project",
  simple: false,
  project: "permark-docs",
  document: "plan.md",
  unsaved: false,
  ...extra,
});

describe("the window title", () => {
  afterEach(() => {
    language.preference = "en";
  });

  it("is the name of the program on the welcome screen", () => {
    expect(windowTitle({ screen: "welcome" })).toBe("PerMark");
  });

  describe("in a project", () => {
    it("names the document, then the project", () => {
      expect(windowTitle(project())).toBe("plan.md - permark-docs - PerMark");
    });

    it("names only the project when no document is open", () => {
      expect(windowTitle(project({ document: null }))).toBe("permark-docs - PerMark");
    });

    it("marks unsaved edits in front of the rest", () => {
      expect(windowTitle(project({ unsaved: true }))).toBe(
        "* plan.md - permark-docs - PerMark",
      );
    });

    it("has nothing to mark when no document is open", () => {
      expect(windowTitle(project({ document: null, unsaved: true }))).toBe(
        "permark-docs - PerMark",
      );
    });

    it("keeps the extension and a long name whole", () => {
      const document = "a-very-long-name-for-a-document-that-says-what-it-is-about.test.md";
      expect(windowTitle(project({ document }))).toBe(`${document} - permark-docs - PerMark`);
    });

    it("does not fill in the braces of a name", () => {
      expect(windowTitle(project({ document: "{project}.md", project: "{document}" }))).toBe(
        "{project}.md - {document} - PerMark",
      );
    });

    it("falls back to the document alone when the project has no name yet", () => {
      expect(windowTitle(project({ project: null }))).toBe("plan.md - PerMark");
      expect(windowTitle(project({ project: null, document: null }))).toBe("PerMark");
    });
  });

  describe("for a single file", () => {
    it("names the document and not the folder it is in", () => {
      expect(windowTitle(project({ simple: true, project: "Downloads" }))).toBe(
        "plan.md - PerMark",
      );
    });

    it("marks unsaved edits", () => {
      expect(windowTitle(project({ simple: true, unsaved: true }))).toBe("* plan.md - PerMark");
    });

    it("is the name of the program when the file did not open", () => {
      expect(windowTitle(project({ simple: true, document: null }))).toBe("PerMark");
    });
  });

  describe("in the Notes window", () => {
    it("names the note", () => {
      expect(windowTitle({ screen: "notes", document: "Shopping list", unsaved: false })).toBe(
        "Shopping list - Notes - PerMark",
      );
    });

    it("says Notes when nothing is open", () => {
      expect(windowTitle({ screen: "notes", document: null, unsaved: false })).toBe(
        "Notes - PerMark",
      );
    });

    it("marks a file with unsaved edits", () => {
      expect(windowTitle({ screen: "notes", document: "todo.txt", unsaved: true })).toBe(
        "* todo.txt - Notes - PerMark",
      );
    });
  });

  describe("in another language", () => {
    it("follows the format of that language", () => {
      language.preference = "pt-BR";
      expect(windowTitle({ screen: "notes", document: "Compras", unsaved: false })).toBe(
        "Compras - Notas - PerMark",
      );
      language.preference = "de";
      expect(windowTitle({ screen: "notes", document: null, unsaved: false })).toBe(
        "Notizen - PerMark",
      );
      language.preference = "es";
      expect(windowTitle(project({ unsaved: true }))).toBe(
        "* plan.md - permark-docs - PerMark",
      );
    });
  });
});
