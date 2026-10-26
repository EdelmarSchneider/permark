import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import docWords from "./doc-words.json";
import {
  findCodeSecrets,
  isSecretKey,
  keyWords,
  MASK,
  privateKeyLines,
  publicKeyLines,
  secretBlockLines,
  tableSecretSpans,
} from "./secrets";

/** The cases both mirrors read (`crates/core/src/secrets.rs` reads the same file). */
type Cases = {
  masked: [string, string][];
  plain: string[];
  secretKeys: string[];
  plainKeys: string[];
};
const cases = JSON.parse(
  readFileSync(join(process.cwd(), "testdata/secrets/cases.json"), "utf8"),
) as Cases;

/** `text` with every secret span replaced by `<hidden>` (what `mask_text` writes). */
function hide(text: string): string {
  let out = text;
  for (const { from, to } of findCodeSecrets(text).reverse()) {
    out = out.slice(0, from) + "<hidden>" + out.slice(to);
  }
  return out;
}

describe("the shared cases", () => {
  it("hides the secret values and nothing else", () => {
    expect(cases.masked.length).toBeGreaterThan(20);
    for (const [input, expected] of cases.masked) expect(hide(input), input).toBe(expected);
  });

  it("leaves code, prose and names that only contain a secret word alone", () => {
    expect(cases.plain.length).toBeGreaterThan(20);
    for (const text of cases.plain) expect(findCodeSecrets(text), text).toEqual([]);
  });

  it("knows secret keys by their last word", () => {
    for (const key of cases.secretKeys) expect(isSecretKey(key), key).toBe(true);
    for (const key of cases.plainKeys) expect(isSecretKey(key), key).toBe(false);
  });
});

describe("the documentation words (doc-words.json, read by both mirrors)", () => {
  it("stand as labels in any case, never in backticks, and nothing else does", () => {
    expect(docWords.length).toBeGreaterThanOrEqual(20);
    expect(docWords.length).toBeLessThanOrEqual(40);
    for (const word of docWords) {
      expect(word, word).toBe(word.toLowerCase());
      for (const text of [`| token | ${word.toUpperCase()} |`, `password ${word}`]) {
        expect(findCodeSecrets(text), text).toEqual([]);
      }
    }
    // in backticks a value is written as a value (`none`, `true` are no secret anywhere)
    for (const word of ["optional", "reset", "length", "default", "n/a"]) {
      expect(hide(`| token | \`${word}\` |`)).toBe("| token | `<hidden>` |");
    }
    expect(hide("| token | words |")).toBe("| token | <hidden> |");
  });
});

describe("secrets in a loose text", () => {
  it("splits names into words", () => {
    expect(keyWords("apiKey2")).toEqual(["api", "key"]);
    expect(keyWords("APIKey")).toEqual(["api", "key"]);
    expect(keyWords("KeyW")).toEqual(["key", "w"]);
    expect(keyWords("x-api-key")).toEqual(["x", "api", "key"]);
    expect(keyWords("__")).toEqual([]);
  });

  it("never hangs on odd input", () => {
    for (const odd of [
      'token="open',
      "key='",
      "key=\\",
      "key:::",
      "=key=",
      "k\u0000ey=1",
      "鍵key=値",
      'api_key="x\\',
      "eyJabcdefgh.",
      "-----BEGIN PRIVATE KEY-----",
    ]) {
      expect(() => findCodeSecrets(odd)).not.toThrow();
    }
    expect(findCodeSecrets("token=" + "x".repeat(100_000)).length).toBe(1);
  });

  it("finds the body lines of a private key block in a text held as lines", () => {
    const lines = [
      "a",
      "-----BEGIN EC PRIVATE KEY-----",
      "MHcCAQ",
      "",
      "  AbCd",
      "-----END EC PRIVATE KEY-----",
      "b",
    ];
    expect([...privateKeyLines(lines)]).toEqual([2, 4]);
    expect(
      privateKeyLines(["-----BEGIN PUBLIC KEY-----", "MFkw", "-----END PUBLIC KEY-----"]).size,
    ).toBe(0);
    // certificates | public keys: their lines are never masked, also a part without its BEGIN
    const cert = ["-----BEGIN CERTIFICATE-----", "MIIDdz", "-----END CERTIFICATE-----"];
    expect([...publicKeyLines(cert)]).toEqual([1]);
    expect([...publicKeyLines(["MIIDdzCCAl", "AbCdEf12==", "-----END CERTIFICATE-----"])]).toEqual([
      1, 0,
    ]);
    expect(publicKeyLines(lines).size).toBe(0);
  });

  it("gives a viewer that masks line by line the block bodies and table cells of its lines", () => {
    const lines = ["db:", "  password: |", "    s3cret-line", "", "    second", "  host: db\r"];
    expect([...secretBlockLines(lines)]).toEqual([2, 4]);
    const table = [
      "| User | Password |",
      "|---|---|",
      "| bob | s3cret99 |",
      "| DB_PASSWORD | `Zq9!x` |",
      "text",
    ];
    const cells = tableSecretSpans(table);
    expect([...cells.keys()]).toEqual([2, 3]);
    expect(cells.get(2)?.map((s) => table[2]?.slice(s.from, s.to))).toEqual(["s3cret99"]);
    expect(cells.get(3)?.map((s) => table[3]?.slice(s.from, s.to))).toEqual(["Zq9!x"]);
    expect(
      tableSecretSpans([
        "| Setting | Description |",
        "|---|---|",
        "| password | The account password |",
      ]).size,
    ).toBe(0);
  });

  it("has a fixed mask", () => {
    expect(MASK).toBe("••••••");
  });
});

describe("verifier: real secrets the rule must not let through", () => {
  const leaks = (
    JSON.parse(readFileSync(join(process.cwd(), "testdata/secrets/leaks.json"), "utf8")) as {
      leaks: [string, string, string][];
    }
  ).leaks;
  it("hides every secret of testdata/secrets/leaks.json", () => {
    const through = leaks
      .filter(([, input, secret]) => hide(input).includes(secret))
      .map(([label]) => label);
    expect(through).toEqual([]);
  });
});
