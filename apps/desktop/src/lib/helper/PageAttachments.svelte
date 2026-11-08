<script lang="ts">
  import { tick } from "svelte";
  import { t, formatNumber } from "../i18n";
  import { attachmentRange } from "./selection";
  export type AttachmentPage = { id: string; number: number; title: string; calculation: boolean; snapshot?: import("../ipc/types").NotebookSnapshot; root?: string; loading?: boolean; unavailable?: boolean };
  let { pages, selected, currentPageId, onToggle }: { pages: AttachmentPage[]; selected: string[]; currentPageId: string; onToggle: (id: string) => void } = $props();
  let openState = $state(false);
  let trigger = $state<HTMLButtonElement>();
  let popover = $state<HTMLDivElement>();
  let previewPage = $state<AttachmentPage>();
  let preview = $state<HTMLDivElement>();
  let previewTrigger: HTMLButtonElement | undefined;
  let query = $state("");
  let range = $state("");
  let invalidRange = $state(false);
  const filtered = $derived(pages.filter(p => `${p.number} ${p.title}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())));
  function addRange() {
    const next = attachmentRange(range, pages, selected);
    invalidRange = !next;
    if (next) { for (const id of next) if (!selected.includes(id)) onToggle(id); range = ""; }
  }
  const id = $props.id();
  function open(pageId = currentPageId) {
    if (popover && trigger) {
      // Open toward the side with more room; above only when it fits a useful list.
      const anchor = trigger.getBoundingClientRect();
      const above = anchor.top - 112;
      const below = window.innerHeight - anchor.bottom - 12;
      const up = above >= 240 || above > below;
      popover.style.top = up ? "auto" : `${anchor.bottom + 6}px`;
      popover.style.bottom = up ? `${window.innerHeight - anchor.top + 6}px` : "auto";
      popover.style.maxHeight = `${Math.max(80, Math.min(300, up ? above : below))}px`;
    }
    popover?.showPopover();
    void tick().then(() => {
      const nodes = [...(popover?.querySelectorAll<HTMLButtonElement>("[role=checkbox]") ?? [])];
      (nodes.find(node => node.dataset.page === pageId) ?? nodes[0])?.focus();
    });
  }
  function close() { popover?.hidePopover(); trigger?.focus(); }
  function keys(event: KeyboardEvent) {
    if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); close(); return; }
    const nodes = [...(popover?.querySelectorAll<HTMLButtonElement>("[role=checkbox]") ?? [])];
    const index = nodes.indexOf(document.activeElement as HTMLButtonElement);
    if (index >= 0 && ["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
      event.preventDefault();
      nodes[event.key === "Home" ? 0 : event.key === "End" ? nodes.length - 1 : (Math.max(index, 0) + (event.key === "ArrowDown" ? 1 : -1) + nodes.length) % nodes.length]?.focus();
    }
  }
</script>
<div class="attachments">
  <button class="attach" bind:this={trigger} type="button" aria-haspopup="dialog" aria-expanded={openState} aria-controls={id} onclick={() => open()}>
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 3h10l4 4v14H5zM14 3v5h5M8 14h8m-4-4v8" /></svg>{$t("codex.attachPage")}
  </button>
  {#each selected.slice(0, 2) as pageId (pageId)}
    {@const page = pages.find(page => page.id === pageId)}
    <span class="page-chip">
      <button class="preview" type="button" onclick={event => { previewPage = page; previewTrigger = event.currentTarget; preview?.showPopover(); }} title={$t("codex.pageAttachment", { page: formatNumber(page?.number ?? 0), title: page?.title ?? "" })} aria-label={$t("codex.pageAttachment", { page: formatNumber(page?.number ?? 0), title: page?.title ?? "" })}>{$t("codex.pageLabel", { page: formatNumber(page?.number ?? 0) })}</button>
      <button class="remove" type="button" aria-label={$t("codex.removePageAttachment", { page: formatNumber(page?.number ?? 0) })} onclick={() => { onToggle(pageId); void tick().then(() => trigger?.focus()); }}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18" /></svg></button>
    </span>
  {/each}
  {#if selected.length > 2}<button class="page-chip more" type="button" onclick={() => open()}>{$t("codex.morePages", { count: selected.length - 2 })}</button>{/if}
</div>
<div class="picker" id={id} bind:this={popover} popover="auto" role="dialog" tabindex="-1" aria-label={$t("codex.pagePicker")} onkeydown={keys} ontoggle={event => { openState = event.newState === "open"; if (!openState) trigger?.focus(); }}>
  <p>{$t("codex.pagePicker")}</p>
  <input type="search" bind:value={query} aria-label={$t("codex.pageSearch")} placeholder={$t("codex.pageSearch")} onkeydown={event => { if (event.key === "Enter") event.preventDefault(); }} />
  <div class="range"><input bind:value={range} aria-label={$t("codex.pageRange")} placeholder={$t("codex.pageRange")} onkeydown={event => { if (event.key === "Enter") { event.preventDefault(); event.stopPropagation(); addRange(); } }} /><button type="button" onclick={addRange}>{$t("codex.addRange")}</button></div>
  {#if invalidRange}<p role="alert">{$t("codex.invalidRange")}</p>{/if}
  <p role="status">{$t("codex.selectedPages", { count: selected.length })}</p>
  {#if pages.some(page => page.loading)}<p role="status">{$t("codex.readingPages")}</p>{:else if !pages.some(page => !page.unavailable)}<p>{$t("codex.noEligiblePages")}</p>{/if}
  {#each filtered as page (page.id)}
    <button class="entry" type="button" role="checkbox" data-page={page.id} aria-checked={selected.includes(page.id)} aria-disabled={page.unavailable} onclick={() => { if (!page.unavailable) onToggle(page.id); }}>
      <span class="check" aria-hidden="true">{#if selected.includes(page.id)}<svg viewBox="0 0 24 24"><path d="m5 12 4 4L19 6" /></svg>{/if}</span>
      <span class="text"><span>{$t("codex.pageAttachment", { page: formatNumber(page.number), title: page.title || $t("codex.ordinaryPage") })}</span>{#if page.loading || page.calculation || page.unavailable}<small>{page.loading ? $t("codex.readingPages") : page.unavailable ? $t("codex.pageUnavailable") : $t("codex.pythonPage")}</small>{/if}</span>
    </button>
  {/each}
  <div class="picker-actions"><button type="button" disabled={!selected.length} onclick={() => { for (const id of selected) onToggle(id); }}>{$t("codex.clearPages")}</button><button type="button" onclick={close}>{$t("codex.donePages")}</button></div>
</div>
<div class="picker" bind:this={preview} popover="auto" role="region" aria-label={$t("codex.selectionPreview")} ontoggle={event => { if (event.newState === "closed") previewTrigger?.focus(); }}>
  {#if previewPage}
    <p>{$t("codex.pageAttachment", { page: formatNumber(previewPage.number), title: previewPage.title })}</p>
    {#if previewPage.calculation}<p>{$t("codex.pythonPage")}</p>{:else if previewPage.snapshot && previewPage.root}{#key previewPage.id}{#await import("./PageAttachmentPreview.svelte")}<p>{$t("codex.readingPages")}</p>{:then module}{@const Preview = module.default}<Preview snapshot={previewPage.snapshot} root={previewPage.root} />{/await}{/key}{:else}<p>{$t("codex.pageUnavailable")}</p>{/if}
  {/if}
</div>
<style>
  input { width: 100%; min-width: 0; min-height: var(--control); padding: 6px; background: var(--surround); border: 1px solid var(--edge); border-radius: var(--radius); font: inherit; color: var(--text); }
  input:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: 1px; }
  .range { display: flex; gap: 4px; margin-top: 6px; }
  .range button { flex: none; }
  .picker-actions { display: flex; justify-content: space-between; position: sticky; bottom: -6px; background: var(--panel); border-top: 1px solid var(--edge); }
  .attachments { display: contents; }
  button { display: inline-flex; align-items: center; gap: 4px; min-height: var(--control); padding: 0 6px; border: 0; border-radius: var(--radius); background: transparent; color: var(--muted); font: inherit; font-weight: 400; font-size: var(--text-sm); cursor: pointer; }
  button:hover { background: var(--wash); color: var(--text); }
  button:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }
  .entry:focus-visible { outline-offset: -2px; }
  svg { flex: none; width: var(--icon-dense); height: var(--icon-dense); fill: none; stroke: currentColor; stroke-width: var(--stroke-dense); stroke-linecap: round; stroke-linejoin: round; }
  .page-chip { background: var(--panel); color: var(--text); display: inline-flex; min-width: 0; border: 1px solid var(--edge); border-radius: var(--radius); }
  .page-chip button { min-height: var(--control-dense); color: var(--text); }
  .more { min-height: var(--control-dense); }
  .preview { overflow: hidden; white-space: nowrap; }
  .remove { padding: 0 3px; }
  .picker { position: fixed; inset: auto 12px 230px auto; width: min(350px, calc(100vw - 24px)); max-height: min(300px, calc(100dvh - 300px)); margin: 0; padding: 6px; overflow: auto; border: 1px solid var(--edge); border-radius: var(--radius-lg); background: var(--panel); color: var(--text); box-shadow: var(--shadow-menu); font: inherit; }
  p { margin: 6px; color: var(--muted); font-size: var(--text-sm); }
  .entry { min-height: var(--control); width: 100%; text-align: left; padding: 6px; color: var(--text); }
  .entry[aria-disabled=true] { color: var(--muted); cursor: not-allowed; }
  .text { display: grid; gap: 2px; min-width: 0; overflow-wrap: anywhere; }
  small { color: var(--muted); font-size: var(--text-sm); }
  .check { display: grid; place-items: center; width: 16px; height: 16px; flex: none; border: 1px solid var(--muted); border-radius: 3px; color: var(--blueprint-light); }
  [aria-checked=true] .check { border-color: var(--blueprint-light); }
  @media (pointer: coarse) { button { min-height: var(--control-touch); } }
</style>
