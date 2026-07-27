<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import BrandMark from "../brand/BrandMark.svelte";
  import NamePrompt from "./NamePrompt.svelte";
  import NotebookCover from "./NotebookCover.svelte";
  import ShelfMenu, { type ShelfMenuItem } from "./ShelfMenu.svelte";
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
    onCreate,
    onStatus,
  }: {
    tauriAvailable: boolean;
    /** Hands back an absolute notebook root, which is what every notebook command takes. */
    onOpen: (root: string) => void;
    /** Same, for a directory that is not a notebook yet and must be filled. */
    onCreate: (root: string) => void;
    onStatus: (message: string) => void;
  } = $props();

  const COVER_WIDTH_PX = 152;

  type Prompt =
    | { kind: "folder" }
    | { kind: "notebook" }
    | { kind: "rename"; path: string; initial: string };

  let libraryRoot = $state<string | null>(null);
  let view = $state<"library" | "favourites">("library");
  let path = $state("");
  let entries = $state.raw<LibraryEntry[]>([]);
  let favourites = $state.raw<string[]>([]);
  let order = $state<SortOrder>("name");
  let busy = $state(false);
  let failure = $state<string | null>(null);

  let menu = $state<"new" | "sort" | null>(null);
  let entryMenu = $state<string | null>(null);
  let prompt = $state<Prompt | null>(null);
  /** Null means plain browsing; a set — even an empty one — means select mode is on. */
  let picked = $state.raw<string[] | null>(null);
  let dragging = $state<string | null>(null);
  let dropTarget = $state<string | null>(null);

  const crumbs = $derived(breadcrumb(path));
  const shelf = $derived(bands(entries, order));
  const up = $derived(parentPath(path));
  const starred = $derived(new Set(favourites));
  const selecting = $derived(picked !== null);

  $effect(() => {
    void start();
  });

  async function start() {
    if (!tauriAvailable) return;
    try {
      libraryRoot = await invoke<string | null>("library_root");
      if (libraryRoot) await reload();
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
      view = "library";
      path = "";
      await reload();
      onStatus(`Library set to ${chosen}`);
    } catch (error) {
      failure = message(error);
    } finally {
      busy = false;
    }
  }

  /**
   * Read whatever the current view is showing.
   *
   * A folder listing echoes the path it was asked for, and anything answering for a folder the
   * writer has already navigated away from is dropped. Without that, a slow listing of a deep
   * folder can land after a fast one of its parent and repaint the wrong shelf.
   */
  async function reload() {
    busy = true;
    failure = null;
    try {
      favourites = await invoke<string[]>("library_favourites");
      if (view === "favourites") {
        entries = await invoke<LibraryEntry[]>("list_library_favourites");
      } else {
        const wanted = path;
        const listing = await invoke<LibraryListing>("list_library", { path: wanted });
        if (listing.path !== path) return;
        entries = listing.entries;
      }
    } catch (error) {
      entries = [];
      failure = message(error);
    } finally {
      busy = false;
    }
  }

  async function show(next: string) {
    view = "library";
    path = next;
    picked = null;
    await reload();
  }

  async function showFavourites() {
    view = "favourites";
    picked = null;
    await reload();
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

  /** Run a change to the library, then re-read rather than patching the list in place. */
  async function mutate(work: () => Promise<unknown>, done?: string) {
    busy = true;
    try {
      await work();
      if (done) onStatus(done);
      await reload();
    } catch (error) {
      failure = message(error);
      busy = false;
    }
  }

  async function confirmPrompt(name: string) {
    const pending = prompt;
    prompt = null;
    if (!pending) return;
    if (pending.kind === "folder") {
      await mutate(
        () => invoke("create_library_folder", { parent: path, name }),
        `Ordner „${name}" angelegt`,
      );
    } else if (pending.kind === "rename") {
      await mutate(
        () => invoke("rename_library_entry", { path: pending.path, name }),
        `In „${name}" umbenannt`,
      );
    } else {
      busy = true;
      try {
        const root = await invoke<string>("create_library_notebook", { parent: path, name });
        // The directory exists and is empty; the notebook itself is written by the same path
        // that has always created one, so the store keeps a single author for its own files.
        onCreate(root);
      } catch (error) {
        failure = message(error);
        busy = false;
      }
    }
  }

  async function toggleFavourite(entryPath: string) {
    await mutate(() =>
      invoke("set_library_favourite", {
        path: entryPath,
        favourite: !starred.has(entryPath),
      }),
    );
  }

  async function remove(paths: string[]) {
    await mutate(async () => {
      for (const each of paths) await invoke("delete_library_entry", { path: each });
    }, paths.length === 1 ? "In den Papierkorb verschoben" : `${paths.length} in den Papierkorb verschoben`);
    picked = null;
  }

  /**
   * Move by dragging onto a folder tile or onto a crumb.
   *
   * This is why there is no folder tree: a crumb is already a visible, correctly-sized target
   * for "put this further up", and a folder tile is one for "put this in there". Neither needed
   * a second navigation surface to exist.
   */
  async function moveTo(source: string, destination: string) {
    dragging = null;
    dropTarget = null;
    if (source === destination) return;
    await mutate(
      () => invoke("move_library_entry", { path: source, destination }),
      "Verschoben",
    );
  }

  function togglePicked(entryPath: string) {
    const current = picked ?? [];
    picked = current.includes(entryPath)
      ? current.filter((each) => each !== entryPath)
      : [...current, entryPath];
  }

  function activate(entry: LibraryEntry) {
    if (selecting) {
      togglePicked(entry.path);
      return;
    }
    if (entry.kind === "folder") void show(entry.path);
    else void openNotebook(entry.path);
  }

  function entryMenuItems(entry: LibraryEntry): ShelfMenuItem[] {
    return [
      {
        id: "rename",
        label: "Umbenennen",
        onSelect: () => (prompt = { kind: "rename", path: entry.path, initial: entry.name }),
      },
      {
        id: "favourite",
        label: starred.has(entry.path) ? "Favorit entfernen" : "Zu Favoriten",
        onSelect: () => void toggleFavourite(entry.path),
      },
      {
        id: "delete",
        label: "In den Papierkorb",
        destructive: true,
        onSelect: () => void remove([entry.path]),
      },
    ];
  }

  const sortItems: ShelfMenuItem[] = $derived([
    {
      id: "name",
      label: "Name",
      marker: order === "name" ? "✓" : undefined,
      onSelect: () => (order = "name"),
    },
    {
      id: "modified",
      label: "Zuletzt geändert",
      marker: order === "modified" ? "✓" : undefined,
      onSelect: () => (order = "modified"),
    },
  ]);

  const newItems: ShelfMenuItem[] = $derived([
    { id: "notebook", label: "Neues Notizbuch", onSelect: () => (prompt = { kind: "notebook" }) },
    { id: "folder", label: "Neuer Ordner", onSelect: () => (prompt = { kind: "folder" }) },
    // The way out of the library, kept because a notebook someone sends you lands in Downloads
    // and has to be openable without being filed first.
    { id: "elsewhere", label: "Notizbuch von außerhalb…", onSelect: () => void openElsewhere() },
  ]);

  async function openElsewhere() {
    busy = true;
    try {
      const chosen = await invoke<string | null>("pick_notebook_root");
      if (chosen) onOpen(chosen);
      else busy = false;
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

      <nav class="views">
        <button
          type="button"
          class="view"
          class:current={view === "library"}
          onclick={() => void show("")}>Bibliothek</button
        >
        <button
          type="button"
          class="view"
          class:current={view === "favourites"}
          onclick={() => void showFavourites()}
        >
          <span class="grow">Favoriten</span>
          {#if favourites.length > 0}<span class="tally">{favourites.length}</span>{/if}
        </button>
      </nav>
    {/if}

    <!-- The design put a folder tree here. It is deliberately absent: at the root it lists
         exactly what the grid already shows, it needs expand state and a cached listing per node
         that every rename and move has to invalidate, and it is the one control on this surface
         you would have to put the pen down for. Jumping between branches is what Favoriten and
         search are for, and moving is a drag onto a crumb or a folder tile. -->
  </aside>

  <section class="shelf">
    <header class="bar">
      {#if view === "favourites"}
        <span class="here-label">Favoriten</span>
      {:else}
        <nav class="crumbs" aria-label="Pfad">
          {#each crumbs as crumb, index (crumb.path)}
            {#if index > 0}<span class="separator" aria-hidden="true">/</span>{/if}
            <button
              type="button"
              class="crumb"
              class:here={index === crumbs.length - 1}
              class:drop={dropTarget === `crumb:${crumb.path}`}
              aria-current={index === crumbs.length - 1 ? "page" : undefined}
              disabled={busy}
              ondragover={(event) => {
                if (!dragging) return;
                event.preventDefault();
                dropTarget = `crumb:${crumb.path}`;
              }}
              ondragleave={() => (dropTarget = null)}
              ondrop={(event) => {
                event.preventDefault();
                if (dragging) void moveTo(dragging, crumb.path);
              }}
              onclick={() => void show(crumb.path)}>{crumb.name}</button
            >
          {/each}
        </nav>
      {/if}

      <div class="spacer"></div>

      {#if libraryRoot}
        <div class="anchor">
          <button type="button" class="control" disabled={busy} onclick={() => (menu = menu === "sort" ? null : "sort")}>
            Sortieren: {order === "name" ? "Name" : "Datum"}
          </button>
          {#if menu === "sort"}
            <ShelfMenu label="Sortieren" items={sortItems} onClose={() => (menu = null)} />
          {/if}
        </div>

        <button
          type="button"
          class="control"
          disabled={busy}
          onclick={() => (picked = selecting ? null : [])}
        >
          {selecting ? "Fertig" : "Auswählen"}
        </button>

        {#if view === "library"}
          <div class="anchor">
            <button type="button" class="primary" disabled={busy} onclick={() => (menu = menu === "new" ? null : "new")}>
              + Neu
            </button>
            {#if menu === "new"}
              <ShelfMenu label="Neu" items={newItems} onClose={() => (menu = null)} />
            {/if}
          </div>
        {/if}
      {/if}
    </header>

    {#if selecting && picked && picked.length > 0}
      <div class="action-bar">
        <span>{picked.length} ausgewählt</span>
        <div class="spacer"></div>
        <button type="button" class="control" disabled={busy} onclick={() => void remove(picked ?? [])}>
          In den Papierkorb
        </button>
      </div>
    {/if}

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
          <p>
            {busy
              ? "Wird gelesen…"
              : view === "favourites"
                ? "Noch keine Favoriten."
                : "Dieser Ordner ist leer."}
          </p>
          {#if view === "library" && up !== null && !busy}
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
              <div
                class="tile folder"
                class:chosen={picked?.includes(folder.path)}
                class:drop={dropTarget === folder.path}
                role="presentation"
                draggable={!selecting}
                ondragstart={() => (dragging = folder.path)}
                ondragend={() => ((dragging = null), (dropTarget = null))}
                ondragover={(event) => {
                  if (!dragging || dragging === folder.path) return;
                  event.preventDefault();
                  dropTarget = folder.path;
                }}
                ondragleave={() => (dropTarget = null)}
                ondrop={(event) => {
                  event.preventDefault();
                  if (dragging) void moveTo(dragging, folder.path);
                }}
              >
                <button type="button" class="hit" disabled={busy} onclick={() => activate(folder)}>
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
                {@render tileMarks(folder)}
              </div>
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
              <div
                class="tile notebook"
                class:chosen={picked?.includes(notebook.path)}
                role="presentation"
                draggable={!selecting}
                ondragstart={() => (dragging = notebook.path)}
                ondragend={() => ((dragging = null), (dropTarget = null))}
              >
                <button type="button" class="hit" disabled={busy} onclick={() => activate(notebook)}>
                  <NotebookCover
                    paper={notebook.paper}
                    path={notebook.path}
                    widthPx={COVER_WIDTH_PX}
                  />
                  <span class="tile-name">{notebook.name}</span>
                  <span class="tile-meta">
                    {pages(notebook.pageCount)} · {whenModified(notebook.modifiedMs)}
                  </span>
                </button>
                {@render tileMarks(notebook)}
              </div>
            {/each}
          </div>
        {/if}
      {/if}
    </div>
  </section>
</div>

{#snippet tileMarks(entry: LibraryEntry)}
  {#if starred.has(entry.path)}
    <span class="star" aria-label="Favorit">
      <svg width="13" height="13" viewBox="0 0 24 24" aria-hidden="true">
        <path d="M12 4l2.4 5 5.6.7-4 3.9 1 5.4-5-2.7-5 2.7 1-5.4-4-3.9 5.6-.7L12 4z" fill="#E0912B" />
      </svg>
    </span>
  {/if}
  {#if selecting}
    <span class="check" class:on={picked?.includes(entry.path)} aria-hidden="true"></span>
  {:else}
    <div class="anchor tile-menu">
      <button
        type="button"
        class="chevron"
        aria-label={`Aktionen für ${entry.name}`}
        disabled={busy}
        onclick={() => (entryMenu = entryMenu === entry.path ? null : entry.path)}>▾</button
      >
      {#if entryMenu === entry.path}
        <ShelfMenu
          label={`Aktionen für ${entry.name}`}
          title={entry.name}
          items={entryMenuItems(entry)}
          onClose={() => (entryMenu = null)}
        />
      {/if}
    </div>
  {/if}
{/snippet}

{#if prompt}
  <NamePrompt
    heading={prompt.kind === "folder"
      ? "Neuer Ordner"
      : prompt.kind === "notebook"
        ? "Neues Notizbuch"
        : "Umbenennen"}
    confirmLabel={prompt.kind === "rename" ? "Umbenennen" : "Anlegen"}
    initial={prompt.kind === "rename" ? prompt.initial : ""}
    {busy}
    onConfirm={(name) => void confirmPrompt(name)}
    onCancel={() => (prompt = null)}
  />
{/if}

<style>
  .library {
    position: relative;
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
    direction: rtl;
    font-size: 11.5px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .views {
    display: flex;
    flex-direction: column;
    gap: 1px;
    margin-top: 14px;
  }

  .view {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 8px 10px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    color: var(--muted, #aeb5be);
    font: inherit;
    font-size: 13.5px;
    text-align: left;
    cursor: pointer;
  }

  .view:hover:not(.current) {
    background: rgb(255 255 255 / 5%);
  }

  .view.current {
    outline: 1px solid rgb(76 141 240 / 50%);
    background: rgb(76 141 240 / 16%);
    color: var(--text, #e9ebee);
  }

  .tally {
    color: var(--quiet, #6a727c);
    font-size: 11px;
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
    gap: 10px;
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

  .crumb,
  .here-label {
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

  .here-label {
    color: var(--text, #e9ebee);
    font-weight: 600;
    cursor: default;
  }

  .crumb:hover:not(:disabled) {
    background: rgb(255 255 255 / 6%);
  }

  /* The last crumb is where you are, so it is the one thing here that is not a way to leave. */
  .crumb.here {
    color: var(--text, #e9ebee);
    font-weight: 600;
  }

  .crumb.drop {
    outline: 1px solid var(--blueprint, #4c8df0);
    background: rgb(76 141 240 / 20%);
  }

  .separator {
    color: var(--quiet, #6a727c);
    font-size: 13px;
  }

  .spacer {
    flex: 1;
  }

  .anchor {
    position: relative;
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
    white-space: nowrap;
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
    white-space: nowrap;
    cursor: pointer;
  }

  .action-bar {
    display: flex;
    flex: none;
    gap: 10px;
    align-items: center;
    padding: 10px 24px;
    border-bottom: 1px solid rgb(255 255 255 / 12%);
    background: rgb(76 141 240 / 10%);
    font-size: 13px;
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

  .tile {
    position: relative;
  }

  .hit {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    padding: 0;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  /* Folders are short, wide and opaque; notebooks are tall paper. The silhouettes differ before
     any label is read, which is what lets the two bands be scanned rather than parsed. */
  .folder {
    width: 172px;
    height: 98px;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 7px;
    background: var(--panel, #23272f);
    color: var(--muted, #aeb5be);
  }

  .folder .hit {
    padding: 11px 13px;
  }

  .folder:hover {
    background: #2a2f38;
  }

  .folder.drop {
    outline: 2px solid var(--blueprint, #4c8df0);
    background: rgb(76 141 240 / 18%);
  }

  .notebook {
    width: 152px;
  }

  .notebook .hit {
    gap: 8px;
  }

  .tile.chosen::after {
    position: absolute;
    border-radius: 9px;
    content: "";
    inset: -4px;
    outline: 2px solid var(--blueprint, #4c8df0);
    pointer-events: none;
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

  .star {
    position: absolute;
    top: 7px;
    right: 7px;
    display: grid;
    width: 22px;
    height: 22px;
    border-radius: 3px;
    background: rgb(22 24 29 / 82%);
    place-items: center;
    pointer-events: none;
  }

  .notebook .star {
    top: 7px;
    left: 7px;
    right: auto;
  }

  .check {
    position: absolute;
    top: 7px;
    left: 7px;
    width: 18px;
    height: 18px;
    border: 1.5px solid rgb(255 255 255 / 55%);
    border-radius: 50%;
    background: rgb(22 24 29 / 82%);
  }

  .check.on {
    border-color: var(--blueprint, #4c8df0);
    background: var(--blueprint, #4c8df0);
  }

  .tile-menu {
    position: absolute;
    right: 2px;
    bottom: 2px;
  }

  .chevron {
    width: 22px;
    height: 22px;
    padding: 0;
    border: 0;
    border-radius: 3px;
    background: transparent;
    color: var(--quiet, #6a727c);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }

  .tile:hover .chevron,
  .chevron:focus-visible {
    background: rgb(255 255 255 / 8%);
    color: var(--text, #e9ebee);
  }

  .hit:focus-visible,
  .crumb:focus-visible,
  .control:focus-visible,
  .primary:focus-visible,
  .view:focus-visible,
  .link:focus-visible,
  .chevron:focus-visible {
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
