// @vitest-environment jsdom
// The status bar's git problem: its reason reaches a keyboard and a screen reader without a hover.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import type { GitErrorCode } from "../../api/generated/GitErrorCode";
import { conditions, neutralContext } from "../../core/conditions/context.svelte";
import GitProblem from "./GitProblem.svelte";

let component: Record<string, unknown> | null = null;

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  conditions.set(neutralContext());
  document.body.replaceChildren();
});

function shown(code: GitErrorCode, reason: string): HTMLButtonElement {
  conditions.set({ ...neutralContext(), git: { status: "error", code, reason } });
  const target = document.createElement("div");
  document.body.append(target);
  component = mount(GitProblem, { target });
  flushSync();
  const button = target.querySelector("button");
  if (!button) throw new Error("no problem shown");
  return button;
}

function description(button: HTMLElement): string {
  return (button.getAttribute("aria-describedby") ?? "")
    .split(" ")
    .map((id) => document.getElementById(id)?.textContent ?? "")
    .join(" ")
    .trim();
}

describe("the git problem in the status bar", () => {
  it("describes the button with the reason and, where another git helps, the way out", () => {
    const failed = shown("failed", "fatal: bad object HEAD");
    expect(description(failed)).toBe(
      "Details: fatal: bad object HEAD.\nOpen the settings to choose another git (the built-in one needs no trust).",
    );
  });

  it("names no other git for a slow answer", () => {
    expect(description(shown("timeout", ""))).toBe("Git didn't answer in time.");
  });
});
