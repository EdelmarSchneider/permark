<script lang="ts">
  // "What each job uses": one select per job, offering the models on this computer that can do it and run here (or Not
  // assigned). No such model: the select is off and the row says so ("coming soon" when no model of the catalog runs
  // here for it). An assigned model the AI engine cannot run stays chosen (never changed silently), named as not
  // usable in its option + the row hint. Assigned models needing more memory than this computer has for them: a
  // warning in words (icon beside, never color alone).
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import { sizeText, t } from "../../core/i18n";
  import Card from "../../core/shell/settings/Card.svelte";
  import SettingRow from "../../core/shell/settings/SettingRow.svelte";
  import { localAi } from "./store.svelte";
  import { TASKS, type AiTask } from "./tasks";
  import { errorText, taskHintKey, taskKey } from "./words";

  const id = $props.id();
  const rows = $derived(
    TASKS.map((task) => {
      const model = localAi.assigned(task);
      const unusable =
        model !== null && localAi.engineNeedOf(model) !== null ? localAi.modelOf(model) : null;
      return {
        task,
        choices: localAi.choicesFor(task),
        model,
        unusable,
        soon: !localAi.offersFor(task),
      };
    }),
  );

  function hintOf(row: (typeof rows)[number]): string {
    if (row.unusable !== null) return t("localAi.tasks.notUsable", { name: row.unusable.name });
    if (row.choices.length > 0) return t(taskHintKey(row.task));
    return t(row.soon ? "localAi.tasks.comingSoon" : "localAi.tasks.noneAvailable");
  }
  /** The first job that can be changed takes the keyboard when the page is opened from the palette. */
  const first = $derived(rows.find((row) => row.choices.length > 0)?.task ?? null);
  const budget = $derived(localAi.machine?.budget ?? null);
  let failure = $state<string | null>(null);

  // the budget warning needs this computer: asked once the card shows
  localAi.needMachine();

  async function choose(task: AiTask, select: HTMLSelectElement): Promise<void> {
    const result = await localAi.assign(task, select.value === "" ? null : select.value);
    failure = result.ok ? null : errorText(result.error);
    // refused: the select shows what is still assigned
    if (!result.ok) select.value = localAi.assigned(task) ?? "";
  }
</script>

<section class="jobs" aria-labelledby={`${id}-title`}>
  <h3 id={`${id}-title`}>{t("localAi.tasks.title")}</h3>
  <Card>
    {#each rows as row (row.task)}
      {@const none = row.choices.length === 0 && row.unusable === null}
      <!-- an unusable choice keeps the select on: the person can still unassign it -->
      <SettingRow label={t(taskKey(row.task))} hint={hintOf(row)} labelFor={`${id}-${row.task}`}>
        <select
          id={`${id}-${row.task}`}
          class="field-select"
          data-task={row.task}
          data-autofocus={row.task === first ? "" : undefined}
          disabled={none}
          value={row.model ?? ""}
          onchange={(e) => void choose(row.task, e.currentTarget)}
        >
          <option value="">{t("localAi.noModelOption")}</option>
          {#if row.unusable !== null}
            <option value={row.unusable.id}
              >{t("localAi.tasks.notUsableOption", { name: row.unusable.name })}</option
            >
          {/if}
          {#each row.choices as model (model.id)}
            <option value={model.id}>{model.name}</option>
          {/each}
        </select>
      </SettingRow>
    {/each}
  </Card>
  {#if budget !== null}
    <p class="budget" role="status">
      <TriangleAlert size={14} aria-hidden="true" />
      <span>
        {t("localAi.tasks.budget", {
          needed: sizeText(budget.needed),
          available: sizeText(budget.available),
        })}
      </span>
    </p>
  {/if}
  {#if failure !== null}
    <p class="failed" role="alert">{failure}</p>
  {/if}
</section>

<style>
  .jobs {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  h3 {
    margin: 0;
    padding-left: var(--sp-1);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
  select {
    max-width: 18rem;
  }
  .budget {
    display: flex;
    align-items: flex-start;
    gap: var(--sp-2);
    margin: 0;
    color: var(--git-modified-text);
    font-size: var(--fs-ui-sm);
  }
  .budget :global(svg) {
    flex: none;
    margin-top: 2px;
  }
  .failed {
    margin: 0;
    color: var(--git-deleted-text);
    font-size: var(--fs-ui-sm);
  }
</style>
