// Verifier (adversarial): drop routes, edge cases.
import { describe, expect, it } from "vitest";
import { createMockApi } from "../../api/mock";
import { isLooseFile } from "../../plugins/notes/model";
import { DropFlow, type DropHost } from "./dropFlow.svelte";

function flowOn(workspace: "project" | "notes", area: "other" | "explorer" = "other") {
  const mock = createMockApi();
  const log: string[] = [];
  const host: DropHost = {
    workspace: () => workspace,
    spot: () => (area === "explorer" ? { area: "explorer", folder: "docs" } : { area: "other" }),
    folderName: (f) => f,
    tree: {
      over: () => undefined,
      hold: () => log.push("tree.hold"),
      end: (kept) => log.push(`tree.end ${kept}`),
    },
    imports: {
      busy: false,
      // the stub's question stays open once asked (its plan never ends it)
      waiting: true,
      plan: () => Promise.resolve(),
      discard: () => log.push("imports.discard"),
    },
    pictures: () => true,
    fail: () => undefined,
    note: () => undefined,
    shown: () => true,
  };
  const flow = new DropFlow(mock, host, (run) => {
    run();
    return () => undefined;
  });
  return { mock, flow, log };
}

describe("verifier: drop flow", () => {
  it("a folder whose name ends in .md, dropped on the Notes tab, opens as a tab (folders never become loose files)", async () => {
    const { flow, mock } = flowOn("notes");
    // the backend looks at the path itself: this one is a folder
    mock.probe.drops.folders.add("C:/proj.md");
    const loose: string[][] = [];
    flow.notes = {
      takes: isLooseFile,
      open: (paths) => {
        loose.push(paths);
        return Promise.resolve();
      },
    };
    await flow.drop(["C:/proj.md"], 1, 1);
    expect(loose).toEqual([]);
  });

  it("a second drag that leaves without dropping keeps the folders the open question is about", async () => {
    const { flow, log } = flowOn("project", "explorer");
    // first drop: question asked (tree held); a second drag passes over the view and leaves
    await flow.drop(["C:/out/a.md"], 5, 5);
    expect(log).toEqual(["imports.discard", "tree.hold"]);
    flow.leave();
    expect(log.filter((l) => l.startsWith("tree.end"))).toEqual([]);
  });
});
