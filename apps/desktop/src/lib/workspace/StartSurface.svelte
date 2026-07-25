<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { RecentNotebook } from "../settings";
  import BrandMark from "../brand/BrandMark.svelte";

  let {
    tauriAvailable,
    onOpen,
    onCreate,
    onStatus,
  }: {
    tauriAvailable: boolean;
    onOpen: (root: string) => void;
    onCreate: (root: string) => void;
    onStatus: (status: string) => void;
  } = $props();

  let recents = $state<RecentNotebook[]>([]);
  let newName = $state("");
  let busy = $state(false);

  $effect(() => {
    void refresh();
  });

  async function refresh() {
    if (!tauriAvailable) return;
    try {
      recents = await invoke<RecentNotebook[]>("list_recent_notebooks");
    } catch (error) {
      onStatus(`Could not read recent notebooks: ${String(error)}`);
    }
  }

  async function openExisting() {
    busy = true;
    try {
      const root = await invoke<string | null>("pick_notebook_root");
      if (root) onOpen(root);
    } catch (error) {
      onStatus(String(error));
    } finally {
      busy = false;
    }
  }

  async function createNew() {
    const name = newName.trim() || "New notebook";
    busy = true;
    try {
      const root = await invoke<string | null>("pick_new_notebook_root", { name });
      if (root) onCreate(root);
    } catch (error) {
      onStatus(String(error));
    } finally {
      busy = false;
    }
  }

  async function openRecent(entry: RecentNotebook) {
    busy = true;
    try {
      const root = await invoke<string>("open_recent_root", { root: entry.root });
      onOpen(root);
    } catch (error) {
      onStatus(`${entry.title}: ${String(error)}`);
      await refresh();
    } finally {
      busy = false;
    }
  }

  async function togglePin(entry: RecentNotebook) {
    try {
      recents = await invoke<RecentNotebook[]>("set_notebook_pinned", {
        root: entry.root,
        pinned: !entry.pinned,
      });
    } catch (error) {
      onStatus(String(error));
    }
  }

  async function remove(entry: RecentNotebook) {
    try {
      recents = await invoke<RecentNotebook[]>("remove_recent_notebook", {
        root: entry.root,
      });
    } catch (error) {
      onStatus(String(error));
    }
  }
</script>

<!-- Minimal start surface (Phase 2 §3.8): open, create, recent, pinned. No accounts,
     scanning, or sync — the list only remembers what the user opened. -->
<section class="start" aria-label="Open a notebook">
  <header>
    <BrandMark size={52} title="" />
    <h1>goodtype</h1>
    <p>Write · Typeset · Arrange</p>
  </header>

  <div class="actions">
    <button class="primary" type="button" disabled={busy || !tauriAvailable} onclick={openExisting}>
      Open notebook…
    </button>
    <form
      onsubmit={(event) => {
        event.preventDefault();
        void createNew();
      }}
    >
      <input
        bind:value={newName}
        type="text"
        maxlength="80"
        placeholder="New notebook name"
        aria-label="New notebook name"
        disabled={busy || !tauriAvailable}
      />
      <button type="submit" disabled={busy || !tauriAvailable}>Create…</button>
    </form>
  </div>

  {#if recents.length > 0}
    <ul aria-label="Recent notebooks">
      {#each recents as entry (entry.root)}
        <li>
          <button class="entry" type="button" disabled={busy} onclick={() => openRecent(entry)}>
            <strong>{entry.title}</strong>
            <span title={entry.root}>{entry.root}</span>
          </button>
          <button
            class="pin"
            class:pinned={entry.pinned}
            type="button"
            aria-label={entry.pinned ? `Unpin ${entry.title}` : `Pin ${entry.title}`}
            aria-pressed={entry.pinned}
            onclick={() => togglePin(entry)}
          >
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3l2.2 4.9 5.3.6-4 3.6 1.1 5.2L12 14.7l-4.6 2.6 1.1-5.2-4-3.6 5.3-.6z"></path></svg>
          </button>
          <button class="remove" type="button" aria-label={`Remove ${entry.title} from the list`} onclick={() => remove(entry)}>×</button>
        </li>
      {/each}
    </ul>
  {:else if tauriAvailable}
    <p class="empty">Notebooks you open appear here.</p>
  {:else}
    <p class="empty">The browser preview cannot open local notebooks.</p>
  {/if}
</section>

<style>
  .start {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 22px;
    height: 100%;
    padding: 24px;
    background: #16181d;
    color: #e9ebee;
  }

  header {
    display: grid;
    justify-items: center;
    gap: 4px;
    text-align: center;
  }

  /* Wordmark. The design sets this in Poppins 500; Goodtype ships no webfont and must not
     fetch one, so this matches the weight, case, and tracking in the UI stack instead. */
  h1 {
    margin: 10px 0 0;
    font-size: 30px;
    font-weight: 500;
    letter-spacing: -0.02em;
    text-transform: lowercase;
  }

  /* Tagline, set like the lockup: mono, uppercase, widely tracked against the wordmark. */
  header p {
    margin: 4px 0 0;
    color: #8a929c;
    font-family: ui-monospace, "Cascadia Mono", "Segoe UI Mono", monospace;
    font-size: 10.5px;
    letter-spacing: 0.26em;
    text-indent: 0.26em;
    text-transform: uppercase;
  }

  .actions {
    display: grid;
    gap: 10px;
    width: min(360px, 100%);
  }

  .primary {
    padding: 10px 16px;
    border: 1px solid #4c8df0;
    border-radius: 9px;
    background: #4c8df0;
    color: #0d1117;
    font-size: 13.5px;
    font-weight: 600;
    cursor: pointer;
  }

  .primary:hover:enabled {
    background: #7fb0f7;
  }

  form {
    display: flex;
    gap: 8px;
  }

  input {
    flex: 1;
    min-width: 0;
    padding: 9px 12px;
    border: 1px solid rgb(255 255 255 / 14%);
    border-radius: 9px;
    background: #23272f;
    color: #e9ebee;
    font-size: 13px;
  }

  form button {
    padding: 9px 14px;
    border: 1px solid rgb(255 255 255 / 14%);
    border-radius: 9px;
    background: transparent;
    color: #e9ebee;
    font-size: 13px;
    cursor: pointer;
  }

  form button:hover:enabled,
  .pin:hover,
  .remove:hover {
    background: rgb(255 255 255 / 6%);
  }

  button:disabled {
    opacity: 0.55;
    cursor: default;
  }

  ul {
    width: min(430px, 100%);
    max-height: 300px;
    overflow: auto;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 6px;
  }

  .entry {
    display: grid;
    flex: 1;
    min-width: 0;
    gap: 2px;
    padding: 9px 12px;
    border: 1px solid rgb(255 255 255 / 9%);
    border-radius: 9px;
    background: transparent;
    color: #e9ebee;
    text-align: left;
    cursor: pointer;
  }

  .entry:hover:enabled {
    border-color: rgb(76 141 240 / 55%);
    background: rgb(76 141 240 / 10%);
  }

  .entry strong {
    font-size: 13px;
    font-weight: 600;
  }

  .entry span {
    overflow: hidden;
    color: #6a727c;
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pin,
  .remove {
    flex: none;
    width: 28px;
    height: 28px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: #6a727c;
    cursor: pointer;
  }

  .pin svg {
    width: 15px;
    height: 15px;
    fill: none;
    stroke: currentcolor;
    stroke-width: 1.6;
    stroke-linejoin: round;
  }

  .pin.pinned {
    color: #e0912b;
  }

  .pin.pinned svg {
    fill: currentcolor;
  }

  .remove {
    font-size: 15px;
  }

  .empty {
    margin: 0;
    color: #6a727c;
    font-size: 12.5px;
  }
</style>
