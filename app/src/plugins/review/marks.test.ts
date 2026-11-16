// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { unfold } from "./marks";

describe("a change inside a folded callout", () => {
  it("is shown: every folded callout of a changed block | ghost opens, other folds stay", () => {
    const root = document.createElement("div");
    root.innerHTML =
      '<details class="markdown-alert markdown-alert-note"><summary class="markdown-alert-title">T</summary><p>hidden</p></details>' +
      '<blockquote><details class="markdown-alert markdown-alert-tip" open><summary class="markdown-alert-title">U</summary><p>x</p></details></blockquote>' +
      '<details class="prompt-block" data-tag="context"><summary>context</summary><p>y</p></details>';
    unfold(root);
    const folds = [...root.querySelectorAll("details")];
    expect(folds.map((fold) => fold.open)).toEqual([true, true, false]);
  });
});
