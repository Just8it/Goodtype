<script lang="ts">
  import { helperApplyCellProposal, cellProblemKeys } from "../ipc/helper";
  import { applyAndRun, cellLabel } from "../calculation/cellIntent";
  import { untrack } from "svelte";
  import { t } from "../i18n";
  import { helperInstallPreset, helperEditProposal, helperDismissProposal, helperRequestChange, type Proposal } from "../ipc/helper";
  import { presetError, applyBlocked, compileDiagnostics, lineDiff, reviewShortcut, reviewState } from "./proposal";

  let { proposal, onInsert, onClose, onUpdate, onRequestChange, requestBusy = false, away = false, onGoTo, pages = [] }: {
    proposal: Proposal;
    pages?: {id:string;number?:number;title?:string}[];
    onInsert: (proposal: Proposal) => Promise<void>;
    onClose: () => void;
    onUpdate: (proposal: Proposal) => void;
    onRequestChange?: (proposal: Proposal, text: string) => Promise<void>;
    onGoTo?: (proposal: Proposal) => void;
    requestBusy?: boolean;
    /** The reviewer is on another page than the proposal's destination. */
    away?: boolean;
  } = $props();
  // Page text reviews an existing PageTypst at full-page geometry; its header stays read-only.
  const preset = $derived(proposal.kind === "preset_install" ? proposal.preset : undefined);
  const newPage = $derived(proposal.newPage);
  const appendTarget = $derived(pages.find(p => p.id === newPage?.targetPageId));
  let locationKind = $state(untrack(() => proposal.newPage?.position.kind ?? "after"));
  let locationId = $state(untrack(() => proposal.newPage && "pageId" in proposal.newPage.position ? proposal.newPage.position.pageId : proposal.pageId));
  const final = $derived(proposal.status !== "pending");
  const pageText = $derived(proposal.kind === "page_text_edit");
  const ui = untrack(() => reviewState(proposal.id));
  let sources = $state(untrack(() => proposal.newPage ? proposal.newPage.pageType === "python" ? proposal.newPage.cells.map(c => c.source) : [proposal.newPage.source] : proposal.cell ? [proposal.cell.newSource] : proposal.preset ? [proposal.preset.source] : proposal.items.map(item => item.source)));
  let busy = $state(false);
  let changing = $state(false);
  let text = $state("");
  let error = $state("");
  let code = $state("");
  let sourceOpen = $state(ui.source);
  // An overflowing page opens whole: the cropped view would hide exactly what overflows.
  let whole = $state(ui.whole || untrack(() => Boolean(proposal.overflow)));
  let preview = $state<"before" | "after">("after");
  const dirty = $derived(sources.some((source, index) => source !== (newPage ? newPage.pageType === "python" ? newPage.cells[index]?.source : newPage.source : proposal.cell ? proposal.cell.newSource : preset ? preset.source : proposal.items[index]?.source)));
  const stale = $derived(proposal.status === "stale" || ["proposal_stale", "cell_proposal_stale", "cell_proposal_duplicate", "cell_proposal_revoked", "cell_proposal_changed"].includes(code));
  const diagnostics = $derived(code === "compile_failed" ? compileDiagnostics(error) : null);
  const page = $derived(proposal.pageNumber || proposal.pageId);
  const blocked = $derived(applyBlocked(proposal, { busy, dirty, stale, away }));

  async function run(action: () => Promise<unknown>) {
    busy = true;
    error = "";
    code = "";
    try {
      await action();
    } catch (e) {
      const failure = e as { code?: string; details?: string };
      code = failure.code ?? (e instanceof Error ? e.message : typeof e === "string" ? e : "");
      if (["proposal changed", "proposal is final", "proposal session ended", "session ended", "scope changed"].includes(code)) code = "proposal_changed";
      if (code === "edited proposal no longer fits page") code = "does_not_fit";
      error = failure.details ?? String(e);
      if (compileDiagnostics(error)) code = "compile_failed";
    } finally {
      busy = false;
    }
  }
  async function recompile() {
    const updated = await helperEditProposal(proposal.id, proposal.version, sources);
    onUpdate(updated);
  }
  async function insert(andRun = false) {
    if (busy || proposal.status !== "pending" || stale) return;
    if (newPage) {
      if (dirty) return run(recompile);
      if (blocked) return;
      const position: import("../model").PagePosition = locationKind === "last" ? {kind:"last"} : {kind:locationKind as "before"|"after",pageId:locationId};
      await run(async () => { await onInsert({...proposal,newPage:{...newPage,position}}); onClose(); }); return;
    }
    if (preset) {
      if (blocked) return;
      const captured = { proposalId: proposal.id, proposalVersion: proposal.version, candidateHash: preset.candidateHash, sessionId: proposal.sessionId, root: proposal.root };
      const reviewed = proposal;
      await run(async () => {
        const result = await helperInstallPreset(captured);
        onUpdate({ ...reviewed, status: result.status });
      });
      return;
    }
    if (proposal.cell) {
      if (away) return;
      await run(async () => {
        const reviewed = dirty ? await helperEditProposal(proposal.id, proposal.version, sources) : proposal;
        await applyAndRun(reviewed, andRun, () => helperApplyCellProposal(reviewed.id, reviewed.version));
        onUpdate({ ...reviewed, status: "inserted" }); onClose();
      });
      return;
    }
    if (dirty) return run(recompile);
    if (blocked) return;
    await run(async () => {
      await onInsert(proposal);
      onClose();
    });
  }
  async function requestChange(value: string) {
    await run(async () => {
      if (onRequestChange) await onRequestChange(proposal, value);
      else await helperRequestChange(proposal.id, value);
      changing = false;
    });
  }
  function keydown(event: KeyboardEvent) {
    reviewShortcut(event, insert, onClose);
  }
  function toggleWhole() {
    whole = ui.whole = !whole;
  }
  const svg = (value: string) => `data:image/svg+xml;charset=utf-8,${encodeURIComponent(value)}`;
  const failure = $derived(code === "revision_failed" ? $t("proposal.revisionFailed") : code in cellProblemKeys ? $t(cellProblemKeys[code as keyof typeof cellProblemKeys]) : preset ? $t(presetError(code + " " + error)) : code === "history_not_retained" ? $t("proposal.history")
    : code === "wrong_page" ? (pageText ? $t("proposal.targetChanged", { page }) : $t("proposal.wrongPage"))
    : code === "busy" ? $t("helper.detail_busy")
    : (code === "proposal_stale" || code === "cell_proposal_stale") ? $t("proposal.stale")
    : code === "proposal_changed" ? $t("helper.detail_changed")
    : code === "does_not_fit" ? $t("proposal.doesNotFit")
    : code === "operation_failed" ? $t("helper.detail_failed")
    : $t("proposal.failed", { details: error }));
</script>
<div class="proposal-card" role="dialog" aria-modal="false" aria-label={$t("proposal.review")}
  tabindex="-1" onkeydown={keydown} data-preserve-selection>
  <header>
    <div class="title">
      <h3>{newPage ? newPage.title : preset ? preset.name : $t("proposal.suggestedChange")}</h3>
      {#if !preset && !newPage}<p class="target">{$t(pageText ? "proposal.pageTextTarget" : "proposal.destination", { page })}</p>{/if}
    </div>
    <button class="close" type="button" aria-label={$t("proposal.close")} title={$t("proposal.close")} onclick={onClose}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18" /></svg></button>
  </header>
  {#if newPage}
    {#if newPage.targetPageId}<p class="target">{$t("newPage.appendTarget", {page: appendTarget?.number ?? (appendTarget ? pages.indexOf(appendTarget)+1 : newPage.targetPageId),title:appendTarget?.title ?? ""})}</p>
    {:else}<p class="target">{$t("newPage.reviewLocation")}</p>
    <div class="actions"><select aria-label={$t("newPage.position")} bind:value={locationKind} disabled={busy}><option value="after">{$t("newPage.after")}</option><option value="before">{$t("newPage.before")}</option><option value="last">{$t("newPage.last")}</option></select>
    {#if locationKind !== "last"}<select aria-label={$t("newPage.anchor")} bind:value={locationId} disabled={busy}>{#each pages as target,index}<option value={target.id}>{$t("codex.pageAttachment",{page:target.number ?? index+1,title:target.title ?? ""})}</option>{/each}</select>{/if}</div>{/if}
    {#if newPage.svg}<div class="preview-paper sheet whole"><img src={svg(newPage.svg)} alt={$t("newPage.preview")} /></div>{/if}
    {#if diagnostics}<p role="alert">{$t("proposal.compileFailed")}</p><pre>{error}</pre>{/if}
    {#if proposal.overflow}<p role="alert">{$t("proposal.pageOverflow")}</p>{/if}
    {#if newPage.pageType === "python"}<p class="hint">{$t("newPage.notRun")}</p>{/if}
    {#if newPage.pageType !== "blank"}{#each sources as source,index}<label>{$t(newPage.pageType === "python" ? "cellAI.source" : "proposal.source")} {newPage.pageType === "python" ? index+1 : ""}<textarea rows="6" bind:value={sources[index]} disabled={busy || final} spellcheck="false"></textarea></label>{/each}{/if}
  {/if}
  {#if preset}
    {#if preset.description}<p class="summary">{preset.description}</p>{/if}
    <p class="hint">{$t("proposal.presetKind")} · {$t(preset.check.form === "function" ? "proposal.presetFunction" : "proposal.presetPreamble")}</p>
    <p class="hint">{$t("proposal.presetDestination", { root: proposal.root })}</p>
    {#if away}<p role="status">{$t("proposal.presetWrongNotebook")}</p>{/if}
    {#if preset.sourceUrl}<p class="hint">{$t("proposal.presetProvenance", { url: preset.sourceUrl })}</p>{/if}
    <div class="preview-paper"><img src={`data:image/png;base64,${preset.png}`} alt={$t("proposal.presetSample")} /></div>
    {#if preset.check.diagnostics.length}
      <div class="hint">{#each preset.check.diagnostics as diagnostic}<p>{diagnostic.message}</p>{/each}</div>
      <details><summary>{$t("proposal.presetDiagnostics")}</summary><pre>{JSON.stringify(preset.check.diagnostics, null, 2)}</pre></details>
    {/if}
    <details bind:open={sourceOpen} ontoggle={() => ui.source = sourceOpen}>
      <summary>{$t("proposal.source")}</summary>
      <textarea aria-label={$t("proposal.source")} bind:value={sources[0]} disabled={busy || final} rows="6" spellcheck="false"></textarea>
      {#if dirty}<div class="update"><p role="status">{$t("proposal.previewOutdated")}</p><button disabled={busy || final} onclick={() => run(recompile)}>{$t("proposal.compile")}</button></div>{/if}
    </details>
    {#if diagnostics}<details open><summary>{$t("proposal.compileFailed")}</summary><pre>{error}</pre></details>{/if}
    {#if proposal.status === "installed" || proposal.status === "already_installed"}
      <p role="status">{$t(proposal.status === "installed" ? "proposal.presetInstalled" : "proposal.presetAlreadyInstalled")}</p>
      <p class="hint">{$t("proposal.presetMenuHint")}</p>
    {/if}
  {/if}
  {#if proposal.cell}
    <p class="hint">{$t(proposal.kind === "cell_insert" ? "cellAI.insertPosition" : "cellAI.editTarget", { cell: cellLabel(proposal.kind === "cell_insert" ? proposal.cell.afterCellId ?? proposal.cell.cellId : proposal.cell.cellId) })}</p>
    <pre class="diff">{#each lineDiff(proposal.cell.oldSource ?? "", sources[0]) as line}<span class={line.kind}><span class="mark" aria-hidden="true">{line.kind === "add" ? "+" : line.kind === "del" ? "-" : " "}</span>{line.text}{"\n"}</span>{/each}</pre>
    <details><summary>{$t("cellAI.source")}</summary><textarea aria-label={$t("cellAI.source")} bind:value={sources[0]} rows="8" spellcheck="false" disabled={busy || final}></textarea></details>
    <p class="hint">{$t(proposal.kind === "cell_edit" ? "cellAI.savedEditOutputs" : "cellAI.savedOutputs")}</p>
  {/if}
  {#each proposal.items as item, index}
    {#if proposal.edit}
      <div class="preview-tabs" role="group" aria-label={$t("proposal.compare")}>
        <button class:chosen={preview === "before"} aria-pressed={preview === "before"} onclick={() => preview = "before"}>{$t("proposal.before")}</button>
        <button class:chosen={preview === "after"} aria-pressed={preview === "after"} onclick={() => preview = "after"}>{$t("proposal.after")}</button>
        {#if pageText}
          <button class="chrome" type="button" aria-pressed={whole} aria-label={$t(whole ? "proposal.reduce" : "proposal.enlarge")} title={$t(whole ? "proposal.reduce" : "proposal.enlarge")} onclick={toggleWhole}>
            <svg viewBox="0 0 24 24" aria-hidden="true">
              {#if whole}<path d="M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7" />{:else}<path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7" />{/if}
            </svg>
          </button>
        {/if}
      </div>
    {/if}
    <div class="preview-paper" class:sheet={pageText} class:whole class:checking={busy && dirty}>
      {#if proposal.edit && preview === "before"}
        {#if item.beforeSvg}<img src={svg(item.beforeSvg)} alt={pageText ? $t("proposal.pageTextPreview", { state: $t("proposal.before"), page }) : $t("proposal.before")} />{:else}<pre>{item.beforeSource}</pre>{/if}
      {:else}
        <img src={svg(item.svg)} alt={pageText ? $t("proposal.pageTextPreview", { state: $t("proposal.after"), page }) : $t("proposal.after")} />
      {/if}
    </div>
    {#if pageText && proposal.overflow}<p class="notice" role="alert">{$t("proposal.pageOverflow")}</p>{/if}
    <details bind:open={sourceOpen} ontoggle={() => ui.source = sourceOpen}>
      <summary>{$t("proposal.source")}</summary>
      {#if proposal.edit}
        <pre class="diff">{#each lineDiff(item.beforeSource ?? "", sources[index]) as line}<span class={line.kind}><span class="mark" aria-hidden="true">{line.kind === "add" ? "+" : line.kind === "del" ? "−" : " "}</span>{line.text}{"\n"}</span>{/each}</pre>
      {/if}
      {#if pageText && proposal.managedHeader}
        <p class="hint">{$t("proposal.managedHeader")}</p>
        <pre class="header">{proposal.managedHeader.replace(/\r?\n$/, "")}</pre>
      {/if}
      <textarea aria-label={pageText ? $t("proposal.pageTextBody") : $t("proposal.source")} bind:value={sources[index]} rows={pageText ? 8 : 5} spellcheck="false"></textarea>
      {#if pageText && (dirty || diagnostics)}
        <div class="update">
          {#if dirty}<p role="status">{busy ? $t("proposal.checkingSource") : $t("proposal.previewOutdated")}</p>{/if}
          {#if dirty}<button type="button" disabled={busy} onclick={() => run(recompile)}>{$t("proposal.compile")}</button>{/if}
        </div>
      {/if}
      {#if diagnostics}
        <div class="diagnostics" role="alert">
          <p>{$t("proposal.compileFailed")}</p>
          <ul>
            {#each diagnostics as d}<li>{d.file ? $t("proposal.diagnosticFile", { file: d.file, message: d.message }) : d.line ? $t("proposal.diagnosticLine", { line: d.line, message: d.message }) : d.message}</li>{/each}
          </ul>
        </div>
      {/if}
    </details>
  {/each}
  <p class="summary">{proposal.summary}</p>
  {#if proposal.assumptions.length}
    <h3>{$t("proposal.assumptions")}</h3>
    <ul>
      {#each proposal.assumptions as assumption}<li>{assumption}</li>{/each}
    </ul>
  {/if}
  <p class="hint">{preset ? $t("proposal.presetReviewHint") : $t("proposal.reviewHint")}</p>
  {#if stale}
    <div class="stale">
      <p role="alert">{code in cellProblemKeys ? $t(cellProblemKeys[code as keyof typeof cellProblemKeys]) : $t(proposal.cell ? "helper.cell_proposal_stale" : "proposal.stale")}</p>
      {#each proposal.changed as change}
        <p class="hint">{$t("proposal.changedObject", { dependency: $t(`proposal.dependency_${change.dependency}`), id: change.id ?? "" })}</p>
      {/each}
      <button type="button" disabled={busy || requestBusy} onclick={() => requestChange("regenerate")}>{$t("proposal.regenerate")}</button>
    </div>
  {/if}
  {#if away && !preset && !stale && proposal.status === "pending"}
    <p class="notice away" role="status">{$t("proposal.targetChanged", { page })}</p>
    {#if onGoTo}<button type="button" onclick={() => onGoTo?.(proposal)}>{$t("proposal.goToDestination")}</button>{/if}
  {/if}
  {#if error && code !== "compile_failed"}
    <p role="alert">{failure}</p>
    {#if code === "operation_failed"}<details open><summary>{$t("codex.details")}</summary><pre>{error}</pre></details>{/if}
  {/if}
  {#if changing}
    <label>
      {$t("proposal.changeText")}
      <textarea bind:value={text} maxlength="2000" rows="3"></textarea>
    </label>
    <div class="actions">
      {#each ["differentRange", "moreExplanation", "shorter"] as chip}
        <button onclick={() => text = $t(`proposal.${chip as "differentRange" | "moreExplanation" | "shorter"}`)}>{$t(`proposal.${chip as "differentRange" | "moreExplanation" | "shorter"}`)}</button>
      {/each}
    </div>
    <button disabled={busy || requestBusy || !text.trim()} onclick={() => requestChange(text)}>{$t("proposal.sendChange")}</button>
  {/if}
  <footer>
    <button disabled={busy || (Boolean(preset) && final)} onclick={() => run(() => helperDismissProposal(proposal.id))}>{$t("proposal.dismiss")}</button>
    <button disabled={busy || requestBusy || (Boolean(preset) && final)} onclick={() => changing = !changing}>{$t("proposal.change")}</button>
    {#if newPage}<button class="primary" disabled={busy || final || stale || away || Boolean(proposal.overflow && !dirty)} onclick={() => insert()}>{dirty ? $t(newPage.pageType === "python" ? "newPage.update" : "proposal.compile") : $t(newPage.targetPageId ? "newPage.append" : "newPage.create")}</button>
    {:else if proposal.cell}
      <button class="primary" disabled={busy || final || stale || away} onclick={() => insert(false)}>{$t(proposal.kind === "cell_edit" ? "proposal.apply" : "proposal.insert")}</button>
      <button disabled={busy || final || stale || away} onclick={() => insert(true)}>{$t(proposal.kind === "cell_edit" ? "cellAI.applyRun" : "cellAI.insertRun")}</button>
    {:else if preset}
      <button class="primary" disabled={blocked} onclick={() => insert()}>{$t("proposal.presetInstall")}</button>
    {:else if pageText}
      <button class="primary" disabled={blocked} onclick={() => insert()}>{$t("proposal.apply")}</button>
    {:else}
      <button class="primary" disabled={busy || proposal.status !== "pending" || stale || away} onclick={() => insert()}>
        {dirty ? $t("proposal.compile") : proposal.edit ? $t("proposal.apply") : $t("proposal.insert")}
      </button>
    {/if}
  </footer>
</div>

<style>
  .proposal-card {
    margin: 16px 0;
    padding-top: 14px;
    color: var(--text);
    border-top: 1px solid var(--edge);
    font-size: var(--text-md);
  }
  header { display: flex; align-items: flex-start; justify-content: space-between; gap: 10px; margin-bottom: 10px; }
  header h3 { margin: 0; font-weight: 600; }
  .title { display: grid; gap: 2px; min-width: 0; padding-top: 4px; }
  .target { margin: 0; color: var(--muted); font-size: var(--text-sm); }
  .close { display: grid; place-items: center; width: var(--control-dense); min-height: var(--control-dense); padding: 0; background: transparent; color: var(--muted); }
  .close svg, .chrome svg { width: var(--icon-dense); height: var(--icon-dense); fill: none; stroke: currentColor; stroke-width: var(--stroke-dense); stroke-linecap: round; stroke-linejoin: round; }
  .preview-tabs { display: flex; gap: 6px; margin-bottom: 8px; }
  .preview-tabs button { flex: 1; color: var(--muted); }
  .preview-tabs .chosen { color: var(--text); background: var(--accent-fill); }
  .preview-tabs .chrome { flex: none; display: grid; place-items: center; width: var(--control); padding: 0; }
  .preview-tabs .chrome[aria-pressed="true"] { color: var(--text); }
  .preview-paper { min-height: 64px; padding: 12px; background: var(--paper); color: var(--charcoal); border-radius: var(--radius); }
  .preview-paper pre { margin: 0; }
  /* A full page at panel width: legible text, cropped to the top until the reader asks for all. */
  .sheet { position: relative; padding: 0; overflow: hidden; max-height: min(340px, 46dvh); transition: opacity var(--fast) ease-out; }
  .sheet img { max-height: none; }
  .sheet.whole { max-height: none; }
  .sheet:not(.whole)::after { content: ""; position: absolute; inset: auto 0 0; height: 36px; background: linear-gradient(rgb(252 252 250 / 0), var(--paper)); pointer-events: none; }
  .sheet.checking { opacity: .6; }
  .sheet pre { padding: 12px; }
  img {
    display: block;
    width: 100%;
    max-height: 260px;
    object-fit: contain;
    background: transparent;
  }
  h3 {
    font-size: var(--text-sm);
    font-weight: 600;
    margin: 12px 0 6px;
  }
  li { font-weight: 400; font-size: var(--text-sm); line-height: 1.5; }
  p,li {
    overflow-wrap: anywhere;
  }
  .summary {
    font-size: var(--text-sm);
    font-weight: 400;
    line-height: 1.6;
  }
  .hint {
    color: var(--muted);
    font-size: var(--text-sm);
  }
  .notice { margin: 8px 0 0; font-size: var(--text-sm); }
  .away { color: var(--text); }
  footer,.actions {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    margin-top: 12px;
    align-items: center;
  }
  footer { margin: 12px 0 0; padding: 10px 0; background: var(--panel); border-top: 1px solid var(--edge-soft); }
  footer .primary { margin-left: auto; background: var(--blueprint); color: var(--on-accent); font-weight: 600; }
  footer .primary:hover:not(:disabled) { background: var(--blueprint-hover); }
  select { min-width: 0; max-width: 100%; min-height: var(--control); color: var(--text); background: var(--inset); border: 1px solid var(--edge); border-radius: var(--radius); font: inherit; }
  button {
    min-height: var(--control);
    padding: 8px 12px;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--text);
    cursor: pointer;
  }
  button:hover:not(:disabled) { background: var(--wash); }
  details { margin-top: 12px; color: var(--muted); font-size: var(--text-sm); }
  summary { cursor: pointer; padding: 6px 0; }
  button:disabled {
    opacity: var(--disabled);
    cursor: not-allowed;
  }
  textarea {
    width: 100%;
    box-sizing: border-box;
    background: var(--inset);
    color: var(--text);
    border: 1px solid var(--edge);
    font: inherit;
    resize: vertical;
    padding: 8px;
    border-radius: var(--radius);
    line-height: 1.5;
  }
  textarea:focus { border-color: var(--blueprint); }
  details textarea, .diff, .header { font-family: var(--font-code); font-size: var(--text-sm); }
  pre {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .diff { margin: 4px 0 10px; padding: 8px 0; border-radius: var(--radius); background: var(--inset); line-height: 1.5; }
  .diff > span { display: block; padding: 0 8px; }
  .diff .mark { display: inline-block; width: 1.4ch; user-select: none; }
  .diff .same { color: var(--muted); }
  .diff .del { color: var(--text); background: var(--oxide-wash); }
  .diff .add { color: var(--text); background: var(--accent-fill); }
  .diff .del .mark { color: var(--oxide-text); }
  .diff .add .mark { color: var(--blueprint-light); }
  .header { margin: 4px 0 8px; padding: 8px; border: 1px dashed var(--edge); border-radius: var(--radius); color: var(--muted); }
  .update { display: flex; align-items: center; justify-content: space-between; gap: 10px; margin-top: 8px; }
  .update p { margin: 0; color: var(--text); }
  .update button { flex: none; background: var(--wash); }
  .update button:hover:not(:disabled), .stale button:hover:not(:disabled) { background: var(--wash-strong); }
  .diagnostics { margin-top: 8px; color: var(--oxide-text); }
  .diagnostics p { margin: 0 0 4px; }
  .diagnostics ul { margin: 0; padding-left: 18px; font-family: var(--font-code); }
  .stale { display: grid; justify-items: start; gap: 2px; margin-top: 10px; }
  .stale p { margin: 0; }
  .stale button { margin-top: 6px; background: var(--wash); }
  [role="alert"] { color: var(--oxide-text); }
  @media (pointer: coarse) { button { min-height: var(--control-touch); } .preview-tabs .chrome { width: var(--control-touch); } }
  @media (prefers-reduced-motion: reduce) { .sheet { transition: none; } }
  button:focus-visible,textarea:focus-visible,summary:focus-visible {
    outline: 2px solid var(--blueprint-light);
    outline-offset: 2px;
  }
</style>
