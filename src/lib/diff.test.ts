import { describe, expect, it } from "vitest";
import { diffBlocks, htmlText, oldToNew, pickAnchor, type BlockDiff } from "./diff";

describe("diffBlocks", () => {
  it("reports nothing for identical documents", () => {
    const d = diffBlocks(["a", "b", "c"], ["a", "b", "c"]);
    expect(d.added).toEqual([]);
    expect(d.removed).toEqual([]);
    expect(d.matches).toEqual([
      [0, 0],
      [1, 1],
      [2, 2],
    ]);
  });

  it("marks a changed block as added and leaves its old version as a removed run", () => {
    const d = diffBlocks(["a", "b", "c"], ["a", "B", "c"]);
    expect(d.added).toEqual([1]);
    // the marker sits after the new version, in front of the next block that survived
    expect(d.removed).toEqual([{ beforeNew: 2, oldIndices: [1] }]);
  });

  it("marks inserted blocks as added", () => {
    const d = diffBlocks(["a", "c"], ["a", "b1", "b2", "c"]);
    expect(d.added).toEqual([1, 2]);
    expect(d.removed).toEqual([]);
  });

  it("groups consecutive removed blocks into one run", () => {
    const d = diffBlocks(["a", "x", "y", "z", "b"], ["a", "b"]);
    expect(d.added).toEqual([]);
    expect(d.removed).toEqual([{ beforeNew: 1, oldIndices: [1, 2, 3] }]);
  });

  it("puts a trailing removed run at the end of the new document", () => {
    const d = diffBlocks(["a", "b", "c"], ["a"]);
    expect(d.removed).toEqual([{ beforeNew: 1, oldIndices: [1, 2] }]);
  });

  it("handles an empty old document (everything is added)", () => {
    const d = diffBlocks([], ["a", "b"]);
    expect(d.added).toEqual([0, 1]);
    expect(d.removed).toEqual([]);
  });

  it("handles an empty new document (everything is removed)", () => {
    const d = diffBlocks(["a", "b"], []);
    expect(d.added).toEqual([]);
    expect(d.removed).toEqual([{ beforeNew: 0, oldIndices: [0, 1] }]);
  });

  it("keeps duplicate blocks aligned", () => {
    const d = diffBlocks(["hr", "p", "hr"], ["hr", "p", "q", "hr"]);
    expect(d.added).toEqual([2]);
    expect(d.removed).toEqual([]);
  });

  it("finds a moved block as one removal plus one addition", () => {
    const d = diffBlocks(["a", "b", "c", "d"], ["b", "c", "d", "a"]);
    expect(d.added).toEqual([3]);
    expect(d.removed).toEqual([{ beforeNew: 0, oldIndices: [0] }]);
  });

  it("produces a valid edit script on a mixed edit", () => {
    const oldKeys = ["h1", "p1", "p2", "tbl", "code", "p3"];
    const newKeys = ["h1", "p1b", "tbl", "code2", "note", "p3"];
    const d = diffBlocks(oldKeys, newKeys);
    // matched + added covers every new block exactly once
    const covered = [...d.matches.map(([, n]) => n), ...d.added].sort((x, y) => x - y);
    expect(covered).toEqual([0, 1, 2, 3, 4, 5]);
    // matched + removed covers every old block exactly once
    const removedOld = d.removed.flatMap((r) => r.oldIndices);
    const coveredOld = [...d.matches.map(([o]) => o), ...removedOld].sort((x, y) => x - y);
    expect(coveredOld).toEqual([0, 1, 2, 3, 4, 5]);
    for (const [o, n] of d.matches) expect(oldKeys[o]).toBe(newKeys[n]);
  });

  it("falls back to replacing the middle for very large changes without throwing", () => {
    const a = Array.from({ length: 2500 }, (_, i) => `a${i}`);
    const b = Array.from({ length: 2500 }, (_, i) => `b${i}`);
    const d = diffBlocks(["head", ...a, "tail"], ["head", ...b, "tail"]);
    expect(d.matches).toEqual([
      [0, 0],
      [2501, 2501],
    ]);
    expect(d.added.length).toBe(2500);
    expect(d.removed[0].oldIndices.length).toBe(2500);
  });
});

describe("pickAnchor", () => {
  it("anchors on the first visible block that survived", () => {
    const d = diffBlocks(["a", "b", "c", "d"], ["a", "B", "c", "d"]);
    expect(pickAnchor([1, 2, 3], d)).toEqual({ oldIndex: 2, newIndex: 2 });
  });

  it("maps to the shifted index when blocks were inserted above", () => {
    const d = diffBlocks(["a", "b"], ["x", "y", "a", "b"]);
    expect(pickAnchor([0, 1], d)).toEqual({ oldIndex: 0, newIndex: 2 });
    expect(oldToNew(d).get(1)).toBe(3);
  });

  it("returns null when no visible block survived", () => {
    const d = diffBlocks(["a", "b"], ["x", "y"]);
    expect(pickAnchor([0, 1], d)).toBeNull();
  });
});

// ---- changed blocks are not removed blocks --------------------------------------------------------------------------

const p = (text: string): string => `<p>${text}</p>`;
const h2 = (text: string): string => `<h2>${text}</h2>`;
const list = (items: string[]): string =>
  `<ul>\n${items.map((item) => `<li>${item}</li>`).join("\n")}\n</ul>`;
const table = (rows: string[][]): string => {
  const cells = (row: string[], tag: string): string =>
    `<tr>\n${row.map((cell) => `<${tag}>${cell}</${tag}>`).join("\n")}\n</tr>`;
  return `<table>\n<thead>\n${cells(rows[0], "th")}\n</thead>\n<tbody>\n${rows
    .slice(1)
    .map((row) => cells(row, "td"))
    .join("\n")}\n</tbody>\n</table>`;
};

/** The live reload's diff: the HTML of a block is its key, and its text is what a changed block is recognised by. */
function diffHtml(before: string[], after: string[]): BlockDiff {
  return diffBlocks(before, after, {
    old: (i) => htmlText(before[i]),
    new: (i) => htmlText(after[i]),
  });
}

const range = (length: number): number[] => Array.from({ length }, (_, i) => i);
const byNumber = (x: number, y: number): number => x - y;

/** Every old block has exactly one outcome (kept, changed or moved, removed) and every new block is kept or glows. */
function expectOneOutcomePerBlock(d: BlockDiff, oldLength: number, newLength: number): void {
  const removedOld = d.removed.flatMap((run) => run.oldIndices);
  const olds = [...d.matches.map(([o]) => o), ...d.paired.map(([o]) => o), ...removedOld];
  expect(olds.sort(byNumber)).toEqual(range(oldLength));
  const news = [...d.matches.map(([, n]) => n), ...d.added];
  expect(news.sort(byNumber)).toEqual(range(newLength));
  for (const [, n] of d.paired) expect(d.added).toContain(n);
  const places = d.removed.map((run) => run.beforeNew);
  expect([...places].sort(byNumber)).toEqual(places);
  for (const place of places) expect(place).toBeLessThanOrEqual(newLength);
}

const REPLIES = [
  ["Success", "200 OK", "Done"],
  ["Busy", "429 Too Many Requests", "Retry on the schedule"],
  ["Server error", "503 Service Unavailable", "Retry on the schedule"],
  ["No reply", "Timeout after 10 s", "Retry on the schedule"],
  ["Client error", "400 Bad Request", "Give up"],
];
const REPLIES_HEAD = ["Reply", "Example", "Rule"];
const EXPLAIN_OLD =
  "A 429 means the receiver is busy, not broken. For now it is retried on the same schedule as a server error.";
const EXPLAIN_NEW =
  "A 429 means the receiver is busy, not broken. It waits as long as Retry-After asks, up to one hour.";

const section = (rows: string[][], explain: string): string[] => [
  h2("Which replies are retried"),
  p(
    "Only some failures are worth another try. A retry helps when the receiver may answer differently later.",
  ),
  table([REPLIES_HEAD, ...rows]),
  p(explain),
  h2("Where it lives"),
  p(
    "Each attempt is made in <code>src/deliver.ts:40</code>. The wait before the next one is worked out in <code>src/retry.ts:29</code>.",
  ),
  p('See the <a href="plan.md">build plan</a> for the order of work.'),
];

describe("diffBlocks pairs a changed block with its old version", () => {
  it("shows two changed blocks and no removed marker for a table edit followed by an edited paragraph", () => {
    // One save: a cell edited and a row appended in a table, a sentence reworded in the paragraph after it.
    const before = section(REPLIES, EXPLAIN_OLD);
    const edited = REPLIES.map((row) =>
      row[0] === "Busy" ? [row[0], row[1], "Wait for Retry-After, at most 1 hour"] : row,
    );
    const after = section(
      [...edited, ["Gone", "410 Gone", "Stop, and pause the endpoint"]],
      EXPLAIN_NEW,
    );
    const d = diffHtml(before, after);
    expect(d.added).toEqual([2, 3]);
    expect(d.removed).toEqual([]);
    expect(d.paired).toEqual([
      [2, 2],
      [3, 3],
    ]);
    expect(d.matches.map(([, n]) => n)).toEqual([0, 1, 4, 5, 6]);
    expectOneOutcomePerBlock(d, before.length, after.length);
  });

  it("pairs adjacent changed blocks one by one, in order", () => {
    const before = [
      p("The first step reads the queue and takes the oldest job that is waiting for a worker."),
      p("The second step sends the job to the receiver and waits for a reply to come back."),
      p("The third step records the outcome of the delivery in the log of the endpoint."),
    ];
    const after = [
      p(
        "The first step reads the queue and takes the oldest job that is waiting for a free worker.",
      ),
      p("The second step sends the job to the receiver and waits for the reply to come back."),
      p("The third step records the outcome of every delivery in the log of the endpoint."),
    ];
    const d = diffHtml(before, after);
    expect(d.added).toEqual([0, 1, 2]);
    expect(d.removed).toEqual([]);
    expect(d.paired).toEqual([
      [0, 0],
      [1, 1],
      [2, 2],
    ]);
  });

  it("pairs a table that gained a row", () => {
    const before = [p("intro"), table([REPLIES_HEAD, ...REPLIES]), p("outro")];
    const after = [
      p("intro"),
      table([REPLIES_HEAD, ...REPLIES, ["Gone", "410 Gone", "Stop, and pause the endpoint"]]),
      p("outro"),
    ];
    const d = diffHtml(before, after);
    expect(d.added).toEqual([1]);
    expect(d.removed).toEqual([]);
    expect(d.paired).toEqual([[1, 1]]);
  });

  it("pairs a list that gained an item", () => {
    const items = ["Write the spec", "Build the queue", "Add the retry schedule"];
    const before = [p("Plan"), list(items), p("End")];
    const after = [p("Plan"), list([...items, "Pause an endpoint that is gone"]), p("End")];
    const d = diffHtml(before, after);
    expect(d.added).toEqual([1]);
    expect(d.removed).toEqual([]);
    expect(d.paired).toEqual([[1, 1]]);
  });

  it("pairs a short edit, such as one word of a heading", () => {
    const d = diffHtml([h2("Which replies are retried")], [h2("Which replies get retried")]);
    expect(d.added).toEqual([0]);
    expect(d.removed).toEqual([]);
  });

  it("keeps a new block next to a changed one as an addition, and pairs the changed one with its own old version", () => {
    const before = [
      p("a"),
      p("The retry waits twice as long after every failed attempt to deliver."),
      p("z"),
    ];
    const after = [
      p("a"),
      p("A brand new paragraph about something else entirely, with nothing in common."),
      p("The retry waits twice as long after each failed attempt to deliver."),
      p("z"),
    ];
    const d = diffHtml(before, after);
    expect(d.added).toEqual([1, 2]);
    expect(d.paired).toEqual([[1, 2]]);
    expect(d.removed).toEqual([]);
    expectOneOutcomePerBlock(d, before.length, after.length);
  });

  it("treats a reordered block as moved: it glows where it landed and nothing is removed", () => {
    const blocks = [
      p("Alpha: the receiver is told about the event as soon as it happens."),
      p("Bravo: the queue keeps the event until a worker is free to send it."),
      p("Charlie: a worker sends the event and waits for the receiver to answer."),
      p("Delta: the answer decides whether the event is tried again or given up."),
    ];
    const after = [blocks[1], blocks[2], blocks[3], blocks[0]];
    const d = diffHtml(blocks, after);
    expect(d.added).toEqual([3]);
    expect(d.removed).toEqual([]);
    expect(d.paired).toEqual([[0, 3]]);
    expectOneOutcomePerBlock(d, 4, 4);
  });

  it("pairs equal blocks that moved one by one when the text repeats", () => {
    const before = [p("same words here"), p("filler one"), p("same words here"), p("filler two")];
    const after = [p("filler one"), p("filler two"), p("same words here"), p("same words here")];
    const d = diffHtml(before, after);
    expect(d.removed).toEqual([]);
    expectOneOutcomePerBlock(d, 4, 4);
  });

  describe("a real deletion next to a change still shows exactly one removed marker", () => {
    const intro = p("The intro paragraph stays exactly as it was.");
    const outro = p("The outro paragraph stays exactly as it was.");
    const changedOld = p(
      "Each wait is twice the one before it, and the first wait is one minute long.",
    );
    const changedNew = p(
      "Each wait is twice the one before it, and the first wait is thirty seconds long.",
    );
    const gone = p("Quarterly planning notes that nobody needs any more, deleted in this save.");

    it("after the changed block", () => {
      const before = [intro, changedOld, gone, outro];
      const after = [intro, changedNew, outro];
      const d = diffHtml(before, after);
      expect(d.added).toEqual([1]);
      expect(d.paired).toEqual([[1, 1]]);
      expect(d.removed).toEqual([{ beforeNew: 2, oldIndices: [2] }]);
      expectOneOutcomePerBlock(d, 4, 3);
    });

    it("before the changed block", () => {
      const before = [intro, gone, changedOld, outro];
      const after = [intro, changedNew, outro];
      const d = diffHtml(before, after);
      expect(d.added).toEqual([1]);
      expect(d.paired).toEqual([[2, 1]]);
      expect(d.removed).toEqual([{ beforeNew: 1, oldIndices: [1] }]);
      expectOneOutcomePerBlock(d, 4, 3);
    });

    it("between two changed blocks", () => {
      const otherOld = p("A delivery that runs out of tries moves to the dead letters list.");
      const otherNew = p(
        "A delivery that runs out of tries moves to the dead letters list at once.",
      );
      const before = [intro, changedOld, gone, otherOld, outro];
      const after = [intro, changedNew, otherNew, outro];
      const d = diffHtml(before, after);
      expect(d.added).toEqual([1, 2]);
      expect(d.paired).toEqual([
        [1, 1],
        [3, 2],
      ]);
      expect(d.removed).toEqual([{ beforeNew: 2, oldIndices: [2] }]);
      expectOneOutcomePerBlock(d, 5, 4);
    });

    it("with nothing else changed", () => {
      const d = diffHtml([intro, gone, outro], [intro, outro]);
      expect(d.added).toEqual([]);
      expect(d.paired).toEqual([]);
      expect(d.removed).toEqual([{ beforeNew: 1, oldIndices: [1] }]);
    });

    it("keeps one marker when the removed blocks are on both sides of a block that moved", () => {
      const moved = p(
        "This paragraph was cut from the top and pasted at the very end of the page.",
      );
      const tail = p("The tail paragraph stays exactly as it was, too.");
      const alsoGone = p("Also gone, together with the one above it.");
      const before = [intro, gone, moved, alsoGone, outro, tail];
      const after = [intro, outro, tail, moved];
      const d = diffHtml(before, after);
      expect(d.removed).toEqual([{ beforeNew: 1, oldIndices: [1, 3] }]);
      expect(d.paired).toEqual([[2, 3]]);
      expectOneOutcomePerBlock(d, 6, 4);
    });
  });

  it("reports one consistent outcome when a block changed too much to be recognised: removed and added, never changed", () => {
    const before = [
      p("intro"),
      p("Each wait is twice the one before it, and the first wait is one minute long."),
      p("outro"),
    ];
    const after = [
      p("intro"),
      p("Deliveries that fail are put back on the queue and the delay doubles up to a day."),
      p("outro"),
    ];
    const d = diffHtml(before, after);
    expect(d.added).toEqual([1]);
    expect(d.paired).toEqual([]);
    expect(d.removed).toEqual([{ beforeNew: 2, oldIndices: [1] }]);
    expectOneOutcomePerBlock(d, 3, 3);
  });

  it("does not pair blocks that have no words in common, such as two different images", () => {
    const d = diffHtml(['<p><img src="a.png"></p>'], ['<p><img src="b.png"></p>']);
    expect(d.paired).toEqual([]);
    expect(d.removed).toEqual([{ beforeNew: 1, oldIndices: [0] }]);
  });

  it("does not read the text of blocks that are not in a gap", () => {
    const read: string[] = [];
    const keys = ["a", "b", "c"];
    diffBlocks(keys, keys, {
      old: (i) => (read.push(`old${i}`), ""),
      new: (i) => (read.push(`new${i}`), ""),
    });
    expect(read).toEqual([]);
  });

  it("leaves a gap too big to compare unpaired, without reading its text and without throwing", () => {
    const before = range(300).map((i) => p(`Paragraph number ${i} says something about delivery.`));
    const after = range(300).map((i) =>
      p(`Paragraph number ${i} says something about delivery today.`),
    );
    let reads = 0;
    const d = diffBlocks(before, after, {
      old: () => (reads++, ""),
      new: () => (reads++, ""),
    });
    expect(reads).toBe(0);
    expect(d.paired).toEqual([]);
    expect(d.added).toHaveLength(300);
    expect(d.removed).toEqual([{ beforeNew: 300, oldIndices: range(300) }]);
  });

  it("still reports every changed block as removed and added when it is not given the text of the blocks", () => {
    const d = diffBlocks(["a", "b", "c"], ["a", "B", "c"]);
    expect(d.paired).toEqual([]);
    expect(d.removed).toEqual([{ beforeNew: 2, oldIndices: [1] }]);
  });

  it("gives one outcome per block on mixed edits", () => {
    const words = (i: number): string =>
      `Block ${i} talks about the retry schedule, the receiver and the queue of jobs.`;
    const base = range(8).map((i) => p(words(i)));
    const scenarios: string[][] = [
      [base[0], p(`${words(1)} Extra.`), base[3], base[2], p("brand new and unrelated"), base[7]],
      [base[7], base[6], base[5], base[4], base[3], base[2], base[1], base[0]],
      [p(`${words(0)} Edited.`), p(`${words(1)} Edited.`), p(`${words(2)} Edited.`)],
      [],
      base,
    ];
    for (const after of scenarios) {
      const d = diffHtml(base, after);
      expectOneOutcomePerBlock(d, base.length, after.length);
    }
  });
});

describe("htmlText", () => {
  it("drops the tags and keeps the words of neighbouring cells apart", () => {
    expect(htmlText("<tr><td>Busy</td><td>429</td></tr>").split(/\s+/).filter(Boolean)).toEqual([
      "Busy",
      "429",
    ]);
  });

  it("drops attributes with the tags", () => {
    expect(htmlText('<p data-sourcepos="3:1-3:9">hi</p>').trim()).toBe("hi");
  });

  it("decodes the entities the renderer writes", () => {
    expect(htmlText("<p>a &lt; b &amp;&amp; c &gt; d, &quot;e&quot; &#39;f&#39;</p>").trim()).toBe(
      "a < b && c > d, \"e\" 'f'",
    );
  });
});
