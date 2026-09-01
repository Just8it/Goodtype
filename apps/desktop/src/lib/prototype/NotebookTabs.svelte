<script lang="ts">
  import { tick } from "svelte";
  import { cycledTab, type NotebookTab } from "./tabs";

  type Props = {
    tabs: NotebookTab[];
    activeRoot: string;
    switchingRoot: string | null;
    busy: boolean;
    saving: boolean;
    warning: boolean;
    onSelect: (root: string) => Promise<boolean>;
    onClose: (root: string) => Promise<void>;
  };

  let {
    tabs,
    activeRoot,
    switchingRoot,
    busy,
    saving,
    warning,
    onSelect,
    onClose,
  }: Props = $props();
  let tablist: HTMLElement;

  async function select(root: string) {
    if (!(await onSelect(root))) return;
    await tick();
    tablist
      ?.querySelector<HTMLButtonElement>(`[data-tab-root="${CSS.escape(root)}"]`)
      ?.focus();
  }

  function keydown(event: KeyboardEvent) {
    const offset = event.key === "ArrowLeft" ? -1 : event.key === "ArrowRight" ? 1 : null;
    if (!offset) return;
    const next = cycledTab(tabs, activeRoot, offset);
    if (!next) return;
    event.preventDefault();
    void select(next);
  }
</script>

<div
  bind:this={tablist}
  class="notebook-tabs"
  role="tablist"
  aria-label="Open notebooks"
  tabindex="-1"
  onkeydown={keydown}
>
  {#each tabs as tab (tab.root)}
    {@const active = tab.root === activeRoot}
    <div class:active class:switching={switchingRoot === tab.root} class="notebook-tab">
      <button
        class="tab-target"
        type="button"
        role="tab"
        aria-selected={active}
        tabindex={active ? 0 : -1}
        data-tab-root={tab.root}
        title={tab.title}
        disabled={busy}
        onclick={() => void select(tab.root)}
      >
        {#if active}
          <span class:warning class:saving class="state-dot"></span>
        {/if}
        <span class="tab-title">{tab.title}</span>
      </button>
      <button
        class="tab-close"
        type="button"
        aria-label={`Close ${tab.title}`}
        title={`Close ${tab.title}`}
        disabled={busy}
        onclick={() => void onClose(tab.root)}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>
    </div>
  {/each}
</div>

<style>
  .notebook-tabs {
    display: flex;
    min-width: 0;
    /* Only as wide as the tabs themselves. Stretching to fill the strip pushed the "open another
       notebook" button to the far side of the window, away from the tabs it adds to. */
    flex: 0 1 auto;
    overflow-x: auto;
    gap: 4px;
    scrollbar-width: none;
  }
  .notebook-tabs::-webkit-scrollbar { display: none; }
  .notebook-tab {
    display: flex;
    min-width: 132px;
    max-width: 220px;
    height: var(--control);
    flex: 1 1 180px;
    align-items: center;
    border: 1px solid var(--edge);
    /* The bottom edge stays 2px in every state so selecting a tab tints it rather than resizing
       it. It used to be transparent when inactive, which left the box visibly open at the
       bottom — a browser-tab move that only reads right when the tab docks onto content below. */
    border-bottom: 2px solid var(--edge);
    border-radius: var(--radius);
    background: rgb(255 255 255 / 3%);
  }
  .notebook-tab.active { border-bottom-color: var(--blueprint); background: var(--panel); }
  .notebook-tab.switching { opacity: .65; }
  .notebook-tab:hover { background: var(--wash); }
  .tab-target {
    display: flex;
    min-width: 0;
    height: 100%;
    flex: 1;
    align-items: center;
    gap: 8px;
    padding: 0 5px 0 11px;
    border: 0;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    text-align: left;
  }
  .notebook-tab.active .tab-target { color: var(--text); }
  /* Without this the title fell through to the browser's 16px default, which is why the tabs
     read a full step larger than every other label in the strip. */
  .tab-title { overflow: hidden; font-size: var(--text-md); text-overflow: ellipsis; white-space: nowrap; }
  .tab-close {
    display: grid;
    width: var(--control-dense);
    height: var(--control-dense);
    flex: none;
    padding: 0;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--quiet);
    cursor: pointer;
    place-items: center;
  }
  .tab-close svg {
    width: var(--icon-dense);
    height: var(--icon-dense);
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-width: var(--stroke-dense);
  }
  .tab-close:hover:not(:disabled) { background: var(--wash); color: var(--text); }
  .state-dot {
    width: 6px;
    height: 6px;
    flex: none;
    border-radius: 50%;
    background: var(--blueprint);
  }
  .state-dot.saving { animation: breathe 1s ease-in-out infinite alternate; }
  .state-dot.warning { background: var(--oxide); }
  @keyframes breathe { from { opacity: .35; } to { opacity: 1; } }
  @media (max-width: 800px) {
    .notebook-tab { min-width: 118px; }
  }
  @media (prefers-reduced-motion: reduce) {
    .state-dot.saving { animation: none; }
  }
</style>
