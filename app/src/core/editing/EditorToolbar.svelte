<script lang="ts">
  // Edit-mode toolbar: block type, marks, link, lists with indent | outdent, quote, table, code block + language,
  // image, rule. Shows what the selection is; does nothing on a locked block.
  import Bold from "@lucide/svelte/icons/bold";
  import Code from "@lucide/svelte/icons/code";
  import Image from "@lucide/svelte/icons/image";
  import IndentDecrease from "@lucide/svelte/icons/indent-decrease";
  import IndentIncrease from "@lucide/svelte/icons/indent-increase";
  import Italic from "@lucide/svelte/icons/italic";
  import Link from "@lucide/svelte/icons/link";
  import List from "@lucide/svelte/icons/list";
  import ListChecks from "@lucide/svelte/icons/list-checks";
  import ListOrdered from "@lucide/svelte/icons/list-ordered";
  import Minus from "@lucide/svelte/icons/minus";
  import Quote from "@lucide/svelte/icons/quote";
  import SquareCode from "@lucide/svelte/icons/square-code";
  import Strikethrough from "@lucide/svelte/icons/strikethrough";
  import Table from "@lucide/svelte/icons/table";
  import { t, type MessageKey } from "../i18n";
  import type { BlockType, Command, Format } from "./visual/commands";

  type Props = {
    format: Format;
    run: (command: Command) => void;
    link: () => void;
    image: () => void;
  };
  let { format, run, link, image }: Props = $props();

  const BLOCKS: { value: BlockType; label: MessageKey }[] = [
    { value: "paragraph", label: "toolbar.paragraph" },
    { value: "h1", label: "toolbar.h1" },
    { value: "h2", label: "toolbar.h2" },
    { value: "h3", label: "toolbar.h3" },
    { value: "h4", label: "toolbar.h4" },
    { value: "h5", label: "toolbar.h5" },
    { value: "h6", label: "toolbar.h6" },
  ];

  let tableMenu = $state(false);
  let tableHost: HTMLElement | undefined = $state();

  // the open table menu takes Escape first (before "leave Edit") and closes on a press outside it
  $effect(() => {
    if (!tableMenu) return;
    const onKey = (event: KeyboardEvent): void => {
      if (event.key !== "Escape" || event.isComposing) return;
      event.preventDefault();
      event.stopPropagation();
      tableMenu = false;
      tableHost?.querySelector<HTMLElement>("button")?.focus();
    };
    const onPointer = (event: PointerEvent): void => {
      if (!(event.target instanceof Node) || !tableHost?.contains(event.target)) tableMenu = false;
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("pointerdown", onPointer, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("pointerdown", onPointer, true);
    };
  });

  function onBlock(event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLSelectElement)) return;
    const found = BLOCKS.find((b) => b.value === target.value);
    if (found) run({ kind: "block", to: found.value });
  }

  function table(command: Command): void {
    tableMenu = false;
    run(command);
  }

  function setLanguage(value: string): void {
    if (value.trim() !== (format.language ?? "")) run({ kind: "language", language: value });
  }

  function onLanguageKey(event: KeyboardEvent & { currentTarget: HTMLInputElement }): void {
    if (event.isComposing) return;
    if (event.key === "Enter") {
      event.preventDefault();
      setLanguage(event.currentTarget.value);
    }
  }
</script>

<div class="toolbar" role="toolbar" aria-label={t("toolbar.label")} class:locked={format.locked}>
  <select
    class="block"
    aria-label={t("toolbar.blockType")}
    value={BLOCKS.some((b) => b.value === format.block) ? format.block : ""}
    disabled={format.locked}
    onchange={onBlock}
  >
    {#if !BLOCKS.some((b) => b.value === format.block)}
      <option value="" disabled>{t("toolbar.mixed")}</option>
    {/if}
    {#each BLOCKS as block (block.value)}
      <option value={block.value}>{t(block.label)}</option>
    {/each}
  </select>
  <span class="sep"></span>
  <button
    type="button"
    class="icon-btn"
    aria-pressed={format.strong}
    title={t("toolbar.bold")}
    aria-label={t("toolbar.bold")}
    disabled={format.locked}
    onclick={() => run({ kind: "strong" })}><Bold size={15} /></button
  >
  <button
    type="button"
    class="icon-btn"
    aria-pressed={format.emphasis}
    title={t("toolbar.italic")}
    aria-label={t("toolbar.italic")}
    disabled={format.locked}
    onclick={() => run({ kind: "emphasis" })}><Italic size={15} /></button
  >
  <button
    type="button"
    class="icon-btn"
    aria-pressed={format.strike}
    title={t("toolbar.strike")}
    aria-label={t("toolbar.strike")}
    disabled={format.locked}
    onclick={() => run({ kind: "strike" })}><Strikethrough size={15} /></button
  >
  <button
    type="button"
    class="icon-btn"
    aria-pressed={format.code}
    title={t("toolbar.code")}
    aria-label={t("toolbar.code")}
    disabled={format.locked}
    onclick={() => run({ kind: "code" })}><Code size={15} /></button
  >
  <button
    type="button"
    class="icon-btn"
    aria-pressed={format.link !== null}
    title={t("toolbar.link")}
    aria-label={t("toolbar.link")}
    disabled={format.locked}
    onclick={link}><Link size={15} /></button
  >
  <span class="sep"></span>
  <button
    type="button"
    class="icon-btn"
    aria-pressed={format.list === "bullet"}
    title={t("toolbar.bulletList")}
    aria-label={t("toolbar.bulletList")}
    disabled={format.locked}
    onclick={() => run({ kind: "bulletList" })}><List size={15} /></button
  >
  <button
    type="button"
    class="icon-btn"
    aria-pressed={format.list === "ordered"}
    title={t("toolbar.orderedList")}
    aria-label={t("toolbar.orderedList")}
    disabled={format.locked}
    onclick={() => run({ kind: "orderedList" })}><ListOrdered size={15} /></button
  >
  <button
    type="button"
    class="icon-btn"
    aria-pressed={format.list === "task"}
    title={t("toolbar.taskList")}
    aria-label={t("toolbar.taskList")}
    disabled={format.locked}
    onclick={() => run({ kind: "taskList" })}><ListChecks size={15} /></button
  >
  <button
    type="button"
    class="icon-btn"
    title={t("toolbar.outdent")}
    aria-label={t("toolbar.outdent")}
    disabled={format.locked || format.list === null}
    onclick={() => run({ kind: "outdent" })}><IndentDecrease size={15} /></button
  >
  <button
    type="button"
    class="icon-btn"
    title={t("toolbar.indent")}
    aria-label={t("toolbar.indent")}
    disabled={format.locked || format.list === null}
    onclick={() => run({ kind: "indent" })}><IndentIncrease size={15} /></button
  >
  <span class="sep"></span>
  <button
    type="button"
    class="icon-btn"
    title={t("toolbar.quote")}
    aria-label={t("toolbar.quote")}
    disabled={format.locked}
    onclick={() => run({ kind: "quote" })}><Quote size={15} /></button
  >
  <div class="menu-host" bind:this={tableHost}>
    <button
      type="button"
      class="icon-btn"
      aria-expanded={tableMenu}
      title={t("toolbar.table")}
      aria-label={t("toolbar.table")}
      disabled={format.locked}
      onclick={() => (tableMenu = !tableMenu)}><Table size={15} /></button
    >
    {#if tableMenu}
      <div class="menu" role="menu">
        <button type="button" role="menuitem" onclick={() => table({ kind: "table" })}
          >{t("toolbar.tableInsert")}</button
        >
        <button
          type="button"
          role="menuitem"
          disabled={!format.table || format.tableHeader}
          onclick={() => table({ kind: "addRowAbove" })}>{t("toolbar.tableAddRowAbove")}</button
        >
        <button
          type="button"
          role="menuitem"
          disabled={!format.table}
          onclick={() => table({ kind: "addRow" })}>{t("toolbar.tableAddRow")}</button
        >
        <button
          type="button"
          role="menuitem"
          disabled={!format.table}
          onclick={() => table({ kind: "addColumnLeft" })}>{t("toolbar.tableAddColumnLeft")}</button
        >
        <button
          type="button"
          role="menuitem"
          disabled={!format.table}
          onclick={() => table({ kind: "addColumn" })}>{t("toolbar.tableAddColumn")}</button
        >
        <button
          type="button"
          role="menuitem"
          disabled={!format.table}
          onclick={() => table({ kind: "deleteRow" })}>{t("toolbar.tableDeleteRow")}</button
        >
        <button
          type="button"
          role="menuitem"
          disabled={!format.table}
          onclick={() => table({ kind: "deleteColumn" })}>{t("toolbar.tableDeleteColumn")}</button
        >
        <button
          type="button"
          role="menuitem"
          disabled={!format.table}
          onclick={() => table({ kind: "deleteTable" })}>{t("toolbar.tableDelete")}</button
        >
      </div>
    {/if}
  </div>
  <button
    type="button"
    class="icon-btn"
    aria-pressed={format.block === "code"}
    title={t("toolbar.codeBlock")}
    aria-label={t("toolbar.codeBlock")}
    disabled={format.locked}
    onclick={() => run({ kind: "codeBlock" })}><SquareCode size={15} /></button
  >
  {#if format.language !== null}
    <input
      class="language"
      aria-label={t("toolbar.language")}
      placeholder={t("toolbar.language")}
      value={format.language}
      onkeydown={onLanguageKey}
      onblur={(e) => setLanguage(e.currentTarget.value)}
    />
  {/if}
  <button
    type="button"
    class="icon-btn"
    title={t("toolbar.image")}
    aria-label={t("toolbar.image")}
    disabled={format.locked}
    onclick={image}><Image size={15} /></button
  >
  <button
    type="button"
    class="icon-btn"
    title={t("toolbar.rule")}
    aria-label={t("toolbar.rule")}
    disabled={format.locked}
    onclick={() => run({ kind: "rule" })}><Minus size={15} /></button
  >
</div>

<style>
  .toolbar {
    position: sticky;
    top: 0;
    z-index: 5;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px;
    padding: 4px var(--sp-3);
    border-bottom: 1px solid var(--border);
    background: var(--bg);
  }
  .sep {
    width: 1px;
    height: 18px;
    margin: 0 4px;
    background: var(--border);
  }
  .block,
  .language {
    height: 26px;
    padding: 0 6px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font-size: var(--fs-ui);
  }
  .language {
    width: 110px;
  }
  .icon-btn[aria-pressed="true"] {
    background: var(--accent-soft);
    color: var(--accent-text);
  }
  .menu-host {
    position: relative;
  }
  .menu {
    position: absolute;
    top: 30px;
    left: 0;
    z-index: 10;
    display: flex;
    flex-direction: column;
    min-width: 180px;
    padding: 4px;
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    box-shadow: var(--shadow-pop);
  }
  .menu button {
    padding: 6px 10px;
    border: none;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
    text-align: left;
    font-size: var(--fs-ui);
    cursor: pointer;
  }
  .menu button:hover:not(:disabled) {
    background: var(--hover);
  }
  .menu button:disabled {
    color: var(--faint);
    cursor: default;
  }
</style>
