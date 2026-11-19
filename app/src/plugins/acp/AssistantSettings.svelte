<script lang="ts">
  // Settings > Assistant (while a folder is open: the plugin's calls answer for a project): the agent in use (its name,
  // or "Your command", and the command that runs) with Change…, which leaves the settings for the panel's picker; the
  // person's own command, with the line of what it would start as they type; what is sent and what it may cost.
  // Inside a Snap one line says nothing can start, instead of the form. The choice is the person's, kept in their
  // profile for every folder: never in the folder's shared file.
  import { onMount, tick } from "svelte";
  import { t } from "../../core/i18n";
  import Card from "../../core/shell/settings/Card.svelte";
  import SettingRow from "../../core/shell/settings/SettingRow.svelte";
  import { ui } from "../../core/shell/ui.svelte";
  import OwnCommandForm from "./OwnCommandForm.svelte";
  import { acp } from "./store.svelte";

  const uid = $props.id();
  const fieldId = `${uid}-command`;
  const status = $derived(acp.status);
  const snap = $derived(status?.needs === "snap");
  const name = $derived.by(() => {
    if (status === null || !status.chosen) return t("acp.page.none");
    return status.agentName ?? t("acp.ownCommand");
  });
  /** The own command in use (the form's first text); read once the list is: a failed read starts empty. */
  const own = $derived(
    acp.catalog.status === "ready"
      ? (acp.catalog.agents.find((a) => a.how === "custom")?.command ?? "")
      : null,
  );
  const listed = $derived(acp.catalog.status === "ready" || acp.catalog.status === "failed");

  // the list is read again as the page shows: an own command kept from the panel meanwhile counts
  onMount(() => {
    if (!snap) void acp.loadAgents();
  });

  /** The panel's picker, over the settings that close first (they cover the panel and give the keyboard back). */
  async function change(): Promise<void> {
    ui.closeSettingsPage();
    await tick();
    acp.openPicker();
  }
</script>

<div class="assistant">
  {#if status === null}
    <p class="quiet">{t("app.loading")}</p>
  {:else}
    <Card>
      <SettingRow label={t("acp.page.inUse")} labelId="{uid}-in-use" scope="you">
        <span class="name">{name}</span>
        {#if !snap}
          <button
            type="button"
            class="btn btn--sm"
            disabled={!acp.canChange}
            onclick={() => void change()}>{t("acp.page.change")}</button
          >
        {/if}
        {#snippet note()}
          {#if status.agent !== ""}
            <code class="command" title={status.agent}>{status.agent}</code>
          {/if}
        {/snippet}
      </SettingRow>
      {#if snap}
        <p class="about">{t("acp.snap")}</p>
      {:else}
        <SettingRow label={t("acp.pick.command")} labelFor={fieldId} wide>
          {#if listed}
            <div class="form">
              <OwnCommandForm initial={own ?? ""} {fieldId} labelled />
            </div>
          {:else}
            <p class="quiet">{t("acp.pick.loading")}</p>
          {/if}
        </SettingRow>
      {/if}
      <p class="about">{t("acp.pick.intro")}</p>
    </Card>
  {/if}
</div>

<style>
  .assistant {
    display: flex;
    flex-direction: column;
    gap: var(--sp-5);
  }
  .name {
    font-size: var(--fs-ui);
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  /* the command as it will run: one line, the whole of it on hover */
  .command {
    display: block;
    overflow: hidden;
    font-family: var(--font-code);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .form {
    flex: 1;
    min-width: 0;
  }
  .about {
    margin: 0;
    padding: var(--sp-3) var(--sp-4);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    overflow-wrap: anywhere;
  }
  .quiet {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
</style>
