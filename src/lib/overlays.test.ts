import { afterEach, describe, expect, it } from "vitest";
import { closeTopOverlay, hasKeyOverlay, hasOpenOverlay, registerOverlay } from "./overlays";

const forget: (() => void)[] = [];
afterEach(() => {
  while (forget.length > 0) forget.pop()?.();
});

describe("the overlay stack", () => {
  it("closes the topmost overlay and says whether there was one", () => {
    const closed: string[] = [];
    forget.push(registerOverlay(() => closed.push("menu")));
    forget.push(registerOverlay(() => closed.push("dialog")));
    expect(hasOpenOverlay()).toBe(true);
    expect(closeTopOverlay()).toBe(true);
    expect(closed).toEqual(["dialog"]);
  });

  it("keeps the plain keys for a menu or a dialog, but not for the find bar that a search result opened", () => {
    expect(hasKeyOverlay()).toBe(false);
    // the find bar opened over a result is not where the person is typing: C, E and T still mean what they mean while reading
    const closeFind = registerOverlay(() => {}, false);
    forget.push(closeFind);
    expect(hasOpenOverlay()).toBe(true);
    expect(hasKeyOverlay()).toBe(false);
    // Esc still closes it
    expect(closeTopOverlay()).toBe(true);
    // a dialog or a menu keeps the keys, whatever else is open
    forget.push(registerOverlay(() => {}));
    forget.push(registerOverlay(() => {}, false));
    expect(hasKeyOverlay()).toBe(true);
  });
});
