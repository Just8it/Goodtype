<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import BrandMark from "../brand/BrandMark.svelte";
  import NotebookCover from "./NotebookCover.svelte";
  import {
    bands,
    breadcrumb,
    parentPath,
    type LibraryEntry,
    type LibraryListing,
    type SortOrder,
  } from "./library";

  let {
    tauriAvailable,
    onOpen,
    onStatus,
  }: {
    tauriAvailable: boolean;
    /** Hands back an absolute notebook root, which is what every notebook command takes. */
    onOpen: (root: string) => void;
    onStatus: (message: string) => void;
  } = $props();

  const COVER_WIDTH_PX = 152;

  let libraryRoot = $state<string | null>(null);
  let path = $state("");
  let entries = $state.raw<LibraryEntry[]>([]);
  let order = $state<SortOrder>("name");
  let busy = $state(false);
  let failure = $state<string | null>(null);

  const crumbs = $derived(breadcrumb(path));
  const shelf = $derived(bands(entries, order));
  const up = $derived(parentPath(path));

  $effect(() => {
    void start();
  });

  async function start() {
    if (!tauriAvailable) return;
    try {
      libraryRoot = await invoke<string | null>("library_root");
      if (libraryRoot) await show("");
    } catch (error) {
      failure = message(error);
    }
  }

  async function chooseLibrary() {
    busy = true;
    try {
      const chosen = await invoke<string | null>("pick_library_root");
      if (!chosen) return;
      libraryRoot = chosen;
      await show("");
      onStatus(`Library set to ${chosen}`);
    } catch (error) {
      failure = message(error);
    } finally {
      busy = false;
    }
  }

  /**
   * Read one folder and show it.
   *
   * The listing echoes the path it was asked for, and anything that comes back for a folder the
   * writer has already navigated away from is dropped. Without that, a slow listing of a deep
   * folder can land after a fast one of its parent and repaint the wrong shelf.
   */
  async function show(next: string) {
    busy = true;
    failure = null;
    path = next;
    try {
      const listing = await invoke<LibraryListing>("list_library", { path: next });
      if (listing.path !== path) return;
      entries = listing.entries;
    } catch (error) {
      entries = [];
      failure = message(error);
    } finally {
      busy = false;
    }
  }

  async function openNotebook(notebookPath: string) {
    busy = true;
    try {
      onOpen(await invoke<string>("open_library_notebook", { path: notebookPath }));
    } catch (error) {
      failure = message(error);
      busy = false;
    }
  }

  function message(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
  }

  function whenModified(modifiedMs: number | null): string {
    if (!modifiedMs) return "—";
    return new Date(modifiedMs).toLocaleDateString(undefined, {
      day: "numeric",
      month: "short",
      year: "numeric",
    });
  }

  function pages(count: number | null): string {
    if (count === null) return "Manifest unlesbar";
    return count === 1 ? "1 Seite" : `${count} Seiten`;
  }

  function items(count: number): string {
    return count === 1 ? "1 Element" : `${count} Elemente`;
  }
</script>

<div class="library">
  <aside class="rail">
    <div class="brand">
      <BrandMark size={20} title="" />
      <span>goodtype</span>
    </div>

    {#if libraryRoot}
      <div class="where">
        <div class="where-text">
          <div class="overline">Bibliothek</div>
          <div class="where-path" title={libraryRoot}>{libraryRoot}</div>
        </div>
        <button type="button" class="link" disabled={busy} onclick={chooseLibrary}>Ändern</button>
      </div>
    {/if}

    <nav class="views">
      <button type="button" class="view current" onclick={() => void show("")}>Bibliothek</button>
    </nav>

    <!-- The design put a folder tree here. It is deliberately absent: at the root it lists
         exactly what the grid already shows, it needs expand state and a cached listing per node
         that every rename and move has to invalidate, and it is the one control on this surface
         you would have to put the pen down for. Favourites, recents and search are the answer to
         jumping between branches, and they belong in this same slot when they arrive. -->
  </aside>

  <section class="shelf">
    <header class="bar">
      <nav class="crumbs" aria-label="Pfad">
        {#each crumbs as crumb, index (crumb.path)}
          {#if index > 0}<span class="separator" aria-hidden="true">/</span>{/if}
          <button
            type="button"
            class="crumb"
            class:here={index === crumbs.length - 1}
            aria-current={index === crumbs.length - 1 ? "page" : undefined}
            disabled={busy}
            onclick={() => void show(crumb.path)}>{crumb.name}</button
          >
        {/each}
      </nav>
      <div class="spacer"></div>
      <button
        type="button"
        class="control"
        disabled={busy}
        onclick={() => (order = order === "name" ? "modified" : "name")}
      >
        Sortieren: {order === "name" ? "Name" : "Datum"}
      </button>
    </header>

    <div class="contents">
      {#if !tauriAvailable}
        <p class="notice">The library needs the desktop app — a browser has no folder to read.</p>
      {:else if !libraryRoot}
        <div class="first-run">
          <h1>Wo sollen deine Notizbücher liegen?</h1>
          <p>
            Wähle einen Ordner. Alles darin gehört dir: Ordner verschachteln sich beliebig, und
            jeder Ordner mit einem Notizbuch darin ist eines. Goodtype legt keine Datenbank an —
            was im Explorer liegt, liegt auch hier.
          </p>
          <button type="button" class="primary" disabled={busy} onclick={chooseLibrary}>
            Ordner wählen
          </button>
        </div>
      {:else if failure}
        <p class="notice failure">{failure}</p>
      {:else if entries.length === 0}
        <div class="empty">
          <p>{busy ? "Wird gelesen…" : "Dieser Ordner ist leer."}</p>
          {#if up !== null && !busy}
            <button type="button" class="control" onclick={() => void show(up)}>
              Eine Ebene zurück
            </button>
          {/if}
        </div>
      {:else}
        {#if shelf.folders.length > 0}
          <div class="band">
            <span class="band-label">Ordner</span>
            <span class="band-count">{shelf.folders.length}</span>
            <span class="band-rule"></span>
          </div>
          <div class="grid folders">
            {#each shelf.folders as folder (folder.path)}
              <button type="button" class="folder" disabled={busy} onclick={() => void show(folder.path)}>
                <svg width="19" height="19" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                  <path
                    d="M3 7.5A1.5 1.5 0 0 1 4.5 6h4l2 2.5h7A1.5 1.5 0 0 1 19 10v7.5A1.5 1.5 0 0 1 17.5 19h-13A1.5 1.5 0 0 1 3 17.5v-10z"
                    stroke="currentColor"
                    stroke-width="1.5"
                  />
                </svg>
                <span class="grow"></span>
                <span class="tile-name">{folder.name}</span>
                <span class="tile-meta">{items(folder.childCount)}</span>
              </button>
            {/each}
          </div>
        {/if}

        {#if shelf.notebooks.length > 0}
          <div class="band">
            <span class="band-label">Notizbücher</span>
            <span class="band-count">{shelf.notebooks.length}</span>
            <span class="band-rule"></span>
          </div>
          <div class="grid">
            {#each shelf.notebooks as notebook (notebook.path)}
              <button
                type="button"
                class="notebook"
                disabled={busy}
                onclick={() => void openNotebook(notebook.path)}
              >
                <NotebookCover paper={notebook.paper} widthPx={COVER_WIDTH_PX} />
                <span class="tile-name">{notebook.name}</span>
                <span class="tile-meta">
                  {pages(notebook.pageCount)} · {whenModified(notebook.modifiedMs)}
                </span>
              </button>
            {/each}
          </div>
        {/if}
      {/if}
    </div>
  </section>
</div>

<style>
  .library {
    display: flex;
    height: 100%;
    overflow: hidden;
    background: var(--surround, #1b1e24);
    color: var(--text, #e9ebee);
    font-family: var(--font-ui, "Bahnschrift", system-ui, sans-serif);
  }

  .rail {
    display: flex;
    flex: none;
    flex-direction: column;
    width: 236px;
    padding: 16px 12px 12px;
    border-right: 1px solid rgb(255 255 255 / 12%);
    background: var(--charcoal, #16181d);
  }

  .brand {
    display: flex;
    gap: 9px;
    align-items: center;
    padding: 0 6px 14px;
    font-size: 15px;
    font-weight: 600;
  }

  .where {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 8px 10px;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 7px;
    background: rgb(255 255 255 / 4%);
  }

  .where-text {
    flex: 1;
    min-width: 0;
  }

  .overline {
    color: var(--quiet, #6a727c);
    font-size: 9.5px;
    letter-spacing: 0.09em;
    text-transform: uppercase;
  }

  .where-path {
    overflow: hidden;
    margin-top: 2px;
    color: var(--muted, #aeb5be);
    font-size: 11.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
  }

  .views {
    display: flex;
    flex-direction: column;
    margin-top: 14px;
  }

  .view {
    padding: 8px 10px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: 13.5px;
    text-align: left;
    cursor: pointer;
  }

  .view.current {
    outline: 1px solid rgb(76 141 240 / 50%);
    background: rgb(76 141 240 / 16%);
  }

  .shelf {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }

  .bar {
    display: flex;
    flex: none;
    gap: 14px;
    align-items: center;
    height: 56px;
    padding: 0 24px;
    border-bottom: 1px solid rgb(255 255 255 / 12%);
  }

  .crumbs {
    display: flex;
    gap: 3px;
    align-items: center;
    min-width: 0;
    overflow: hidden;
  }

  .crumb {
    flex: none;
    padding: 3px 6px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--muted, #aeb5be);
    font: inherit;
    font-size: 14.5px;
    cursor: pointer;
  }

  .crumb:hover:not(:disabled) {
    background: rgb(255 255 255 / 6%);
  }

  /* The last crumb is where you are, so it is the one thing here that is not a way to leave. */
  .crumb.here {
    color: var(--text, #e9ebee);
    font-weight: 600;
    cursor: default;
  }

  .separator {
    color: var(--quiet, #6a727c);
    font-size: 13px;
  }

  .spacer {
    flex: 1;
  }

  .control {
    height: 34px;
    padding: 0 11px;
    border: 1px solid rgb(255 255 255 / 16%);
    border-radius: 7px;
    background: transparent;
    color: var(--muted, #aeb5be);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }

  .control:hover:not(:disabled) {
    background: rgb(255 255 255 / 5%);
  }

  .primary {
    height: 34px;
    padding: 0 13px;
    border: 0;
    border-radius: 7px;
    background: var(--blueprint, #4c8df0);
    color: #0e1b31;
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }

  .contents {
    flex: 1;
    padding: 22px 24px 32px;
    overflow-y: auto;
  }

  .band {
    display: flex;
    gap: 10px;
    align-items: center;
    padding-bottom: 11px;
  }

  .band ~ .band {
    padding-top: 26px;
  }

  .band-label {
    color: var(--quiet, #6a727c);
    font-size: 10px;
    letter-spacing: 0.11em;
    text-transform: uppercase;
  }

  .band-count,
  .tile-meta {
    color: var(--quiet, #6a727c);
    font-size: 10.5px;
  }

  .band-rule {
    flex: 1;
    height: 1px;
    background: rgb(255 255 255 / 12%);
  }

  .grid {
    display: flex;
    flex-wrap: wrap;
    gap: 20px;
  }

  .grid.folders {
    padding-bottom: 26px;
  }

  /* Folders are short, wide and opaque; notebooks are tall paper. The silhouettes differ before
     any label is read, which is what lets the two bands be scanned rather than parsed. */
  .folder {
    display: flex;
    flex-direction: column;
    width: 172px;
    height: 98px;
    padding: 11px 13px;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 7px;
    background: var(--panel, #23272f);
    color: var(--muted, #aeb5be);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .folder:hover:not(:disabled) {
    background: #2a2f38;
  }

  .notebook {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 152px;
    padding: 0;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .grow {
    flex: 1;
  }

  .tile-name {
    overflow: hidden;
    color: var(--text, #e9ebee);
    font-size: 13.5px;
    font-weight: 500;
    line-height: 1.3;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .notebook .tile-name {
    font-size: 13px;
  }

  .tile-meta {
    margin-top: 3px;
  }

  .folder:focus-visible,
  .notebook:focus-visible,
  .crumb:focus-visible,
  .control:focus-visible,
  .primary:focus-visible,
  .view:focus-visible,
  .link:focus-visible {
    outline: 2px solid var(--blueprint, #4c8df0);
    outline-offset: 2px;
  }

  .link {
    flex: none;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--blueprint, #4c8df0);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }

  .first-run {
    max-width: 460px;
    margin: 12vh auto 0;
    text-align: center;
  }

  .first-run h1 {
    margin: 0 0 12px;
    font-size: 24px;
    font-weight: 600;
  }

  .first-run p {
    margin: 0 0 22px;
    color: var(--muted, #aeb5be);
    font-size: 14px;
    line-height: 1.55;
  }

  .empty {
    display: flex;
    flex-direction: column;
    gap: 14px;
    align-items: center;
    margin-top: 14vh;
    color: var(--muted, #aeb5be);
  }

  .empty p,
  .notice {
    margin: 0;
    font-size: 14px;
  }

  .notice {
    margin-top: 14vh;
    color: var(--muted, #aeb5be);
    text-align: center;
  }

  .notice.failure {
    color: var(--oxide, #e5645e);
  }

  button:disabled {
    cursor: default;
    opacity: 0.55;
  }
</style>
