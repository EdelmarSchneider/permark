import { describe, expect, it } from "vitest";
import { findCodeSecrets, isSecretKey, MASK } from "./secrets";

/** `text` with every secret span replaced by `<x>`. */
function hide(text: string): string {
  let out = text;
  for (const { from, to } of findCodeSecrets(text).reverse()) {
    out = out.slice(0, from) + "<x>" + out.slice(to);
  }
  return out;
}

describe("secret keys", () => {
  it("knows secrets by their key words, with separators ignored", () => {
    for (const key of ["API_KEY", "pass_word", "authToken", "client-secret", "db.pwd", "PRIVATE"]) {
      expect(isSecretKey(key), key).toBe(true);
    }
    for (const key of ["authors", "keywords", "max_tokens", "keyboard", "name", "version"]) {
      expect(isSecretKey(key), key).toBe(false);
    }
  });
});

describe("secrets in a loose text", () => {
  it("hides values after secret keys and keeps the keys", () => {
    const cases: [string, string][] = [
      ["API_KEY=abc123", "API_KEY=<x>"],
      ["export GITHUB_TOKEN=ghp_x", "export GITHUB_TOKEN=<x>"],
      ["password: hunter2", "password: <x>"],
      ['  db.password = "p@ss w"  # x', '  db.password = "<x>"  # x'],
      ['{"apiKey": "sk-1", "name": "n"}', '{"apiKey": "<x>", "name": "n"}'],
      ["client_secret='s3'", "client_secret='<x>'"],
      ["AUTH=x,PWD=y", "AUTH=<x>,PWD=<x>"],
      ["PRIVATE_KEY := zz", "PRIVATE_KEY := <x>"],
      ['"tokens": ["a", "b"],', '"tokens": [<x>],'],
      ["auth: { token: x }", "auth: { token: <x> }"],
      ["DATABASE_URL=postgres://u:dbpass@host/db", "DATABASE_URL=postgres://u:<x>@host/db"],
    ];
    for (const [input, expected] of cases) expect(hide(input), input).toBe(expected);
  });

  it("leaves ordinary text, references, hidden values and empty values alone", () => {
    for (const text of [
      "The key idea: keep it simple.",
      "name = value",
      "use crate::auth::login;",
      "password",
      "token:",
      'authors = ["Ana"]',
      "NPM_TOKEN: ${{ secrets.NPM_TOKEN }}",
      "API_KEY=${OTHER}",
      "password: <hidden>",
      "see https://example.org/a@b",
      "",
    ]) {
      expect(findCodeSecrets(text), text).toEqual([]);
    }
  });

  it("works over several lines and never hangs on odd input", () => {
    const text = "a=1\nSECRET=x\npwd: y\nend";
    expect(hide(text)).toBe("a=1\nSECRET=<x>\npwd: <x>\nend");
    for (const odd of [
      'token="open',
      "key='",
      "key=\\",
      "key:::",
      "=key=",
      "k\u0000ey=1",
      "鍵key=値",
    ]) {
      expect(() => findCodeSecrets(odd)).not.toThrow();
    }
    expect(findCodeSecrets("token=" + "x".repeat(100_000)).length).toBe(1);
  });

  it("has a fixed mask", () => {
    expect(MASK).toBe("••••••");
  });
});
