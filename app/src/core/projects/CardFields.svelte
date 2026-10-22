<script lang="ts">
  // The card of a project (or of the opened folder): name, short name, description, category. Enter | leaving a field
  // saves it at once; an empty field goes back to the default (the folder name). Where it is saved shows above the
  // fields, before any typing (S2-1), and each field points to it. Category: the opened folder's official categories
  // are offered as the person types (any other name is kept as it is); where official lists exist, its mark says
  // official | local | unlisted, and a local | unlisted one can be made official; the official names are also said under
  // the field (the browser's suggestion arrow shows only on hover | focus). When the mark changes while the focus is on
  // "Make official…" (Tab out of the field saves it official), focus goes to the status words, never to the page.
  import { tick, untrack } from "svelte";
  import type { CardField, CardPlace, CardTarget, TagStatus } from "../../api";
  import { t, type MessageKey } from "../i18n";
  import { projects } from "./instance.svelte";
  import PromoteAsk from "./PromoteAsk.svelte";
  import StatusMark from "./StatusMark.svelte";
  import { officialOf, statusWords } from "./vocabulary";

  type Values = Record<CardField, string>;
  type Props = {
    target: CardTarget;
    values: Values;
    place: CardPlace;
    /** Unique per card on the page (ids of the fields). */
    id: string;
    /** The saved category against the official lists (`null`: none | dormant). */
    categoryStatus?: TagStatus | null;
    /** The project's short name (a local category is its own). */
    short?: string | null;
  };
  let { target, values, place, id, categoryStatus = null, short = null }: Props = $props();

  const SAID_NAMES = 8;
  /** Official categories offered while typing (none: no list attached). */
  const offered = $derived(officialOf(projects.view, "category"));
  const category = $derived(values.category.trim());
  /** The first official categories, said under the field (the rest come as the person types). */
  const offeredNames = $derived(
    offered.length <= SAID_NAMES
      ? offered.map((e) => e.name).join(", ")
      : `${offered
          .slice(0, SAID_NAMES)
          .map((e) => e.name)
          .join(", ")}, …`,
  );

  const FIELDS: readonly { field: CardField; label: MessageKey; max: number }[] = [
    { field: "name", label: "projects.card.name", max: 80 },
    { field: "short", label: "projects.card.short", max: 24 },
    { field: "description", label: "projects.card.description", max: 160 },
    { field: "category", label: "projects.card.category", max: 40 },
  ];

  let statusLine: HTMLElement | undefined = $state();
  let categoryInput: HTMLInputElement | undefined = $state();

  // the status (or the category) is about to change: focus inside the status line would go with the button that holds
  // it (Make official…) -> the status words take it (else the field)
  $effect.pre(() => {
    void categoryStatus;
    void category;
    untrack(() => {
      const line = statusLine;
      const active = document.activeElement;
      if (!line || !(active instanceof Node) || !line.contains(active) || active === line) return;
      void tick().then(() => {
        const now = document.activeElement;
        if (now !== null && now !== document.body && now.isConnected) return;
        (statusLine ?? categoryInput)?.focus();
      });
    });
  });

  /** What each field held when it was last saved | shown (a save only when the person changed it). */
  let saving = $state<CardField | null>(null);

  /** `next`: where the focus was going when the field was left (Tab | a click). */
  async function save(
    field: CardField,
    input: HTMLInputElement,
    next: EventTarget | null = null,
  ): Promise<void> {
    const value = input.value.trim();
    if (value === values[field].trim() || saving !== null) return;
    saving = field;
    const card = await projects.setCard(target, { field, value });
    saving = null;
    // refused: the field shows what is saved
    if (card === null) input.value = values[field];
    if (field === "category" && next instanceof Node) void keepFocus(next);
  }

  /** Tab out of the category went to "Make official…", which the save took away (now official) -> the status words
   *  take the focus, never the page. The browser moves the focus after the blur: checked once that is done. */
  async function keepFocus(next: Node): Promise<void> {
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
    const now = document.activeElement;
    if (next.isConnected || (now !== null && now !== document.body && now.isConnected)) return;
    (statusLine ?? categoryInput)?.focus();
  }

  function onkeydown(e: KeyboardEvent, field: CardField): void {
    if (e.isComposing || e.keyCode === 229) return;
    const input = e.currentTarget as HTMLInputElement;
    if (e.key === "Enter") {
      e.preventDefault();
      void save(field, input);
    } else if (e.key === "Escape" && input.value !== values[field]) {
      // Esc undoes the typing first (the page closes on the next one)
      e.preventDefault();
      e.stopPropagation();
      input.value = values[field];
    }
  }
</script>

<div class="fields">
  <p class="place" id="{id}-place">
    {t(place === "project" ? "projects.card.savedProject" : "projects.card.savedProfile")}
  </p>
  {#each FIELDS as f (f.field)}
    <label class="field" for="{id}-{f.field}">{t(f.label)}</label>
    <input
      id="{id}-{f.field}"
      class="field-input"
      type="text"
      maxlength={f.max}
      autocomplete="off"
      spellcheck="false"
      aria-describedby={f.field === "category" && offered.length > 0
        ? `${id}-place ${id}-offered`
        : `${id}-place`}
      list={f.field === "category" && offered.length > 0 ? `${id}-categories` : undefined}
      value={values[f.field]}
      onkeydown={(e) => onkeydown(e, f.field)}
      onblur={(e) => void save(f.field, e.currentTarget, e.relatedTarget)}
      {@attach (el) => {
        if (f.field === "category") categoryInput = el;
      }}
    />
    {#if f.field === "category" && offered.length > 0}
      <p class="offered" id="{id}-offered">
        {t("projects.category.offered", { names: offeredNames })}
      </p>
    {/if}
    {#if f.field === "category" && category !== "" && categoryStatus !== null}
      <div class="category-status" tabindex="-1" bind:this={statusLine}>
        <span class="status"
          ><StatusMark
            status={categoryStatus}
            words={statusWords(categoryStatus, "category", short)}
            size={13}
            silent
          /><span>{statusWords(categoryStatus, "category", short)}</span></span
        >
        {#if categoryStatus !== "official"}
          <PromoteAsk kind="category" name={category} done={() => statusLine?.focus()} />
        {/if}
      </div>
    {/if}
  {/each}
  {#if offered.length > 0}
    <datalist id="{id}-categories">
      {#each offered as entry (entry.name)}<option value={entry.name}></option>{/each}
    </datalist>
  {/if}
</div>

<style>
  .fields {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    align-items: center;
    gap: var(--sp-2) var(--sp-3);
  }
  .field {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .field-input {
    min-width: 0;
  }
  .offered {
    grid-column: 2;
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    overflow-wrap: anywhere;
  }
  .category-status:focus {
    outline: none;
  }
  .category-status:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
    border-radius: var(--radius-control);
  }
  .category-status {
    grid-column: 2;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2);
    min-width: 0;
  }
  .status {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .place {
    grid-column: 1 / -1;
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  @media (max-width: 640px) {
    .fields {
      grid-template-columns: minmax(0, 1fr);
    }
    .category-status,
    .offered {
      grid-column: 1;
    }
  }
</style>
