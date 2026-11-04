<script lang="ts">
  import OverflowMenu from "../workspace/OverflowMenu.svelte";
  import type { MenuSection } from "../workspace/menu";
  import { placeFloatingToolbar } from "../geometry/placement";
  import { calculationPreviewUrl } from "./previewTheme";
  import { tick, untrack } from "svelte";
  import { t, locale, renderMessage } from "../i18n";
  import type { NotebookSnapshot } from "../ipc/types";
  import type { CalculationWorkspace } from "./calculationWorkspace.svelte";
  let { root, pageId, snapshot, workspace, active, sideOpen, activate, openSide }:
    { root: string; pageId: string; snapshot: NotebookSnapshot; workspace: CalculationWorkspace;
      active: boolean; sideOpen: boolean; activate: () => Promise<void>; openSide: () => void } = $props();
  let page = $state<HTMLElement>();
  let renaming = $state(false);
  let titleDraft = $state("");
  let titleInput = $state<HTMLInputElement>();
  let menuOpen = $state<string | null>(null);
  let menuTrigger = $state<HTMLButtonElement>();
  async function toggleMenu(id: string, trigger: HTMLButtonElement) {
    if (menuOpen === id) { closeMenu(); return; }
    menuTrigger = trigger;
    menuOpen = id;
    await tick();
    const popover = trigger.parentElement?.querySelector<HTMLElement>(".menu-popover");
    const menu = popover?.querySelector<HTMLElement>(".overflow-menu");
    if (!popover || !menu) return;
    popover.showPopover();
    const placed = placeFloatingToolbar(trigger.getBoundingClientRect(), menu.getBoundingClientRect(),
      { left: 0, top: 0, right: innerWidth, bottom: innerHeight, width: innerWidth, height: innerHeight }, 4);
    popover.style.left = `${placed.left}px`; popover.style.top = `${placed.top}px`;
    menu.querySelector<HTMLElement>("button:not(:disabled)")?.focus();
  }
  function closeMenu() {
    const restore = menuTrigger?.parentElement?.contains(globalThis.document.activeElement);
    menuOpen = null;
    if (restore) menuTrigger?.focus();
  }
  function cellMenu(index: number): MenuSection[] {
    const cell = cells[index];
    return [
      { title: $t("calculation.cellType"), entries: ["code", "markdown"].map(type => ({
        kind: "action", id: type, label: type === "code" ? $t("calculation.code") : $t("calculation.markdown"),
        disabled: busy || cell.cell_type === type,
        onSelect: () => void workspace.operation(target, { kind: "cell_type", cellId: cell.id, cellType: type }),
      })) },
      { entries: [
        { kind: "action", id: "up", label: $t("calculation.moveUp"), disabled: index === 0 || busy, onSelect: () => void move(index, false) },
        { kind: "action", id: "down", label: $t("calculation.moveDown"), disabled: index === cells.length - 1 || busy, onSelect: () => void move(index, true) },
        { kind: "action", id: "delete", label: $t("calculation.deleteCell"), disabled: busy, destructive: true, onSelect: () => void remove(index) },
      ] },
    ];
  }
  async function rename() {
    if (!renaming) return;
    renaming = false;
    const title = titleDraft.trim();
    if (title && title !== document?.metadata.goodtype.title) await workspace.operation(target, { kind: "rename", title });
  }
  async function beginRename() {
    titleDraft = document?.metadata.goodtype.title ?? "";
    renaming = true;
    await tick(); titleInput?.focus(); titleInput?.select();
  }
  $effect(() => { const currentRoot = root, currentSnapshot = snapshot; untrack(() => workspace.ingest(currentRoot, currentSnapshot)); });
  const reference = $derived(snapshot.page.calculation!);
  const document = $derived(workspace.documentFor(root, reference));
  const cells = $derived(workspace.cellsFor(root, reference));
  const layout = $derived(workspace.layoutFor(root, reference));
  $effect(() => { if (document) void workspace.preview(root, pageId, reference, document.metadata.goodtype.revision, $locale); });
  $effect(() => { const current = active && document ? target : null; if (current) untrack(() => workspace.activated(current)); });
  const target = $derived({ root, pageId, reference });
  const selected = $derived(active && workspace.selected?.pageId === pageId ? workspace.selected.cellId : null);
  const running = $derived(workspace.runningFor(root, reference));
  const runningCell = $derived(workspace.runningCell(root, reference));
  const session = $derived(workspace.sessionFor(root, reference));
  const status = $derived(workspace.statusFor(root, reference));
  const history = $derived(workspace.historyFor(root, reference));
  const busy = $derived(running || workspace.pending > 0);
  const parts = $derived(document?.metadata.goodtype.parts.length ?? 0);
  const partNumber = $derived((document?.metadata.goodtype.parts.findIndex(part => part.id === reference.partId) ?? 0) + 1);
  const sessionLabel = $derived(session === "running" ? $t("calculation.running") : session === "live" ? $t("calculation.sessionLive") : $t("calculation.notRunning"));
  // The session indicator already says "Running"; the same words in the status line would repeat it.
  const statusText = $derived(status && !(typeof status === "object" && status.key === "calculation.running") ? renderMessage(status) : "");
  const pageMenu = $derived<MenuSection[]>([
    { entries: [
      { kind: "action", id: "undo", label: $t("calculation.undo"), disabled: !history.canUndo || busy, onSelect: () => void workspace.history(target, true) },
      { kind: "action", id: "redo", label: $t("calculation.redo"), disabled: !history.canRedo || busy, onSelect: () => void workspace.history(target, false) },
    ] },
    { entries: [
      { kind: "action", id: "restart", label: $t("calculation.restartRun"), disabled: busy || session !== "live", onSelect: () => void workspace.restartAndRun(target) },
      { kind: "action", id: "end", label: $t("calculation.endSession"), disabled: busy || session !== "live", onSelect: () => void workspace.endSession(target) },
      // With one part, running the calculation is the same as the Run page button beside the menu.
      ...(parts > 1 ? [{ kind: "action" as const, id: "all", label: $t("calculation.runCalculation"), disabled: busy, onSelect: () => void workspace.runCalculation(target) }] : []),
    ] },
  ]);
  const mount = (node: HTMLElement) => workspace.mountEditor(node);
  const editingInline = $derived(selected !== null && !sideOpen);
  $effect(() => { const current = layout, inline = editingInline, id = pageId, currentRoot = root; if (current && !inline) untrack(() => workspace.viewHeight(currentRoot, id, current.heightPt)); });
  // A page move, duplicate or reload hands this page a fresh snapshot carrying its saved height,
  // while this component (and the size it measured) stays mounted. Nothing resizes, so neither
  // effect above fires again; re-report the real height or the next page overlaps this one.
  $effect(() => {
    void snapshot.page.geometry.heightPt;
    untrack(() => {
      if (editingInline && page) workspace.viewHeight(root, pageId, page.offsetHeight);
      else if (layout) workspace.viewHeight(root, pageId, layout.heightPt);
    });
  });
  function localEditorHeight(node: HTMLElement) {
    const observer = new ResizeObserver(entries => {
      if (editingInline) workspace.viewHeight(root, pageId, entries[0].borderBoxSize[0]?.blockSize ?? node.offsetHeight);
    });
    observer.observe(node);
    return { destroy() { observer.disconnect(); } };
  }
  /** Reserve local UI chrome in the page stack. It is excluded from document/export height. */
  function controlSpace(node: HTMLElement, edge: "top" | "bottom") {
    const item = node.closest<HTMLElement>(".page-stack-item");
    const property = `--calculation-${edge}-space`;
    const observer = new ResizeObserver(entries => {
      const size = entries[0].borderBoxSize[0]?.blockSize ?? node.offsetHeight;
      item?.style.setProperty(property, `${size + 6}px`);
    });
    observer.observe(node);
    return { destroy() { observer.disconnect(); item?.style.removeProperty(property); } };
  }

  const edit = (cellId: string) => workspace.choose({ ...target, cellId }, active, activate);
  async function runCell(cellId: string) {
    if (await edit(cellId)) await workspace.runCell({ ...target, cellId });
  }
  async function insert(after: string | null, cellType: string) {
    const cellId = `cell-${crypto.randomUUID()}`;
    if (await workspace.operation(target, { kind: "insert", cellId, after, cellType })) await edit(cellId);
  }
  async function remove(index: number) {
    const cell = cells[index];
    const neighbour = cells[index + 1] ?? cells[index - 1];
    if (!(await workspace.deleteCell({ ...target, cellId: cell.id }))) return;
    await tick();
    // The deleted cell took the focus with it. Hand it to the nearest remaining cell.
    page?.querySelector<HTMLElement>(neighbour ? `[data-calculation-cell="${neighbour.id}"] .source` : ".add-cell button")?.focus();
  }
  async function move(index: number, down: boolean) {
    const before = down ? (cells[index + 2]?.id ?? null) : cells[index - 1].id;
    await workspace.operation(target, { kind: "move", cellId: cells[index].id, before });
  }
</script>

<section class="calculation-page" aria-label={$t("calculation.page")} bind:this={page} use:localEditorHeight>
  <header role="toolbar" aria-label={$t("calculation.pageTools")} use:controlSpace={"top"}>
    <div class="title">
      {#if renaming}
        <input bind:this={titleInput} bind:value={titleDraft} maxlength="1024" aria-label={$t("calculation.name")}
          onblur={() => void rename()} onkeydown={event => { if (event.key === "Enter") { event.preventDefault(); void rename(); } else if (event.key === "Escape") { event.preventDefault(); renaming = false; } }} />
      {:else}<button type="button" class="name" onclick={() => void beginRename()} aria-label={$t("calculation.rename")} title={$t("calculation.rename")}><strong>{document?.metadata.goodtype.title}</strong></button>{/if}
      {#if parts > 1}<span class="part">{$t("calculation.part", { number: partNumber, count: parts })}</span>{/if}
    </div>
    <span class="session" data-state={session} title={sessionLabel}>
      <span class="dot" aria-hidden="true"></span>Python<span class="state" class:visually-hidden={session !== "running" && session !== "live"}>· {sessionLabel}</span>
    </span>
    <span class="status" role="status">{statusText}</span>
    <div class="actions">
      {#if running}
        <button type="button" class="stop" onclick={() => void workspace.endSession(target)} title={$t("calculation.stopHint")}>{$t("calculation.stop")}</button>
      {:else}
        <button type="button" class="primary" disabled={busy} onclick={() => void workspace.runPage(target)}>{$t("calculation.runPage")}</button>
      {/if}
      <div class="menu-anchor">
        <button type="button" class="icon" aria-label={$t("calculation.morePageActions")} title={$t("calculation.morePageActions")} aria-expanded={menuOpen === "page"} onclick={event => void toggleMenu("page", event.currentTarget)}>
          <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M4 9v2m6-2v2m6-2v2"/></svg>
        </button>
        {#if menuOpen === "page"}<div class="menu-popover" popover="manual"><OverflowMenu compact title={$t("calculation.morePageActions")} sections={pageMenu} onClose={closeMenu} trigger={menuTrigger} /></div>{/if}
      </div>
    </div>
  </header>
  {#each cells as cell, index (cell.id)}
    {@const settled = layout?.cells.find(item => item.id === cell.id)}
    {@const current = workspace.source({ ...target, cellId: cell.id }, cell)}
    <section class="cell" data-calculation-cell={cell.id} class:selected={selected === cell.id} aria-label={$t("calculation.cell", { number: index + 1 })}>
      <div class="cell-tools" role="toolbar" aria-label={$t("calculation.cellTools")}>
        {#if cell.cell_type === "code"}
          <span class="count" class:stale={settled?.stale} title={settled?.stale ? $t("calculation.outputStale") : undefined}>[{runningCell === cell.id ? "*" : (cell.execution_count ?? " ")}]</span>
          <button type="button" disabled={busy} aria-busy={runningCell === cell.id} onclick={() => void runCell(cell.id)}
            aria-label={runningCell === cell.id ? $t("calculation.runningCell") : $t("calculation.runCell")} title={$t("calculation.runCellShortcut")}>
            <svg viewBox="0 0 20 20" aria-hidden="true"><path d="m6 4 10 6-10 6Z"/></svg>
          </button>
        {/if}
        {#if selected === cell.id}
          <button type="button" onclick={openSide} aria-label={$t("calculation.sideEditor")} title={$t("calculation.sideEditor")}><svg viewBox="0 0 20 20" aria-hidden="true"><rect x="2" y="3" width="16" height="14" rx="1"/><path d="M9 3v14"/></svg></button>
          <div class="menu-anchor">
            <button type="button" aria-label={$t("calculation.moreCellActions")} title={$t("calculation.moreCellActions")} aria-expanded={menuOpen === cell.id} onclick={event => void toggleMenu(cell.id, event.currentTarget)}>
              <svg viewBox="0 0 20 20" aria-hidden="true"><path d="M4 9v2m6-2v2m6-2v2"/></svg>
            </button>
            {#if menuOpen === cell.id}<div class="menu-popover" popover="manual"><OverflowMenu compact title={$t("calculation.moreCellActions")} sections={cellMenu(index)} onClose={closeMenu} trigger={menuTrigger} /></div>{/if}
          </div>
        {/if}
      </div>
      {#if selected === cell.id && !sideOpen}
        <div class="cell-editor" style:min-height={`${settled?.sourceHeightPt ?? 38}px`} use:mount></div>
      {:else}
        <button class="source" type="button" onclick={() => void edit(cell.id)} aria-label={$t("calculation.editCell", { number: index + 1 })}>{#if settled && current === cell.source}
          <img class="settled" src={calculationPreviewUrl(settled.sourceSvg)} alt={cell.source || $t("calculation.emptyCell")} style:height={`${settled.sourceHeightPt}px`} />
        {:else}<pre>{current || $t("calculation.emptyCell")}</pre>{/if}</button>
      {/if}
      {#if settled?.outputSvg}
        <img class="settled" class:stale-output={settled.stale} src={calculationPreviewUrl(settled.outputSvg)} alt={$t("calculation.output")} style:height={`${settled.outputHeightPt}px`} />
      {:else}
      {#each cell.outputs ?? [] as output}
        <div class="output" class:error={output.output_type === "error"}>
          {#if output.data?.["image/png"]}
            <img src={`data:image/png;base64,${output.data["image/png"]}`} alt={$t("calculation.plot")} />
          {:else}
            <pre>{output.text instanceof Array ? output.text.join("") : output.text ?? output.data?.["text/plain"] ?? (output.ename ? `${output.ename}: ${output.evalue}` : $t("calculation.unsupportedOutput"))}</pre>
          {/if}
        </div>
      {/each}
      {/if}
      {#if cell.metadata.goodtype.copiedFrom}<p class="provenance">{$t("calculation.copiedOutput")}</p>{/if}
    </section>
  {/each}
  <div class="add-cell" use:controlSpace={"bottom"}>
    <button type="button" disabled={busy} onclick={() => void insert(cells.at(-1)?.id ?? null, "code")}>{$t("calculation.addCode")}</button>
    <button type="button" disabled={busy} onclick={() => void insert(cells.at(-1)?.id ?? null, "markdown")}>{$t("calculation.addMarkdown")}</button>
    {#if workspace.failed}<button type="button" onclick={() => void workspace.flush()}>{$t("calculation.retrySave")}</button>{/if}
  </div>
</section>

<style>
  .calculation-page { min-height: 60px; box-sizing: border-box; width: 100%; position: relative; padding: 18px; color: var(--text); background: var(--charcoal, #16181d); }
  /* Page chrome floats above and below the A4 sheet. It never takes height from the page content. */
  header { position: absolute; z-index: 4; bottom: calc(100% + 6px); left: 0; right: 0; display: flex; align-items: center; gap: 10px; min-width: 0; color: var(--text, #e9ebee); font: 10px var(--font-ui, system-ui); }
  .title { display: flex; align-items: baseline; gap: 8px; min-width: 0; flex: 0 1 auto; }
  .name { min-width: 0; overflow: hidden; border: 0; background: transparent; color: inherit; padding: 0; text-align: left; text-overflow: ellipsis; white-space: nowrap; }
  .name strong { font-size: 12px; font-weight: 600; }
  .name:hover strong { text-decoration: underline; text-decoration-color: var(--quiet, #6a727c); text-underline-offset: 2px; }
  .title input { min-width: 80px; max-width: 200px; padding: 1px 4px; border: 1px solid var(--edge, #ffffff1f); border-radius: 3px; background: var(--panel, #23272f); color: var(--text, #e9ebee); font: 600 12px var(--font-ui, system-ui); }
  .part { color: var(--muted, #aeb5be); white-space: nowrap; }

  /* One indicator for the Python session: the dot carries state at a glance, the words confirm it. */
  .session { display: inline-flex; align-items: center; flex: none; color: var(--muted, #aeb5be); white-space: nowrap; }
  .dot { width: 6px; height: 6px; margin-right: 5px; border-radius: 50%; background: var(--quiet, #6a727c); }
  .session[data-state="live"] .dot { background: #4fb586; }
  .session[data-state="running"] { color: var(--text, #e9ebee); }
  .session[data-state="running"] .dot { background: var(--blueprint-light, #7fb0f7); animation: pulse 1s ease-in-out infinite; }
  .state { margin-left: 4px; }
  .visually-hidden { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .status { flex: 1 1 60px; min-width: 0; overflow: hidden; color: var(--muted, #aeb5be); text-overflow: ellipsis; white-space: nowrap; }

  .actions { display: flex; align-items: center; gap: 4px; margin-left: auto; flex: none; }
  button { font: inherit; cursor: pointer; }
  button:focus-visible { outline: 2px solid var(--blueprint-light, #7fb0f7); outline-offset: 2px; }
  /* Controls carry no border (workspace rule): a fill on hover is enough to read as a button. */
  .actions button, .add-cell button { height: 24px; padding: 0 9px; border: 0; border-radius: 4px; background: transparent; color: var(--text, #e9ebee); }
  .actions button:hover:not(:disabled), .add-cell button:hover:not(:disabled) { background: var(--wash, #ffffff14); }
  .actions button.icon { display: grid; place-items: center; width: 24px; padding: 0; color: var(--muted, #aeb5be); }
  .actions button.stop { background: var(--wash, #ffffff14); }
  .actions button.primary { background: var(--blueprint, #4c8df0); color: #fff; font-weight: 600; }
  .actions button.primary:hover:not(:disabled) { background: #3f7fe0; }

  .cell { position: relative; margin-bottom: 14px; }
  .cell:last-of-type { margin-bottom: 0; }
  .cell.selected { box-shadow: -2px 0 var(--blueprint, #4c8df0); }
  .cell:has(:global(.overflow-menu)) { z-index: 3; }
  /* Cell tools straddle the top edge inside the sheet width, so they stay on screen at every zoom. */
  .cell-tools { position: absolute; z-index: 2; top: -11px; right: 6px; display: flex; align-items: center; gap: 1px; max-width: calc(100% - 12px); padding: 1px 2px; border: 1px solid var(--edge, #ffffff1f); border-radius: 5px; background: var(--panel, #23272f); box-shadow: 0 2px 6px rgb(0 0 0 / 30%); font: 10px var(--font-ui, system-ui); }
  .cell-tools:empty { display: none; }
  .count { min-width: 24px; color: var(--muted, #aeb5be); font: 10px 'DejaVu Sans Mono', Consolas, monospace; text-align: center; }
  .count.stale { opacity: .5; font-style: italic; }
  .cell-tools button { display: grid; place-items: center; width: 20px; height: 20px; padding: 0; border: 0; border-radius: 3px; background: transparent; color: var(--muted, #aeb5be); }
  .cell-tools button:hover:not(:disabled) { background: var(--wash, #ffffff14); color: var(--text, #e9ebee); }
  .cell-tools button[aria-busy="true"] svg { animation: pulse 1s ease-in-out infinite; }
  @keyframes pulse { 50% { opacity: .3; } }
  @media (prefers-reduced-motion: reduce) { .cell-tools button[aria-busy="true"] svg, .session .dot { animation: none; } }
  svg { width: 14px; height: 14px; fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linecap: round; stroke-linejoin: round; }

  .source { display: block; width: 100%; min-height: 0; padding: 0; border: 0; background: transparent; color: inherit; text-align: left; }
  pre { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; font: 11px/1.45 'DejaVu Sans Mono', Consolas, monospace; }
  /* Same raised surface as the settled code box (previewTheme), so pending and settled match. */
  .source pre { padding: 10px; background: #1f2329; }
  .output { padding: 6px 10px; }
  .output img { display: block; max-width: 100%; height: auto; }
  .output.error pre { margin: 0 -10px; padding: 4px 10px; border-radius: 3px; background: #3a2224; color: #f2928c; }
  .provenance { position: absolute; left: 0; right: 0; bottom: -13px; margin: 0; color: var(--muted, #aeb5be); font: 9px var(--font-ui, system-ui); pointer-events: none; }
  .settled { display: block; width: 100%; }
  /* Output from older code stays readable but steps back, so it is not mistaken for the current result. */
  .settled.stale-output { opacity: .6; }

  .add-cell { position: absolute; top: calc(100% + 6px); left: 0; right: 0; display: flex; flex-wrap: wrap; gap: 2px; font: 10px var(--font-ui, system-ui); }
  .add-cell button { color: var(--muted, #aeb5be); }
  .add-cell button:hover:not(:disabled) { color: var(--text, #e9ebee); }

  .menu-anchor { position: relative; }
  .menu-popover { position: fixed; inset: auto; margin: 0; padding: 0; border: 0; background: transparent; overflow: visible; }
  .menu-popover :global(.overflow-menu) { position: relative; top: auto; right: auto; max-height: calc(100vh - 24px); overflow-y: auto; text-align: left; }
  button:disabled { cursor: default; opacity: .45; }
</style>
