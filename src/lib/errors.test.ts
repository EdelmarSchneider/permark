import { afterEach, describe, expect, it } from "vitest";
import { errorKind, errorMessage, errorText, noteText } from "./errors";
import { language, LOCALES } from "./i18n";

describe("errorText", () => {
  it("reads strings and Error instances", () => {
    expect(errorText("boom")).toBe("boom");
    expect(errorText(new Error("bad path"))).toBe("bad path");
  });

  it("reads a message field from a serialized error", () => {
    expect(errorText({ kind: "io", message: "access denied" })).toBe("access denied");
  });

  it("unwraps externally tagged enums", () => {
    expect(errorText({ notFound: "docs/a.md" })).toBe("notFound: docs/a.md");
    expect(errorText({ io: { message: "sharing violation" } })).toBe("sharing violation");
  });

  it("falls back to JSON for unknown shapes", () => {
    expect(errorText({ a: 1, b: 2 })).toBe('{"a":1,"b":2}');
    expect(errorText(42)).toBe("42");
  });
});

describe("errorKind", () => {
  it("reads the stable kind of a serialized error", () => {
    expect(errorKind({ kind: "exists", message: "a file named a.md already exists" })).toBe(
      "exists",
    );
  });

  it("is null for anything without a kind", () => {
    expect(errorKind("boom")).toBeNull();
    expect(errorKind(new Error("x"))).toBeNull();
    expect(errorKind({ kind: 3 })).toBeNull();
    expect(errorKind(null)).toBeNull();
  });
});

describe("errorMessage", () => {
  afterEach(() => {
    language.preference = "en";
  });

  it("words an error of a known kind with the interface's own strings, filled from the backend's blanks", () => {
    const error = { kind: "exists", message: "a file named a.md already exists", subject: "a.md" };
    expect(errorMessage(error)).toBe("a.md already exists.");
    language.preference = "de";
    expect(errorMessage(error)).toBe("a.md existiert bereits.");
    language.preference = "pt-BR";
    expect(errorMessage(error)).toBe("a.md já existe.");
    language.preference = "es";
    expect(errorMessage(error)).toBe("a.md ya existe.");
  });

  it("fills the system's own text into the sentence as it is", () => {
    const error = {
      kind: "io",
      message: "cannot access docs/a.md: disk on fire",
      subject: "docs/a.md",
      detail: "disk on fire",
    };
    expect(errorMessage(error)).toBe("Couldn't access docs/a.md: disk on fire");
    language.preference = "de";
    expect(errorMessage(error)).toBe("Zugriff auf docs/a.md nicht möglich: disk on fire");
  });

  it("words the reason a name is refused", () => {
    const error = { kind: "invalidName", message: "That name is reserved", reason: "reserved" };
    expect(errorMessage(error)).toBe("That name is reserved by the system");
    language.preference = "pt-BR";
    expect(errorMessage(error)).toBe("Esse nome é reservado pelo sistema");
  });

  it("needs no blanks for kinds that say it all", () => {
    expect(errorMessage({ kind: "noVault", message: "no folder is open" })).toBe(
      "No folder is open.",
    );
    language.preference = "es";
    expect(errorMessage({ kind: "nothing", message: "there is nothing to do" })).toBe(
      "No hay nada que hacer.",
    );
  });

  it("says where meta.toml went wrong when the backend knows the line", () => {
    const error = {
      kind: "metaUnreadable",
      message: "meta.toml can't be read (line 3): expected `=`",
      subject: "3",
      detail: "expected `=`",
    };
    expect(errorMessage(error)).toBe("Can't read .trailmark/meta.toml (line 3): expected `=`");
    expect(errorMessage({ ...error, subject: undefined })).toBe(
      "Can't read .trailmark/meta.toml: expected `=`",
    );
  });

  it("keeps the backend's text when it has no words for the kind or the sentence lacks a blank", () => {
    expect(errorMessage({ kind: "fromTheFuture", message: "something new" })).toBe("something new");
    // a sentence that needs the subject, without one
    expect(errorMessage({ kind: "exists", message: "a file named a.md already exists" })).toBe(
      "a file named a.md already exists",
    );
    // an invalidName reason this build does not know
    expect(errorMessage({ kind: "invalidName", message: "odd", reason: "weird" })).toBe("odd");
    expect(errorMessage("boom")).toBe("boom");
    expect(errorMessage(new Error("bad path"))).toBe("bad path");
  });

  it("has words, in every language, for every kind the backend reports", () => {
    // the kinds of AppError::kind() and the sub-errors in src-tauri/src (kept in step by hand)
    const kinds = [
      "pathEscape",
      "invalidPath",
      "notFound",
      "io",
      "denied",
      "watch",
      "git",
      "gitUnavailable",
      "session",
      "metaUnreadable",
      "metaWorktree",
      "binary",
      "tooLarge",
      "notMarkdown",
      "encoding",
      "exists",
      "unsupported",
      "invalidData",
      "intoItself",
      "otherRoot",
      "protected",
      "holdsWorktree",
      "overlap",
      "simpleMode",
      "nothing",
      "unknownPlan",
      "unknownOperation",
      "partlyMoved",
      "trash",
      "noVault",
      "superseded",
      "internal",
    ];
    for (const locale of LOCALES) {
      language.preference = locale;
      for (const kind of kinds) {
        const filled = { kind, message: "RAW", subject: "S", detail: "D" };
        expect(errorMessage(filled), `${locale} ${kind}`).not.toBe("RAW");
      }
    }
  });
});

describe("noteText", () => {
  afterEach(() => {
    language.preference = "en";
  });

  it("words the codes the backend attaches to a document", () => {
    expect(noteText("utf16Le")).toBe("Converted from UTF-16 LE");
    expect(noteText("invalidUtf8,deepNesting")).toBe(
      "Not valid UTF-8; unreadable bytes were replaced; Shown as plain text: nesting too deep to render",
    );
    language.preference = "de";
    expect(noteText("utf16Be")).toBe("Aus UTF-16 BE konvertiert");
  });

  it("shows a code it has no words for as it is", () => {
    expect(noteText("fromTheFuture")).toBe("fromTheFuture");
  });
});
