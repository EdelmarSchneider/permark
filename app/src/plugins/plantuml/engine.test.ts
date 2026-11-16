// @vitest-environment jsdom
// PlantUML's error picture: the message and the lines around it, without the engine's build line (its version and
// commit placeholders mean nothing to the reader).

import { beforeEach, describe, expect, it, vi } from "vitest";

/** The picture the engine draws for the next render. */
let picture = "";

vi.mock("@plantuml/core", () => ({
  renderToString: (_lines: string[], ok: (svg: string) => void) => ok(picture),
}));
vi.mock("@plantuml/core/viz-global.js", () => ({ instance: undefined }));

const { drawPlantuml } = await import("./engine");
const { diagrams } = await import("../../core/doc/diagram");

const ERROR =
  '<svg xmlns="http://www.w3.org/2000/svg" version="1.1" viewBox="0 0 481 107" width="481" height="107"><defs/><g>' +
  '<text x="5" y="17" font-size="12" fill="#33FF02" xml:space="preserve" style="white-space: pre" font-weight="bold" font-style="italic" font-family="sans-serif">PlantUML version $version$ / $git.commit.id$ [Unknown compile time]</text>' +
  '<rect x="5" y="27" width="177" height="19" fill="#33FF02" stroke="#33FF02" stroke-width="1"/>' +
  '<text x="6" y="42" font-size="14" fill="#000000" font-weight="bold" font-family="sans-serif">[From textarea (line 2) ]</text>' +
  '<text x="5" y="102" font-size="14" fill="#FF0000" font-weight="bold" font-family="sans-serif"> Syntax Error? (Assumed diagram type: sequence)</text>' +
  "</g></svg>";

beforeEach(() => diagrams.clear());

describe("plantuml error picture", () => {
  it("shows the message without the engine's version line", async () => {
    picture = ERROR;
    const svg = await drawPlantuml("@startuml\nthis is -> -> not valid\n@enduml", "light");
    expect(svg).not.toContain("PlantUML version");
    expect(svg).not.toContain("$git.commit.id$");
    expect(svg).toContain("[From textarea (line 2) ]");
    expect(svg).toContain("Syntax Error? (Assumed diagram type: sequence)");
  });

  it("keeps a label of a diagram that happens to read the same", async () => {
    picture =
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 20"><g><text x="5" y="17" font-size="13">PlantUML version 2 shipped</text></g></svg>';
    const svg = await drawPlantuml(
      "@startuml\nA -> B : PlantUML version 2 shipped\n@enduml",
      "dark",
    );
    expect(svg).toContain("PlantUML version 2 shipped");
  });
});
