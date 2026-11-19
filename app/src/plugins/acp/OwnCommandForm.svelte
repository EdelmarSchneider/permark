<script lang="ts">
  // The person's own command, in the picker and on the settings page alike: the field, the line that PerMark starts
  // this program, a line that follows the typing with what it would start (the program's full path | why not, asked a
  // moment after the keys stop: a run of keys asks once; an empty field says nothing), Use this command, Cancel (on the
  // page, where it only puts the command in use back: there only while something differs). Why a command was refused
  // and why it must wait (the agent works) are said here, beside the button they concern.
  import { onMount, untrack } from "svelte";
  import { t } from "../../core/i18n";
  import Note from "./Note.svelte";
  import { acp, previewText } from "./store.svelte";

  type Props = {
    /** The text the field starts with and gets back on Cancel (the own command in use, else empty). */
    initial: string;
    /** The field takes the keyboard as the form shows. */
    focus?: boolean;
    /** The field's id; `labelled`: a label outside the form names it (a settings row). */
    fieldId?: string;
    labelled?: boolean;
    /** Cancel ends the form (the picker); none: Cancel only puts the text in use back. */
    oncancel?: () => void;
    /** The command was kept and the form ends (the picker moves the keyboard on); none: the form stays and says so. */
    onsaved?: () => void;
  };
  let { initial, focus = false, fieldId, labelled = false, oncancel, onsaved }: Props = $props();

  /** The pause after the last key before the line asks what the text would start (ms). */
  const PREVIEW_MS = 250;

  const uid = $props.id();
  const id = $derived(fieldId ?? `${uid}-command`);
  let typed = $state(untrack(() => initial));
  /** What the typed text would start, in words (`null`: nothing to say). */
  let preview = $state<string | null>(null);
  /** The text was just kept (said on the same line until the next key). */
  let saved = $state(false);
  let field = $state<HTMLInputElement>();
  let timer: ReturnType<typeof setTimeout> | undefined;
  /** Counts the asks: an answer to an older text is dropped. */
  let asked = 0;

  const locked = $derived(!acp.canChange || acp.choosing);
  /** Cancel has something to undo: it always ends the picker's form; on the page it puts the command in use back. */
  const cancellable = $derived(
    oncancel !== undefined || typed !== initial || acp.choiceRefusal !== null,
  );

  function ask(): void {
    clearTimeout(timer);
    const seq = ++asked;
    saved = false;
    const command = typed;
    if (command.trim() === "") {
      preview = null;
      return;
    }
    timer = setTimeout(() => {
      void acp.resolve(command).then((resolved) => {
        if (seq === asked) preview = resolved === null ? null : previewText(resolved);
      });
    }, PREVIEW_MS);
  }

  onMount(() => {
    if (focus) field?.focus();
    ask();
    return () => {
      clearTimeout(timer);
      asked++;
    };
  });

  /** The focused button was disabled while the call ran (the page itself took the keyboard). */
  function lost(): boolean {
    return document.activeElement === null || document.activeElement === document.body;
  }

  async function save(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (await acp.choose({ command: typed })) {
      if (onsaved !== undefined) {
        onsaved();
        return;
      }
      clearTimeout(timer);
      asked++;
      saved = true;
    }
    if (lost()) field?.focus();
  }

  function cancel(): void {
    typed = initial;
    ask();
    acp.choiceRefusal = null;
    if (oncancel !== undefined) oncancel();
    // the button turns disabled under the keyboard: the field takes it
    else field?.focus();
  }
</script>

<form class="own" onsubmit={(event) => void save(event)}>
  {#if !labelled}
    <label for={id}>{t("acp.pick.command")}</label>
  {/if}
  <input
    bind:this={field}
    bind:value={typed}
    {id}
    class="field-input field"
    spellcheck="false"
    autocomplete="off"
    placeholder={t("acp.pick.commandPlaceholder")}
    aria-describedby="{uid}-warning {uid}-preview"
    oninput={ask}
  />
  <Note tone="warning" id="{uid}-warning">{t("acp.pick.commandWarning")}</Note>
  <!-- always present: a live line must exist before its words change to be heard -->
  <Note tone="muted" live id="{uid}-preview">{saved ? t("acp.page.saved") : (preview ?? "")}</Note>
  {#if !acp.canChange}
    <Note>{t("acp.error.busy")}</Note>
  {/if}
  {#if acp.choiceRefusal !== null}
    <Note tone="refusal" alert>{acp.choiceRefusal}</Note>
  {/if}
  <div class="actions">
    <button type="button" class="btn btn--sm" disabled={!cancellable} onclick={cancel}
      >{t("common.cancel")}</button
    >
    <button type="submit" class="btn btn--sm btn--primary" disabled={typed.trim() === "" || locked}
      >{t("acp.pick.useCommand")}</button
    >
  </div>
</form>

<style>
  .own {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    min-width: 0;
  }
  label {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
  /* the settings' text field, in the code face: a command is read character by character */
  .field {
    width: 100%;
    box-sizing: border-box;
    font-family: var(--font-code);
    font-size: var(--fs-ui-sm);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--sp-1);
  }
</style>
