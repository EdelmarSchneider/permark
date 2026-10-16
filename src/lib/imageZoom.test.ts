import { describe, expect, it } from "vitest";
import {
  FIT,
  ZOOM_STEPS,
  drawnSize,
  fitScale,
  percentLabel,
  scaleOf,
  stepZoom,
  toggleActualSize,
  type ImageZoom,
} from "./imageZoom";

describe("fitScale", () => {
  it("shrinks a big image to the box, keeping its shape", () => {
    expect(fitScale({ width: 2000, height: 1000 }, { width: 1000, height: 1000 })).toBe(0.5);
    // the height is the tighter side
    expect(fitScale({ width: 1000, height: 2000 }, { width: 1000, height: 500 })).toBe(0.25);
  });

  it("never blows a small image up", () => {
    expect(fitScale({ width: 16, height: 16 }, { width: 1000, height: 800 })).toBe(1);
    expect(fitScale({ width: 1000, height: 800 }, { width: 1000, height: 800 })).toBe(1);
  });

  it("is 100% while nothing is known about the image or the box", () => {
    expect(fitScale(null, { width: 500, height: 500 })).toBe(1);
    expect(fitScale({ width: 0, height: 0 }, { width: 500, height: 500 })).toBe(1);
    expect(fitScale({ width: 800, height: 600 }, { width: 0, height: 0 })).toBe(1);
  });
});

describe("scaleOf", () => {
  it("is the fit scale when fitted and the scale otherwise", () => {
    expect(scaleOf(FIT, 0.38)).toBe(0.38);
    expect(scaleOf({ mode: "scale", scale: 2 }, 0.38)).toBe(2);
  });
});

describe("stepZoom", () => {
  const scale = (zoom: ImageZoom): number => scaleOf(zoom, NaN);

  it("steps to the next preset above or below", () => {
    expect(scale(stepZoom({ mode: "scale", scale: 1 }, 1, 1))).toBe(1.25);
    expect(scale(stepZoom({ mode: "scale", scale: 1 }, -1, 1))).toBe(0.75);
    expect(scale(stepZoom({ mode: "scale", scale: 2 }, 1, 1))).toBe(3);
  });

  it("starts from the fit scale when the image is fitted", () => {
    // fitted at 38%: in goes to 50%, out to 33%
    expect(scale(stepZoom(FIT, 1, 0.38))).toBe(0.5);
    expect(scale(stepZoom(FIT, -1, 0.38))).toBe(0.33);
    // a small image fits at 100%: in goes to 125%
    expect(scale(stepZoom(FIT, 1, 1))).toBe(1.25);
  });

  it("walks the whole list and stops at its ends", () => {
    let zoom: ImageZoom = { mode: "scale", scale: 1 };
    for (let i = 0; i < 40; i++) zoom = stepZoom(zoom, 1, 1);
    expect(scale(zoom)).toBe(ZOOM_STEPS[ZOOM_STEPS.length - 1]);
    for (let i = 0; i < 40; i++) zoom = stepZoom(zoom, -1, 1);
    expect(scale(zoom)).toBe(ZOOM_STEPS[0]);
    // at the end nothing changes, and a fitted image stays fitted when there is nowhere to go
    expect(stepZoom(zoom, -1, 1)).toBe(zoom);
    expect(stepZoom(FIT, -1, 0.04)).toBe(FIT);
  });

  it("does not stay on a preset that is the scale already", () => {
    // fitted at exactly 50%: in is 67%, out is 33%
    expect(scale(stepZoom(FIT, 1, 0.5))).toBe(0.67);
    expect(scale(stepZoom(FIT, -1, 0.5))).toBe(0.33);
  });

  it("has presets in rising order", () => {
    expect([...ZOOM_STEPS]).toEqual([...ZOOM_STEPS].sort((a, b) => a - b));
    expect(ZOOM_STEPS).toContain(1);
  });
});

describe("toggleActualSize", () => {
  it("goes from fit to 100% and back", () => {
    const actual = toggleActualSize(FIT, 0.38);
    expect(actual).toEqual({ mode: "scale", scale: 1 });
    expect(toggleActualSize(actual, 0.38)).toBe(FIT);
  });

  it("goes to 100% from any other scale", () => {
    expect(toggleActualSize({ mode: "scale", scale: 3 }, 0.38)).toEqual({
      mode: "scale",
      scale: 1,
    });
  });

  it("changes nothing for an image that fits at 100% already", () => {
    expect(toggleActualSize(FIT, 1)).toBe(FIT);
  });
});

describe("percentLabel and drawnSize", () => {
  it("writes the scale as a rounded percent", () => {
    expect(percentLabel(1)).toBe("100%");
    expect(percentLabel(0.375)).toBe("38%");
    expect(percentLabel(0.05)).toBe("5%");
    expect(percentLabel(16)).toBe("1600%");
  });

  it("draws whole pixels, at least one", () => {
    expect(drawnSize({ width: 801, height: 451 }, 0.5)).toEqual({ width: 401, height: 226 });
    expect(drawnSize({ width: 3, height: 3 }, 0.05)).toEqual({ width: 1, height: 1 });
  });
});
