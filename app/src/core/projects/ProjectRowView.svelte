<script lang="ts">
  // One row of Settings > Projects: include checkbox, name + path, what plugins say about the folder (unread), the
  // agent line (while the folder shares with an agent), and the Rules of an included project (each kept at once).
  // A project not found: where it was + Locate… | Remove from list. Consent mode: only the checkbox.
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import type { ProjectRow } from "../../api";
  import { api } from "../../api";
  import { conditions } from "../conditions/context.svelte";
  import { t } from "../i18n";
  import { contributions } from "../plugins/instance.svelte";
  import type { TreeBadge } from "../plugins/registry.svelte";
  import Badge from "../shell/Badge.svelte";
  import Segments from "../shell/settings/Segments.svelte";
  import { silence } from "../silence/silence.svelte";
  import CardFields from "./CardFields.svelte";
  import { projects } from "./instance.svelte";
  import { middleCut } from "./model";

  type Props = {
    row: ProjectRow;
    depth: number;
    /** `choose`: consent mode (checkbox only, nothing saved until the page's button). */
    mode: "normal" | "choose";
    expanded: boolean;
    toggle: () => void;
    rootName: string;
  };
  let { row, depth, mode, expanded, toggle, rootName }: Props = $props();

  const id = $derived(`prow-${row.key.replace(/[^a-z0-9]/gi, "_")}`);
  const agentOn = $derived(conditions.current.agent.status !== "off");
  const quietOn = $derived(conditions.current.plugins.includes("quiet"));
  const locked = $derived(projects.readOnly);
  const included = $derived(row.state === "included");
  const missing = $derived(row.state === "missing");
  const checked = $derived(
    mode === "choose" ? (projects.choosing?.has(row.path) ?? false) : included,
  );
  /** What plugins say about the folder (unread count, changed files), included projects only. */
  const said = $derived.by((): TreeBadge[] => {
    if (!included || mode === "choose") return [];
    return contributions.visible("treeBadge", conditions.current).flatMap((b) => {
      const badge = b.badge(row.path, true);
      return badge === null || badge.text === "" ? [] : [badge];
    });
  });

  let refusal = $state<string | null>(null);
  let busy = $state(false);

  async function act(run: () => Promise<void>): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      await run();
    } finally {
      busy = false;
    }
  }

  function onCheck(on: boolean): void {
    if (mode === "choose") projects.setChosen(row.path, on);
    else void act(() => projects.setIncluded(row, on));
  }

  async function locate(): Promise<void> {
    refusal = await projects.locate(row, t("projects.row.locate"));
  }

  async function setQuiet(value: "follow" | "quiet"): Promise<void> {
    const result = await api.pluginCall("quiet.set", {
      paths: [row.path],
      interest: value === "quiet" ? "quiet" : "speaking",
    });
    if (result.ok) silence.set(result.value.marks);
  }
</script>

<div class="prow" class:is-open={expanded} style:--depth={depth} data-project={row.path}>
  <div class="head">
    {#if missing}
      <span class="box-space" aria-hidden="true"></span>
    {:else}
      <input
        type="checkbox"
        aria-label={t("projects.row.include", { name: row.name })}
        {checked}
        disabled={(locked && mode === "normal") || busy}
        title={locked && mode === "normal" ? t("projects.record.locked") : undefined}
        onchange={(e) => onCheck(e.currentTarget.checked)}
      />
    {/if}
    <div class="who">
      <span class="name" title={row.name}>{row.name}</span>
      <span class="path" title={row.path}>
        {#if missing}
          {row.missing === "ignored"
            ? t("projects.row.ignored")
            : t("projects.row.wasAt", { path: middleCut(row.path) })}
        {:else}
          {middleCut(row.path)}
        {/if}
      </span>
    </div>
    <div class="status">
      {#each said as badge, n (n)}
        <Badge {badge} />
      {/each}
      {#if included && agentOn && mode === "normal"}
        <span class="agent"
          >{t(row.agent === "off" ? "projects.row.agentOff" : "projects.row.agentSees")}</span
        >
      {/if}
    </div>
    {#if missing && mode === "normal"}
      <div class="actions">
        {#if row.missing !== "ignored"}
          <button
            type="button"
            class="btn btn--sm"
            disabled={locked}
            title={locked ? t("projects.record.locked") : undefined}
            onclick={() => void act(locate)}>{t("projects.row.locate")}</button
          >
        {/if}
        <button
          type="button"
          class="btn btn--sm"
          disabled={locked}
          title={locked ? t("projects.record.locked") : undefined}
          onclick={() => void act(() => projects.remove(row))}>{t("projects.row.remove")}</button
        >
      </div>
    {:else if included && mode === "normal"}
      <button
        type="button"
        class="btn btn--ghost btn--sm rules"
        aria-expanded={expanded}
        aria-controls="{id}-rules"
        onclick={toggle}
      >
        {#if expanded}<ChevronDown size={13} aria-hidden="true" />{:else}<ChevronRight
            size={13}
            aria-hidden="true"
          />{/if}
        {t("projects.row.rules")}
      </button>
    {/if}
  </div>
  {#if refusal !== null}
    <p class="refusal" role="alert">{refusal}</p>
  {/if}
  {#if included && mode === "normal" && expanded}
    <div class="rules-body" id="{id}-rules">
      <div class="rule">
        <span class="rule-name" id="{id}-hidden">{t("projects.rules.hidden")}</span>
        <div class="rule-control">
          <Segments
            labelledby="{id}-hidden"
            value={row.hidden}
            disabled={locked}
            options={[
              { value: "own", label: t("projects.rules.hiddenOwn") },
              {
                value: "own+root",
                label: t("projects.rules.hiddenOwnRoot", { root: rootName }),
              },
            ]}
            onchange={(rule) => void act(() => projects.setHidden(row, rule))}
          />
          <span class="hint">{t("projects.rules.hiddenHint", { root: rootName })}</span>
          {#if row.hiddenUnknown}
            <span class="hint warn">{t("projects.record.hiddenUnknown", { root: rootName })}</span>
          {/if}
        </div>
      </div>
      <div class="rule">
        <span class="rule-name" id="{id}-agent">{t("projects.rules.agent")}</span>
        <div class="rule-control">
          <Segments
            labelledby="{id}-agent"
            value={row.agent}
            options={[
              { value: "same", label: t("projects.rules.agentSame", { root: rootName }) },
              { value: "off", label: t("projects.rules.agentOff") },
            ]}
            onchange={(choice) => void act(() => projects.setAgent(row, choice))}
          />
          {#if !agentOn}
            <span class="hint">{t("projects.rules.agentRootOff", { root: rootName })}</span>
          {/if}
        </div>
      </div>
      {#if quietOn}
        <div class="rule">
          <span class="rule-name" id="{id}-notify">{t("projects.rules.notify")}</span>
          <div class="rule-control">
            <Segments
              labelledby="{id}-notify"
              value={silence.isSilenced(row.path) ? "quiet" : "follow"}
              options={[
                { value: "follow", label: t("projects.rules.follow") },
                { value: "quiet", label: t("projects.rules.quiet") },
              ]}
              onchange={(value) => void act(() => setQuiet(value))}
            />
          </div>
        </div>
      {/if}
      <div class="rule card-rule">
        <span class="rule-name">{t("projects.card.title")}</span>
        <div class="rule-control">
          <CardFields
            {id}
            target={{ kind: "member", uid: row.uid ?? "" }}
            place={row.cardPlace ?? "profile"}
            values={{
              name: row.name,
              short: row.short,
              description: row.description ?? "",
              category: row.category ?? "",
            }}
          />
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .prow {
    padding: var(--sp-2) var(--sp-4) var(--sp-2) calc(var(--sp-4) + var(--depth) * 18px);
    /* long lists: rows off screen are not laid out | painted */
    content-visibility: auto;
    contain-intrinsic-size: auto 48px;
  }
  .prow.is-open {
    content-visibility: visible;
  }
  .head {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--sp-2) var(--sp-3);
    min-height: 32px;
  }
  input[type="checkbox"] {
    flex: none;
    width: 16px;
    height: 16px;
    margin: 0;
    accent-color: var(--accent);
  }
  .box-space {
    flex: none;
    width: 16px;
  }
  .who {
    display: flex;
    flex: 1 1 200px;
    flex-direction: column;
    min-width: 0;
  }
  .name,
  .path {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name {
    font-size: var(--fs-ui);
    font-weight: 500;
  }
  .path {
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .status {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .status:empty {
    display: none;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2);
  }
  .rules {
    flex: none;
  }
  .refusal {
    margin: var(--sp-1) 0 0 calc(16px + var(--sp-3));
    color: var(--git-deleted-text);
    font-size: var(--fs-ui-sm);
  }
  .rules-body {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    margin: var(--sp-2) 0 var(--sp-1) calc(16px + var(--sp-3));
    padding: var(--sp-3);
    border-radius: var(--radius-control);
    background: var(--surface);
  }
  .rule {
    display: grid;
    grid-template-columns: 120px minmax(0, 1fr);
    align-items: start;
    gap: var(--sp-3);
  }
  .rule-name {
    padding-top: 4px;
    font-size: var(--fs-ui-sm);
    font-weight: 500;
  }
  .rule-control {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--sp-1);
    min-width: 0;
  }
  .card-rule .rule-control {
    align-items: stretch;
  }
  .hint {
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .hint.warn {
    color: var(--git-modified-text);
  }
  @media (max-width: 640px) {
    .rule {
      grid-template-columns: minmax(0, 1fr);
      gap: var(--sp-1);
    }
  }
</style>
