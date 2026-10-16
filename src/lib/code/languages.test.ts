import hljs from "highlight.js";
import { describe, expect, it } from "vitest";
import { HLJS_LANGUAGES } from "./languageData";
import {
  allLanguages,
  fenceLanguage,
  fenceNameOf,
  fenceWord,
  grammarIds,
  isPlainName,
  languageDisplayName,
  pickerRows,
  resolveLanguage,
  sanitizeInfo,
} from "./languages";

describe("the info string of a fence", () => {
  it("takes the first word, in lower case", () => {
    expect(fenceLanguage("rust")).toBe("rust");
    expect(fenceLanguage("Rust")).toBe("rust");
    expect(fenceLanguage('rust title="main.rs"')).toBe("rust");
    expect(fenceLanguage("  python   linenos ")).toBe("python");
    expect(fenceLanguage("")).toBe("");
    expect(fenceLanguage("   ")).toBe("");
  });

  it("keeps the case the author wrote in the word itself", () => {
    expect(fenceWord("TerraForm title=x")).toBe("TerraForm");
  });

  it("reads the forms docs tools put around the language", () => {
    expect(fenceLanguage("rust,ignore")).toBe("rust");
    expect(fenceLanguage("rust,no_run")).toBe("rust");
    expect(fenceLanguage("{python}")).toBe("python");
    expect(fenceLanguage("{r}")).toBe("r");
    expect(fenceLanguage("{.python .numberLines}")).toBe("python");
    expect(fenceLanguage("{ .js }")).toBe("js");
    expect(fenceLanguage("js:app.js")).toBe("js");
    expect(fenceLanguage("js{1,3}")).toBe("js");
    expect(fenceLanguage("language-python")).toBe("python");
    expect(fenceLanguage(".python")).toBe("python");
  });

  it("keeps the characters of names such as c++ and c#", () => {
    expect(fenceLanguage("c++")).toBe("c++");
    expect(fenceLanguage("C#")).toBe("c#");
    expect(fenceLanguage("objective-c++")).toBe("objective-c++");
  });
});

describe("resolving a name to a grammar", () => {
  const cases: [string, string][] = [
    ["js", "javascript"],
    ["JS", "javascript"],
    ["jsx", "javascript"],
    ["mjs", "javascript"],
    ["node", "javascript"],
    ["ts", "typescript"],
    ["tsx", "typescript"],
    ["sh", "bash"],
    ["shell", "bash"],
    ["zsh", "bash"],
    ["bash", "bash"],
    ["console", "shell"],
    ["ps1", "powershell"],
    ["powershell", "powershell"],
    ["pwsh", "powershell"],
    ["yml", "yaml"],
    ["c#", "csharp"],
    ["cs", "csharp"],
    ["csharp", "csharp"],
    ["c++", "cpp"],
    ["cpp", "cpp"],
    ["kt", "kotlin"],
    ["py", "python"],
    ["rb", "ruby"],
    ["rs", "rust"],
    ["golang", "go"],
    ["dockerfile", "dockerfile"],
    ["docker", "dockerfile"],
    ["make", "makefile"],
    ["makefile", "makefile"],
    ["jsonc", "json"],
    ["json5", "json"],
    ["svelte", "xml"],
    ["vue", "xml"],
    ["html", "xml"],
    ["toml", "ini"],
    ["objc", "objectivec"],
    ["postgres", "pgsql"],
    ["tex", "latex"],
    ["md", "markdown"],
    ["proto", "protobuf"],
  ];
  it.each(cases)("%s is %s", (name, id) => {
    expect(resolveLanguage(name)?.id).toBe(id);
  });

  it("reads the whole info string", () => {
    expect(resolveLanguage('Rust title="x"')?.id).toBe("rust");
    expect(resolveLanguage("{.python}")?.id).toBe("python");
    expect(resolveLanguage("rust,ignore")?.id).toBe("rust");
  });

  it("has no grammar for a name highlight.js does not know", () => {
    expect(resolveLanguage("terraform")).toBeNull();
    expect(resolveLanguage("hcl")).toBeNull();
    expect(resolveLanguage("zzz")).toBeNull();
    expect(resolveLanguage("")).toBeNull();
  });

  it("knows which names mean plain text", () => {
    for (const name of ["text", "txt", "plain", "plaintext", "none", "nohighlight", "TEXT"]) {
      expect(isPlainName(name), name).toBe(true);
    }
    expect(isPlainName("python")).toBe(false);
    expect(isPlainName("")).toBe(false);
  });
});

describe("the name shown for a language", () => {
  it("is its proper name, whichever alias was written", () => {
    expect(languageDisplayName("py")).toBe("Python");
    expect(languageDisplayName("c++")).toBe("C++");
    expect(languageDisplayName("c#")).toBe("C#");
    expect(languageDisplayName("ts")).toBe("TypeScript");
    expect(languageDisplayName("php")).toBe("PHP");
  });

  it("keeps the name the author wrote when it stands for a flavour", () => {
    expect(languageDisplayName("html")).toBe("HTML");
    expect(languageDisplayName("xml")).toBe("XML");
    expect(languageDisplayName("toml")).toBe("TOML");
    expect(languageDisplayName("svelte")).toBe("Svelte");
    expect(languageDisplayName("jsonc")).toBe("JSONC");
    expect(languageDisplayName("sh")).toBe("Shell");
  });

  it("is the word as written when the language is not known", () => {
    expect(languageDisplayName('Terraform title="x"')).toBe("Terraform");
  });
});

describe("the table of languages", () => {
  it("is the one highlight.js ships (run scripts/gen-code-languages.mjs after updating the dependency)", () => {
    const shipped = hljs
      .listLanguages()
      .sort()
      .map((id) => {
        const language = hljs.getLanguage(id);
        return [id, language?.name ?? id, language?.aliases ?? []];
      });
    expect(HLJS_LANGUAGES.map(([id, name, aliases]) => [id, name, [...aliases]])).toEqual(shipped);
  });

  it("lists every grammar once, and plain text is not a language to pick", () => {
    const ids = grammarIds();
    expect(new Set(ids).size).toBe(ids.length);
    expect(ids.length).toBeGreaterThan(180);
    const picked = allLanguages().map((language) => language.id);
    expect(picked).not.toContain("plaintext");
    expect(picked).toContain("python");
  });
});

describe("the language picker", () => {
  it("lists plain text first and then every language by name when nothing is typed", () => {
    const rows = pickerRows("");
    expect(rows[0]).toEqual({ kind: "plain" });
    const languages = rows.filter((row) => row.kind === "language");
    expect(languages.length).toBe(allLanguages().length);
    expect(rows.some((row) => row.kind === "custom")).toBe(false);
  });

  it("finds a language by its name, its id or any alias", () => {
    const first = (query: string): string | undefined => {
      const row = pickerRows(query).find((r) => r.kind === "language");
      return row?.kind === "language" ? row.language.id : undefined;
    };
    expect(first("python")).toBe("python");
    expect(first("py")).toBe("python");
    expect(first("c#")).toBe("csharp");
    expect(first("golang")).toBe("go");
    expect(first("typesc")).toBe("typescript");
    expect(first("visual basic")).toBe("vbnet");
    expect(first("ps1")).toBe("powershell");
  });

  it("writes what was typed when it names a language exactly, and the language's own name otherwise", () => {
    const rows = pickerRows("JS");
    const exact = rows[0];
    expect(exact.kind === "language" && exact.language.id).toBe("javascript");
    expect(exact.kind === "language" && exact.write).toBe("JS");
    const partial = pickerRows("javas").find((row) => row.kind === "language");
    expect(partial?.kind === "language" && partial.write).toBe("javascript");
  });

  it("offers the typed text as it is when no language is named by it", () => {
    const rows = pickerRows("terraform");
    expect(rows[rows.length - 1]).toEqual({ kind: "custom", write: "terraform" });
    expect(pickerRows("zzzz")).toEqual([{ kind: "custom", write: "zzzz" }]);
    expect(pickerRows('rust title="x"')).toEqual([{ kind: "custom", write: 'rust title="x"' }]);
  });

  it("does not offer a custom row for a name a language answers to", () => {
    expect(pickerRows("python").some((row) => row.kind === "custom")).toBe(false);
    expect(pickerRows("py").some((row) => row.kind === "custom")).toBe(false);
  });

  it("can also pick plain text by typing a word for it", () => {
    expect(pickerRows("plain")[0]).toEqual({ kind: "plain" });
    expect(pickerRows("none")[0]).toEqual({ kind: "plain" });
  });

  it("leaves out of the typed text what an info string cannot hold", () => {
    expect(sanitizeInfo("  rust\n")).toBe("rust");
    expect(sanitizeInfo("ru`st")).toBe("ru st");
    expect(sanitizeInfo("a\r\nb")).toBe("a b");
  });

  it("writes a name that reads back as the same language", () => {
    for (const language of allLanguages()) {
      expect(resolveLanguage(fenceNameOf(language))?.id, language.id).toBe(language.id);
    }
  });
});
