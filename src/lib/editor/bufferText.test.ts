import { describe, expect, it } from "vitest";
import { BOM, dominantEol } from "./blockModel";
import { fromBuffer, toBuffer, type BufferBase } from "./bufferText";

/** The buffer of a file's text, and what `fromBuffer` makes of an edited buffer. */
function open(text: string): { base: BufferBase; format: ReturnType<typeof toBuffer>["format"] } {
  const { buffer, format } = toBuffer(text);
  return { base: { text, buffer }, format };
}

function save(text: string, edit: (buffer: string) => string): string {
  const { base, format } = open(text);
  return fromBuffer(edit(base.buffer), format, base);
}

const STYLES: Record<string, string> = {
  lf: "# T\n\nbody one\nbody two\n\n- a\n- b\n",
  crlf: "# T\r\n\r\nbody one\r\nbody two\r\n\r\n- a\r\n- b\r\n",
  "lone cr": "# T\r\rbody one\rbody two\r\r- a\r- b\r",
  "mixed, crlf most": "# T\r\n\r\nbody one\nbody two\r\n\r\n- a\r\n- b\n",
  "mixed, lf most": "# T\n\nbody one\r\nbody two\n\n- a\n- b\r\n",
  "mixed, cr most": "# T\r\rbody one\nbody two\r\r- a\r\n- b\r",
  "all three": "a\r\nb\nc\rd\r\ne\nf\r",
  "no final newline": "# T\r\n\r\nlast line",
  empty: "",
  "only breaks": "\r\n\n\r",
};

describe("dominantEol", () => {
  it("counts a lone CR as a line ending of its own", () => {
    expect(dominantEol("# T\r\rbody\r")).toBe("\r");
    expect(dominantEol("a\r\nb\r\nc\n")).toBe("\r\n");
    expect(dominantEol("a\nb\nc\r")).toBe("\n");
    expect(dominantEol("a\rb\nc\n")).toBe("\n");
    expect(dominantEol("a\rb\r\nc\n")).toBe("\n"); // all three once: LF wins the tie
  });
});

describe("saving a buffer that was not edited", () => {
  it.each(Object.entries(STYLES))("writes the very same bytes of a %s file", (_name, text) => {
    expect(save(text, (b) => b)).toBe(text);
    const withBom = BOM + text;
    expect(save(withBom, (b) => b)).toBe(withBom);
  });
});

describe("saving an edited buffer", () => {
  it("rewrites nothing but the changed text in a file of lone CR line endings", () => {
    expect(save(STYLES["lone cr"], (b) => b.replace("body one", "body 1"))).toBe(
      "# T\r\rbody 1\rbody two\r\r- a\r- b\r",
    );
  });

  it("keeps the other line endings of a file that mixes them", () => {
    const mixed = "a\r\nb\r\nc\n";
    expect(save(mixed, (b) => b.replace("a", "A"))).toBe("A\r\nb\r\nc\n");
    expect(save(mixed, (b) => b.replace("c", "C"))).toBe("a\r\nb\r\nC\n");
    expect(save(mixed, (b) => `${b}d\n`)).toBe("a\r\nb\r\nc\nd\r\n"); // new text: the most common ending
  });

  it("writes a new line with the file's most common line ending", () => {
    expect(save(STYLES.crlf, (b) => b.replace("body one", "new\nlines\nhere"))).toBe(
      "# T\r\n\r\nnew\r\nlines\r\nhere\r\nbody two\r\n\r\n- a\r\n- b\r\n",
    );
    expect(save(STYLES["lone cr"], (b) => b.replace("body one\n", "body one\n\n"))).toBe(
      "# T\r\rbody one\r\rbody two\r\r- a\r- b\r",
    );
    expect(save("a\nb\n", (b) => b.replace("b", "x\ny"))).toBe("a\nx\ny\n");
  });

  it("deletes lines without touching the others", () => {
    expect(save("a\r\nb\nc\r\nd\n", (b) => b.replace("b\nc\n", ""))).toBe("a\r\nd\n");
    expect(save(STYLES["all three"], (b) => b.replace("c\n", ""))).toBe("a\r\nb\nd\r\ne\nf\r");
  });

  it("does not let a kept CR and a written LF fuse into one line ending", () => {
    // "a" ends in a lone CR; a blank line is typed after it, and the file is mostly LF
    const text = "a\rb\nc\nd\n";
    const saved = save(text, (b) => b.replace("a\n", "a\n\n"));
    expect(toBuffer(saved).buffer).toBe("a\n\nb\nc\nd\n");
    // the reverse: a written CR (lone CR file) followed by a kept LF
    const other = "a\rb\rc\rd\ne\r";
    const savedOther = save(other, (b) => b.replace("c\n", "c\n\n"));
    expect(toBuffer(savedOther).buffer).toBe("a\nb\nc\n\nd\ne\n");
  });

  it("keeps the BOM and puts it back once", () => {
    expect(save(BOM + STYLES.crlf, (b) => b.replace("# T", "# U"))).toBe(
      BOM + STYLES.crlf.replace("# T", "# U"),
    );
    expect(save(BOM + BOM + "x\n", (b) => b.replace("x", "y"))).toBe(`${BOM}${BOM}y\n`);
  });

  it("works without a base too: every line gets the file's line ending", () => {
    const { buffer, format } = toBuffer("a\r\nb\nc\r\n");
    expect(fromBuffer(buffer, format)).toBe("a\r\nb\r\nc\r\n");
    expect(fromBuffer("a\nb\n", { bom: true, eol: "\r" })).toBe(`${BOM}a\rb\r`);
  });

  it("round-trips any edit: the buffer comes back as it was typed, and untouched line endings stay", () => {
    // a small deterministic generator, so a failure can be reproduced
    let seed = 12345;
    const random = (n: number): number => {
      seed = (Math.imul(seed, 1103515245) + 12345) & 0x7fffffff;
      return seed % n;
    };
    const pieces = ["x", "yz", "\n", "\n\n", " ", "é", "😀", "# h\n"];
    for (const text of Object.values(STYLES)) {
      for (let round = 0; round < 60; round++) {
        const { base, format } = open(text);
        const at = random(base.buffer.length + 1);
        const end = Math.min(base.buffer.length, at + random(4));
        const piece = pieces[random(pieces.length)];
        const edited = base.buffer.slice(0, at) + piece + base.buffer.slice(end);
        const saved = fromBuffer(edited, format, base);
        const why = JSON.stringify({ text, at, end, piece });
        expect(toBuffer(saved).buffer, why).toBe(edited);
        // an edit that adds and removes no line break leaves every line break of the file alone
        const breaks = (x: string): string[] => x.match(/\r\n|\r|\n/g) ?? [];
        if (breaks(base.buffer.slice(at, end)).length === 0 && !piece.includes("\n")) {
          expect(breaks(saved), why).toEqual(breaks(text));
        }
      }
    }
  });
});
