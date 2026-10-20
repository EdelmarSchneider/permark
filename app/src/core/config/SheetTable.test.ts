// @vitest-environment jsdom
// A data sheet (TOON) as the backend reads it: tables with their rows, keyed tables, secret columns masked; in Edit
// mode the cells, the keys, the fields and the rows each turn into one edit for the backend.
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { mockView } from "../../api/mockDocs";
import type { ConfigEdit, NewValue, Sheet, Table } from "../../api";
import ConfigSheet from "./ConfigSheet.svelte";
import { ROW_HEIGHT, VIRTUAL_FROM, windowOf } from "./tableWindow";

let target: HTMLElement;
let component: Record<string, unknown> | null = null;

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
});

/** The real sheet the backend made for the TOON fixture (`cargo test` writes it). */
function team(): Sheet {
  const body = mockView("data/team.toon", [], true)?.body;
  if (body?.kind !== "sheet") throw new Error("no sheet for data/team.toon");
  return body.sheet;
}

type Props = {
  editable?: boolean;
  commit?: (path: string, value: NewValue) => Promise<string | null>;
  edit?: (edit: ConfigEdit) => Promise<string | null>;
};

async function show(sheet: Sheet, props: Props = {}): Promise<HTMLElement> {
  target = document.createElement("div");
  document.body.append(target);
  component = mount(ConfigSheet, {
    target,
    props: { sheet, editable: false, source: null, ...props },
  });
  await tick();
  return target;
}

const cells = (row: Element): string[] =>
  Array.from(row.querySelectorAll("td")).map((td) => td.textContent?.trim() ?? "");

describe("a TOON file as a sheet", () => {
  it("draws tables with their fields, rows and count, and names the spec", async () => {
    const root = await show(team());
    const tables = root.querySelectorAll("table");
    expect(tables).toHaveLength(2);
    const [people, access] = Array.from(tables);
    expect(
      Array.from(people?.querySelectorAll("th") ?? []).map((th) => th.textContent?.trim()),
    ).toEqual(["id", "name", "role", "active"]);
    const rows = people?.querySelectorAll("tbody tr") ?? [];
    expect(rows).toHaveLength(3);
    expect(cells(rows[1] as Element)).toEqual(["2", "Alan Turing", "review, specs", "true"]);
    expect(root.textContent).toContain("3 rows");
    // a keyed table: the entry key first; the token column is secret and masked
    const first = access?.querySelector("tbody tr");
    expect(first?.querySelector("td")?.textContent?.trim()).toBe("ci");
    expect(root.textContent).not.toContain("sk-ci-5f2a91");
    expect(root.textContent).toContain("read");
    // the object card and its inline list
    expect(root.textContent).toContain("PerMark");
    expect(root.querySelectorAll(".chip")).toHaveLength(3);
    expect(root.querySelector(".spec")?.textContent).toBe("TOON 4.4");
    // read only: no editing controls
    expect(root.querySelector(".grip")).toBeNull();
    expect(root.textContent).not.toContain("Add a row");
  });

  it("reveals one secret cell on request", async () => {
    const root = await show(team());
    const reveal = root.querySelector<HTMLButtonElement>("table .reveal");
    reveal?.click();
    flushSync();
    expect(root.textContent).toContain("sk-ci-5f2a91");
    expect(root.textContent).not.toContain("sk-bot-77c0de");
  });
});

describe("editing a TOON sheet", () => {
  it("sends one edit per change: rows, fields, keys", async () => {
    const edits: ConfigEdit[] = [];
    const edit = vi.fn((change: ConfigEdit) => {
      edits.push(change);
      return Promise.resolve(null);
    });
    const root = await show(team(), { editable: true, edit });
    const people = root.querySelector("table");
    // add a row at the end
    Array.from(root.querySelectorAll<HTMLButtonElement>("button"))
      .find((b) => b.textContent?.trim() === "Add a row")
      ?.click();
    // the row menu: duplicate, move down, delete
    const menuOf = (index: number): void => {
      people?.querySelectorAll<HTMLButtonElement>("tbody tr .tools button")[index]?.click();
      flushSync();
    };
    const pick = (label: string): void => {
      Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'))
        .find((b) => b.textContent?.trim().startsWith(label))
        ?.click();
      flushSync();
    };
    menuOf(1);
    pick("Duplicate the row");
    menuOf(0);
    pick("Move down");
    menuOf(2);
    pick("Delete the row");
    // the keyboard way of dragging
    people
      ?.querySelectorAll("tbody tr")[2]
      ?.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowUp", altKey: true, bubbles: true }),
      );
    await tick();
    expect(edits).toEqual([
      { op: "addRow", table: "team", at: 3 },
      { op: "duplicateRow", path: "team[1]" },
      { op: "moveRow", path: "team[0]", to: 1 },
      { op: "removeRow", path: "team[2]" },
      { op: "moveRow", path: "team[2]", to: 1 },
    ]);
  });

  it("renames a field, an entry key and an object key; adds and removes keys", async () => {
    const edits: ConfigEdit[] = [];
    const edit = (change: ConfigEdit): Promise<string | null> => {
      edits.push(change);
      return Promise.resolve(null);
    };
    const root = await show(team(), { editable: true, edit });
    const type = (input: HTMLInputElement | null, text: string): void => {
      if (!input) throw new Error("no field");
      input.value = text;
      input.dispatchEvent(new Event("input", { bubbles: true }));
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    };
    const button = (title: string): HTMLButtonElement | undefined =>
      Array.from(root.querySelectorAll<HTMLButtonElement>("button")).find(
        (b) => b.title === title || b.textContent?.trim() === title,
      );
    button("Rename the column name")?.click();
    flushSync();
    type(root.querySelector("th input"), "full_name");
    await tick();
    button("Rename the key ci")?.click();
    flushSync();
    type(root.querySelector("td input.name"), "build");
    await tick();
    button("Rename version")?.click();
    flushSync();
    type(root.querySelector("input.key-field"), "release");
    await tick();
    button("Remove url")?.click();
    await tick();
    button("Add a key")?.click();
    flushSync();
    const [key, value] = Array.from(root.querySelectorAll<HTMLInputElement>(".add-key input"));
    if (!key || !value) throw new Error("no add-key form");
    key.value = "stars";
    key.dispatchEvent(new Event("input", { bubbles: true }));
    type(value, "42");
    await tick();
    expect(edits).toEqual([
      { op: "renameField", table: "team", index: 1, name: "full_name" },
      { op: "rename", path: "access.ci", key: "build" },
      { op: "rename", path: "project.version", key: "release" },
      { op: "remove", path: "project.url" },
      { op: "addKey", parent: "project", key: "stars", value: { type: "literal", text: "42" } },
    ]);
  });

  it("shows the word of a refusal under the table", async () => {
    const root = await show(team(), {
      editable: true,
      edit: () => Promise.resolve("duplicate"),
    });
    Array.from(root.querySelectorAll<HTMLButtonElement>("button"))
      .find((b) => b.textContent?.trim() === "Add a row")
      ?.click();
    await tick();
    await tick();
    expect(root.querySelector(".table-wrap .refusal")?.textContent).toMatch(/already used/);
  });

  it("a cell edit goes through the value commit", async () => {
    const commit = vi.fn(() => Promise.resolve(null));
    const root = await show(team(), { editable: true, commit });
    const name = root.querySelectorAll("table tbody tr")[0]?.querySelectorAll("td")[2];
    name?.querySelector<HTMLButtonElement>(".hit")?.click();
    flushSync();
    const input = name?.querySelector("input");
    if (!input) throw new Error("no input");
    input.value = "Ada King";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await tick();
    expect(commit).toHaveBeenCalledWith("team[0].name", { type: "text", text: "Ada King" });
  });
});

describe("long tables", () => {
  function longTable(n: number): Sheet {
    const table: Table = {
      path: "rows",
      fields: ["i"],
      count: n,
      keyed: false,
      delimiter: ",",
      edit: { editable: "surgical" },
      rows: Array.from({ length: n }, (_, i) => ({
        path: `rows[${i}]`,
        line: i + 2,
        key: null,
        cells: [
          {
            path: `rows[${i}].i`,
            kind: "number",
            text: String(i),
            edit: { editable: "surgical" },
            secret: false,
          },
        ],
      })),
    };
    return {
      kind: "data",
      groups: [
        {
          path: "rows",
          title: "rows",
          line: 1,
          end: n + 1,
          comment: null,
          rows: [
            {
              path: "rows",
              key: "rows",
              line: 1,
              end: n + 1,
              value: { type: "table", ...table },
              comment: null,
            },
          ],
        },
      ],
    };
  }

  it("draws only the rows near the screen and keeps the full height", async () => {
    const root = await show(longTable(5000));
    const drawn = root.querySelectorAll("tbody tr:not(.spacer)");
    expect(drawn.length).toBeLessThan(200);
    const spacer = root.querySelector<HTMLElement>("tbody tr.spacer");
    expect(Number.parseInt(spacer?.style.height ?? "0", 10)).toBeGreaterThan(100_000);
    expect(root.textContent).toContain("5000 rows");
  });

  it("the window follows the offset, with rows to spare", () => {
    expect(windowOf(-500, 600, 5000)).toEqual({ first: 0, count: 60 });
    expect(windowOf(ROW_HEIGHT * 1000, 600, 5000)).toEqual({ first: 980, count: 60 });
    expect(windowOf(ROW_HEIGHT * 10_000, 600, 5000).first).toBe(4999);
    expect(VIRTUAL_FROM).toBe(200);
  });
});
