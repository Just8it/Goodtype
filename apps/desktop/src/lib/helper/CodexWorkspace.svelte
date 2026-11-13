<script lang="ts">
  import { onMount, tick, untrack, type Snippet } from "svelte";
  import { t, formatNumber, notice, renderMessage, type UiText } from "../i18n";
  import { helperSelectionNeedsPixels, helperRevokePresets, helperRevokeAttachment, helperRevokeNotebook, helperCodexRun, helperCodexStop, helperCodexClose, listenHelper,
    type SharePayload, type CodexStatus, type CodexRunEvent, type CodexToolEvent, type Proposal } from "../ipc/helper";
  import type { AppSettings } from "../settings";
  import { contextReply, type ContextSession } from "./context";
  import { canSend, codexProblem } from "./codex";
  import { matchesScope } from "./scope";
  import { sendProposalRevision } from "./proposal";
  import { captureHelperIntent, sameHelperIntent } from "./selection";
  import PageAttachments, { type AttachmentPage } from "./PageAttachments.svelte";
  import type { RequestContextIntent } from "../ipc/helper";
  import CodexComposer from "./CodexComposer.svelte";
  import CodexRunPanel from "./CodexRunPanel.svelte";
  import { validateCellContext, cellsAbove, type CellEntry } from "../calculation/cellIntent";
  let { cellEntry, entry, share, pages = [], summary, status, defaults, session, thumbnail, review, selectionLabel, active = true, cropVersion = 0, needsPixels = false, onHide, onClose }:
    { cellEntry?: CellEntry; entry?: { focus: import("./editorEntry").EditorFocus | null; pageId?: string; version: number }; share: SharePayload; pages?: AttachmentPage[]; summary?: { pageId: string; prompt: string; version: number }; status: CodexStatus; review?: Snippet<[((proposal: Proposal, text: string) => Promise<void>) | undefined, boolean]>; onHide: () => void;
      active?: boolean; cropVersion?: number; needsPixels?: boolean; thumbnail?: Snippet; selectionLabel?: string; defaults: AppSettings["aiHelper"]; session: ContextSession & { readonly pageInputLocked?: boolean }; onClose: () => void } = $props();
  let allowImages = $state(false);
  $effect(() => { if (!active) allowImages = false; });
  let model = $state<string | null>(untrack(() => defaults.codexModel)), webSearch = $state(untrack(() => defaults.webSearch));
  let run = $state<CodexRunEvent | null>(null), text = $state(""), tools = $state<CodexToolEvent[]>([]);
  let lastPrompt = $state("");
  let history = $state<{ prompt: string; answer: string }[]>([]);
  let body = $state<HTMLDivElement>();
  let responseEnd = $state<HTMLDivElement>();
  let followOutput = true;
  function outputScrollTop() {
    if (!body) return 0;
    const bottom = Math.max(0, body.scrollHeight - body.clientHeight);
    return run && !["done", "failed", "stopped"].includes(run.state) && responseEnd
      ? Math.min(bottom, Math.max(0, responseEnd.offsetTop - body.clientHeight + 16)) : bottom;
  }
  function trackScroll() {
    if (body) followOutput = Math.abs(outputScrollTop() - body.scrollTop) < 48;
  }
  $effect(() => {
    text; lastPrompt; run?.state;
    if (body && followOutput) void tick().then(() => {
      if (body && followOutput) body.scrollTop = outputScrollTop();
    });
  });
  const scopeVersions = new Map<string, number>();
  let conversationId: string | null = null;
  let requestId: string | null = null;
  let grantedPages: string[] = [];
  let pageIds = $state<string[]>([]);
  // Shared only after the assistant asks for them (request_scope page_names); then shown as a chip.
  let sharePageNames = $state(false);
  let destinationPageId = $state(untrack(() => share.pageId));
  let presets = $state(false);
  let grantedPresets = false;
  let intentVersion = $state(0);
  $effect(() => { cropVersion; intentVersion = untrack(() => intentVersion) + 1; });
  let draftRoot = $state("");
  let initialPrompt = $state("");
  $effect(() => {
    const root = session.root;
    if (root !== draftRoot) {
      const previous = draftRoot;
      scopeVersions.clear();
      draftRoot = root; destinationPageId = share.pageId; pageIds = []; presets = false; grantedPresets = false; intentVersion = untrack(() => intentVersion) + 1;
      if (previous) void helperRevokeNotebook(previous).catch(problem => error = codexProblem(problem));
    }
  });
  // "Ask AI about this page" adds the page to the tray like any attachment, Python pages included.
  $effect(() => { if (summary) { const id = summary.pageId; untrack(() => { if (!pageIds.includes(id) && pageIds.length < 8) pageIds = [...pageIds, id]; if (!pages.find(p => p.id === id)?.calculation) destinationPageId = id; intentVersion++; }); } });
  let suggestionVersion = $state(0);
  function suggest(key: "codex.suggestSummaryPrompt" | "codex.suggestExplainPrompt" | "codex.suggestCheckPrompt") { initialPrompt = $t(key); suggestionVersion++; }
  $effect(() => { if (entry) { destinationPageId = share.pageId; pageIds = entry.pageId ? [entry.pageId] : []; initialPrompt = ""; } });
  let cells = $state<import("../ipc/helper").CellGrantIntent>();
  $effect(() => { cells = cellEntry?.grant; if (cellEntry) destinationPageId = cellEntry.pageId; intentVersion = untrack(() => intentVersion) + 1; });
  const pixelsOff = $derived(defaults.usage === "low" && (needsPixels || pageIds.some(id => { const page = pages.find(p => p.id === id)?.snapshot; return page && helperSelectionNeedsPixels([...page.page.objects.map(o => o.type), ...(page.inkLayers.some(l => l.strokes.length) ? ["ink_group"] : [])], page.page.background.kind === "pdf", false); })));
  function updateCells(next: typeof cells) { if (next?.cellIds.length) { try { validateCellContext(next); } catch { error = notice("helper.cell_grant_denied"); return; } } cells = next; intentVersion++; if (conversationId && requestId) void helperRevokeAttachment(conversationId, requestId, "@cells").catch(problem => error = codexProblem(problem)); }
  function intent(): RequestContextIntent {
    return { sharePageNames, destination: { root: session.root, pageId: destinationPageId },
      // Removing every cell chip simply sends without cells; attached Python pages share theirs.
      cells: cells?.cellIds.length ? cells : undefined, selection: cellEntry || pages.some(page => page.id === share.pageId && page.calculation) ? null : share.root === session.root ? share : null,
      pageIds, presets, intentVersion: `${intentVersion}:${cropVersion}` };
  }
  async function togglePageNames(checked: boolean) {
    sharePageNames = checked; intentVersion++;
    if (!checked && conversationId && requestId) { try { await helperRevokeAttachment(conversationId,requestId,"@page_names"); info = notice("codex.removedContext"); } catch (problem) { error = codexProblem(problem); } }
  }
  async function togglePresets(checked: boolean) {
    presets = checked; intentVersion++;
    if (!checked && conversationId && requestId) {
      try {
        await helperRevokePresets(conversationId, requestId);
        grantedPresets = false; info = notice("codex.removedContext");
      } catch (problem) { error = codexProblem(problem); }
    }
  }
  async function togglePage(id: string) {
    if (pageIds.includes(id)) {
      pageIds = pageIds.filter(page => page !== id); intentVersion++;
      if (conversationId && requestId) {
        try { await helperRevokeAttachment(conversationId, requestId, id); info = notice("codex.removedContext"); }
        catch (problem) { error = codexProblem(problem); }
      }
    } else if (pageIds.length >= 8) attachmentNotice = notice("codex.attachmentLimit", { count: "8" });
    else { pageIds = [...pageIds, id]; intentVersion++; }
    if (pageIds.length < 8) attachmentNotice = "";
  }
  let sending = $state(false), listening = $state(false), stopping = $state(false), closing = $state(false);
  let attachmentNotice = $state<UiText>("");
  let error = $state<UiText>("");
  let info = $state<UiText>("");
  let disposed = false;
  const pointers = new Set<number>();
  const busy = $derived(sending || closing || !listening || Boolean(run && !["done", "failed", "stopped"].includes(run.state)));
  type Pending = { kind: "run"; value: CodexRunEvent } | { kind: "delta"; value: { runId: string; text: string; itemId?: string } } | { kind: "tool"; value: CodexToolEvent };
  let pending: Pending[] = [];
  let lastItem = "";
  function receive(event: Pending) {
    if (sending) { pending = [...pending, event].slice(-2000); return; }
    if (event.value.runId !== run?.runId) return;
    if (event.kind === "run") run = event.value;
    else if (event.kind === "delta") {
      // Each Codex message is its own item; keep them as separate paragraphs instead of gluing them together.
      const gap = event.value.itemId && lastItem && event.value.itemId !== lastItem && text && !text.endsWith("\n") ? "\n\n" : "";
      if (event.value.itemId) lastItem = event.value.itemId;
      text = (text + gap + event.value.text).slice(-200000);
    }
    else tools = [...tools, event.value].slice(-500);
  }
  async function send(prompt: string, retry = false) {
    if (!canSend(prompt, busy)) return false;
    const usageOverride = allowImages ? "balanced" as const : undefined;
    const captured = captureHelperIntent(intent());
    if (captured.cells) captured.cells = { ...captured.cells, cellIds: [...captured.cells.cellIds] };
    scopeVersions.clear();
    sending = true; error = ""; info = ""; pending = [];
    try {
      const reply = await contextReply("codex", () => session, () => pointers.size > 0 || Boolean(session.pageInputLocked));
      if (disposed) return false;
      if (reply.status !== "ok") {
        error = notice(reply.status === "flush_failed" ? "codex.flushFailed" : reply.status === "no_notebook" ? "codex.notebookUnavailable" : "codex.contextBusy");
        return false;
      }
      if (reply.root !== captured.destination.root) {
        error = notice("codex.destinationChanged"); return false;
      }
      if (!sameHelperIntent(captured, intent())) {
        error = notice("codex.contextChanged"); return false;
      }
      const result = await helperCodexRun({ prompt, intent: captured, model, webSearch, conversationId, ...(usageOverride ? { usageOverride } : {}) });
      conversationId = result.conversationId; requestId = result.requestId; grantedPages = [...captured.pageIds]; grantedPresets = captured.presets;
      if (captured.cells && JSON.stringify(captured.cells) !== JSON.stringify(cells)) {
        await helperRevokeAttachment(conversationId, requestId, "@cells"); info = notice("codex.removedContext");
      }
      if (captured.presets && !presets) {
        await helperRevokePresets(conversationId, requestId);
        grantedPresets = false; info = notice("codex.removedContext");
      }
      for (const id of captured.pageIds.filter(id => !pageIds.includes(id))) {
        await helperRevokeAttachment(conversationId, requestId, id);
        info = notice("codex.removedContext");
      }
      if (disposed) { await helperCodexClose(conversationId); return false; }
      followOutput = true;
      if (run && !retry) history = [...history, { prompt: lastPrompt, answer: text }];
      run = { ...result, state: "starting" };
      lastPrompt = prompt; text = ""; lastItem = "";
      tools = []; stopping = false;
      return true;
    } catch (problem) { if (!disposed) error = codexProblem(problem); return false; }
    finally {
      allowImages = false;
      sending = false;
      const queued = pending; pending = [];
      for (const event of queued) receive(event);
    }
  }
  async function retryConnection() {
    if (busy || run?.state !== "failed" || run.problem?.code !== "provider_unreachable") return;
    await send(lastPrompt, true);
  }
  async function stop() {
    if (!run || stopping) return;
    stopping = true;
    try { await helperCodexStop(run.runId); grantedPages = []; grantedPresets = false; }
    catch (problem) { error = codexProblem(problem); stopping = false; }
  }
  async function close() {
    if (sending || closing) return;
    closing = true;
    try {
      if (conversationId) await helperCodexClose(conversationId);
      conversationId = null;
      onClose();
    } catch (problem) { error = codexProblem(problem); }
    finally { closing = false; }
  }
  onMount(() => {
    const cleanups: (() => void)[] = [];
    const down = (event: PointerEvent) => { pointers.add(event.pointerId); };
    const up = (event: PointerEvent) => { setTimeout(() => pointers.delete(event.pointerId), 0); };
    const blur = () => pointers.clear();
    window.addEventListener("pointerdown", down, true);
    window.addEventListener("pointerup", up, true);
    window.addEventListener("pointercancel", up, true);
    window.addEventListener("blur", blur);
    void (async () => {
      try {
        for (const registration of [
          () => listenHelper("scope-request", value => {
            if (value.conversationId === conversationId && value.contextRequestId === requestId && value.root === session.root) { scopeVersions.set(value.requestId, intentVersion); setTimeout(() => scopeVersions.delete(value.requestId), 121000); }
          }),
          () => listenHelper("scope-granted", value => {
            // Settings set to Allow grant without a card, so no scope-request was seen for this one.
            const version = value.automatic ? intentVersion : scopeVersions.get(value.requestId); scopeVersions.delete(value.requestId);
            if (matchesScope(value, conversationId, requestId, session.root, intentVersion, version)) {
              pageIds = [...new Set([...pageIds, ...(value.pageIds ?? [])])];
              grantedPages = [...new Set([...grantedPages, ...(value.pageIds ?? [])])];
              if (value.scope.kind === "presets") { presets = true; grantedPresets = true; }
              if (value.scope.kind === "page_names") sharePageNames = true;
              intentVersion++;
            } else if (value.conversationId && value.contextRequestId) {
              // A delayed result must never re-check consent removed in the meantime.
              for (const id of value.pageIds ?? []) if (!pageIds.includes(id) || value.root !== session.root) void helperRevokeAttachment(value.conversationId, value.contextRequestId, id).catch(() => {});
              if (value.scope.kind === "presets" && (!presets || value.root !== session.root)) void helperRevokePresets(value.conversationId, value.contextRequestId).catch(() => {});
              if (value.scope.kind === "page_names" && (!sharePageNames || value.root !== session.root)) void helperRevokeAttachment(value.conversationId, value.contextRequestId, "@page_names").catch(() => {});
            }
          }),
          () => listenHelper("codex-run", value => receive({ kind: "run", value })),
          () => listenHelper("codex-delta", value => receive({ kind: "delta", value })),
          () => listenHelper("codex-tool", value => receive({ kind: "tool", value })),
        ]) {
          const off = await registration();
          if (disposed) { off(); return; }
          cleanups.push(off);
        }
        listening = true;
      } catch (problem) { error = codexProblem(problem); }
    })();
    return () => {
      disposed = true;
      cleanups.forEach(off => off());
      window.removeEventListener("pointerdown", down, true);
      window.removeEventListener("pointerup", up, true);
      window.removeEventListener("pointercancel", up, true);
      window.removeEventListener("blur", blur);
      if (conversationId) void helperCodexClose(conversationId).catch(() => {});
    };
  });
</script>

<div class="codex-workspace" class:started={Boolean(run)}>
  <div class="body" class:empty={!run} bind:this={body} onscroll={trackScroll} role="region" aria-label={$t("codex.conversation")}>
  {#each history as message}
    <p class="request">{message.prompt}</p>
    <div class="answer">{message.answer}</div>
  {/each}
  {#if run}
    <p class="request">{lastPrompt}</p>
    <CodexRunPanel state={run.state} {text} {tools} problem={run.problem} {stopping} onStop={stop} onRetry={retryConnection} retryDisabled={busy} onClose={close} />
    <div bind:this={responseEnd} aria-hidden="true"></div>
  {:else}
    <div class="intro-heading"><h3>{$t("codex.askHeading")}</h3></div>
    <p class="intro">{$t(cellEntry ? "cellAI.hint" : intent().selection ? "codex.askHint" : "codex.pageOnlyHint")}</p>
    <div class="suggestions" role="group" aria-label={$t("codex.suggestions")}>
      <button type="button" onclick={() => suggest("codex.suggestSummaryPrompt")}>{$t("codex.suggestSummary")}</button>
      <button type="button" onclick={() => suggest("codex.suggestExplainPrompt")}>{$t("codex.suggestExplain")}</button>
      <button type="button" onclick={() => suggest("codex.suggestCheckPrompt")}>{$t("codex.suggestCheck")}</button>
    </div>
  {/if}
  {#if review}{@render review((proposal, text) => sendProposalRevision(proposal, text, send), busy)}{/if}
  {#if info}<p class="context-note" role="status">{renderMessage(info, $t)}</p>{/if}
  </div>
  <div class="composer">
  {#if error}<div class="error" role="alert"><svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9"/><path d="M12 7v6m0 3h.01"/></svg><p>{renderMessage(error, $t)}</p><button class="dismiss" type="button" aria-label={$t("codex.dismissError")} onclick={() => error = ""}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18"/></svg></button></div>{/if}
  <CodexComposer regionContext={Boolean(intent().selection && !share.objectIds.length && !share.strokeIds.length)} blockContext={Boolean(entry)} cellContext={Boolean(cellEntry)} disclosure={$t("codex.handoffShort")} details={$t("codex.contextHandoff", { page: formatNumber(pages.find(page => page.id === destinationPageId)?.number ?? 0), assistant: "Codex", material: $t(cells ? "newPage.mixedMaterial" : presets ? "codex.materialWithPresets" : "codex.material") })} focus={cellEntry?.focus ?? entry?.focus ?? null} focusVersion={cellEntry?.version ?? entry?.version ?? summary?.version ?? 0} promptVersion={suggestionVersion} {initialPrompt} thumbnail={intent().selection ? thumbnail : undefined} {status} selectionLabel={intent().selection ? selectionLabel : undefined} count={intent().selection ? share.objectIds.length + share.strokeIds.length : 0}
    bind:model bind:webSearch {busy} followUp={Boolean(run)} running={Boolean(run && !["done", "failed", "stopped"].includes(run.state))} {stopping} onStop={stop} onEnd={run ? close : undefined} ending={sending || closing} onSend={send} onClose={onHide}>
    {#snippet context()}
      {#if pixelsOff}<div class="image-notice"><p class="context-note" role="status">{$t(allowImages ? "codex.imagesAllowed" : "codex.imagesOff")}</p>{#if !allowImages}<button type="button" disabled={busy} onclick={() => allowImages = true}>{$t("codex.allowImages")}</button>{/if}</div>{/if}
      {#if cellEntry}
        {#if !cells?.cellIds.length}<button type="button" onclick={() => updateCells(cellEntry?.grant)}>{$t("cellAI.attachActive")}</button>{/if}
        <div class="cell-chips">{#each cells?.cellIds ?? [] as id}<button type="button" aria-label={$t("cellAI.remove", { number: cellEntry.ordered.indexOf(id) + 1 })} onclick={() => updateCells(cells ? { ...cells, cellIds: cells.cellIds.filter(c => c !== id) } : undefined)}>{$t("calculation.cell", { number: cellEntry.ordered.indexOf(id) + 1 })}<svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18" /></svg></button>{/each}</div>
        <button class="cells-above" type="button" disabled={!cells?.cellIds.length || cellEntry.ordered.indexOf(cells.cellIds[cells.cellIds.length - 1]) >= 100} onclick={() => cells && updateCells(cellsAbove(cells, cellEntry!.ordered))}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>{$t("cellAI.above")}</button>
        <label class="presets-consent"><input type="checkbox" checked={cells?.includeOutputs ?? true} onchange={e => cells && updateCells({ ...cells, includeOutputs: e.currentTarget.checked })} />{$t("cellAI.outputs")}</label>

      {/if}
      <fieldset>
      <PageAttachments {pages} selected={pageIds} currentPageId={share.pageId} onToggle={id => void togglePage(id)} />
      {#if presets}<button class="shared-chip" type="button" aria-label={$t("codex.removeShared", { item: $t("codex.presetsChip") })} title={$t("codex.removeShared", { item: $t("codex.presetsChip") })} onclick={() => void togglePresets(false)}>{$t("codex.presetsChip")}<svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18" /></svg></button>{/if}
      {#if sharePageNames}<button class="shared-chip" type="button" aria-label={$t("codex.removeShared", { item: $t("codex.pageNamesChip") })} title={$t("codex.removeShared", { item: $t("codex.pageNamesChip") })} onclick={() => void togglePageNames(false)}>{$t("codex.pageNamesChip")}<svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18" /></svg></button>{/if}
      </fieldset>
      {#if attachmentNotice}<p class="context-note" role="status" aria-live="polite">{renderMessage(attachmentNotice, $t)}</p>{/if}
    {/snippet}
  </CodexComposer>
  </div>
</div>

<style>
  .suggestions { display: flex; flex-wrap: wrap; gap: 6px; margin: 0 0 12px; }
  .suggestions button { min-height: var(--control-dense); padding: 0 10px; border: 1px solid var(--edge); background: transparent; color: var(--text); font-size: var(--text-sm); font-weight: 400; }
  .suggestions button:hover { background: var(--wash); }
  fieldset { display: contents; border: 0; padding: 0; margin: 0; min-width: 0; }
  .cells-above:hover:not(:disabled) { background: var(--wash); color: var(--text); }
  .cells-above { background: transparent; color: var(--muted); display: inline-flex; align-items: center; gap: 4px; font-weight: 400; justify-self: start; padding: 4px 8px; min-height: var(--control-dense); font-size: var(--text-sm); }
  .cell-chips svg { width: var(--icon-dense); height: var(--icon-dense); fill: none; stroke: currentColor; stroke-width: var(--stroke-dense); stroke-linecap: round; }
  .cell-chips button { display: inline-flex; align-items: center; gap: 6px; border: 1px solid var(--edge); min-height: var(--control-dense); padding: 0 6px; font-size: var(--text-sm); background: transparent; color: var(--muted); }
  .cell-chips button:hover:not(:disabled) { background: var(--wash); color: var(--text); }
  .cells-above svg { width: var(--icon-dense); height: var(--icon-dense); fill: none; stroke: currentColor; stroke-width: var(--stroke-dense); }
  .cell-chips { display: contents; }
  button { min-height: var(--control); padding: 8px 12px; color: var(--text); background: var(--wash); border: 0; border-radius: var(--radius); cursor: pointer; }
  button:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }
  button:hover:not(:disabled) { background: var(--wash-strong); }
  .codex-workspace { overflow: hidden; display: flex; min-height: 0; flex: 1; flex-direction: column; }
  .body { position: relative; min-height: 0; flex: 1; overflow-y: auto; overscroll-behavior: contain; padding: 16px; }
  .body.empty { flex: 0 1 auto; overflow: auto; padding-bottom: 0; }
  .composer { flex: none; padding: 12px 16px; border-top: 1px solid var(--edge); background: var(--surround); }
  .codex-workspace:not(.started) .composer { border-top: 0; background: transparent; padding-top: 4px; }
  h3 { margin: 0; font-size: var(--text-md); font-weight: 600; }
  .intro { margin: 0 0 12px; color: var(--muted); font-size: var(--text-sm); line-height: 1.6; }
  .request { margin: 0 0 12px; padding: 8px 10px; border-radius: var(--radius); background: var(--wash); overflow-wrap: anywhere; font-weight: 400; }
  .answer { margin-bottom: 16px; white-space: pre-wrap; overflow-wrap: anywhere; font-weight: 400; line-height: 1.65; user-select: text; }
  .shared-chip { display: inline-flex; align-items: center; gap: 4px; min-height: var(--control-dense); padding: 0 6px; border: 1px solid var(--edge); background: transparent; color: var(--text); font-size: var(--text-sm); }
  .shared-chip svg { width: var(--icon-dense); height: var(--icon-dense); fill: none; stroke: currentColor; stroke-width: var(--stroke-dense); stroke-linecap: round; }
  .shared-chip:hover:not(:disabled) { background: var(--wash); }
  .presets-consent { width: 100%; display: flex; align-items: center; gap: 8px; min-height: var(--control-dense); color: var(--muted); font-size: var(--text-sm); cursor: pointer; }
  .presets-consent input { accent-color: var(--blueprint); }
  .presets-consent input:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }
  @media (pointer: coarse) { .presets-consent { min-height: var(--control-touch); } }
  .context-note { margin: 6px 0; color: var(--muted); font-size: var(--text-sm); line-height: 1.5; }
  .intro-heading { display: flex; align-items: baseline; flex-wrap: wrap; gap: 6px 12px; margin-bottom: 6px; }
  .error { display: flex; align-items: flex-start; gap: 6px; margin-bottom: 8px; padding: 8px; border: 1px solid var(--edge); border-radius: var(--radius); color: var(--muted); font-size: var(--text-sm); line-height: 1.5; overflow-wrap: anywhere; }
  .error p { margin: 0; flex: 1; }
  .error svg { width: var(--icon-dense); height: var(--icon-dense); flex: none; fill: none; stroke: var(--oxide); stroke-width: var(--stroke-dense); stroke-linecap: round; }
  .error .dismiss { display: grid; place-items: center; min-height: var(--control-dense); min-width: var(--control-dense); margin: -4px; padding: 4px; background: transparent; }
  .dismiss svg { stroke: var(--muted); }
  .image-notice { padding: 6px 8px; border: 1px solid var(--edge); border-radius: var(--radius); }
  .image-notice .context-note { margin: 0; }
  .image-notice button { min-height: var(--control-dense); padding: 0; color: var(--blueprint-light); background: transparent; font-size: var(--text-sm); text-align: left; }
  .image-notice button:hover { background: transparent; text-decoration: underline; }
  .dismiss:hover { background: var(--wash); }
  button:disabled { cursor: not-allowed; opacity: var(--disabled); }
  @media (max-height: 500px) { .composer { padding: 8px 16px; } }
  @media (pointer: coarse) { button { min-height: var(--control-touch); } }
</style>
