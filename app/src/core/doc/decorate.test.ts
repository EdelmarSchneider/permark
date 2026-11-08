import { describe, expect, it } from "vitest";
import type { LinkTarget, Result } from "../../api";
import { createMockApi } from "../../api/mock";
import { LinkCache } from "./decorate";

describe("LinkCache", () => {
  it("drops answers asked before it forgot them (files changed meanwhile)", async () => {
    let answer: (value: Result<LinkTarget[]>) => void = () => {};
    let asked = 0;
    const api = {
      ...createMockApi(),
      resolveLinks: () => {
        asked += 1;
        return new Promise<Result<LinkTarget[]>>((done) => {
          answer = done;
        });
      },
    };
    const cache = new LinkCache(api);
    cache.forDocument("a.md");
    const stale = cache.resolve(["b.md"]);
    // the request is out once the batch is sent
    await new Promise((done) => setTimeout(done, 5));
    expect(asked).toBe(1);
    cache.forget();
    answer({ ok: true, value: [{ type: "missing", path: "b.md" }] });
    expect(await stale).toEqual([null]);
    expect(cache.peek("b.md")).toBeUndefined();
    // asked again, the answer counts
    const fresh = cache.resolve(["b.md"]);
    await new Promise((done) => setTimeout(done, 5));
    answer({ ok: true, value: [{ type: "doc", path: "b.md", anchor: null }] });
    expect(await fresh).toEqual([{ type: "doc", path: "b.md", anchor: null }]);
  });
});
