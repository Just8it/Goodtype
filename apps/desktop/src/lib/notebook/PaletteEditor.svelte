<script lang="ts">
  import { t } from "../i18n";
  import { tick, type Snippet } from "svelte";
  import type { PaletteDock } from "./palette";
  import { dismissable } from "../workspace/dismiss";

  let { title, anchor, dock, onClose, onDismiss, children }: {
    title: string;
    anchor: HTMLElement;
    dock: PaletteDock;
    onClose: () => void;
    onDismiss: () => void;
    children: Snippet;
  } = $props();

  function place(node: HTMLElement) {
    const palette = anchor.closest<HTMLElement>(".instrument-palette")!;
    const workspace = anchor.closest<HTMLElement>(".workspace-chrome") ?? palette.parentElement!;
    // Include the rail in the dismissal boundary: pressing the opener again must close, not reopen.
    const dismiss = dismissable(palette, onDismiss);
    const position = () => {
      const chip = anchor.getBoundingClientRect();
      const rail = palette.getBoundingClientRect();
      const bounds = workspace.getBoundingClientRect();
      const box = node.getBoundingClientRect();
      const x = dock === "left" ? rail.right + 10 : dock === "right" ? rail.left - box.width - 10 : chip.left + chip.width / 2 - box.width / 2;
      const y = dock === "top" ? rail.bottom + 10 : dock === "bottom" ? rail.top - box.height - 10 : chip.top + chip.height / 2 - box.height / 2;
      node.style.left = `${Math.max(bounds.left + 8, Math.min(x, bounds.right - box.width - 8))}px`;
      node.style.top = `${Math.max(bounds.top + 8, Math.min(y, bounds.bottom - box.height - 8))}px`;
    };
    position();
    const observer = new ResizeObserver(position);
    observer.observe(node);
    observer.observe(workspace);
    palette.addEventListener("scroll", position, true);
    let mounted = true;
    void tick().then(() => {
      if (mounted) node.querySelector<HTMLElement>("[aria-pressed='true'], input, button")?.focus();
    });
    return { destroy() { mounted = false; observer.disconnect(); palette.removeEventListener("scroll", position, true); dismiss.destroy(); } };
  }
</script>

<div use:place class="palette-editor" role="dialog" aria-label={title} tabindex="-1"
  onkeydown={(event) => { if (event.key === "Escape") { event.stopPropagation(); onClose(); } }}>
  <header><strong>{title}</strong><button type="button" aria-label={$t("workspace.close2", { "value1": title.toLowerCase() })} onclick={onClose}>
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18" /></svg>
  </button></header>
  {@render children()}
</div>

<style>
  .palette-editor { position: fixed; z-index: 60; width: 272px; max-width: calc(100vw - 32px); max-height: calc(100vh - 140px); overflow-y: auto; border: 1px solid var(--edge); border-radius: var(--radius-lg); background: var(--panel); color: var(--text); box-shadow: 0 18px 44px rgb(0 0 0 / 55%); }
  header { display: flex; align-items: center; justify-content: space-between; min-height: var(--control); padding: 0 6px 0 12px; border-bottom: 1px solid var(--edge-soft); }
  strong { font-size: var(--text-md); font-weight: 600; }
  button { display: grid; place-items: center; width: var(--control-dense); height: var(--control-dense); border: 0; border-radius: var(--radius); background: transparent; color: var(--text); cursor: pointer; }
  button:hover { background: var(--wash); }
  button:focus-visible { outline: 2px solid var(--blueprint); }
  svg { width: var(--icon-dense); height: var(--icon-dense); fill: none; stroke: currentColor; stroke-width: var(--stroke); stroke-linecap: round; }
</style>
