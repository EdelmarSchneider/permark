<script lang="ts">
  // A small configuration file as a sheet: a card per table | mapping | section, a row per key with its value and
  // the comment that sits with it. Secret values are masked; a part that cannot be edited exactly says why. In Edit
  // mode values take typing; an ignore file also takes new lines and deleted lines; a data file (TOON) also takes
  // new key names, new and removed keys, and its tables take rows. "Edit in Source" jumps to the line.
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import CodeXml from "@lucide/svelte/icons/code-xml";
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import Lock from "@lucide/svelte/icons/lock";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash from "@lucide/svelte/icons/trash";
  import type { ConfigEdit, Group, NewValue, Row, Scalar, Sheet, Table } from "../../api";
  import { t, tn } from "../i18n";
  import { reasonKey, refusalKey } from "./reasons";
  import { MASK } from "./secrets";
  import SheetTable from "./SheetTable.svelte";
  import SheetValue from "./SheetValue.svelte";

  type Props = {
    sheet: Sheet;
    editable: boolean;
    /** Commits a new value; resolves to the word of a refusal, or null when it was made. */
    commit?: (path: string, value: NewValue) => Promise<string | null>;
    /** Ignore files: deletes the line of a row. */
    remove?: (path: string) => Promise<string | null>;
    /** Ignore files: adds a pattern after a row (`null`: at the end). */
    add?: (after: string | null, text: string) => Promise<string | null>;
    /** Data sheets: any other edit (a key name, a new | removed key, table rows). */
    edit?: (edit: ConfigEdit) => Promise<string | null>;
    /** Puts the person in Source at a line (`null`: the document cannot be edited). */
    source: ((line: number) => void) | null;
    /** Ignore files: how many project files each pattern matches, by line. */
    matches?: ReadonlyMap<number, number> | null;
    capped?: boolean;
  };
  let {
    sheet,
    editable,
    commit = () => Promise.resolve("unsupported"),
    remove = () => Promise.resolve("unsupported"),
    add = () => Promise.resolve("unsupported"),
    edit = () => Promise.resolve("unsupported"),
    source,
    matches = null,
    capped = false,
  }: Props = $props();

  let revealed = $state.raw<ReadonlySet<string>>(new Set());
  let folded = $state.raw<ReadonlySet<string>>(new Set());
  /** The row an "add below" field is open under (`""`: the end of the list), and its text. */
  let adding = $state<string | null>(null);
  let addText = $state("");
  let addRefusal = $state<string | null>(null);

  const ignore = $derived(sheet.kind === "ignore");
  const data = $derived(sheet.kind === "data");
  /** A key being renamed (by row path), a key being added (by object path), and what was typed. */
  let renaming = $state<string | null>(null);
  let renameText = $state("");
  let addingKey = $state<string | null>(null);
  let newKey = $state("");
  let newValue = $state("");
  let dataRefusal = $state<{ at: string; word: string } | null>(null);
  const headed = $derived(sheet.groups.length > 1 || sheet.groups.some((g) => g.title !== ""));

  function toggle(set: ReadonlySet<string>, key: string): ReadonlySet<string> {
    const next = new Set(set);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    return next;
  }

  const groupKey = (group: Group): string => `${group.path}@${group.line}`;

  /** `services.web` -> the part before the last dot, and the last part. */
  function split(title: string): [string, string] {
    const at = title.lastIndexOf(".");
    return at <= 0 || title.includes('"')
      ? ["", title]
      : [title.slice(0, at + 1), title.slice(at + 1)];
  }

  function pseudo(row: Row, text: string): Scalar {
    return {
      path: row.path,
      kind: "string",
      text,
      edit: { editable: "surgical" },
      secret: false,
    };
  }

  async function submitAdd(after: string | null): Promise<void> {
    const text = addText.trim();
    if (text === "") {
      adding = null;
      return;
    }
    addRefusal = await add(after, text);
    if (addRefusal === null) {
      adding = null;
      addText = "";
    }
  }

  function onAddKey(event: KeyboardEvent, after: string | null): void {
    if (event.isComposing) return;
    if (event.key === "Enter") {
      event.preventDefault();
      void submitAdd(after);
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      adding = null;
      addRefusal = null;
    }
  }

  function focusOnMount(node: HTMLInputElement): void {
    node.focus();
  }

  const reveal = (path: string): void => {
    revealed = toggle(revealed, path);
  };

  /** The table a card holds alone (a TOON table is its own card). */
  function tableOf(group: Group): Table | null {
    const only = group.rows.length === 1 ? group.rows[0] : undefined;
    return only?.value.type === "table" && only.path === group.path ? only.value : null;
  }

  /** An item of a list (`[2]`): it has no key of its own to rename or remove. */
  const isItem = (row: Row): boolean => /^\[\d+\]$/.test(row.key) && row.path.endsWith(row.key);

  /** Whether a card is an object that takes new keys (not a table, not a list). */
  const takesKeys = (group: Group): boolean =>
    tableOf(group) === null && !group.rows.some((r) => isItem(r));

  async function runData(at: string, change: ConfigEdit): Promise<boolean> {
    const word = await edit(change);
    dataRefusal = word === null ? null : { at, word };
    return word === null;
  }

  async function finishRename(row: Row): Promise<void> {
    renaming = null;
    const text = renameText.trim();
    if (text === "" || text === row.key) return;
    await runData(row.path, { op: "rename", path: row.path, key: text });
  }

  /** A typed value: a number, `true | false | null` as written; anything else is text. */
  function typed(text: string): NewValue {
    return /^-?(0|[1-9]\d*)(\.\d+)?([eE][+-]?\d+)?$/.test(text) ||
      ["true", "false", "null"].includes(text)
      ? { type: "literal", text }
      : { type: "text", text };
  }

  async function submitKey(): Promise<void> {
    const parent = addingKey;
    if (parent === null) return;
    const key = newKey.trim();
    if (key === "") {
      addingKey = null;
      return;
    }
    const done = await runData(`add:${parent}`, {
      op: "addKey",
      parent,
      key,
      value: typed(newValue),
    });
    if (done) addingKey = null;
  }

  function onKeyField(event: KeyboardEvent, finish: () => void, cancel: () => void): void {
    if (event.isComposing) return;
    if (event.key === "Enter") {
      event.preventDefault();
      finish();
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      cancel();
    }
  }
</script>

{#snippet addKeyForm(parent: string)}
  {#if addingKey === parent}
    <div class="add-key">
      <input
        class="field"
        placeholder={t("config.data.keyPlaceholder")}
        aria-label={t("config.data.newKey")}
        spellcheck="false"
        bind:value={newKey}
        onkeydown={(e) =>
          onKeyField(
            e,
            () => void submitKey(),
            () => (addingKey = null),
          )}
        use:focusOnMount
      />
      <input
        class="field"
        placeholder={t("config.data.valuePlaceholder")}
        aria-label={t("config.data.newValue")}
        spellcheck="false"
        bind:value={newValue}
        onkeydown={(e) =>
          onKeyField(
            e,
            () => void submitKey(),
            () => (addingKey = null),
          )}
      />
      <button type="button" class="btn btn--sm" onclick={() => void submitKey()}>
        {t("config.data.add")}
      </button>
    </div>
  {:else}
    <button
      type="button"
      class="btn btn--sm add-key-btn"
      onclick={() => {
        addingKey = parent;
        newKey = "";
        newValue = "";
        dataRefusal = null;
      }}
    >
      <Plus size={14} />
      {t("config.data.addKey")}
    </button>
  {/if}
  {#if dataRefusal?.at === `add:${parent}`}
    <p class="refusal" role="alert">{t(refusalKey(dataRefusal.word))}</p>
  {/if}
{/snippet}

{#snippet sourceButton(line: number)}
  {#if source}
    <button
      type="button"
      class="icon-btn icon-btn--sm to-source"
      title={t("config.fromSource")}
      aria-label={t("config.fromSource")}
      onclick={() => source(line)}
    >
      <CodeXml size={14} />
    </button>
  {/if}
{/snippet}

<div class="sheet" data-kind={sheet.kind} aria-label={t("config.sheet")} role="group">
  {#each sheet.groups as group (groupKey(group))}
    {@const open = !folded.has(groupKey(group))}
    {@const [parent, last] = split(group.title)}
    {@const table = tableOf(group)}
    <section class="card">
      {#if headed}
        <header>
          <button
            type="button"
            class="icon-btn icon-btn--sm fold"
            aria-expanded={open}
            aria-label={t("config.fold", {
              name: group.title === "" ? t("config.root") : group.title,
            })}
            onclick={() => (folded = toggle(folded, groupKey(group)))}
          >
            {#if open}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
          </button>
          <h3>
            {#if group.title === ""}
              <span class="last">{t("config.root")}</span>
            {:else}
              <span class="parent">{parent}</span><span class="last">{last}</span>
            {/if}
          </h3>
          {#if group.comment}<p class="caption">{group.comment}</p>{/if}
        </header>
      {/if}
      {#if open && table}
        <SheetTable {table} {editable} {revealed} {reveal} {commit} {edit} />
      {:else if open}
        <div class="rows">
          {#each group.rows as row (row.path + "@" + row.line)}
            {@const value = row.value}
            <div class="row" class:ignore>
              {#if value.type === "pattern"}
                <span class="mark" title={value.negated ? t("config.ignore.negation") : undefined}>
                  {value.negated ? "!" : ""}
                </span>
                <div class="body">
                  {#if editable}
                    <SheetValue
                      scalar={pseudo(row, value.text)}
                      label={value.text}
                      {editable}
                      revealed={false}
                      reveal={() => {}}
                      {commit}
                    />
                  {:else}
                    <code class="pattern" class:negated={value.negated}>
                      {value.negated ? value.text.slice(1) : value.text}
                    </code>
                  {/if}
                </div>
                {#if matches}
                  {@const count = matches.get(row.line)}
                  <span class="count" class:none={count === 0}>
                    {#if count === undefined}
                      …
                    {:else if count === 0}
                      {t("config.ignore.noFiles")}
                    {:else}
                      {capped ? "≥ " : ""}{tn("config.ignore.files", count)}
                    {/if}
                  </span>
                {/if}
                <span class="tools">
                  {#if editable}
                    <button
                      type="button"
                      class="icon-btn icon-btn--sm"
                      title={t("config.ignore.addBelow")}
                      aria-label={t("config.ignore.addBelow")}
                      onclick={() => {
                        adding = row.path;
                        addText = "";
                        addRefusal = null;
                      }}
                    >
                      <Plus size={14} />
                    </button>
                    <button
                      type="button"
                      class="icon-btn icon-btn--sm"
                      title={t("config.ignore.delete")}
                      aria-label={t("config.ignore.delete")}
                      onclick={() => void remove(row.path)}
                    >
                      <Trash size={14} />
                    </button>
                  {/if}
                  {@render sourceButton(row.line)}
                </span>
              {:else}
                {#if renaming === row.path}
                  <input
                    class="field key-field"
                    aria-label={t("config.data.renameKey", { key: row.key })}
                    spellcheck="false"
                    bind:value={renameText}
                    onkeydown={(e) =>
                      onKeyField(
                        e,
                        () => void finishRename(row),
                        () => (renaming = null),
                      )}
                    onblur={() => void finishRename(row)}
                    use:focusOnMount
                  />
                {:else if data && editable && row.key !== "" && !isItem(row) && value.type !== "raw"}
                  <button
                    type="button"
                    class="key key-btn"
                    title={t("config.data.renameKey", { key: row.key })}
                    onclick={() => {
                      renaming = row.path;
                      renameText = row.key;
                      dataRefusal = null;
                    }}
                  >
                    {row.key}
                  </button>
                {:else}
                  <span class="key" title={row.path}>{row.key === "" ? "—" : row.key}</span>
                {/if}
                <div class="body">
                  {#if value.type === "scalar"}
                    <SheetValue
                      scalar={value}
                      label={row.key}
                      {editable}
                      revealed={revealed.has(value.path)}
                      {reveal}
                      {commit}
                    />
                  {:else if value.type === "list"}
                    <span class="chips">
                      {#each value.items as item (item.path)}
                        <span class="chip">
                          <SheetValue
                            scalar={item}
                            label={`${row.key} ${item.path.slice(row.path.length)}`}
                            {editable}
                            revealed={revealed.has(item.path)}
                            {reveal}
                            {commit}
                          />
                        </span>
                      {/each}
                    </span>
                  {:else if value.type === "table"}
                    <SheetTable table={value} {editable} {revealed} {reveal} {commit} {edit} />
                  {:else}
                    {@const masked = value.secret && !revealed.has(row.path)}
                    <span class="rawbox">
                      <pre class="raw">{masked ? MASK : value.text}</pre>
                      {#if value.secret}
                        <button
                          type="button"
                          class="icon-btn icon-btn--sm"
                          aria-pressed={!masked}
                          aria-label={masked ? t("config.reveal") : t("config.hide")}
                          title={masked ? t("config.reveal") : t("config.hide")}
                          onclick={() => reveal(row.path)}
                        >
                          {#if masked}<Eye size={14} />{:else}<EyeOff size={14} />{/if}
                        </button>
                      {/if}
                      <span class="lock" title={t(reasonKey(value.reason))}>
                        <Lock size={12} aria-label={t(reasonKey(value.reason))} />
                      </span>
                    </span>
                  {/if}
                  {#if row.comment}<p class="note">{row.comment}</p>{/if}
                  {#if dataRefusal?.at === row.path}
                    <p class="refusal" role="alert">{t(refusalKey(dataRefusal.word))}</p>
                  {/if}
                </div>
                <span class="tools">
                  {#if data && editable && !isItem(row) && (value.type === "scalar" || value.type === "list")}
                    <button
                      type="button"
                      class="icon-btn icon-btn--sm"
                      title={t("config.data.removeKey", { key: row.key })}
                      aria-label={t("config.data.removeKey", { key: row.key })}
                      onclick={() => void runData(row.path, { op: "remove", path: row.path })}
                    >
                      <Trash size={14} />
                    </button>
                  {/if}
                  {@render sourceButton(row.line)}
                </span>
              {/if}
            </div>
            {#if adding === row.path}
              <div class="row adding">
                <span class="mark"></span>
                <div class="body">
                  <input
                    class="field"
                    placeholder={t("config.ignore.placeholder")}
                    aria-label={t("config.ignore.addBelow")}
                    spellcheck="false"
                    bind:value={addText}
                    onkeydown={(e) => onAddKey(e, row.path)}
                    use:focusOnMount
                  />
                  {#if addRefusal}<p class="refusal" role="alert">
                      {t(refusalKey(addRefusal))}
                    </p>{/if}
                </div>
              </div>
            {/if}
          {/each}
          {#if data && editable && takesKeys(group)}
            <div class="row end-row">{@render addKeyForm(group.path)}</div>
          {/if}
        </div>
      {/if}
    </section>
  {/each}
  {#if data && editable && !sheet.groups.some((g) => g.path === "")}
    <div class="end">{@render addKeyForm("")}</div>
  {/if}
  {#if ignore && editable}
    <div class="end">
      {#if adding === ""}
        <input
          class="field"
          placeholder={t("config.ignore.placeholder")}
          aria-label={t("config.ignore.add")}
          spellcheck="false"
          bind:value={addText}
          onkeydown={(e) => onAddKey(e, null)}
          use:focusOnMount
        />
        {#if addRefusal}<p class="refusal" role="alert">{t(refusalKey(addRefusal))}</p>{/if}
      {:else}
        <button
          type="button"
          class="btn btn--sm"
          onclick={() => {
            adding = "";
            addText = "";
            addRefusal = null;
          }}
        >
          <Plus size={14} />
          {t("config.ignore.add")}
        </button>
      {/if}
    </div>
  {/if}
  {#if sheet.groups.length === 0}
    <p class="blank">{t("config.nothing")}</p>
  {/if}
  {#if sheet.spec}
    <p class="spec" title={t("config.data.specHint")}>{sheet.spec}</p>
  {/if}
</div>

<style>
  .sheet {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    width: min(100%, var(--measure-wide));
    margin: 0 auto;
    padding: var(--sp-4) var(--sp-5) var(--sp-7);
    font: var(--fs-ui) / 1.5 var(--font-ui);
  }
  .card {
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--surface);
    overflow: hidden;
  }
  header {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0 var(--sp-2);
    padding: var(--sp-2) var(--sp-3);
    border-bottom: 1px solid var(--border);
  }
  header:has(+ :not(.rows)) {
    border-bottom: 0;
  }
  .fold {
    align-self: center;
  }
  h3 {
    margin: 0;
    font: 600 var(--fs-ui) / 1.4 var(--font-code);
  }
  .parent {
    color: var(--faint);
    font-weight: 400;
  }
  .caption,
  .note {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .caption {
    flex-basis: 100%;
    padding-left: 30px;
  }
  .note {
    margin-top: 2px;
  }
  .rows {
    display: flex;
    flex-direction: column;
  }
  .row {
    display: grid;
    grid-template-columns: minmax(110px, 26%) minmax(0, 1fr) auto;
    gap: var(--sp-3);
    align-items: baseline;
    padding: 6px var(--sp-3);
    border-top: 1px solid var(--border);
  }
  .row:first-child {
    border-top: 0;
  }
  .row.ignore,
  .row.adding {
    grid-template-columns: 1.4em minmax(0, 1fr) auto auto;
  }
  .key {
    min-width: 0;
    overflow-wrap: anywhere;
    color: var(--muted);
    font-family: var(--font-code);
    font-size: 0.95em;
  }
  .body {
    min-width: 0;
  }
  .tools {
    display: inline-flex;
    gap: 2px;
    align-self: center;
  }
  .to-source {
    opacity: 0;
    transition: opacity var(--dur-fast);
  }
  .row:hover .to-source,
  .row:focus-within .to-source,
  .to-source:focus-visible {
    opacity: 1;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .chip {
    display: inline-flex;
    padding: 0 8px;
    border-radius: 10px;
    background: var(--surface-2);
    font-size: var(--fs-ui-sm);
  }
  .rawbox {
    display: inline-flex;
    align-items: flex-start;
    gap: var(--sp-2);
    max-width: 100%;
  }
  .raw {
    margin: 0;
    min-width: 0;
    max-height: 12em;
    overflow: auto;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--muted);
    font: 0.92em / 1.5 var(--font-code);
  }
  .lock {
    display: inline-flex;
    padding-top: 3px;
    color: var(--faint);
    cursor: help;
  }
  .mark {
    color: var(--git-renamed-text);
    font: 700 var(--fs-ui) var(--font-code);
    text-align: center;
  }
  .pattern {
    font-family: var(--font-code);
    overflow-wrap: anywhere;
  }
  .pattern.negated {
    color: var(--git-renamed-text);
  }
  .count {
    align-self: center;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    white-space: nowrap;
  }
  .count.none {
    color: var(--faint);
  }
  .end {
    padding: 0 var(--sp-1);
  }
  .refusal {
    margin: 2px 0 0;
    color: var(--git-deleted-text);
    font-size: var(--fs-ui-sm);
  }
  .blank {
    margin: var(--sp-6) 0;
    color: var(--muted);
    text-align: center;
  }
  .spec {
    align-self: flex-end;
    margin: 0;
    color: var(--faint);
    font: var(--fs-ui-xs) / 1.4 var(--font-code);
  }
  .key-btn {
    justify-self: start;
    padding: 0;
    border: 0;
    background: none;
    text-align: left;
    cursor: text;
    border-radius: 3px;
  }
  .key-btn:hover {
    background: var(--hover);
  }
  .key-btn:focus-visible {
    outline: 2px solid var(--accent);
  }
  .key-field {
    min-width: 0;
    font-family: var(--font-code);
  }
  .end-row {
    grid-template-columns: minmax(0, 1fr);
  }
  .add-key {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2);
    align-items: center;
  }
  .add-key .field {
    flex: 1 1 10em;
    min-width: 0;
    font-family: var(--font-code);
  }
  .add-key-btn {
    justify-self: start;
  }
  @media (max-width: 560px) {
    .row {
      grid-template-columns: minmax(0, 1fr) auto;
    }
    .key {
      grid-column: 1 / -1;
    }
  }
</style>
