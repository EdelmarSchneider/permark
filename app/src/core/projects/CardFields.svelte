<script lang="ts">
  // The card of a project (or of the opened folder): name, short name, description, category. Enter | leaving a field
  // saves it at once; an empty field goes back to the default (the folder name). Where it is saved shows above the
  // fields, before any typing (S2-1), and each field points to it.
  import type { CardField, CardPlace, CardTarget } from "../../api";
  import { t, type MessageKey } from "../i18n";
  import { projects } from "./instance.svelte";

  type Values = Record<CardField, string>;
  type Props = {
    target: CardTarget;
    values: Values;
    place: CardPlace;
    /** Unique per card on the page (ids of the fields). */
    id: string;
  };
  let { target, values, place, id }: Props = $props();

  const FIELDS: readonly { field: CardField; label: MessageKey; max: number }[] = [
    { field: "name", label: "projects.card.name", max: 80 },
    { field: "short", label: "projects.card.short", max: 24 },
    { field: "description", label: "projects.card.description", max: 160 },
    { field: "category", label: "projects.card.category", max: 40 },
  ];

  /** What each field held when it was last saved | shown (a save only when the person changed it). */
  let saving = $state<CardField | null>(null);

  async function save(field: CardField, input: HTMLInputElement): Promise<void> {
    const value = input.value.trim();
    if (value === values[field].trim() || saving !== null) return;
    saving = field;
    const card = await projects.setCard(target, { field, value });
    saving = null;
    // refused: the field shows what is saved
    if (card === null) input.value = values[field];
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
      aria-describedby="{id}-place"
      value={values[f.field]}
      onkeydown={(e) => onkeydown(e, f.field)}
      onblur={(e) => void save(f.field, e.currentTarget)}
    />
  {/each}
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
  }
</style>
