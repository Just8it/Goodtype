<script lang="ts">
  import ProposalTray from "./ProposalTray.svelte";
  import { retainSessionProposals } from "./proposal";
  import ProposalCard from "./ProposalCard.svelte";
  import { helperProposals, type Proposal } from "../ipc/helper";
  import { onMount, tick, type Snippet } from "svelte";
  import { answerScope } from "./scope";
  import { helper, proposalPreview, codexStatus } from "./state";
  import { contextReply, type ContextSession } from "./context";
  import { helperCodexStatus, helperStatus, helperActivity, helperContextReply, helperAnswerConnection, helperRevoke, listenHelper, type SessionInfo, type ScopeRequest, type ActivityEntry } from "../ipc/helper";
  import { t, formatNumber, formatDate } from "../i18n";
  type Review = Snippet<[((proposal: Proposal, text: string) => Promise<void>) | undefined, boolean]>;
  let { session, pages = [], activityOpen = $bindable(false), panelOpen = $bindable(false), hasConversation = false, conversation, available, onError }: { pages?: {id:string;number?:number;title?:string}[]; session: ContextSession & { readonly pageInputLocked?: boolean; readonly pageEntries?: {id:string}[]; insertHelperProposal:(p:Proposal)=>Promise<void>; goToPageNumber?: (number: number) => Promise<void> }; activityOpen?: boolean; panelOpen?: boolean; hasConversation?: boolean; conversation?: Snippet<[Review]>; available: boolean; onError: () => void } = $props();
  let proposals=$state<Proposal[]>([]), selected=$state<string|null>(null), discarded=$state(false);
  const reviewed=$derived(proposals.find(p=>p.id===selected));
  $effect(()=>{proposalPreview.set(reviewed && ["pending", "change_requested"].includes(reviewed.status) && reviewed.kind !== "preset_install" && !reviewed.edit && !reviewed.newPage?reviewed:null);return()=>proposalPreview.set(null);});
  let applied = $state(false);
  function updateProposal(p:Proposal){proposals=[...proposals.filter(v=>v.id!==p.id),p]; if(p.status === "pending") { selected=p.id; applied=false; panelOpen=true; }}
  function hidePanel() { denyInApp(); panelOpen=false; restore(document.querySelector<HTMLElement>(".workspace-app")); }
  let connections = $state<SessionInfo[]>([]), scopes = $state<ScopeRequest[]>([]), entries = $state<ActivityEntry[]>([]);
  function denyInApp() {
    const pending = scopes.filter(s => s.conversationId);
    if (!pending.length) return;
    scopes = scopes.filter(s => !s.conversationId);
    for (const item of pending) void run(() => answerScope(item.requestId, "deny"));
  }
  $effect(() => { if (!panelOpen || !available) denyInApp(); });
  let activityPanel = $state<HTMLElement>();
  let returnTo: HTMLElement | null = null;
  const activePointers = new Set<number>();
  async function insertProposal(p:Proposal){
    if(p.kind === "preset_install") throw new Error("wrong_page");
    const root=session.root,pageId=session.activePageId,version=p.version;
    for(let elapsed=0;activePointers.size||session.pageInputLocked;elapsed+=25){
      if(elapsed>=2000)throw new Error("busy");
      await new Promise(resolve=>setTimeout(resolve,25));
    }
    if(session.root!==root||session.activePageId!==pageId||p.version!==version)throw new Error("wrong_page");
    await session.insertHelperProposal(p);
    applied = true;
  }
  async function run(action: () => Promise<unknown>) { try { await action(); return true; } catch { onError(); return false; } }
  function restore(node: HTMLElement | null) { if (node?.isConnected && !activePointers.size) node.focus(); }
  async function connection(allow: boolean) {
    const item = connections[0]; if (!item) return;
    const origin = document.activeElement as HTMLElement;
    if (await run(() => helperAnswerConnection(item.id, allow))) { connections = connections.filter(s => s.id !== item.id); if (origin?.closest(".helper-card")) restore(document.querySelector<HTMLElement>(".workspace-app")); }
  }
  async function scope(answer: "once" | "session" | "deny" | "request") {
    const item = scopes[0]; if (!item) return;
    const origin = document.activeElement as HTMLElement;
    if (await run(() => answerScope(item.requestId, answer))) { scopes = scopes.filter(s => s.requestId !== item.requestId); if (origin?.closest(".helper-card")) restore(document.querySelector<HTMLElement>(".workspace-app")); }
  }
  function closeActivity() { activityOpen = false; restore(returnTo?.isConnected ? returnTo : document.querySelector<HTMLElement>("[data-helper-activity-trigger]")); }
  $effect(() => {
    if (!activityOpen || !available) return;
    returnTo = document.activeElement as HTMLElement;
    entries = [];
    void run(async () => { const history = await helperActivity(); if (!activityOpen) return; const seen = new Set<string>(); entries = [...entries, ...history].filter(entry => { const key = JSON.stringify(entry); if (seen.has(key)) return false; seen.add(key); return true; }).sort((a, b) => b.at - a.at).slice(0, 500); });
    // Opened deliberately through the menu; never focus an unsolicited connection card.
    void tick().then(() => { if (activityOpen && !activePointers.size) activityPanel?.focus(); });
  });
  $effect(() => {
    if (!available || !$helper?.enabled) { codexStatus.set(null); return; }
    let disposed = false;
    void helperCodexStatus().then(value => { if (!disposed) codexStatus.set(value); }).catch(() => { if (!disposed) codexStatus.set(null); });
    return () => { disposed = true; };
  });
  onMount(() => {
    if (!available) return;
    let disposed = false;
    const cleanups: (() => void)[] = [];
    const add = async (registration: Promise<() => void>) => { try { const off = await registration; if (disposed) off(); else cleanups.push(off); } catch { if (!disposed) onError(); } };
    const down = (event: PointerEvent) => { activePointers.add(event.pointerId); };
    const up = (event: PointerEvent) => { setTimeout(() => activePointers.delete(event.pointerId), 0); };
    const blur = () => activePointers.clear();
    const visibility = () => { if (document.hidden) denyInApp(); };
    document.addEventListener("visibilitychange", visibility);
    const key = (event: KeyboardEvent) => { if (event.key !== "Escape") return; if (connections.length) { void connection(false); } else if (scopes.length) { void scope("deny"); } else if (activityOpen) closeActivity(); else return; event.preventDefault(); event.stopPropagation(); };
    window.addEventListener("pointerdown", down, true); window.addEventListener("pointerup", up, true); window.addEventListener("pointercancel", up, true); window.addEventListener("blur", blur); window.addEventListener("keydown", key, true);
    void add(listenHelper("status", value => { helper.set(value); const retained = retainSessionProposals(proposals, value.sessions); proposals = retained.proposals; discarded ||= retained.discarded; connections = connections.filter(s => value.sessions.some(v => v.id === s.id && !v.approved)); scopes = scopes.filter(s => value.sessions.some(v => v.id === s.sessionId && v.approved)); }));
    void add(listenHelper("connection-request", value => { if (!connections.some(s => s.id === value.id)) connections = [...connections, value]; }));
    void add(listenHelper("scope-expired", value => { const focused = document.activeElement?.closest(".helper-card"); scopes = scopes.filter(s => s.requestId !== value.requestId); if (focused && !scopes.length) restore(document.querySelector<HTMLElement>(".workspace-app")); }));
    void add(listenHelper("scope-request", value => { if (value.conversationId && (!panelOpen || document.hidden || connections.length > 0)) { void run(() => answerScope(value.requestId, "deny")); return; } scopes = [...scopes, value]; }));
    void add(listenHelper("proposal", updateProposal));
    void run(async()=>{const found=await helperProposals();if(!disposed)proposals=found;});
    void add(listenHelper("activity", value => { if (activityOpen) entries = [value, ...entries].slice(0, 500); }));
    void add(listenHelper("context-request", value => { void run(async () => { await helperContextReply(await contextReply(value.requestId, () => session, () => activePointers.size > 0 || Boolean(session.pageInputLocked))); }); }));
    void run(async () => { const value = await helperStatus(); if (!disposed) { helper.set(value); connections = value.sessions.filter(s => !s.approved); } });
    return () => { disposed = true; denyInApp(); document.removeEventListener("visibilitychange", visibility); cleanups.forEach(off => off()); window.removeEventListener("pointerdown", down, true); window.removeEventListener("pointerup", up, true); window.removeEventListener("pointercancel", up, true); window.removeEventListener("blur", blur); window.removeEventListener("keydown", key, true); };
  });
</script>
{#if connections[0]}
  <aside class="helper-card" aria-label={$t("helper.title")} data-preserve-selection>
    <p>{$t("helper.connection", { name: connections[0].clientName })}</p><p>{$t("helper.disclosure")}</p>
    <button type="button" onclick={() => connection(true)}>{$t("helper.allow")}</button><button type="button" onclick={() => connection(false)}>{$t("helper.deny")}</button>
  </aside>
{:else if scopes[0]}
  {@const request = scopes[0]}
  <aside class="helper-card" aria-label={$t("helper.title")} data-preserve-selection>
    <p>{$t("helper.scope", { name: request.clientName, scope: request.scope.kind === "notebook" ? $t("helper.notebook") : request.scope.kind === "presets" ? $t("helper.presetsScope") : request.scope.kind === "page_names" ? $t("helper.pageNamesScope") : $t("helper.pages", { pages: request.scope.pages.map(n => formatNumber(n)).join(", ") }) })}</p>
    {#if request.scope.kind === "presets"}<ul>{#each request.targets ?? [] as path}<li>{path}</li>{/each}</ul>{/if}
    <p>{$t("helper.scopeDisclosure", { name: request.clientName })}</p>
    <p>{$t("helper.scopeReason", { reason: request.reason })}</p><p>{$t("helper.disclosure")}</p>
    {#if request.conversationId}<p>{$t("helper.requestScopeNotice")}</p><button type="button" onclick={() => scope("request")}>{$t("helper.allowRequest")}</button>{:else}<button type="button" onclick={() => scope("once")}>{$t("helper.once")}</button><button type="button" onclick={() => scope("session")}>{$t("helper.session")}</button>{/if}<button type="button" onclick={() => scope("deny")}>{$t("helper.deny")}</button>
  </aside>
{/if}
{#if activityOpen && $helper?.enabled}
  <aside bind:this={activityPanel} class="helper-activity" tabindex="-1" aria-label={$t("helper.activity")} data-preserve-selection onwheel={event => event.stopPropagation()}>
    <header>
      <h2>{$t("helper.activity")}</h2>
      <button class="chrome" type="button" aria-label={$t("helper.close")} title={$t("helper.close")} onclick={closeActivity}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18" /></svg>
      </button>
    </header>
    <div class="activity-body">
    <p class="hint">{$t("helper.honesty")}</p><p class="hint">{$t("helper.recall")}</p>
    {#each $helper?.sessions ?? [] as client (client.id)}<div class="client"><p>{$t("helper.names", { names: client.clientName })}</p><button type="button" onclick={() => run(() => helperRevoke(client.id))}>{$t("helper.revoke")}</button></div>{/each}
    {#each entries as entry}<article><time>{formatDate(entry.at)}</time><p>{$t("helper.entry", { client: entry.clientName, tool: entry.tool, grant: $t(`helper.grant_${entry.grant}`), outcome: $t(`helper.outcome_${entry.outcome}`) })}</p><p>{$t("helper.handed", { bytes: formatNumber(entry.handedOver.sourceBytes), strokes: formatNumber(entry.handedOver.strokes), images: formatNumber(entry.handedOver.images.length) })}</p>{#each entry.handedOver.images as image}<p>{formatNumber(image.width)} × {formatNumber(image.height)}</p>{/each}{#if entry.detailCode}<p>{$t(`helper.detail_${entry.detailCode}`)}</p>{/if}</article>{:else}<p>{$t("helper.empty")}</p>{/each}
    </div>
  </aside>
{/if}
{#snippet review(requestChange: ((proposal: Proposal, text: string) => Promise<void>) | undefined, requestBusy: boolean)}
  <ProposalTray {proposals} {discarded} selected={selected} onOpen={id => selected = id} onClearNotice={() => discarded = false} />
  {#if reviewed && ["pending","stale","change_requested"].includes(reviewed.status)}
    {#key reviewed.id+reviewed.version}<ProposalCard {pages} proposal={reviewed} onGoTo={p => { if (session.root === p.root && p.pageNumber) void session.goToPageNumber?.(p.pageNumber); }} onInsert={insertProposal} onClose={()=>selected=null} onUpdate={updateProposal} onRequestChange={requestChange} {requestBusy} away={session.root!==reviewed.root||(!reviewed.newPage && reviewed.kind !== "preset_install" && session.activePageId!==reviewed.pageId)}/>{/key}
  {/if}
  {#if applied}<p class="applied" role="status">{$t("proposal.applied")}</p>{/if}
{/snippet}
<aside class="ai-panel" hidden={!panelOpen || !$helper?.enabled} aria-label={$t("codex.panelTitle")} data-preserve-selection onwheel={event => event.stopPropagation()}>
  <header>
    <h2>{$t("codex.panelTitle")}</h2>
    <button class="chrome" type="button" aria-label={$t("codex.details")} title={$t("codex.details")} onclick={() => activityOpen = true}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="8.5" /><path d="M12 11v5.5M12 7.5h.01" /></svg>
    </button>
    <button class="chrome" type="button" aria-label={$t("codex.hidePanel")} title={$t("codex.hidePanel")} onclick={hidePanel}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18" /></svg>
    </button>
  </header>
  {#if hasConversation && conversation}
    {@render conversation(review)}
  {:else}
    <div class="panel-body">{@render review(undefined, false)}{#if !reviewed}<p class="hint">{$t("codex.selectHint")}</p>{/if}</div>
  {/if}
</aside>
{#if $helper?.enabled && !panelOpen && (hasConversation || proposals.some(p => p.status === "pending" || p.status === "stale") || discarded)}
  <button class="reopen-ai" type="button" data-preserve-selection onclick={() => panelOpen=true}>{$t("codex.openPanel")}</button>
{/if}
<style>

.helper-card, .helper-activity { position: fixed; z-index: 70; top: 64px; right: 16px; width: 420px; max-width: calc(100vw - 32px); max-height: calc(100dvh - 90px); overflow: auto; padding: 16px; border: 1px solid var(--edge); border-radius: var(--radius-lg); background: var(--panel); color: var(--text); font-size: var(--text-md); line-height: 1.5; box-shadow: var(--shadow-panel); overflow-wrap: anywhere; }
.helper-activity { z-index: 69; display: flex; flex-direction: column; padding: 0; overflow: hidden; }
header { display: flex; flex: none; align-items: center; gap: 8px; padding: 4px 6px 4px 12px; min-height: var(--control); border-bottom: 1px solid var(--edge-soft); }
h2 { flex: 1; margin: 0; font-size: var(--text-md); font-weight: 600; }
.activity-body { min-height: 0; padding: 12px; overflow-y: auto; overscroll-behavior: contain; }
p { margin: 0 0 10px; }
.hint, time { color: var(--muted); font-size: var(--text-sm); }
.client { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 12px 0; border-top: 1px solid var(--edge-soft); }
.client p { min-width: 0; margin: 0; }
button { min-height: var(--control); padding: 8px 12px; border: 0; border-radius: var(--radius); background: var(--wash); color: var(--text); cursor: pointer; }
button:hover { background: var(--wash-strong); }
.chrome { display: grid; width: var(--control-dense); min-height: var(--control-dense); padding: 0; background: transparent; color: var(--muted); place-items: center; }
.chrome:hover { background: var(--wash); color: var(--text); }
.chrome svg { width: var(--icon-dense); height: var(--icon-dense); fill: none; stroke: currentColor; stroke-width: var(--stroke-dense); stroke-linecap: round; }
button:focus-visible, aside:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }
article { padding: 12px 0 2px; border-top: 1px solid var(--edge-soft); }
time { display: block; margin-bottom: 6px; font-variant-numeric: tabular-nums; }
@media (pointer: coarse) { button, .chrome { min-height: var(--control-touch); } .chrome { width: var(--control-touch); } }
.ai-panel { position: fixed; z-index: 45; top: 58px; right: 0; bottom: 34px; display: flex; flex-direction: column; width: min(400px, 100vw); background: var(--panel); color: var(--text); border-left: 1px solid var(--edge); font-size: var(--text-md); line-height: 1.5; }
.ai-panel[hidden] { display: none; }
.ai-panel header { min-height: 44px; padding-left: 16px; }
.panel-body { min-height: 0; flex: 1; overflow-y: auto; padding: 16px; }
.reopen-ai { position: fixed; z-index: 44; right: 12px; bottom: 46px; background: var(--panel); border: 1px solid var(--edge); box-shadow: var(--shadow-menu); }
.reopen-ai:hover { background: var(--panel); border-color: var(--edge-strong); }
.applied { color: var(--sage-text); font-size: var(--text-sm); }
</style>
