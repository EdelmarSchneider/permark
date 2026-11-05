<script lang="ts">
  // "Hugging Face token" (shown only while a listed model needs one): whether a token is saved, a masked field to save
  // one, Clear. The value never comes back from the backend: after Save the field empties and the card says "saved".
  import { t } from "../../core/i18n";
  import Card from "../../core/shell/settings/Card.svelte";
  import SettingRow from "../../core/shell/settings/SettingRow.svelte";
  import { localAi } from "./store.svelte";
  import { errorText } from "./words";

  const id = $props.id();
  let value = $state("");
  let failure = $state<string | null>(null);
  const saved = $derived(localAi.state?.tokenSet ?? false);

  async function save(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    const result = await localAi.setToken(value);
    if (result.ok) value = "";
    failure = result.ok ? null : errorText(result.error);
  }

  async function clear(): Promise<void> {
    const result = await localAi.clearToken();
    failure = result.ok ? null : errorText(result.error);
  }
</script>

<section class="token" aria-labelledby={`${id}-title`}>
  <h3 id={`${id}-title`}>{t("localAi.token.title")}</h3>
  <Card>
    <SettingRow
      label={t("localAi.token.label")}
      hint={t("localAi.token.about")}
      labelFor={`${id}-input`}
      wide
    >
      <form class="form" onsubmit={(e) => void save(e)}>
        <p class="status" data-token-status>
          {saved ? t("localAi.token.saved") : t("localAi.token.none")}
        </p>
        <div class="fields">
          <input
            id={`${id}-input`}
            class="field-input"
            type="password"
            autocomplete="off"
            spellcheck="false"
            bind:value
          />
          <button type="submit" class="btn btn--sm" data-action="saveToken"
            >{t("localAi.token.save")}</button
          >
          {#if saved}
            <button
              type="button"
              class="btn btn--sm btn--ghost"
              data-action="clearToken"
              aria-label={t("localAi.token.clearLabel")}
              onclick={() => void clear()}>{t("localAi.token.clear")}</button
            >
          {/if}
        </div>
        {#if failure !== null}
          <p class="problem" role="alert">{failure}</p>
        {/if}
      </form>
    </SettingRow>
  </Card>
</section>

<style>
  .token {
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
  p {
    margin: 0;
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    font-size: var(--fs-ui-sm);
  }
  .status {
    color: var(--muted);
  }
  .fields {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2);
  }
  .fields input {
    flex: 1 1 14rem;
    min-width: 0;
    max-width: 24rem;
  }
  .problem {
    color: var(--git-deleted-text);
    overflow-wrap: anywhere;
  }
</style>
