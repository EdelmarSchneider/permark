import { describe, expect, it } from "vitest";
import {
  AUTO_MIN_RATIO,
  AUTO_MIN_RELEVANCE,
  AUTO_SUBSET,
  GRAMMARS,
  acceptGuess,
  ensureLanguage,
  highlightFence,
  highlightLoaded,
  isLoaded,
  sniffLanguage,
} from "./highlighter";
import { grammarIds } from "./languages";

const PYTHON = 'def greet(name):\n    print(f"Hello, {name}")\n\nclass Foo:\n    pass\n';

// scripts that a line which opens a def, class or import, and a line only Python has, make Python
const PYTHON_SCRIPTS = [
  'class Greeter:\n    def __init__(self, name: str) -> None:\n        self.name = name\n\nif __name__ == "__main__":\n    print(Greeter("world").name)\n',
  'import requests\n\nresp = requests.get("https://example.com/api")\nif resp.status_code == 200:\n    print(resp.json())\nelse:\n    raise RuntimeError("failed")\n',
  "from pathlib import Path\n\nfor p in Path('.').glob('*.md'):\n    print(p.name)\n",
  "def fib(n):\n    if n < 2:\n        return n\n    return fib(n - 1) + fib(n - 2)\n",
];

// and languages and texts that look like it in part
const NOT_PYTHON = [
  "import Foundation\n\nfunc add(a: Int, b: Int) -> Int {\n    return a + b\n}\nprint(add(a: 1, b: 2))\n",
  "import kotlin.math.max\n\nfun big(a: Int, b: Int): Int {\n    if (a > b) return a\n    return b\n}\n",
  "class Greeter\n  def initialize(name)\n    @name = name\n  end\nend\n",
  "import fs from 'fs';\nconst files = fs.readdirSync('.');\nfor (const f of files) {\n  console.log(f);\n}\n",
  "name: CI\njobs:\n  build:\n    steps:\n      - name: Setup\n        with:\n          node-version: 20\n",
  "Steps:\nimport the data\nthen if it works:\nrun the export\n",
  "def greet(name):\n    print(name)\n",
];

describe("the lazy loader", () => {
  it("has one loader for every grammar of the table, and no other", () => {
    expect([...GRAMMARS.keys()].sort()).toEqual(grammarIds().sort());
  });

  it("has a loader for every language the guess compares", () => {
    for (const id of AUTO_SUBSET) expect(GRAMMARS.has(id), id).toBe(true);
  });

  it("loads a grammar on request, once", async () => {
    expect(await ensureLanguage("lua")).toBe(true);
    expect(isLoaded("lua")).toBe(true);
    expect(await ensureLanguage("lua")).toBe(true);
    expect(highlightLoaded("local x = 1", "lua")).toContain("hljs-");
  });

  it("answers false for a grammar it does not have", async () => {
    expect(await ensureLanguage("terraform")).toBe(false);
    expect(isLoaded("terraform")).toBe(false);
    expect(highlightLoaded("x", "terraform")).toBeNull();
  });

  it("does not highlight a grammar that is not loaded", () => {
    expect(isLoaded("zephir")).toBe(false);
    expect(highlightLoaded("x", "zephir")).toBeNull();
  });
});

describe("the bar for a guessed language", () => {
  it("needs a strong relevance", () => {
    expect(acceptGuess(AUTO_MIN_RELEVANCE - 1, 0)).toBe(false);
    expect(acceptGuess(AUTO_MIN_RELEVANCE, 0)).toBe(true);
    expect(acceptGuess(3, 0)).toBe(false);
  });

  it("needs a clear lead over the runner-up", () => {
    const best = 30;
    expect(acceptGuess(best, best)).toBe(false);
    expect(acceptGuess(best, best - 1)).toBe(false);
    expect(acceptGuess(best, best / AUTO_MIN_RATIO + 1)).toBe(false);
    expect(acceptGuess(best, best / AUTO_MIN_RATIO)).toBe(true);
    expect(acceptGuess(best, 5)).toBe(true);
  });

  it("is never met by a strong score with an equally strong rival", () => {
    expect(acceptGuess(16, 16)).toBe(false);
    expect(acceptGuess(40, 39)).toBe(false);
  });
});

describe("what a text gives away by its shape", () => {
  it("reads a shebang", () => {
    expect(sniffLanguage("#!/bin/bash\nset -e\n")).toBe("bash");
    expect(sniffLanguage("#!/usr/bin/env python3\nprint(1)")).toBe("python");
    expect(sniffLanguage("#!/usr/bin/env -S node --flag\nconsole.log(1)")).toBe("javascript");
    expect(sniffLanguage("#!/usr/bin/python3.11\n")).toBe("python");
    expect(sniffLanguage("#!/opt/weird/tool\n")).toBeNull();
  });

  it("reads JSON that parses", () => {
    expect(sniffLanguage('{\n  "a": [1, 2, 3]\n}')).toBe("json");
    expect(sniffLanguage("[1, 2, 3]")).toBe("json");
    expect(sniffLanguage("{ not json }")).toBeNull();
    expect(sniffLanguage("[x]")).toBeNull();
  });

  it("reads an XML or HTML prolog", () => {
    expect(sniffLanguage('<?xml version="1.0"?><a/>')).toBe("xml");
    expect(sniffLanguage("<!DOCTYPE html>\n<html></html>")).toBe("xml");
    expect(sniffLanguage("<html lang=en>")).toBe("xml");
    expect(sniffLanguage("<b>bold</b> text")).toBeNull();
  });

  it("reads a diff", () => {
    expect(sniffLanguage("diff --git a/x b/x\nindex 1..2\n")).toBe("diff");
    expect(sniffLanguage("--- a/x.txt\n+++ b/x.txt\n@@ -1 +1 @@\n-a\n+b")).toBe("diff");
    expect(sniffLanguage("@@ -1,3 +1,3 @@\n a")).toBe("diff");
    expect(sniffLanguage("--- not a diff ---")).toBeNull();
  });

  it("reads Python by what only Python writes", () => {
    for (const source of PYTHON_SCRIPTS) expect(sniffLanguage(source), source).toBe("python");
  });

  it("does not take other languages, or a list of steps, for Python", () => {
    for (const source of NOT_PYTHON) expect(sniffLanguage(source), source).toBeNull();
  });

  it("leaves everything else to the guess", () => {
    expect(sniffLanguage(PYTHON)).toBeNull();
    expect(sniffLanguage("src/\n  lib/\n  main.ts")).toBeNull();
  });
});

describe("how a fenced block is drawn", () => {
  it("highlights the language it names and labels it with the language's name", async () => {
    const outcome = await highlightFence(PYTHON, "python");
    expect(outcome.html).toContain('<span class="hljs-keyword">def</span>');
    expect(outcome.label).toEqual({ name: "Python", auto: false });
  });

  it("reads aliases, case and attributes after the language", async () => {
    for (const info of ["py", "PY", 'python title="a.py"', "{.python .numberLines}", "{python}"]) {
      const outcome = await highlightFence(PYTHON, info);
      expect(outcome.html, info).toContain("hljs-keyword");
      expect(outcome.label, info).toEqual({ name: "Python", auto: false });
    }
    const rust = await highlightFence("fn main() {}", "rust,ignore");
    expect(rust.html).toContain("hljs-keyword");
    expect(rust.label?.name).toBe("Rust");
  });

  it("keeps the code's characters when it escapes them", async () => {
    const outcome = await highlightFence('print("<a> & b")', "python");
    expect(outcome.html).toContain("&lt;a&gt; &amp; b");
  });

  it("leaves plain text alone: no colour, no label, no guess", async () => {
    for (const info of ["text", "txt", "plaintext", "none", "Text"]) {
      expect(await highlightFence(PYTHON, info), info).toEqual({ html: null, label: null });
    }
  });

  it("labels a language it has no grammar for with the author's own word", async () => {
    const source = 'resource "aws_s3_bucket" "b" {\n  bucket = "my-tf-test-bucket"\n}\n';
    const outcome = await highlightFence(source, "Terraform title=main.tf");
    expect(outcome).toEqual({ html: null, label: { name: "Terraform", auto: false } });
  });

  it("guesses a block with no language from what its text gives away", async () => {
    const outcome = await highlightFence('{\n  "name": "permark",\n  "private": true\n}', "");
    expect(outcome.html).toContain("hljs-attr");
    expect(outcome.label).toEqual({ name: "JSON", auto: true });
    const shell = await highlightFence("#!/bin/sh\nset -e\necho hello world\n", "");
    expect(shell.label).toEqual({ name: "Bash", auto: true });
  });

  it("guesses a block that names a language highlight.js lacks, with the same bar", async () => {
    const script = "#!/bin/sh\nset -e\necho hello world\n";
    expect((await highlightFence(script, "mystery")).label).toEqual({ name: "Bash", auto: true });
    expect(await highlightFence("just some words, nothing more", "mystery")).toEqual({
      html: null,
      label: { name: "mystery", auto: false },
    });
  });

  it("guesses a script that is clearly one", async () => {
    const script =
      'export PATH="$HOME/bin:$PATH"\nif [ -f .env ]; then\n  source .env\nfi\nmkdir -p dist && cp -r src/* dist/\n';
    const outcome = await highlightFence(script, "");
    expect(outcome.label).toEqual({ name: "Bash", auto: true });
    expect(outcome.html).toContain("hljs-keyword");
  });

  it("guesses Python from a script with its structure and its traits", async () => {
    const outcome = await highlightFence(PYTHON_SCRIPTS[1], "");
    expect(outcome.label).toEqual({ name: "Python", auto: true });
    expect(outcome.html).toContain("hljs-keyword");
  });

  it("calls a guessed TOML file what it is", async () => {
    const outcome = await highlightFence(
      '[package]\nname = "permark"\nversion = "0.5.0"\n\n[dependencies]\nserde = "1"\n',
      "",
    );
    expect(outcome.label).toEqual({ name: "INI, TOML", auto: true });
  });

  it("does not guess at what is not code", async () => {
    const samples = [
      "src/\n├── lib/\n│   ├── api.ts\n│   └── paths.ts\n└── main.ts",
      "This is just some plain output text.\nIt has several lines and words in it.\nNothing here is code at all, honestly.",
      "2026-10-16 12:00:01 INFO  starting server\n2026-10-16 12:00:02 INFO  listening on 127.0.0.1:1420\n2026-10-16 12:00:05 WARN  slow request",
      "+-------+-------+\n| a     | b     |\n+-------+-------+\n| 1     | 2     |\n+-------+-------+",
      "- first item\n- second item\n- third item",
      "name = foo\nvalue = bar",
      "npm install",
      "",
    ];
    for (const source of samples) {
      expect(await highlightFence(source, ""), source).toEqual({ html: null, label: null });
    }
  });

  it("does not guess at a block too short or too long to tell", async () => {
    expect((await highlightFence("[1, 2]", "")).label).toBeNull();
    const long = `[${"1, ".repeat(4000)}2]`;
    expect((await highlightFence(long, "")).label).toBeNull();
  });

  it("does not highlight what is too long to highlight", async () => {
    const huge = `${"x = 1\n".repeat(40_000)}`;
    const outcome = await highlightFence(huge, "python");
    expect(outcome).toEqual({ html: null, label: { name: "Python", auto: false } });
  });
});
