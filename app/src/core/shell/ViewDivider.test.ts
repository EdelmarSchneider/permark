// @vitest-environment jsdom
// The divider between two groups of a region: a bar across the stack. Groups one above the other: a lying bar, Up |
// Down, the pointer's way down. Groups side by side (the bottom region): an upright bar, Left | Right, the pointer's
// way across. It reports the share the group before it takes of both.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MIN_GROUP_PX, type Axis } from "./arrangement";
import ViewDivider from "./ViewDivider.svelte";

// jsdom has no pointer capture: a divider that is dragged asks for it
Element.prototype.setPointerCapture ??= () => {};
Element.prototype.releasePointerCapture ??= () => {};
Element.prototype.hasPointerCapture ??= () => false;

let component: Record<string, unknown> | null = null;

type Sizes = { above: number; below: number };

/** A divider between two groups of 300 px each along the stack (or as `measure` says); the splits it reported. */
function show(
  axis?: Axis,
  measure: () => Sizes = () => ({ above: 300, below: 300 }),
): { bar: HTMLElement; splits: number[] } {
  const target = document.createElement("div");
  document.body.append(target);
  const splits: number[] = [];
  component = mount(ViewDivider, {
    target,
    props: {
      top: "Outline",
      bottom: "Live changes",
      controls: "view-outline",
      measure,
      onsplit: (ratio: number) => splits.push(ratio),
      ...(axis === undefined ? {} : { axis }),
    },
  });
  flushSync();
  const bar = target.querySelector<HTMLElement>('[role="separator"]');
  if (bar === null) throw new Error("no divider");
  return { bar, splits };
}

/** The key pressed on the divider: whether it took it. */
function press(bar: HTMLElement, key: string, shiftKey = false): boolean {
  const event = new KeyboardEvent("keydown", { key, shiftKey, bubbles: true, cancelable: true });
  bar.dispatchEvent(event);
  return event.defaultPrevented;
}

function pointer(bar: HTMLElement, type: string, clientX: number, clientY: number): void {
  bar.dispatchEvent(
    new PointerEvent(type, { button: 0, pointerId: 1, clientX, clientY, bubbles: true }),
  );
}

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
  vi.unstubAllGlobals();
});

/** What the divider tells assistive tech: its smallest, its largest and its split now (percent). */
const published = (bar: HTMLElement): (string | null)[] =>
  ["aria-valuemin", "aria-valuemax", "aria-valuenow"].map((name) => bar.getAttribute(name));

describe("what the divider publishes", () => {
  it.each(["vertical", "horizontal"] as const)(
    "between groups stacked %s: its real limits (a group at its smallest) from the start, not 0 | 100 until focused",
    (axis) => {
      const { bar } = show(axis);
      const least = Math.round((MIN_GROUP_PX / 600) * 100);
      expect(published(bar)).toEqual([String(least), String(100 - least), "50"]);
      // Home | End stop exactly there
      press(bar, "Home");
      flushSync();
      expect(bar.getAttribute("aria-valuenow")).toBe(String(least));
      press(bar, "End");
      flushSync();
      expect(bar.getAttribute("aria-valuenow")).toBe(String(100 - least));
    },
  );

  it("follows the groups' sizes: the view before it changes size (the window resized, a split moved) and the limits are read again", () => {
    const watched: Element[] = [];
    let resized: () => void = () => {};
    let stopped = 0;
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(heard: () => void) {
          resized = heard;
        }
        observe(el: Element): void {
          watched.push(el);
        }
        unobserve(): void {}
        disconnect(): void {
          stopped += 1;
        }
      },
    );
    const before = document.createElement("div");
    before.id = "view-outline";
    document.body.append(before);
    let sizes: Sizes = { above: 300, below: 300 };
    const { bar } = show("horizontal", () => sizes);
    expect(watched).toEqual([before]);
    expect(published(bar)).toEqual(["35", "65", "50"]);
    sizes = { above: 250, below: 750 };
    resized();
    flushSync();
    expect(published(bar)).toEqual(["21", "79", "25"]);
    // a group that is not drawn yet has no size: nothing new to publish
    sizes = { above: 250, below: 0 };
    resized();
    flushSync();
    expect(published(bar)).toEqual(["21", "79", "25"]);
    if (component) void unmount(component);
    component = null;
    expect(stopped).toBe(1);
  });
});

describe("between groups one above the other", () => {
  it("is a lying bar: Up | Down move it by 16 | 64 with Shift; Left | Right are not its keys", () => {
    const { bar, splits } = show();
    expect(bar.getAttribute("aria-orientation")).toBe("horizontal");
    expect(bar.getAttribute("aria-controls")).toBe("view-outline");
    expect(bar.getAttribute("aria-label")).toBe("Resize Outline and Live changes");
    expect(press(bar, "ArrowDown")).toBe(true);
    expect(press(bar, "ArrowUp")).toBe(true);
    expect(press(bar, "ArrowDown", true)).toBe(true);
    expect(splits).toEqual([316 / 600, 284 / 600, 364 / 600]);
    expect(press(bar, "ArrowLeft")).toBe(false);
    expect(press(bar, "ArrowRight")).toBe(false);
    expect(splits).toHaveLength(3);
  });

  it("follows the pointer down and up, not across", () => {
    const { bar, splits } = show("vertical");
    pointer(bar, "pointerdown", 100, 100);
    pointer(bar, "pointermove", 100, 150);
    expect(splits.at(-1)).toBe(350 / 600);
    pointer(bar, "pointermove", 500, 150);
    expect(splits.at(-1)).toBe(350 / 600);
    pointer(bar, "pointerup", 500, 150);
  });
});

describe("between groups side by side", () => {
  it("is an upright bar: Left | Right move it by 16 | 64 with Shift; Up | Down are not its keys", () => {
    const { bar, splits } = show("horizontal");
    expect(bar.getAttribute("aria-orientation")).toBe("vertical");
    expect(bar.getAttribute("aria-controls")).toBe("view-outline");
    expect(press(bar, "ArrowRight")).toBe(true);
    expect(press(bar, "ArrowLeft")).toBe(true);
    expect(press(bar, "ArrowRight", true)).toBe(true);
    expect(press(bar, "ArrowLeft", true)).toBe(true);
    expect(splits).toEqual([316 / 600, 284 / 600, 364 / 600, 236 / 600]);
    expect(press(bar, "ArrowUp")).toBe(false);
    expect(press(bar, "ArrowDown")).toBe(false);
    expect(splits).toHaveLength(4);
  });

  it("Home | End leave one group at its smallest, Enter | a double click give half each", () => {
    const { bar, splits } = show("horizontal");
    press(bar, "Home");
    press(bar, "End");
    press(bar, "Enter");
    bar.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    expect(splits).toEqual([MIN_GROUP_PX / 600, (600 - MIN_GROUP_PX) / 600, 0.5, 0.5]);
  });

  it("follows the pointer across, not down; never past a group's smallest; says the split", () => {
    const { bar, splits } = show("horizontal");
    // not held: a pointer passing over it changes nothing
    pointer(bar, "pointermove", 180, 100);
    expect(splits).toEqual([]);
    pointer(bar, "pointerdown", 100, 100);
    pointer(bar, "pointermove", 150, 100);
    expect(splits.at(-1)).toBe(350 / 600);
    pointer(bar, "pointermove", 150, 900);
    expect(splits.at(-1)).toBe(350 / 600);
    pointer(bar, "pointermove", 60, 100);
    expect(splits.at(-1)).toBe(260 / 600);
    pointer(bar, "pointermove", -900, 100);
    expect(splits.at(-1)).toBe(MIN_GROUP_PX / 600);
    flushSync();
    expect(bar.getAttribute("aria-valuenow")).toBe("35");
    expect(bar.getAttribute("aria-valuetext")).toBe("Outline 35%");
    pointer(bar, "pointerup", -900, 100);
    const count = splits.length;
    pointer(bar, "pointermove", 300, 100);
    expect(splits).toHaveLength(count);
  });
});
