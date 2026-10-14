import { describe, expect, it } from "vitest";
import {
  INITIAL_UPDATE_STATE,
  describeUpdate,
  isBusy,
  reduceUpdate,
  updatePercent,
  type UpdateEvent,
  type UpdateState,
} from "./updates";

function run(events: UpdateEvent[], from: UpdateState = INITIAL_UPDATE_STATE): UpdateState {
  return events.reduce(reduceUpdate, from);
}

const found: UpdateEvent = { type: "found", version: "0.2.0" };
const available = run([{ type: "checkStarted", manual: false }, found]);

describe("checking", () => {
  it("starts from idle, up to date and failed", () => {
    for (const from of [
      INITIAL_UPDATE_STATE,
      run([{ type: "checkStarted", manual: true }, { type: "none" }]),
      run([
        { type: "checkStarted", manual: true },
        { type: "checkFailed", message: "x" },
      ]),
    ]) {
      expect(reduceUpdate(from, { type: "checkStarted", manual: true })).toEqual({
        phase: "checking",
        manual: true,
      });
    }
  });

  it("finds an update", () => {
    expect(available).toEqual({ phase: "available", version: "0.2.0" });
  });

  it("reports up to date only for a manual check", () => {
    expect(run([{ type: "checkStarted", manual: true }, { type: "none" }])).toEqual({
      phase: "upToDate",
    });
    expect(run([{ type: "checkStarted", manual: false }, { type: "none" }])).toEqual({
      phase: "idle",
    });
  });

  it("ignores network errors of the automatic check and reports those of a manual one", () => {
    const failed: UpdateEvent = { type: "checkFailed", message: "offline" };
    expect(run([{ type: "checkStarted", manual: false }, failed])).toEqual({ phase: "idle" });
    expect(run([{ type: "checkStarted", manual: true }, failed])).toEqual({
      phase: "failed",
      message: "offline",
    });
  });

  it("does not start a second check while one runs or an update is known", () => {
    const checking = run([{ type: "checkStarted", manual: false }]);
    expect(reduceUpdate(checking, { type: "checkStarted", manual: true })).toBe(checking);
    expect(reduceUpdate(available, { type: "checkStarted", manual: true })).toBe(available);
  });

  it("ignores results that arrive when no check is running", () => {
    expect(reduceUpdate(INITIAL_UPDATE_STATE, found)).toBe(INITIAL_UPDATE_STATE);
    expect(reduceUpdate(available, { type: "none" })).toBe(available);
    expect(reduceUpdate(available, { type: "checkFailed", message: "late" })).toBe(available);
  });
});

describe("installing", () => {
  const downloading = run([{ type: "installRequested" }], available);

  it("starts from an available update only", () => {
    expect(downloading).toEqual({
      phase: "downloading",
      version: "0.2.0",
      downloaded: 0,
      total: null,
    });
    expect(reduceUpdate(INITIAL_UPDATE_STATE, { type: "installRequested" })).toBe(
      INITIAL_UPDATE_STATE,
    );
    expect(reduceUpdate(downloading, { type: "installRequested" })).toBe(downloading);
  });

  it("tracks the download and ends in the installing phase", () => {
    const state = run(
      [
        { type: "downloadStarted", total: 1000 },
        { type: "downloadProgress", chunk: 250 },
        { type: "downloadProgress", chunk: 250 },
      ],
      downloading,
    );
    expect(state).toMatchObject({ phase: "downloading", downloaded: 500, total: 1000 });
    expect(updatePercent(state)).toBe(50);
    expect(reduceUpdate(state, { type: "downloadFinished" })).toEqual({
      phase: "installing",
      version: "0.2.0",
    });
  });

  it("goes back to the available update, with the reason, when it fails", () => {
    const failed = run([{ type: "installFailed", message: "disk full" }], downloading);
    expect(failed).toEqual({
      phase: "available",
      version: "0.2.0",
      error: "disk full",
    });
    // and it can be tried again
    expect(reduceUpdate(failed, { type: "installRequested" })).toMatchObject({
      phase: "downloading",
    });
  });

  it("keeps progress events of an unknown phase from changing anything", () => {
    expect(reduceUpdate(available, { type: "downloadProgress", chunk: 5 })).toBe(available);
    expect(reduceUpdate(INITIAL_UPDATE_STATE, { type: "downloadFinished" })).toBe(
      INITIAL_UPDATE_STATE,
    );
  });
});

describe("dismissing a result", () => {
  it("clears 'up to date' and a failed check, and nothing else", () => {
    const upToDate = run([{ type: "checkStarted", manual: true }, { type: "none" }]);
    const failed = run([
      { type: "checkStarted", manual: true },
      { type: "checkFailed", message: "x" },
    ]);
    expect(reduceUpdate(upToDate, { type: "dismissResult" })).toEqual({ phase: "idle" });
    expect(reduceUpdate(failed, { type: "dismissResult" })).toEqual({ phase: "idle" });
    expect(reduceUpdate(available, { type: "dismissResult" })).toBe(available);
  });
});

describe("updatePercent", () => {
  it("is unknown without a total, and capped at 100", () => {
    const base = run([{ type: "installRequested" }], available);
    expect(updatePercent(base)).toBeNull();
    expect(updatePercent(run([{ type: "downloadStarted", total: 0 }], base))).toBeNull();
    const over = run(
      [
        { type: "downloadStarted", total: 100 },
        { type: "downloadProgress", chunk: 130 },
      ],
      base,
    );
    expect(updatePercent(over)).toBe(100);
    expect(updatePercent(INITIAL_UPDATE_STATE)).toBeNull();
  });
});

describe("isBusy", () => {
  it("is true while checking, downloading or installing", () => {
    expect(isBusy(INITIAL_UPDATE_STATE)).toBe(false);
    expect(isBusy(available)).toBe(false);
    expect(isBusy(run([{ type: "checkStarted", manual: true }]))).toBe(true);
    expect(isBusy(run([{ type: "installRequested" }], available))).toBe(true);
    expect(
      isBusy(run([{ type: "installRequested" }, { type: "downloadFinished" }], available)),
    ).toBe(true);
  });
});

describe("describeUpdate", () => {
  it("says nothing while idle", () => {
    expect(describeUpdate(INITIAL_UPDATE_STATE)).toBe("");
  });

  it("names the version of an available update", () => {
    expect(describeUpdate(available)).toBe("Update available: v0.2.0");
  });

  it("reports the outcome of a manual check", () => {
    expect(describeUpdate(run([{ type: "checkStarted", manual: true }]))).toBe("Checking...");
    expect(describeUpdate(run([{ type: "checkStarted", manual: true }, { type: "none" }]))).toBe(
      "You're up to date",
    );
    expect(
      describeUpdate(
        run([
          { type: "checkStarted", manual: true },
          { type: "checkFailed", message: "x" },
        ]),
      ),
    ).toBe("Couldn't check for updates");
  });

  it("shows the download percentage when the size is known", () => {
    const start = run([{ type: "installRequested" }], available);
    expect(describeUpdate(start)).toBe("Downloading v0.2.0...");
    const half = run(
      [
        { type: "downloadStarted", total: 200 },
        { type: "downloadProgress", chunk: 100 },
      ],
      start,
    );
    expect(describeUpdate(half)).toBe("Downloading v0.2.0: 50%");
    expect(describeUpdate(reduceUpdate(half, { type: "downloadFinished" }))).toBe(
      "Installing v0.2.0...",
    );
  });
});
