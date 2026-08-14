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
      >×</button>
    </div>
  {/each}
</div>

<style>
  .notebook-tabs {
    display: flex;
    min-width: 0;
    flex: 1;
    overflow-x: auto;
    gap: 4px;
    scrollbar-width: none;
  }
  .notebook-tabs::-webkit-scrollbar { display: none; }
  .notebook-tab {
    display: flex;
    min-width: 132px;
    max-width: 220px;
    height: 40px;
    flex: 1 1 180px;
    align-items: center;
    border: 1px solid rgb(255 255 255 / 10%);
    border-bottom: 2px solid transparent;
    border-radius: 7px 7px 2px 2px;
    background: rgb(255 255 255 / 3%);
  }
  .notebook-tab.active { border-bottom-color: var(--blueprint); background: var(--panel); }
  .notebook-tab.switching { opacity: .65; }
  .notebook-tab:hover { background: rgb(255 255 255 / 7%); }
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
  .tab-title { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tab-close {
    width: 34px;
    height: 34px;
    flex: none;
    padding: 0;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--quiet);
    cursor: pointer;
    font-size: 17px;
  }
  .tab-close:hover:not(:disabled) { background: rgb(255 255 255 / 8%); color: var(--text); }
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
