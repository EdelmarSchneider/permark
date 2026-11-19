<script lang="ts">
  // The view "Assistant": the conversation with the project's agent (the person's messages, the agent's reply as
  // plain text while it comes and rendered as a document once whole, one "Thinking…" while it thinks, one line per
  // tool, cards for what the agent asks to do outside its copy), then the files its copy changed, hunk by hunk, each
  // one accepted into the real file or rejected
  // (Open shows the real file at that line); a bar above the message box leads to them; and the message box with the
  // passage attached from the open document, whose Send turns into Stop while the agent works. The list follows the
  // conversation's end only while the person is there; changes arriving never move it. A question | a sign-in the
  // agent waits for is brought into view and takes the keyboard (never from someone typing; a question's card, never
  // one of its answers). Escape in the message box stops the agent at work. No agent chosen yet | the person
  // changing it: the picker stands in for the conversation. Empty and idle, it offers to continue the project's last
  // conversation (the agent replays it as lines of the conversation). Under the agent's name, its settings the person
  // may change while it runs (its model, reasoning level, model settings), each a labelled control.
  import MessageSquarePlus from "@lucide/svelte/icons/message-square-plus";
  import MessageSquareReply from "@lucide/svelte/icons/message-square-reply";
  import Quote from "@lucide/svelte/icons/quote";
  import SendHorizontal from "@lucide/svelte/icons/send-horizontal";
  import Square from "@lucide/svelte/icons/square";
  import Wrench from "@lucide/svelte/icons/wrench";
  import X from "@lucide/svelte/icons/x";
  import { onMount, tick, type Snippet } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import type {
    AcpConfigOption,
    AcpState,
    FileChange,
    FileChangeKind,
    PermissionChoice,
  } from "../../api";
  import { runRows } from "../../core/editing/diffRows";
  import { t, tn, type MessageKey } from "../../core/i18n";
  import { arrangement } from "../../core/shell/arrangement.svelte";
  import { adrift, closeView, placeView } from "../../core/shell/sides";
  import AgentPicker from "./AgentPicker.svelte";
  import DiffBox from "./DiffBox.svelte";
  import HunkCard from "./HunkCard.svelte";
  import LoginCard from "./LoginCard.svelte";
  import MarkdownReply from "./MarkdownReply.svelte";
  import Note from "./Note.svelte";
  import {
    acp,
    configLabel,
    firstPlace,
    optionLabel,
    PANEL,
    PANEL_MARK,
    passageLabel,
    passageStart,
    reachKey,
    shownOptions,
    toolKindKey,
    type ToolState,
  } from "./store.svelte";

  type Props = {
    /** The view options button for this header (given while the panel is alone in its group). */
    viewMenu?: Snippet;
  };
  let { viewMenu }: Props = $props();

  const uid = $props.id();
  /** Marks the panel: a selection | the keyboard in it leaves the attached passage as it is. */
  const mark = { [PANEL_MARK]: "" };
  /** Room kept between what the list brings into view and the list's edge (px). */
  const EDGE = 12;
  const EXAMPLES: readonly MessageKey[] = [
    "acp.example.clearer",
    "acp.example.summary",
    "acp.example.find",
  ];

  const now = $derived<AcpState>(acp.status?.state ?? "idle");
  const command = $derived(acp.status?.agent ?? "");
  /** The chosen agent's name (`null`: none chosen); the backend names no own command: worded here. */
  const name = $derived(
    acp.status?.chosen === true ? (acp.status.agentName ?? t("acp.ownCommand")) : null,
  );
  const working = $derived(now === "starting" || now === "working");
  /** The agent's settings show under its name (never over the picker). */
  const settings = $derived(acp.showsPicker ? [] : acp.options);

  /** Shows in a setting's control what the agent last told, each time it tells it: a refused change comes back as the
   *  value shown before, which a plain `value` | `checked` never writes again. */
  function holds(option: AcpConfigOption): Attachment<HTMLSelectElement | HTMLInputElement> {
    return (control) => {
      if (option.kind.type === "select" && control instanceof HTMLSelectElement)
        control.value = option.kind.current;
      else if (option.kind.type === "toggle" && control instanceof HTMLInputElement)
        control.checked = option.kind.on;
    };
  }
  const canSend = $derived(!acp.busy && acp.draft.trim() !== "");
  /** What the status line says: a question waiting outranks "Working…". */
  const said = $derived(acp.asking ? t("acp.state.waiting") : t(stateKey(now)));
  const changed = $derived(acp.changes.length + acp.moreChanges);
  const skipped = $derived(acp.status?.skippedFiles ?? 0);

  let section = $state<HTMLElement>();
  let scroller = $state<HTMLElement>();
  let talk = $state<HTMLElement>();
  let changesBox = $state<HTMLElement>();
  let changesTitle = $state<HTMLElement>();
  let message = $state<HTMLTextAreaElement>();
  let change = $state<HTMLButtonElement>();
  let login = $state<{ focusTitle(): HTMLElement | null }>();
  /** The person is at the conversation's end (then it follows new lines). */
  let follow = true;
  /** The question last brought into view (by entry id; 0: none). */
  let asked = 0;
  /** The sign-in card was shown at the last look. */
  let signInShown = false;

  onMount(() => {
    // first shown: a tab beside the outline, selected, rather than a cramped group of its own
    const placed = firstPlace(arrangement.saved);
    if (placed !== null) placeView(PANEL, placed);
  });

  // new lines: the conversation's end stays in view while the person was there (changes below it never pull it)
  $effect(() => {
    void acp.transcript;
    void acp.thinking;
    void now;
    if (follow) void tick().then(showEnd);
  });

  // a question the agent waits at: into view, and its card takes the keyboard. Never an answer: Enter | Space meant
  // for the message would allow a command that runs with the person's rights; Tab reaches the answers. The message
  // box keeps the keyboard (the status line and the live region tell of the card).
  $effect(() => {
    const open = acp.transcript.findLast(
      (e) => e.kind === "question" && e.answer.status === "open",
    );
    if (open === undefined || open.id === asked) return;
    asked = open.id;
    const id = open.id;
    void tick().then(() => {
      const card = scroller?.querySelector<HTMLElement>(`[data-question="${String(id)}"]`);
      if (!card) return;
      reveal(card);
      if (mayTakeFocus() && document.activeElement !== message) card.focus({ preventScroll: true });
    });
  });

  // the picker shows from its top (the list kept the conversation's place)
  $effect(() => {
    if (acp.showsPicker)
      void tick().then(() => {
        if (scroller) scroller.scrollTop = 0;
      });
  });

  // a sign-in the agent waits for: its card into view, its title takes the keyboard
  $effect(() => {
    const shown = now === "needsLogin" && !acp.showsPicker;
    if (shown && !signInShown)
      void tick().then(() => {
        const title = mayTakeFocus() ? (login?.focusTitle() ?? null) : null;
        const card = title?.closest("section") ?? talk?.querySelector<HTMLElement>(".login");
        if (card instanceof HTMLElement) reveal(card);
      });
    signInShown = shown;
  });

  /** The keyboard may move to what the agent waits for: it is nowhere, or in this panel but not on words being
   *  written (a key meant for the message would answer the agent). Elsewhere in the window the person works there. */
  function mayTakeFocus(): boolean {
    if (adrift()) return true;
    const active = document.activeElement;
    if (!(active instanceof Node) || section?.contains(active) !== true) return false;
    return !(active === message && acp.draft.trim() !== "");
  }

  /** Scrolls the list (only it) so `el` is in view: its start, or its end when only that is missing. */
  function reveal(el: HTMLElement): void {
    const box = scroller;
    if (!box) return;
    const at = el.getBoundingClientRect();
    const view = box.getBoundingClientRect();
    if (at.top < view.top || at.height > view.height) box.scrollTop += at.top - view.top - EDGE;
    else if (at.bottom > view.bottom) box.scrollTop += at.bottom - view.bottom + EDGE;
  }

  /** The conversation's end into view (what comes after it, the changes, stays below). */
  function showEnd(): void {
    const box = scroller;
    const end = talk;
    if (!box || !end) return;
    const below = end.getBoundingClientRect().bottom - box.getBoundingClientRect().bottom + EDGE;
    if (below > 0) box.scrollTop += below;
  }

  function followed(): void {
    const box = scroller;
    const end = talk;
    if (box && end)
      follow = end.getBoundingClientRect().bottom - box.getBoundingClientRect().bottom < 32;
  }

  /** The button just used goes away (a choice, a sign-in, Stop): the keyboard moves on to the message box, never
   *  stays on the page itself; back from the picker, the list follows the conversation's end again. */
  async function moveOn(): Promise<void> {
    await tick();
    follow = true;
    showEnd();
    (message ?? change)?.focus();
  }

  function composerKey(event: KeyboardEvent): void {
    // an IME's Enter | Escape ends its composition
    if (event.isComposing) return;
    if (event.key === "Escape") {
      // the agent at work stops, as the hint under the box says; else the key is not the box's
      if (!working) return;
      event.preventDefault();
      event.stopPropagation();
      stop();
      return;
    }
    // Shift+Enter is a new line
    if (event.key !== "Enter" || event.shiftKey) return;
    event.preventDefault();
    void acp.send();
  }

  function stop(): void {
    void acp.cancel();
    void moveOn();
  }

  function answer(requestId: string, option: PermissionChoice): void {
    void acp.answer(requestId, option);
    void moveOn();
  }

  /** Continues the last conversation; the button goes with the empty panel, so the keyboard goes to the message box. */
  function continueLast(): void {
    void acp.continueLast();
    void moveOn();
  }

  /** An example request in the box, ready to send or to change. */
  function example(key: MessageKey): void {
    acp.draft = t(key);
    message?.focus();
  }

  /** The bar's jump: the changes' start into view, their title takes the keyboard. */
  function toChanges(): void {
    if (changesBox) reveal(changesBox);
    changesTitle?.focus({ preventScroll: true });
  }

  /** A decision ran with its buttons waiting: the keyboard goes to the same answer of the next change (Enter goes on
   *  as the person went), else its first answer, else the message box. */
  async function decided(running: Promise<void>, choice: "accept" | "reject"): Promise<void> {
    await running;
    await tick();
    if (!adrift()) return;
    const same = choice === "accept" ? ".btn--primary" : ".btn:not(.btn--primary)";
    const next =
      changesBox?.querySelector<HTMLElement>(`${same}:not(:disabled)`) ??
      changesBox?.querySelector<HTMLElement>(".btn:not(:disabled)");
    (next ?? message)?.focus();
  }

  function decide(choice: "accept" | "reject", path: string, hunks: string[] | null): void {
    void decided(acp.decide(choice, path, hunks), choice);
  }

  /** The whole file: by the keys of the hunks shown (a list that changed meanwhile is refused, never applied). */
  function decideFile(file: FileChange, choice: "accept" | "reject"): void {
    const keys = file.hunks.map((hunk) => hunk.key);
    decide(choice, file.path, keys.length > 0 ? keys : null);
  }

  /** The line a file opens at: its first change (a new file has no real one to open). */
  function firstLine(file: FileChange): number | null {
    return file.hunks[0]?.oldStart ?? null;
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
      case "link":
        return "acp.kind.link";
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
      case "link":
        return "acp.whole.link";
      default:
        return kind satisfies never;
    }
  }

  /** A tool's status in words (one the protocol may add later: none). */
  function toolStatus(status: ToolState): string {
    switch (status) {
      case "pending":
        return t("acp.tool.pending");
      case "in_progress":
        return t("acp.tool.running");
      case "completed":
        return t("acp.tool.done");
      case "failed":
        return t("acp.tool.failed");
      case "stopped":
        return t("acp.tool.stopped");
      case "other":
        return "";
      default:
        return status satisfies never;
    }
  }
</script>

{#snippet setting(option: AcpConfigOption)}
  {@const name = configLabel(option)}
  {#if option.kind.type === "select"}
    {@const kind = option.kind}
    <label class="setting" title={option.description ?? undefined}>
      <span class="setting-name">{name}</span>
      <!-- a refused change: the control shows what holds again -->
      <select
        class="field-select"
        value={kind.current}
        {@attach holds(option)}
        disabled={!acp.canSetOptions}
        onchange={(event) => {
          const control = event.currentTarget;
          void acp.setOption(option.id, control.value).then((ok) => {
            if (!ok) control.value = kind.current;
          });
        }}
      >
        {#each kind.choices as choice (choice.value)}
          <option value={choice.value} title={choice.description ?? undefined}>{choice.name}</option
          >
        {/each}
      </select>
    </label>
  {:else}
    {@const on = option.kind.on}
    <label class="setting" title={option.description ?? undefined}>
      <input
        type="checkbox"
        role="switch"
        checked={on}
        {@attach holds(option)}
        disabled={!acp.canSetOptions}
        onchange={(event) => {
          const control = event.currentTarget;
          void acp.setOption(option.id, control.checked).then((ok) => {
            if (!ok) control.checked = on;
          });
        }}
      />
      <span class="setting-name">{name}</span>
    </label>
  {/if}
{/snippet}

{#snippet details(text: string | null)}
  {#if text !== null}
    <details class="details">
      <summary>{t("acp.details")}</summary>
      <!-- a scroll region: the keyboard scrolls it once it holds the focus -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <pre class="output" tabindex="0" role="region" aria-label={t("acp.details")}>{text}</pre>
    </details>
  {/if}
{/snippet}

<!-- the pointer going into the panel would take the selection with it: a selection change still waiting is read first -->
<section
  class="acp"
  aria-label={t("acp.label")}
  bind:this={section}
  {...mark}
  onpointerdowncapture={() => acp.flushCapture()}
>
  <header data-view-header>
    <!-- the title and the buttons on one line at any width: the title gives way (ellipsis), the buttons never wrap -->
    <div class="top">
      <h2 title={t("acp.label")}>{t("acp.label")}</h2>
      <span class="spacer"></span>
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
      <span class={["dot", acp.asking ? "asking" : now]} aria-hidden="true"></span>
      <span class="state">{said}</span>
      {#if name !== null}
        <span class="agent" title={t("acp.agentCommand", { command })}>· {name}</span>
        {#if !acp.showsPicker}
          <button
            bind:this={change}
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
    {#if settings.length > 0}
      <div class="settings" role="group" aria-label={t("acp.options")}>
        {#each settings as option (option.id)}
          {@render setting(option)}
        {/each}
      </div>
    {/if}
  </header>

  <!-- focusable: a long reply of plain text scrolls with the keyboard too (the picker is a named section of its own,
       full of controls) -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    class="scroll"
    bind:this={scroller}
    onscroll={followed}
    tabindex={acp.showsPicker ? undefined : 0}
    role={acp.showsPicker ? undefined : "region"}
    aria-label={acp.showsPicker ? undefined : t("acp.conversation")}
  >
    {#if acp.showsPicker}
      <AgentPicker focus={acp.picking} onclose={() => void moveOn()} />
    {:else}
      <div class="talk" bind:this={talk}>
        {#if acp.transcript.length === 0 && !acp.thinking}
          <div class="empty">
            <p>{t("acp.empty")}</p>
            {#if acp.passage === null}
              <p>{t("acp.emptyPassage")}</p>
            {/if}
            {#if acp.resumable !== null}
              <button type="button" class="continue" disabled={acp.busy} onclick={continueLast}>
                <MessageSquareReply size={14} aria-hidden="true" />
                <span class="continue-text">
                  <span class="continue-label">{t("acp.continue")}</span>
                  {#if acp.resumable.title !== null}
                    <span class="continue-title">{acp.resumable.title}</span>
                  {/if}
                </span>
              </button>
            {/if}
            <p class="examples-title">{t("acp.examples")}</p>
            <ul class="examples">
              {#each EXAMPLES as key (key)}
                <li>
                  <button type="button" class="example" onclick={() => example(key)}
                    >{t(key)}</button
                  >
                </li>
              {/each}
            </ul>
          </div>
        {/if}
        {#if acp.hidden > 0}
          <Note>{t("acp.hiddenEntries")}</Note>
        {/if}
        <ol class="transcript">
          {#each acp.transcript as entry, at (entry.id)}
            {#if entry.kind === "you"}
              <li class="you">
                <span class="who">{t("acp.you")}</span>
                {#if entry.passage !== null}
                  <span class="passage">
                    <span class="passage-where">{passageLabel(entry.passage)}</span>
                    <q class="passage-start">{passageStart(entry.passage)}</q>
                  </span>
                {/if}
                <p class="text">{entry.text}</p>
              </li>
            {:else if entry.kind === "agent"}
              <li class="agent-text">
                <!-- whole once another line follows it or the turn ended -->
                <MarkdownReply
                  text={entry.text}
                  final={at < acp.transcript.length - 1 || !working}
                  last={at === acp.transcript.length - 1}
                />
              </li>
            {:else if entry.kind === "tool"}
              <li class="tool">
                <Wrench size={12} aria-hidden="true" />
                <span class="tool-title" title={entry.title}>{entry.title}</span>
                <span class={["tool-status", entry.status]}>{toolStatus(entry.status)}</span>
              </li>
            {:else if entry.kind === "question"}
              {@const offered = shownOptions(entry.options)}
              {@const warning = reachKey(entry.reach)}
              <!-- the kind of action is PerMark's word; the title, summary and change are the agent's own -->
              <li
                class="question"
                tabindex="-1"
                aria-labelledby="{uid}-q{entry.id}"
                data-question={entry.id}
              >
                <p class="ask" id="{uid}-q{entry.id}">{t("acp.question")}</p>
                <p class="ask-head">
                  <span class="ask-kind">{t(toolKindKey(entry.toolKind))}</span>
                  <span class="ask-title">{entry.title}</span>
                </p>
                {#if entry.summary !== null && entry.summary !== entry.title}
                  <p class="ask-summary">{entry.summary}</p>
                {/if}
                {#if entry.detail !== "" && entry.detail !== entry.title}
                  <pre class="detail">{entry.detail}</pre>
                {/if}
                {#if entry.diff !== null}
                  {#if !entry.detail.split("\n").includes(entry.diff.path)}
                    <p class="diff-path">{entry.diff.path}</p>
                  {/if}
                  <div class="ask-diff">
                    <DiffBox rows={runRows(entry.diff.oldLines, entry.diff.newLines, 1)} />
                  </div>
                  {#if entry.diff.truncated}
                    <Note>{t("acp.diffCut")}</Note>
                  {/if}
                {/if}
                {#if entry.detail === "" && entry.diff === null}
                  <!-- never a choice on the agent's title alone -->
                  <Note tone="muted">{t("acp.noDetails")}</Note>
                {/if}
                {#if entry.answer.status === "open"}
                  <!-- what allowing reaches past the copy: said before the buttons, gone once answered -->
                  {#if warning !== null}
                    <Note tone="muted">{t(warning)}</Note>
                  {/if}
                  <div class="options">
                    {#each offered.shown as option (option.id)}
                      {@const label = optionLabel(option)}
                      <button
                        type="button"
                        class={["btn", "btn--sm", option.kind === "allow_once" && "btn--primary"]}
                        title={option.name === label ? undefined : option.name}
                        onclick={() => answer(entry.requestId, option)}>{label}</button
                      >
                    {/each}
                  </div>
                  {#if offered.hidden}
                    <Note>{t("acp.alwaysHidden")}</Note>
                  {/if}
                {:else if entry.answer.status === "answered"}
                  <Note>{t("acp.answered", { option: entry.answer.option })}</Note>
                {:else}
                  <Note>{t("acp.questionClosed")}</Note>
                {/if}
              </li>
            {:else if entry.kind === "divider"}
              <li class="divider">{t("acp.switched", { name: entry.name })}</li>
            {:else}
              <li class="notice">
                {entry.text}
                {@render details(entry.details)}
              </li>
            {/if}
          {/each}
        </ol>
        {#if acp.thinking}
          <p class="thinking">{t("acp.thinking")}</p>
        {/if}
        {#if now === "needsLogin"}
          <LoginCard
            bind:this={login}
            methods={acp.status?.methods ?? []}
            name={acp.status?.agentName ?? null}
            onsignin={() => void moveOn()}
          />
        {/if}
      </div>

      {#if skipped > 0}
        <Note>{tn("acp.skipped", skipped)}</Note>
      {/if}

      {#if acp.changes.length > 0}
        <section class="changes" aria-labelledby="{uid}-changes" bind:this={changesBox}>
          <h3 id="{uid}-changes" tabindex="-1" bind:this={changesTitle}>
            {t("acp.changes")}
            <span class="count">{tn("acp.changedFiles", acp.changes.length)}</span>
          </h3>
          <Note>{t("acp.changesHint")}</Note>
          {#each acp.changes as file (file.path)}
            {@const whole = wholeKey(file.kind)}
            <div class="file">
              <div class="file-head">
                <!-- a new file has no real one to open yet; a link is never followed -->
                {#if file.kind === "added" || file.kind === "link"}
                  <span class="path" title={file.path}>{file.path}</span>
                {:else}
                  <button
                    type="button"
                    class="path open"
                    title={t("acp.openFile", { path: file.path })}
                    aria-label={t("acp.openFile", { path: file.path })}
                    onclick={() => acp.openAt(file.path, firstLine(file))}>{file.path}</button
                  >
                {/if}
                <span class={["kind", file.kind]}>{t(kindKey(file.kind))}</span>
              </div>
              {#if whole !== null}
                <Note>{t(whole)}</Note>
              {/if}
              {#each file.hunks as hunk (hunk.key)}
                <HunkCard
                  {hunk}
                  path={file.path}
                  whole={whole !== null}
                  disabled={acp.deciding !== null}
                  onaccept={() => decide("accept", file.path, [hunk.key])}
                  onreject={() => decide("reject", file.path, [hunk.key])}
                  onopen={() => acp.openAt(file.path, hunk.oldStart)}
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
                <!-- a link is never offered as content: it can only be removed -->
                {#if file.kind !== "link"}
                  <button
                    type="button"
                    class="btn btn--sm btn--primary"
                    aria-label={t("acp.acceptFile", { path: file.path })}
                    disabled={acp.deciding !== null}
                    onclick={() => decideFile(file, "accept")}
                    >{t(whole === null ? "acp.acceptAll" : "acp.accept")}</button
                  >
                {/if}
              </div>
            </div>
          {/each}
        </section>
      {/if}
      {#if acp.refusal !== null}
        <Note tone="refusal" alert>
          {t("acp.refusedFor", { path: acp.refusal.path, reason: acp.refusal.text })}
        </Note>
      {/if}
    {/if}
  </div>

  {#if !acp.showsPicker}
    {#if changed > 0}
      <div class="bar">
        <button type="button" class="bar-button" onclick={toChanges}
          >{t("acp.changesBar", { n: changed })}</button
        >
      </div>
    {/if}
    <div class="composer">
      {#if acp.passage !== null}
        {@const label = passageLabel(acp.passage)}
        {@const start = passageStart(acp.passage)}
        <div class="chip" role="group" aria-label={t("acp.attachedName", { label, start })}>
          <Quote size={12} aria-hidden="true" />
          <span class="chip-text" title={start}>{t("acp.attached", { label })}</span>
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
        bind:this={message}
        class="message"
        rows="3"
        bind:value={acp.draft}
        placeholder={t("acp.placeholder")}
        aria-label={t("acp.message")}
        aria-describedby="{uid}-hint"
        onkeydown={composerKey}></textarea>
      <div class="send-row">
        <span class="hint" id="{uid}-hint">{t(working ? "acp.stopHint" : "acp.sendHint")}</span>
        <!-- one button in one place: Send, and Stop while the agent works -->
        <button
          type="button"
          class={["btn", "btn--sm", "send", !working && "btn--primary"]}
          disabled={!working && !canSend}
          onclick={working ? stop : () => void acp.send()}
        >
          {#if working}
            <Square size={12} aria-hidden="true" />
            {t("acp.stop")}
          {:else}
            <SendHorizontal size={13} aria-hidden="true" />
            {t("acp.send")}
          {/if}
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
  /* wraps in a narrow panel: the state and the agent's name are never cut */
  .line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0 var(--sp-1);
    min-width: 0;
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
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
  .dot.needsLogin,
  .dot.asking {
    background: var(--git-modified);
  }
  .state,
  .agent {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  /* a word-sized button with a target of at least 24 px */
  .change {
    flex: none;
    min-height: var(--icon-btn-sm);
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
  /* one row that wraps in a narrow panel: each control keeps its label beside it */
  .settings {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-1) var(--sp-3);
    min-width: 0;
    padding-top: var(--sp-1);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .setting {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    min-width: 0;
    max-width: 100%;
  }
  .setting-name {
    flex: none;
  }
  .setting select {
    min-width: 0;
    max-width: 100%;
    height: var(--icon-btn-sm);
    padding: 0 var(--sp-1);
    font-size: var(--fs-ui-sm);
    text-overflow: ellipsis;
  }
  .setting select:disabled,
  .setting input:disabled + .setting-name {
    opacity: 0.6;
  }
  .icon {
    flex: none;
    display: inline-grid;
    place-items: center;
    width: var(--icon-btn-sm);
    height: var(--icon-btn-sm);
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
  .talk {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  .empty {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .empty p {
    margin: 0 0 var(--sp-2);
  }
  .examples-title {
    font-weight: 600;
  }
  .continue {
    display: flex;
    gap: var(--sp-2);
    align-items: flex-start;
    width: 100%;
    margin: 0 0 var(--sp-3);
    padding: var(--sp-2);
    border: 1px solid var(--accent);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui-sm);
    text-align: start;
    cursor: pointer;
  }
  .continue:hover:not(:disabled) {
    background: var(--hover);
  }
  .continue:disabled {
    cursor: default;
    opacity: 0.6;
  }
  .continue :global(svg) {
    flex: none;
    margin-top: 2px;
    color: var(--accent);
  }
  .continue-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .continue-label {
    font-weight: 600;
  }
  .continue-title {
    overflow: hidden;
    color: var(--muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .examples {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .example {
    width: 100%;
    min-height: var(--icon-btn-sm);
    padding: var(--sp-1) var(--sp-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui-sm);
    text-align: start;
    cursor: pointer;
  }
  .example:hover {
    border-color: var(--border-strong);
    background: var(--hover);
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
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
  .passage {
    display: flex;
    flex-direction: column;
    min-width: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .passage-where,
  .passage-start {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .passage-start {
    font-style: italic;
  }
  .agent-text {
    padding: 0 var(--sp-1);
  }
  .thinking {
    margin: 0;
    padding: 0 var(--sp-1);
    color: var(--faint);
    font-size: var(--fs-ui-sm);
    font-style: italic;
  }
  .tool {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    min-width: 0;
    padding: 0 var(--sp-1);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
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
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
  .ask-head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 2px var(--sp-1);
    min-width: 0;
    margin: 0;
    font-size: var(--fs-ui-sm);
  }
  .ask-kind {
    flex: none;
    max-width: 100%;
    padding: 0 var(--sp-1);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--muted);
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .ask-title {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .ask-summary,
  .diff-path {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    overflow-wrap: anywhere;
  }
  .diff-path {
    font-family: var(--font-code);
  }
  .ask-diff {
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg);
    overflow: hidden;
  }
  .detail,
  .output {
    margin: 0;
    padding: var(--sp-1) var(--sp-2);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    font-family: var(--font-code);
    font-size: var(--fs-ui-sm);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .output {
    max-height: 12rem;
    overflow-y: auto;
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
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    padding: var(--sp-1) var(--sp-2);
    border-left: 2px solid var(--border-strong);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .details summary {
    min-height: var(--icon-btn-sm);
    line-height: var(--icon-btn-sm);
    cursor: pointer;
    white-space: normal;
  }
  /* another agent took over: a rule across the list, the words under it */
  .divider {
    margin-top: var(--sp-1);
    padding-top: var(--sp-2);
    border-top: 1px solid var(--border-strong);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    text-align: center;
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
    font-size: var(--fs-ui-sm);
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
    align-items: center;
    gap: var(--sp-2);
    min-width: 0;
  }
  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    font-family: var(--font-code);
    font-size: var(--fs-ui-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }
  /* the file's name opens it: a link's look, a 24 px target */
  .open {
    min-height: var(--icon-btn-sm);
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--accent-text);
    cursor: pointer;
  }
  .open:hover {
    text-decoration: underline;
  }
  .kind {
    flex: none;
    color: var(--git-modified-text);
    font-size: var(--fs-ui-sm);
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
  .bar {
    padding: var(--sp-1) var(--sp-3) 0;
    border-top: 1px solid var(--border);
  }
  .bar-button {
    width: 100%;
    min-height: var(--icon-btn-sm);
    padding: 0 var(--sp-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--accent-soft);
    color: var(--accent-text);
    font: inherit;
    font-size: var(--fs-ui-sm);
    font-weight: 600;
    text-align: start;
    cursor: pointer;
  }
  .bar-button:hover {
    border-color: var(--accent);
  }
  .composer {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    padding: var(--sp-2) var(--sp-3) var(--sp-3);
    border-top: 1px solid var(--border);
  }
  /* the bar above draws the line */
  .bar + .composer {
    padding-top: var(--sp-1);
    border-top: 0;
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
    font-size: var(--fs-ui-sm);
  }
  .chip-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chip-drop {
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
    font-size: var(--fs-ui-sm);
  }
  /* Send and Stop take the same room: the row never shifts when one becomes the other */
  .send {
    min-width: 5.5rem;
    justify-content: center;
  }
</style>
