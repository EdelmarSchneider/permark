// @vitest-environment jsdom
// A formula drawn in an Edit table cell after the table was measured: the editor's own column fitter measures that
// table again (its spans were read from the formula's source text, far wider than the formula).
import "./jsdomShims";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { Segment } from "../../../api/generated/Segment";
import { createMockApi } from "../../../api/mock";
import { Decorator } from "../../doc/decorate";
import { TableFitter } from "../../doc/tableFitter";
import { buildDoc } from "./compose";
import { createCore, type Core, type LockedHost } from "./core";

const decorator = new Decorator(createMockApi());
decorator.setEmbeds(() => () => import("../../testing/EmbedProbe.svelte"));

const host: LockedHost = {
  render: () => null,
  refresh: () => Promise.resolve(),
  inline: () => null,
  blocks: { enhance: (element) => element, near: (element) => decorator.near(element) },
  card: () => null,
  properties: () => null,
  reasonText: (reason) => reason,
  editLabel: "Source",
  edit: () => {},
  sourceLabel: "Source of this block",
  sourceHint: "Esc to finish",
  blockLabel: (name) => name,
  openHint: "Enter to edit",
};

let core: Core;

beforeAll(async () => {
  const target = document.createElement("div");
  document.body.append(target);
  core = await createCore({
    host: target,
    locked: host,
    embedMounted: (told) => decorator.onMounted(told),
  });
});

afterAll(async () => {
  await core.destroy();
  decorator.dispose();
});

afterEach(() => {
  vi.restoreAllMocks();
});

const TABLE = "| Name | Formula |\n| --- | --- |\n| area | $a^2 + b^2$ |\n";
const SEGMENTS: Segment[] = [{ lineStart: 1, lineEnd: 3, kind: "table", lock: null }];

describe("embeds in an Edit table", () => {
  it("a formula mounted in a cell has the editor's table measured again by the editor's fitter", async () => {
    const redrawn = vi.spyOn(TableFitter.prototype, "redrawn");
    core.setDoc((await buildDoc(core, TABLE, SEGMENTS)).doc);
    await decorator.idle();
    const table = core.view.dom.querySelector("table");
    expect(table?.querySelector(".embed-probe")?.textContent).toBe("a^2 + b^2");
    expect(redrawn.mock.calls).toEqual([[table]]);
  });

  it("stops listening when the editor goes", async () => {
    const offs: string[] = [];
    const target = document.createElement("div");
    document.body.append(target);
    const other = await createCore({
      host: target,
      embedMounted: () => () => void offs.push("off"),
    });
    expect(offs).toEqual([]);
    await other.destroy();
    expect(offs).toEqual(["off"]);
    target.remove();
  });
});
