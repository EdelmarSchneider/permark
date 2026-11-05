<script lang="ts">
  // "This computer": its memory (free too when the system tells), processor threads, graphics cards with their memory
  // (own | shared with the computer | unknown). Software renderers are not graphics cards: left out. Anything the
  // system does not tell is said in words, never a blank.
  import type { GpuAdapter } from "../../api/generated/GpuAdapter";
  import { t, tn } from "../../core/i18n";
  import Card from "../../core/shell/settings/Card.svelte";
  import SettingRow from "../../core/shell/settings/SettingRow.svelte";
  import { localAi } from "./store.svelte";
  import { memoryText } from "./words";

  const id = $props.id();
  const profile = $derived(localAi.machine?.profile ?? null);
  const gpus = $derived((profile?.gpus ?? []).filter((gpu) => !gpu.software));

  function memoryLine(total: number | null, free: number | null): string {
    if (total === null) return t("localAi.machine.unknown");
    const size = memoryText(total);
    return free === null ? size : t("localAi.machine.memoryFree", { size, free: memoryText(free) });
  }

  function gpuMemory(gpu: GpuAdapter): string {
    if (gpu.unified) return t("localAi.machine.gpuUnified");
    if (gpu.dedicatedVram === null) return t("localAi.machine.gpuUnknown");
    return t("localAi.machine.gpuOwn", { size: memoryText(gpu.dedicatedVram) });
  }

  // read once per page open
  localAi.needMachine();
</script>

<section class="machine" aria-labelledby={`${id}-title`}>
  <h3 id={`${id}-title`}>{t("localAi.machine.title")}</h3>
  <Card>
    {#if profile === null}
      <p class="quiet">{t("app.loading")}</p>
    {:else}
      <SettingRow label={t("localAi.machine.memory")} labelId={`${id}-memory`}>
        <span class="value">
          {memoryLine(profile.ramTotal, profile.ramAvailable)}
        </span>
      </SettingRow>
      <SettingRow label={t("localAi.machine.processor")} labelId={`${id}-cpu`}>
        <span class="value">
          {profile.cpuLogical === null
            ? t("localAi.machine.unknown")
            : tn("localAi.machine.threads", profile.cpuLogical)}
        </span>
      </SettingRow>
      <SettingRow label={t("localAi.machine.graphics")} labelId={`${id}-gpu`} wide>
        {#if gpus.length === 0}
          <span class="value">{t("localAi.machine.noGpu")}</span>
        {:else}
          <ul class="gpus" aria-labelledby={`${id}-gpu`}>
            {#each gpus as gpu, i (i)}
              <li>
                <span class="gpu-name">{gpu.name}</span>
                <span class="quiet">{gpuMemory(gpu)}</span>
              </li>
            {/each}
          </ul>
        {/if}
      </SettingRow>
    {/if}
  </Card>
</section>

<style>
  .machine {
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
  .value {
    font-size: var(--fs-ui-sm);
    font-variant-numeric: tabular-nums;
    overflow-wrap: anywhere;
  }
  .quiet {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  p.quiet {
    padding: var(--sp-3) var(--sp-4);
  }
  .gpus {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: var(--fs-ui-sm);
  }
  .gpus li {
    display: flex;
    flex-wrap: wrap;
    gap: 0 var(--sp-2);
    overflow-wrap: anywhere;
  }
</style>
