<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { SearchHit } from "../settings";

  let {
    root,
    tauriAvailable,
    onNavigate,
    onClose,
  }: {
    root: string;
    tauriAvailable: boolean;
    onNavigate: (hit: SearchHit) => void;
    onClose: () => void;
  } = $props();

  let query = $state("");
  let hits = $state<SearchHit[]>([]);
  let searched = $state(false);
  let failure = $state("");
  let inputElement = $state<HTMLInputElement>();
  let searchTimer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    inputElement?.focus();
    return () => {
      if (searchTimer) clearTimeout(searchTimer);
    };
  });

  function scheduleSearch() {
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(run, 200);
  }

  async function run() {
    const trimmed = query.trim();
    if (!trimmed || !tauriAvailable) {
      hits = [];
      searched = false;
      failure = "";
      return;
    }
    try {
      hits = await invoke<SearchHit[]>("search_notebook", { root, query: trimmed });
      searched = true;
      failure = "";
    } catch (error) {
      failure = String(error);
    }
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    } else if (event.key === "Enter" && hits.length > 0) {
      event.preventDefault();
      onNavigate(hits[0]);
    }
  }
</script>

<!-- Notebook-scoped Typst source search (Phase 2 §3.7): typed content only, no OCR. -->
<section class="search" role="search" aria-label="Search typed notebook content">
  <div class="field">
    <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="7"></circle><path d="m20 20-4-4"></path></svg>
    <input
      bind:this={inputElement}
      bind:value={query}
      type="search"
      placeholder="Search typed content…"
      aria-label="Search typed notebook content"
      oninput={scheduleSearch}
      onkeydown={keydown}
    />
    <button type="button" aria-label="Close search" onclick={onClose}>×</button>
  </div>
  {#if failure}
    <p class="state">{failure}</p>
  {:else if searched && hits.length === 0}
    <p class="state">No typed content matches.</p>
  {:else if hits.length > 0}
    <ol aria-label="Search results">
      {#each hits as hit (hit.pageId + hit.objectId)}
        <li>
          <button type="button" onclick={() => onNavigate(hit)}>
            <span class="page">p. {hit.pageNumber}</span>
            <span class="excerpt">{hit.excerpt}</span>
          </button>
        </li>
      {/each}
    </ol>
  {/if}
</section>

<style>
  .search {
    position: absolute;
    top: 12px;
    right: 16px;
    z-index: 30;
    width: min(380px, calc(100% - 32px));
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 10px;
    background: #23272f;
    box-shadow: 0 14px 36px rgb(0 0 0 / 40%);
  }

  .field {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
  }

  .field svg {
    width: 15px;
    height: 15px;
    flex: none;
    fill: none;
    stroke: #6a727c;
    stroke-width: 1.8;
  }

  input {
    flex: 1;
    min-width: 0;
    border: 0;
    background: transparent;
    color: #e9ebee;
    font-size: 13px;
    outline: none;
  }

  .field > button {
    flex: none;
    width: 24px;
    height: 24px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: #6a727c;
    font-size: 16px;
    cursor: pointer;
  }

  .field > button:hover {
    background: rgb(255 255 255 / 8%);
    color: #e9ebee;
  }

  .state {
    margin: 0;
    padding: 10px 14px 12px;
    border-top: 1px solid rgb(255 255 255 / 8%);
    color: #6a727c;
    font-size: 12px;
  }

  ol {
    max-height: 280px;
    overflow: auto;
    margin: 0;
    padding: 6px;
    border-top: 1px solid rgb(255 255 255 / 8%);
    list-style: none;
  }

  li button {
    display: flex;
    align-items: baseline;
    gap: 10px;
    width: 100%;
    padding: 7px 9px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: #e9ebee;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
  }

  li button:hover,
  li button:focus-visible {
    background: rgb(76 141 240 / 14%);
  }

  .page {
    flex: none;
    color: #4c8df0;
    font-variant-numeric: tabular-nums;
    font-size: 11.5px;
  }

  .excerpt {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
