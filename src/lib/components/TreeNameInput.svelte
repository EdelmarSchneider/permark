<script lang="ts">
  // The name of a new file, typed in the tree under its folder (docs/design/v0.2.md section 3, "New file"). It starts
  // as `untitled.md` or `untitled.txt` with the stem selected; that extension is added when there is none. Enter
  // creates the file and opens it for editing, Esc (or leaving the field) cancels.
  import { onMount, tick } from "svelte";
  import {
    NAME_PROBLEM_TEXT,
    freshName,
    nameProblem,
    namesIn,
    stemRange,
    withExtension,
    type NewFileKind,
  } from "../editor/newFile";
  import { t } from "../i18n";
  import { isComposingKey } from "../ime";
  import { app } from "../stores/app.svelte";
  import { ui } from "../stores/ui.svelte";
  import { vault } from "../stores/vault.svelte";
  import Icon from "./Icon.svelte";

  type Props = { folder: string; depth: number; kind: NewFileKind };
  let { folder, depth, kind }: Props = $props();

  let input: HTMLInputElement | undefined = $state();
  let value = $state("");
  let refused = $state<string | null>(null);
  let busy = $state(false);

  const siblings = $derived(namesIn(folder, vault.files));
  const name = $derived(withExtension(value, kind));
  const problem = $derived(value.trim() === "" ? null : nameProblem(name, siblings));
  const message = $derived(refused ?? (problem ? t(NAME_PROBLEM_TEXT[problem]) : null));

  onMount(() => {
    value = freshName(siblings, kind);
    // the field shows the name after the next update; only then can the stem be selected
    void tick().then(() => {
      const field = input;
      if (!field) return;
      field.focus();
      const [from, to] = stemRange(value);
      field.setSelectionRange(from, to);
    });
  });

  function cancel(refocus = false): void {
    if (busy) return;
    ui.newFile = null;
    // Esc goes back to the tree; leaving the field by clicking elsewhere keeps the focus where it went
    if (refocus) ui.explorerFocusTick++;
  }

  async function submit(): Promise<void> {
    if (busy) return;
    const final = withExtension(value, kind);
    const found = nameProblem(final, siblings);
    if (found) {
      refused = t(NAME_PROBLEM_TEXT[found]);
      return;
    }
    busy = true;
    refused = await app.createNewFile(folder, final);
    busy = false;
    if (refused) input?.focus();
  }

  function onkeydown(e: KeyboardEvent): void {
    if (isComposingKey(e)) return;
    if (e.key === "Enter") {
      e.preventDefault();
      void submit();
    } else if (e.key === "Escape") {
      // the field's own Esc: it must not also leave editing
      e.preventDefault();
      e.stopPropagation();
      cancel(true);
    }
  }
</script>

<div class="tree-new" role="presentation" style:--depth={depth}>
  <div class="tree-row tree-row--new">
    <span class="twisty-space"></span>
    <Icon name="file-plus" class="file-icon" />
    <input
      bind:this={input}
      bind:value
      class="tree-input"
      type="text"
      spellcheck="false"
      autocomplete="off"
      aria-label={t("newFile.label")}
      aria-invalid={message !== null}
      aria-describedby="new-file-message"
      title={t("newFile.hint")}
      disabled={busy}
      oninput={() => (refused = null)}
      {onkeydown}
      onblur={() => cancel()}
    />
  </div>
  <div id="new-file-message" class="tree-new__message" role="alert">{message ?? ""}</div>
</div>
