<script lang="ts">
  // A name typed in the Explorer (docs/design/v0.2.md section 17): the new name of an entry being renamed (F2), shown in
  // place of its name, or the name of a new folder, shown as a row under its parent. Enter confirms, Esc cancels, and
  // leaving the field confirms a good name that changed and cancels anything else.
  import { onMount, tick } from "svelte";
  import {
    ENTRY_PROBLEM_TEXT,
    entryNameProblem,
    freshFolderName,
    renameSelection,
  } from "../fileOps";
  import { t } from "../i18n";
  import { basename, dirname } from "../paths";
  import { fileops } from "../stores/fileops.svelte";
  import { ui } from "../stores/ui.svelte";
  import { vault } from "../stores/vault.svelte";
  import { childNames } from "../tree";
  import Icon from "./Icon.svelte";

  type Props =
    | { variant: "rename"; path: string; isDir: boolean }
    | { variant: "folder"; folder: string; depth: number };
  let props: Props = $props();

  const rename = $derived(props.variant === "rename" ? props : null);
  const parent = $derived(
    rename ? dirname(rename.path) : props.variant === "folder" ? props.folder : "",
  );
  const current = $derived(rename ? basename(rename.path) : null);
  const siblings = $derived(childNames(vault.tree, parent));

  let input: HTMLInputElement | undefined = $state();
  let value = $state("");
  let refused = $state<string | null>(null);
  let busy = $state(false);
  /** The field is done (confirmed or given up): the blur that comes when it is removed means nothing. */
  let finished = false;

  const problem = $derived(value.trim() === "" ? null : entryNameProblem(value, siblings, current));
  const message = $derived(refused ?? (problem ? t(ENTRY_PROBLEM_TEXT[problem]) : null));
  const label = $derived(
    rename ? t("fileOps.renameLabel", { name: current ?? "" }) : t("fileOps.newFolderLabel"),
  );

  onMount(() => {
    value = current ?? freshFolderName(t("fileOps.defaultFolderName"), siblings);
    // the field shows the name after the next update; only then can part of it be selected
    void tick().then(() => {
      const field = input;
      if (!field) return;
      field.focus();
      const [from, to] = renameSelection(value, rename?.isDir ?? true);
      field.setSelectionRange(from, to);
    });
  });

  function close(refocus: boolean): void {
    if (busy) return;
    finished = true;
    if (rename) fileops.cancelRename();
    else fileops.cancelNewFolder();
    // Esc goes back to the tree; leaving the field by clicking elsewhere keeps the focus where it went
    if (refocus) ui.explorerFocusTick++;
  }

  async function submit(): Promise<void> {
    if (busy) return;
    const name = value.trim();
    if (rename && name === current) return close(true);
    const found = entryNameProblem(name, siblings, current);
    if (found) {
      refused = t(ENTRY_PROBLEM_TEXT[found]);
      return;
    }
    busy = true;
    try {
      refused = rename
        ? await fileops.rename(rename.path, name)
        : await fileops.createFolder(parent, name);
    } finally {
      busy = false;
    }
    if (refused) {
      // the field is enabled again only after the next update: focusing it now would not stick
      await tick();
      input?.focus();
    } else {
      finished = true;
      if (rename) ui.explorerFocusTick++;
    }
  }

  function onblur(): void {
    if (busy || finished) return;
    // a rename that is good and different is kept; everything else is given up
    if (rename && value.trim() !== current && !problem && value.trim() !== "") void submit();
    else close(false);
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "Enter") {
      e.preventDefault();
      void submit();
    } else if (e.key === "Escape") {
      // the field's own Esc: it must not also close something else
      e.preventDefault();
      e.stopPropagation();
      close(true);
    }
  }
</script>

{#snippet field(id: string)}
  <input
    bind:this={input}
    bind:value
    class="tree-input"
    type="text"
    spellcheck="false"
    autocomplete="off"
    aria-label={label}
    aria-invalid={message !== null}
    aria-describedby={id}
    title={rename ? t("fileOps.renameHint") : t("fileOps.newFolderHint")}
    disabled={busy}
    oninput={() => (refused = null)}
    {onkeydown}
    {onblur}
  />
{/snippet}

{#if props.variant === "rename"}
  {@render field("rename-message")}
  <span id="rename-message" class="tree-input__message" role="alert">{message ?? ""}</span>
{:else}
  <div class="tree-new" role="presentation" style:--depth={props.depth}>
    <div class="tree-row tree-row--new">
      <span class="twisty-space"></span>
      <Icon name="folder-plus" class="file-icon" />
      {@render field("new-folder-message")}
    </div>
    <div id="new-folder-message" class="tree-new__message" role="alert">{message ?? ""}</div>
  </div>
{/if}
