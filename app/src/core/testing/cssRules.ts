// Plain-text reading of CSS rules for tests that lock layout rules (jsdom draws no layout to measure).

/** CSS with comments removed, so a note cannot hide or satisfy a declaration. */
export function codeOnly(source: string): string {
  return source.replace(/\/\*[\s\S]*?\*\//g, "");
}

export type Rule = { selectors: string[]; body: string };

/** The flat rules of `source` (rules inside `@media` included, the at-rule line itself skipped). */
export function rulesOf(source: string): Rule[] {
  const found: Rule[] = [];
  for (const match of codeOnly(source).matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
    const selectors = (match[1] ?? "")
      .split(",")
      .map((selector) => selector.trim().replace(/\s+/g, " "))
      .filter((selector) => selector.length > 0);
    if (selectors.length === 0 || selectors.every((selector) => selector.startsWith("@"))) continue;
    found.push({ selectors, body: match[2] ?? "" });
  }
  return found;
}

/** The bodies of every rule that names `selector`, joined. */
export function ruleBody(source: string, selector: string): string {
  return rulesOf(source)
    .filter((rule) => rule.selectors.includes(selector))
    .map((rule) => rule.body)
    .join("\n");
}

export function declarations(body: string): string[] {
  return body
    .split(";")
    .map((part) => part.trim().replace(/\s+/g, " "))
    .filter((part) => part.length > 0);
}

/** The `<style>` block of a Svelte component's source. */
export function styleOf(component: string): string {
  return /<style>([\s\S]*?)<\/style>/.exec(component)?.[1] ?? "";
}
