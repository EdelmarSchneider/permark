// @vitest-environment jsdom
// The editing session used from effects: a page sets its holds up while it attaches, inside an effect. Doing so must not
// make that effect depend on the session's own lists, or it runs again without end and the window freezes.
import { flushSync } from "svelte";
import { expect, it } from "vitest";
import { createMockApi } from "../../api/mock";
import { EditingSession } from "./session.svelte";

it("a hold set up from an effect does not make that effect run again, and its removal lets go", () => {
  const session = new EditingSession(createMockApi());
  let runs = 0;
  const stop = $effect.root(() => {
    $effect(() => {
      runs += 1;
      return session.block((path) => (path === "a.md" ? "held" : null));
    });
  });
  flushSync();
  expect(runs).toBe(1);
  expect(session.blockedBy("a.md")).toBe("held");
  expect(session.blockedBy("b.md")).toBeNull();
  stop();
  expect(session.blockedBy("a.md")).toBeNull();
});

it("two holds from two effects both count, and each removal takes only its own", () => {
  const session = new EditingSession(createMockApi());
  let firstOn = $state(true);
  const stop = $effect.root(() => {
    $effect(() => {
      if (!firstOn) return undefined;
      return session.block((path) => (path === "a.md" ? "first" : null));
    });
    $effect(() => session.block((path) => (path === "b.md" ? "second" : null)));
  });
  flushSync();
  expect(session.blockedBy("a.md")).toBe("first");
  expect(session.blockedBy("b.md")).toBe("second");
  firstOn = false;
  flushSync();
  expect(session.blockedBy("a.md")).toBeNull();
  expect(session.blockedBy("b.md")).toBe("second");
  stop();
  expect(session.blockedBy("b.md")).toBeNull();
});
