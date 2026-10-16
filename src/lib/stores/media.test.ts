import { beforeEach, describe, expect, it } from "vitest";
import { media } from "./media.svelte";

beforeEach(() => {
  media.clear();
  media.setBox({ width: 0, height: 0 });
});

describe("the viewer state", () => {
  it("fits a big picture to the pane and writes the scale it ends up at", () => {
    media.show("assets/shot.png");
    media.setBox({ width: 1000, height: 500 });
    media.noteFacts("assets/shot.png", { width: 2000, height: 1000 });
    expect(media.isFit).toBe(true);
    expect(media.fit).toBe(0.5);
    expect(media.scale).toBe(0.5);
    expect(media.label).toBe("50%");
  });

  it("zooms with the keys, resets to fit, and toggles 100%", () => {
    media.show("a.png");
    media.setBox({ width: 1000, height: 500 });
    media.noteFacts("a.png", { width: 2000, height: 1000 });

    media.zoomBy(1); // from 50% to the next step
    expect(media.isFit).toBe(false);
    expect(media.scale).toBe(0.67);
    media.zoomBy(-1);
    media.zoomBy(-1);
    expect(media.scale).toBe(0.33);

    media.reset();
    expect(media.isFit).toBe(true);
    expect(media.label).toBe("50%");

    media.toggleActual();
    expect(media.scale).toBe(1);
    expect(media.label).toBe("100%");
    media.toggleActual();
    expect(media.isFit).toBe(true);
  });

  it("starts over for another file and keeps its zoom when the same file is read again", () => {
    media.show("a.png");
    media.noteFacts("a.png", { width: 100, height: 80 });
    media.zoomBy(1);
    const zoomed = media.scale;

    media.show("a.png"); // the file changed on disk and was read again
    expect(media.scale).toBe(zoomed);
    expect(media.facts).toEqual({ width: 100, height: 80 });

    media.show("b.png");
    expect(media.isFit).toBe(true);
    expect(media.facts).toEqual({});
  });

  it("ignores what is learned of a file that is not on show", () => {
    media.show("a.png");
    media.noteFacts("old.png", { width: 5, height: 5 });
    expect(media.facts).toEqual({});
    media.noteFacts("a.png", { duration: 12 });
    media.noteFacts("a.png", { width: 30, height: 20 });
    expect(media.facts).toEqual({ duration: 12, width: 30, height: 20 });
  });

  it("fits at 100% until the picture and the pane are known", () => {
    media.show("a.png");
    expect(media.fit).toBe(1);
    expect(media.label).toBe("100%");
    media.clear();
    expect(media.path).toBeNull();
  });
});
