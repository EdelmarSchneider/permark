// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import { hashString } from "../text";
import type { DocumentView } from "../types";
import { DocRenderer, type RenderLabels } from "./docRender";
import type { EnhanceContext } from "./enhance";

const labels: RenderLabels = {
  removed: (count) => `${count} removed`,
  show: "Show",
  hide: "Hide",
  showTitle: "Show what was removed",
};

const enhance: EnhanceContext = {
  docPath: "docs/spec.md",
  theme: "light",
  labels: {
    copyCode: "Copy",
    anchorLabel: (name) => `Link to ${name}`,
    footnoteBackref: (ref) => `Back ${ref}`,
    diagramPending: "Drawing",
    diagramFailed: "Failed",
    alertTitles: {},
    codeAuto: (name) => `auto: ${name}`,
    codeAutoHint: "Guessed language",
  },
  resolveImage: async (path) => path,
};

const p = (text: string): string => `<p data-sourcepos="1:1-1:9">${text}</p>`;
const h2 = (text: string): string => `<h2 data-sourcepos="1:1-1:9">${text}</h2>`;
const table = (rows: string[][]): string => {
  const cells = (row: string[], tag: string): string =>
    `<tr>\n${row.map((cell) => `<${tag}>${cell}</${tag}>`).join("\n")}\n</tr>`;
  const [head, ...body] = rows;
  return `<table data-sourcepos="1:1-9:9">\n<thead>\n${cells(head, "th")}\n</thead>\n<tbody>\n${body
    .map((row) => cells(row, "td"))
    .join("\n")}\n</tbody>\n</table>`;
};

/** What the backend sends for a document: the HTML of the blocks and one hash per block. */
function view(blocks: string[]): DocumentView {
  return {
    path: "docs/spec.md",
    kind: "markdown",
    html: blocks.join("\n"),
    blocks: blocks.map(hashString),
    title: null,
    frontmatter: null,
    modifiedMs: 0,
    sizeBytes: 0,
    hash: null,
    encodingNote: null,
  };
}

const REPLIES = [
  ["Success", "200 OK", "Done"],
  ["Busy", "429 Too Many Requests", "Retry on the schedule"],
  ["Server error", "503 Service Unavailable", "Retry on the schedule"],
  ["No reply", "Timeout after 10 s", "Retry on the schedule"],
  ["Client error", "400 Bad Request", "Give up"],
];

/** A section of a spec ("Which replies are retried": a table and the paragraph after it), and what follows it. */
function spec(rows: string[][], explain: string, tail: string[]): string[] {
  return [
    h2("Which replies are retried"),
    p(
      "Only some failures are worth another try. A retry helps when the receiver may answer differently later.",
    ),
    table([["Reply", "Example", "Rule"], ...rows]),
    p(explain),
    h2("Where it lives"),
    ...tail,
  ];
}

const EXPLAIN_OLD =
  "A 429 means the receiver is busy, not broken. For now it is retried on the same schedule as a server error.";
const EXPLAIN_NEW =
  "A 429 means the receiver is busy, not broken. It waits as long as Retry-After asks, up to one hour.";
const TAIL = [
  p("Each attempt is made in <code>src/deliver.ts:40</code>."),
  p('See the <a href="plan.md">build plan</a> for the order of work.'),
];

const EDITED_ROWS = [
  ...REPLIES.map((row) =>
    row[0] === "Busy" ? [row[0], row[1], "Wait for Retry-After, at most 1 hour"] : row,
  ),
  ["Gone", "410 Gone", "Stop, and pause the endpoint"],
];

let body: HTMLElement;
let renderer: DocRenderer;

function show(blocks: string[]): void {
  renderer.render(view(blocks), { mode: "fresh", enhance, labels });
}

function update(blocks: string[]): ReturnType<DocRenderer["render"]> {
  return renderer.render(view(blocks), { mode: "update", enhance, labels });
}

beforeEach(() => {
  document.body.replaceChildren();
  body = document.createElement("div");
  const scroller = document.createElement("div");
  scroller.append(body);
  document.body.append(scroller);
  Object.assign(scroller, { scrollTo: () => undefined }); // jsdom has no scrolling
  renderer = new DocRenderer(body, scroller);
});

describe("a live update of the document on screen", () => {
  it("glows a changed table and the paragraph after it, and shows no removed marker", () => {
    show(spec(REPLIES, EXPLAIN_OLD, TAIL));
    const result = update(spec(EDITED_ROWS, EXPLAIN_NEW, TAIL));

    expect(result.removedMarkers).toHaveLength(0);
    expect(body.querySelectorAll(".tm-removed")).toHaveLength(0);
    expect(result.changed).toHaveLength(2);
    expect(result.changed[0].classList.contains("table-wrap")).toBe(true);
    expect(result.changed[1].textContent).toContain("It waits as long as Retry-After asks");
    expect(body.querySelectorAll(".tm-changed")).toHaveLength(2);
  });

  it("shows one marker, for the block that was deleted, next to the two that changed", () => {
    show(spec(REPLIES, EXPLAIN_OLD, TAIL));
    const result = update(spec(EDITED_ROWS, EXPLAIN_NEW, [TAIL[1]]));

    expect(result.changed).toHaveLength(2);
    expect(result.removedMarkers).toHaveLength(1);
    const marker = result.removedMarkers[0];
    expect(marker.querySelector(".tm-removed__label")?.textContent).toBe("1 removed");
    expect(marker.querySelector(".tm-removed__content")?.textContent).toContain(
      "src/deliver.ts:40",
    );
    // it stands where the paragraph was: under the heading, in front of the block that follows
    expect(marker.previousElementSibling?.textContent).toContain("Where it lives");
    expect(marker.nextElementSibling?.textContent).toContain("See the build plan");
  });

  it("shows a block that was rewritten as a removal plus an addition, and not also as changed", () => {
    show(spec(REPLIES, EXPLAIN_OLD, TAIL));
    const result = update(
      spec(
        REPLIES,
        "Deliveries that fail go back on the queue and the delay doubles up to a day.",
        TAIL,
      ),
    );

    expect(result.changed).toHaveLength(1);
    expect(result.removedMarkers).toHaveLength(1);
    expect(result.removedMarkers[0].querySelector(".tm-removed__label")?.textContent).toBe(
      "1 removed",
    );
  });

  it("marks nothing when the blocks did not change", () => {
    show(spec(REPLIES, EXPLAIN_OLD, TAIL));
    const result = update(spec(REPLIES, EXPLAIN_OLD, TAIL));
    expect(result.changed).toHaveLength(0);
    expect(result.removedMarkers).toHaveLength(0);
  });
});
