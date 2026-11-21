// @vitest-environment jsdom
// The text analyses measure in View: one read per version shared by every analysis, and a read that failed is not
// kept (the next analysis asking reads again).
import { flushSync } from "svelte";
import { afterEach, expect, it, vi } from "vitest";
import { MOCK_FOLDER } from "../../api/mock";
import type { AnalysisText } from "./analysisText.svelte";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { api } = await import("../../api");
const { documents } = await import("./document.svelte");
const { createAnalysisText } = await import("./analysisText.svelte");

const stops: (() => void)[] = [];

function analysis(): AnalysisText {
  let made: AnalysisText | null = null;
  stops.push(
    $effect.root(() => {
      made = createAnalysisText();
    }),
  );
  flushSync();
  if (made === null) throw new Error("no analysis");
  return made;
}

afterEach(() => {
  for (const stop of stops.splice(0)) stop();
  documents.setWorkspace(null);
  vi.restoreAllMocks();
});

it("a read that failed is asked again by the next analysis, which gets the text", async () => {
  await api.openPath(MOCK_FOLDER, "thisTab");
  const boot = await api.windowBoot();
  if (!boot.ok || boot.value.view.state !== "showing") throw new Error("no workspace");
  documents.setWorkspace(boot.value.view.workspace);
  await documents.show("README.md");
  await vi.waitFor(() => expect(documents.view?.path).toBe("README.md"));
  const read = vi.spyOn(api, "documentText").mockRejectedValueOnce(new Error("transport gone"));
  const first = analysis();
  await vi.waitFor(() => expect(read).toHaveBeenCalledTimes(1));
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(first.text).toBe("");
  const second = analysis();
  await vi.waitFor(() => expect(second.text).not.toBe(""), { timeout: 2000 });
  expect(read).toHaveBeenCalledTimes(2);
});
