<script lang="ts">
  // The view "Assistant": the conversation with the project's agent (the person's messages, the agent's reply as
  // plain text while it comes, "Thinking…", one line per tool, cards for what the agent asks to do outside its copy),
  // then the files its copy changed, hunk by hunk, each one accepted into the real file or rejected; and the message
  // box with the passage attached from the open document. The list follows the newest line only while the person is
  // at its end: scrolled up to read, it stays where it is. No agent chosen yet | the person changing it: the picker
  // stands in for the conversation; an agent waiting for a sign-in: its card ends the conversation.
  import MessageSquarePlus from "@lucide/svelte/icons/message-square-plus";
  import Quote from "@lucide/svelte/icons/quote";
  import SendHorizontal from "@lucide/svelte/icons/send-horizontal";
  import Square from "@lucide/svelte/icons/square";
  import Wrench from "@lucide/svelte/icons/wrench";
  import X from "@lucide/svelte/icons/x";
  import { onMount, type Snippet } from "svelte";
  import type { AcpState, FileChange, FileChangeKind } from "../../api";
  import { t, tn, type MessageKey } from "../../core/i18n";
  import { closeView } from "../../core/shell/sides";
  import AgentPicker from "./AgentPicker.svelte";
  import HunkCard from "./HunkCard.svelte";
  import LoginCard from "./LoginCard.svelte";
  import { acp, agentLabel, PANEL, PANEL_MARK, passageLabel } from "./store.svelte";

  type Props = {
    /** The view options button for this header (given while the panel is alone in its group). */
    viewMenu?: Snippet;
  };
  let { viewMenu }: Props = $props();

  const uid = $props.id();
  /** Marks the panel: a selection | the keyboard in it leaves the attached passage as it is. */
  const mark = { [PANEL_MARK]: "" };

  const now = $derived<AcpState>(acp.status?.state ?? "idle");
  const command = $derived(acp.status?.agent ?? "");
  /** The chosen agent's name (`null`: none chosen). */
  const name = $derived(
    acp.status?.chosen === true ? (acp.status.agentName ?? agentLabel(command)) : null,
  );
  const working = $derived(now === "starting" || now === "working");
  const canSend = $derived(!acp.busy && acp.draft.trim() !== "");

  let scroller = $state<HTMLElement>();
  /** The person is at the end of the list (then it follows new lines). */
  let atEnd = true;

  onMount(() => {
    void acp.refreshChanges();
  });

  // new lines | changes: the list keeps its end in view while the person was there
  $effect(() => {
    void acp.transcript;
    void acp.changes;
    const el = scroller;
    if (el && atEnd) el.scrollTop = el.scrollHeight;
  });

  function followed(): void {
    const el = scroller;
    if (el) atEnd = el.scrollHeight - el.scrollTop - el.clientHeight < 32;
  }

  function composerKey(event: KeyboardEvent): void {
    // Shift+Enter is a new line; an IME's Enter ends its composition
    if (event.key !== "Enter" || event.shiftKey || event.isComposing) return;
    event.preventDefault();
    void acp.send();
  }

  function stateKey(state: AcpState): MessageKey {
    switch (state) {
      case "idle":
        return "acp.state.idle";
      case "starting":
        return "acp.state.starting";
      case "ready":
        return "acp.state.ready";
      case "working":
        return "acp.state.working";
      case "error":
        return "acp.state.error";
      case "needsLogin":
        return "acp.state.needsLogin";
      default:
        return state satisfies never;
    }
  }

  function kindKey(kind: FileChangeKind): MessageKey {
    switch (kind) {
      case "modified":
        return "acp.kind.modified";
      case "added":
        return "acp.kind.added";
      case "deleted":
        return "acp.kind.deleted";
      case "binary":
        return "acp.kind.binary";
      case "conflict":
        return "acp.kind.conflict";
      default:
        return kind satisfies never;
    }
  }

  /** What a file taken only as a whole says about accepting it (`null`: a file taken hunk by hunk). */
  function wholeKey(kind: FileChangeKind): MessageKey | null {
    switch (kind) {
      case "modified":
        return null;
      case "added":
        return "acp.whole.added";
      case "deleted":
        return "acp.whole.deleted";
      case "binary":
        return "acp.whole.binary";
      case "conflict":
        return "acp.whole.conflict";
      default:
        return kind satisfies never;
    }
  }

  /** A tool's status in words (one the protocol may add later: as the agent says it). */
  function toolStatus(status: string): string {
    switch (status) {
      case "pending":
        return t("acp.tool.pending");
      case "in_progress":
        return t("acp.tool.running");
      case "completed":
        return t("acp.tool.done");
      case "failed":
        return t("acp.tool.failed");
      default:
        return status;
    }
  }

  function decideFile(file: FileChange, choice: "accept" | "reject"): void {
    void acp.decide(choice, file.path, null);
  }
</script>

<section class="acp" aria-label={t("acp.label")} {...mark}>
  <header data-view-header>
    <!-- the title and the buttons on one line at any width: the title gives way (ellipsis), the buttons never wrap -->
    <div class="top">
      <h2 title={t("acp.label")}>{t("acp.label")}</h2>
      <span class="spacer"></span>
      {#if working}
        <button
          type="button"
          class="icon"
          aria-label={t("acp.stop")}
          title={t("acp.stop")}
          onclick={() => void acp.cancel()}
        >
          <Square size={13} aria-hidden="true" />
        </button>
      {/if}
      <button
        type="button"
        class="icon"
        aria-label={t("acp.newConversation")}
        title={t("acp.newConversation")}
        onclick={() => void acp.newConversation()}
      >
        <MessageSquarePlus size={14} aria-hidden="true" />
      </button>
      {@render viewMenu?.()}
      <button
        type="button"
        class="icon close"
        aria-label={t("acp.close")}
        title={t("acp.close")}
        onclick={() => closeView(PANEL)}>×</button
      >
    </div>
    <p class="line">
      <span class={["dot", now]} aria-hidden="true"></span>
      <span class="state">{t(stateKey(now))}</span>
      {#if name !== null}
        <span class="agent" title={t("acp.agentCommand", { command })}>· {name}</span>
        {#if !acp.showsPicker}
          <button
            type="button"
            class="change"
            aria-label={t("acp.changeAgent")}
            title={t("acp.changeAgent")}
            disabled={!acp.canChange}
            onclick={() => acp.openPicker()}>{t("acp.change")}</button
          >
        {/if}
      {/if}
    </p>
  </header>

  <div class="scroll" bind:this={scroller} onscroll={followed}>
    {#if acp.showsPicker}
      <AgentPicker focus={acp.picking} />
    {:else}
      {#if acp.transcript.length === 0}
        <div class="empty">
          <p>{t("acp.empty")}</p>
          <p>{t("acp.emptyPassage")}</p>
        </div>
      {/if}
      <ol class="transcript" aria-live="polite" aria-label={t("acp.conversation")}>
        {#each acp.transcript as entry (entry.id)}
          {#if entry.kind === "you"}
            <li class="you">
              <span class="who">{t("acp.you")}</span>
              {#if entry.passage !== null}
                <span class="passage" title={entry.passage.text}>{passageLabel(entry.passage)}</span
                >
              {/if}
              <p class="text">{entry.text}</p>
            </li>
          {:else if entry.kind === "agent"}
            <li class="agent-text">
              <p class="text">{entry.text}</p>
            </li>
          {:else if entry.kind === "thinking"}
            <li class="quiet">{t("acp.thinking")}</li>
          {:else if entry.kind === "tool"}
            <li class="tool">
              <Wrench size={12} aria-hidden="true" />
              <span class="tool-title" title={entry.title}>{entry.title}</span>
              <span class={["tool-status", entry.status]}>{toolStatus(entry.status)}</span>
            </li>
          {:else if entry.kind === "question"}
            <li class="question" aria-labelledby="{uid}-q{entry.id}">
              <p class="ask" id="{uid}-q{entry.id}">{t("acp.question")}</p>
              <p class="ask-title">{entry.title}</p>
              {#if entry.detail !== "" && entry.detail !== entry.title}
                <pre class="detail">{entry.detail}</pre>
              {/if}
              {#if entry.answer.status === "open"}
                <div class="options">
                  {#each entry.options as option (option.id)}
                    <button
                      type="button"
                      class={["btn", "btn--sm", option.kind === "allow_once" && "btn--primary"]}
                      onclick={() => void acp.answer(entry.requestId, option)}>{option.name}</button
                    >
                  {/each}
                </div>
              {:else if entry.answer.status === "answered"}
                <p class="quiet">{t("acp.answered", { option: entry.answer.option })}</p>
              {:else}
                <p class="quiet">{t("acp.questionClosed")}</p>
              {/if}
            </li>
          {:else}
            <li class="notice">{entry.text}</li>
          {/if}
        {/each}
      </ol>

      {#if now === "needsLogin"}
        <LoginCard methods={acp.status?.methods ?? []} name={name ?? t("acp.label")} />
      {/if}

      {#if acp.changes.length > 0}
        <section class="changes" aria-labelledby="{uid}-changes">
          <h3 id="{uid}-changes">
            {t("acp.changes")}
            <span class="count">{tn("acp.changedFiles", acp.changes.length)}</span>
          </h3>
          <p class="quiet">{t("acp.changesHint")}</p>
          {#each acp.changes as file (file.path)}
            {@const whole = wholeKey(file.kind)}
            <div class="file">
              <div class="file-head">
                <span class="path" title={file.path}>{file.path}</span>
                <span class={["kind", file.kind]}>{t(kindKey(file.kind))}</span>
              </div>
              {#if whole !== null}
                <p class="quiet">{t(whole)}</p>
              {/if}
              {#each file.hunks as hunk (hunk.id)}
                <HunkCard
                  {hunk}
                  whole={whole !== null}
                  disabled={acp.deciding !== null}
                  onaccept={() => void acp.decide("accept", file.path, [hunk.id])}
                  onreject={() => void acp.decide("reject", file.path, [hunk.id])}
                />
              {/each}
              <div class="file-actions">
                <button
                  type="button"
                  class="btn btn--sm"
                  aria-label={t("acp.rejectFile", { path: file.path })}
                  disabled={acp.deciding !== null}
                  onclick={() => decideFile(file, "reject")}
                  >{t(whole === null ? "acp.rejectAll" : "acp.reject")}</button
                >
                <button
                  type="button"
                  class="btn btn--sm btn--primary"
                  aria-label={t("acp.acceptFile", { path: file.path })}
                  disabled={acp.deciding !== null}
                  onclick={() => decideFile(file, "accept")}
                  >{t(whole === null ? "acp.acceptAll" : "acp.accept")}</button
                >
              </div>
            </div>
          {/each}
        </section>
      {/if}
      {#if acp.refusal !== null}
        <p class="refusal" role="alert">
          {t("acp.refusedFor", { path: acp.refusal.path, reason: acp.refusal.text })}
        </p>
      {/if}
    {/if}
  </div>

  {#if !acp.showsPicker}
    <div class="composer">
      {#if acp.passage !== null}
        <div class="chip" title={acp.passage.text}>
          <Quote size={12} aria-hidden="true" />
          <span class="chip-text">{passageLabel(acp.passage)}</span>
          <button
            type="button"
            class="icon chip-drop"
            aria-label={t("acp.dropPassage")}
            title={t("acp.dropPassage")}
            onclick={() => acp.dropPassage()}
          >
            <X size={12} aria-hidden="true" />
          </button>
        </div>
      {/if}
      <textarea
        class="message"
        rows="3"
        bind:value={acp.draft}
        placeholder={t("acp.placeholder")}
        aria-label={t("acp.message")}
        aria-describedby="{uid}-hint"
        onkeydown={composerKey}></textarea>
      <div class="send-row">
        <span class="hint" id="{uid}-hint">{t("acp.sendHint")}</span>
        <button
          type="button"
          class="btn btn--sm btn--primary"
          disabled={!canSend}
          onclick={() => void acp.send()}
        >
          <SendHorizontal size={13} aria-hidden="true" />
          {t("acp.send")}
        </button>
      </div>
    </div>
  {/if}
</section>

<style>
  .acp {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  header {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--sp-2) var(--sp-3);
    border-bottom: 1px solid var(--border);
  }
  .top {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    min-width: 0;
  }
  /* same title style as the other panels of the column */
  h2 {
    flex: 0 1 auto;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
    letter-spacing: 0.05em;
    text-overflow: ellipsis;
    text-transform: uppercase;
    white-space: nowrap;
  }
  .spacer {
    flex: 1 1 0;
  }
  .line {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    min-width: 0;
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--faint);
  }
  .dot.ready {
    background: var(--git-added);
  }
  .dot.starting,
  .dot.working {
    background: var(--accent);
  }
  .dot.error {
    background: var(--git-deleted);
  }
  .dot.needsLogin {
    background: var(--git-modified);
  }
  .state {
    flex: none;
  }
  .agent {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* a word-sized button: the status line keeps its height */
  .change {
    flex: none;
    padding: 0 var(--sp-1);
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--accent-text);
    font: inherit;
    cursor: pointer;
  }
  .change:hover:not(:disabled) {
    background: var(--hover);
  }
  .change:disabled {
    color: var(--faint);
    cursor: default;
  }
  .icon {
    flex: none;
    display: inline-grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover {
    background: var(--hover);
    color: var(--text);
  }
  .close {
    font-size: 16px;
    line-height: 1;
  }
  .scroll {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    flex: 1;
    min-height: 0;
    padding: var(--sp-3);
    overflow-y: auto;
  }
  .empty {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .empty p {
    margin: 0 0 var(--sp-2);
  }
  .transcript {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .transcript:empty {
    display: none;
  }
  .text {
    margin: 0;
    font-size: var(--fs-ui-sm);
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .you {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--sp-2);
    border-radius: var(--radius-card);
    background: var(--accent-soft);
  }
  .who {
    color: var(--accent-text);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
  }
  .passage {
    overflow: hidden;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .agent-text {
    padding: 0 var(--sp-1);
  }
  .quiet {
    margin: 0;
    color: var(--faint);
    font-size: var(--fs-ui-xs);
  }
  li.quiet {
    padding: 0 var(--sp-1);
    font-style: italic;
  }
  .tool {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    min-width: 0;
    padding: 0 var(--sp-1);
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .tool-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tool-status {
    flex: none;
    color: var(--faint);
  }
  .tool-status.failed {
    color: var(--git-deleted-text);
  }
  .question {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    padding: var(--sp-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-card);
    background: var(--surface);
  }
  .ask {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
  }
  .ask-title {
    margin: 0;
    font-size: var(--fs-ui-sm);
    overflow-wrap: anywhere;
  }
  .detail {
    margin: 0;
    padding: var(--sp-1) var(--sp-2);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    font-family: var(--font-code);
    font-size: var(--fs-ui-xs);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .options {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-1);
  }
  /* agents name their options freely ("Yes, and don't ask again for …"): long names wrap inside the panel */
  .options .btn {
    max-width: 100%;
    height: auto;
    min-height: 24px;
    white-space: normal;
    text-align: start;
    overflow-wrap: anywhere;
  }
  .notice {
    padding: var(--sp-1) var(--sp-2);
    border-left: 2px solid var(--border-strong);
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .changes {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    padding-top: var(--sp-2);
    border-top: 1px solid var(--border);
  }
  h3 {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }
  .count {
    color: var(--faint);
    font-weight: 400;
    letter-spacing: normal;
    text-transform: none;
  }
  .file {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }
  .file-head {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
    min-width: 0;
  }
  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    font-family: var(--font-code);
    font-size: var(--fs-ui-xs);
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }
  .kind {
    flex: none;
    color: var(--git-modified-text);
    font-size: var(--fs-ui-xs);
  }
  .kind.added {
    color: var(--git-added-text);
  }
  .kind.deleted,
  .kind.conflict {
    color: var(--git-deleted-text);
  }
  .file-actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--sp-1);
  }
  .refusal {
    margin: 0;
    color: var(--git-deleted-text);
    font-size: var(--fs-ui-xs);
    overflow-wrap: anywhere;
  }
  .composer {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    padding: var(--sp-2) var(--sp-3) var(--sp-3);
    border-top: 1px solid var(--border);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    align-self: flex-start;
    max-width: 100%;
    padding: 0 0 0 var(--sp-2);
    border-radius: var(--radius-sm);
    background: var(--accent-soft);
    color: var(--accent-text);
    font-size: var(--fs-ui-xs);
  }
  .chip-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chip-drop {
    flex: none;
    width: 20px;
    height: 20px;
    color: inherit;
  }
  .message {
    width: 100%;
    min-height: 3.5rem;
    max-height: 12rem;
    box-sizing: border-box;
    padding: var(--sp-1) var(--sp-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui-sm);
    resize: vertical;
  }
  .message:hover {
    border-color: var(--border-strong);
  }
  .send-row {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }
  .hint {
    flex: 1;
    min-width: 0;
    color: var(--faint);
    font-size: var(--fs-ui-xs);
  }
</style>
