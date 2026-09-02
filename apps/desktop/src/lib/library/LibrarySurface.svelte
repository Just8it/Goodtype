<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import BrandMark from "../brand/BrandMark.svelte";
  import NamePrompt from "./NamePrompt.svelte";
  import NotebookSetup from "./NotebookSetup.svelte";
  import type { NotebookSetup as NotebookSetupValue } from "../page/presets";
  import NotebookCover from "./NotebookCover.svelte";
  import ShelfMenu, { type ShelfMenuItem } from "./ShelfMenu.svelte";
  import type { ShelfLocation, ShelfView } from "./location";
  import {
    createLibraryFolder,
    createLibraryNotebook,
    deleteLibraryEntry,
    libraryFavourites,
    libraryRoot as readLibraryRoot,
    listLibrary,
    listLibraryFavourites,
    openLibraryNotebook,
    pickLibraryRoot,
    pickNotebookRoot,
    renameLibraryEntry,
    setLibraryFavourite,
  } from "../ipc/library";
  import {
    bands,
    breadcrumb,
    canMoveLibraryEntries,
    parentPath,
    type LibraryEntry,
    type LibraryListing,
    type SortOrder,
  } from "./library";

  let {
    tauriAvailable,
    location = { view: "library", path: "" },
    onOpen,
    onCreate,
    onMove,
    returnLabel,
    onReturn,
    onLocationChange,
    onStatus,
    showNotebookSetup = false,
  }: {
    tauriAvailable: boolean;
    /**
     * Where the shelf was left.
     *
     * Held by the caller rather than here, because this component is unmounted for as long as a
     * notebook is open — so anything it remembers itself is forgotten the moment you open
     * something, and closing would drop you back at the root. Coming out of a notebook should
     * put you where you went in from.
     */
    location?: ShelfLocation;
    /** Hands back an absolute notebook root, which is what every notebook command takes. */
    onOpen: (root: string) => void;
    /** Same, for a directory that is not a notebook yet and must be filled. */
    onCreate: (root: string, setup: NotebookSetupValue) => void;
    /** The workspace saves and remaps any open tabs around the Rust-owned filesystem move. */
    onMove: (paths: string[], destination: string) => Promise<string | null>;
    returnLabel?: string;
    onReturn?: () => void;
    onLocationChange?: (location: ShelfLocation) => void;
    onStatus: (message: string) => void;
    showNotebookSetup?: boolean;
  } = $props();

  const COVER_WIDTH_PX = 152;

  type Prompt =
    | { kind: "folder" }
    | { kind: "notebook" }
    | { kind: "rename"; path: string; initial: string };

  type MoveIntent = {
    lead: LibraryEntry;
    paths: string[];
  };

  type DragSession = MoveIntent & {
    pointerId: number;
    pointerType: string;
    startX: number;
    startY: number;
    x: number;
    y: number;
    active: boolean;
  };

  type DropTarget = {
    kind: "folder" | "crumb";
    path: string;
  };

  let libraryRoot = $state<string | null>(null);
  // Seeded once from the caller and reported back on every move, so the two never fight: this
  // owns it while the shelf is on screen, the caller holds it while a notebook is.
  let view = $state<ShelfView>(untrack(() => location.view));
  let path = $state(untrack(() => location.path));
  let entries = $state.raw<LibraryEntry[]>([]);
  let favourites = $state.raw<string[]>([]);
  let order = $state<SortOrder>("name");
  let busy = $state(false);
  let failure = $state<string | null>(null);
  let mutationFailure = $state<string | null>(null);
  let setupShown = false;

  let menu = $state<"new" | "sort" | null>(null);
  let entryMenu = $state<string | null>(null);
  let prompt = $state<Prompt | null>(null);
  /** Null means plain browsing; a set — even an empty one — means select mode is on. */
  let picked = $state.raw<string[] | null>(null);
  let moving = $state.raw<MoveIntent | null>(null);
  let drag = $state<DragSession | null>(null);
  let dropTarget = $state<DropTarget | null>(null);
  let contentsElement: HTMLDivElement | undefined;
  let queuedDragPoint: { x: number; y: number } | null = null;
  let dragFrame: number | null = null;
  let suppressClickPath: string | null = null;

  const crumbs = $derived(breadcrumb(path));
  const shelf = $derived(bands(entries, order));
  const up = $derived(parentPath(path));
  const starred = $derived(new Set(favourites));
  const selecting = $derived(picked !== null);

  $effect(() => {
    void start();
  });

  onDestroy(() => clearDrag());

  async function start() {
    if (!tauriAvailable) return;
    try {
      libraryRoot = await readLibraryRoot();
      if (libraryRoot) {
        await reload();
        if (showNotebookSetup && !setupShown) {
          setupShown = true;
          prompt = { kind: "notebook" };
        }
      }
    } catch (error) {
      failure = message(error);
    }
  }

  async function chooseLibrary() {
    busy = true;
    try {
      const chosen = await pickLibraryRoot();
      if (!chosen) return;
      libraryRoot = chosen;
      view = "library";
      path = "";
      await reload();
      if (showNotebookSetup && !setupShown) {
        setupShown = true;
        prompt = { kind: "notebook" };
      }
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
      favourites = await libraryFavourites();
      if (view === "favourites") {
        entries = await listLibraryFavourites();
      } else {
        const wanted = path;
        const listing = await listLibrary(wanted);
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
    clearDrag();
    moving = null;
    view = "library";
    path = next;
    picked = null;
    onLocationChange?.({ view, path });
    await reload();
  }

  async function showFavourites() {
    clearDrag();
    moving = null;
    view = "favourites";
    picked = null;
    onLocationChange?.({ view, path });
    await reload();
  }

  async function openNotebook(notebookPath: string) {
    busy = true;
    try {
      onOpen(await openLibraryNotebook(notebookPath));
    } catch (error) {
      failure = message(error);
      busy = false;
    }
  }

  /** Run a change to the library, then re-read rather than patching the list in place. */
  async function mutate(work: () => Promise<unknown>, done?: string): Promise<boolean> {
    busy = true;
    mutationFailure = null;
    try {
      await work();
      if (done) onStatus(done);
      await reload();
      return true;
    } catch (error) {
      mutationFailure = message(error);
      busy = false;
      return false;
    }
  }

  async function confirmPrompt(name: string) {
    const pending = prompt;
    prompt = null;
    if (!pending) return;
    if (pending.kind === "folder") {
      await mutate(
        () => createLibraryFolder(path, name),
        `Ordner „${name}" angelegt`,
      );
    } else if (pending.kind === "rename") {
      await mutate(
        () => renameLibraryEntry(pending.path, name),
        `In „${name}" umbenannt`,
      );
    }
  }

  async function createNotebook(setup: NotebookSetupValue) {
    prompt = null;
    busy = true;
    try {
      const root = await createLibraryNotebook(path, setup.name);
      onCreate(root, setup);
    } catch (error) {
      failure = message(error);
      busy = false;
    }
  }

  async function toggleFavourite(entryPath: string) {
    await mutate(() =>
      setLibraryFavourite(entryPath, !starred.has(entryPath)),
    );
  }

  async function remove(paths: string[]) {
    await mutate(async () => {
      for (const each of paths) await deleteLibraryEntry(each);
    }, paths.length === 1 ? "In den Papierkorb verschoben" : `${paths.length} in den Papierkorb verschoben`);
    picked = null;
  }

  /** Move one tile or a whole selection onto a folder tile or crumb. */
  async function moveTo(sourcePaths: string[], destination: string) {
    clearDrag();
    dropTarget = null;
    moving = null;
    if (!canMoveLibraryEntries(sourcePaths, destination)) return;
    picked = null;
    let moveStatus: string | null = null;
    const moved = await mutate(async () => {
      moveStatus = await onMove(sourcePaths, destination);
    });
    if (moved) {
      onStatus(
        moveStatus ?? (sourcePaths.length === 1
          ? `„${pathName(sourcePaths[0])}“ verschoben`
          : `${sourcePaths.length} Elemente verschoben`),
      );
    }
  }

  function beginMove(lead: LibraryEntry, sourcePaths = [lead.path]) {
    moving = { lead, paths: [...sourcePaths] };
    entryMenu = null;
    menu = null;
  }

  function beginPickedMove() {
    const paths = picked ?? [];
    const lead = entries.find((entry) => paths.includes(entry.path));
    if (lead && paths.length > 0) beginMove(lead, paths);
  }

  function chooseDestination(destination: string) {
    if (moving && canMoveLibraryEntries(moving.paths, destination)) {
      void moveTo(moving.paths, destination);
    }
  }

  function dragPaths(entry: LibraryEntry): string[] {
    if (!selecting) return [entry.path];
    return picked?.includes(entry.path) ? [...picked] : [];
  }

  function beginEntryDrag(event: PointerEvent, entry: LibraryEntry) {
    if (
      busy ||
      moving ||
      !event.isPrimary ||
      event.button !== 0 ||
      event.pointerType === "touch"
    ) {
      return;
    }
    const paths = dragPaths(entry);
    if (paths.length === 0) return;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    drag = {
      lead: entry,
      paths,
      pointerId: event.pointerId,
      pointerType: event.pointerType,
      startX: event.clientX,
      startY: event.clientY,
      x: event.clientX,
      y: event.clientY,
      active: false,
    };
  }

  function continueEntryDrag(event: PointerEvent) {
    const session = drag;
    if (!session || session.pointerId !== event.pointerId) return;
    if (!session.active) {
      const threshold = session.pointerType === "pen" ? 4 : 6;
      if (Math.hypot(event.clientX - session.startX, event.clientY - session.startY) < threshold) {
        return;
      }
      entryMenu = null;
      menu = null;
      drag = { ...session, active: true, x: event.clientX, y: event.clientY };
    }
    event.preventDefault();
    queueDrag(event.clientX, event.clientY);
  }

  function finishEntryDrag(event: PointerEvent) {
    const session = drag;
    if (!session || session.pointerId !== event.pointerId) return;
    // Resolve once more at release: a fast pen can enter a target and lift before the queued
    // animation frame has painted its hover state.
    const destination = targetAt(session.paths, event.clientX, event.clientY)?.path;
    const wasActive = session.active;
    clearDrag();
    const target = event.currentTarget as HTMLElement;
    if (target.hasPointerCapture(event.pointerId)) target.releasePointerCapture(event.pointerId);
    if (!wasActive) return;

    event.preventDefault();
    event.stopPropagation();
    suppressClickPath = session.lead.path;
    window.setTimeout(() => {
      if (suppressClickPath === session.lead.path) suppressClickPath = null;
    }, 0);
    if (destination !== undefined) void moveTo(session.paths, destination);
  }

  function cancelEntryDrag(event?: PointerEvent) {
    if (!event || drag?.pointerId === event.pointerId) clearDrag();
  }

  function clearDrag() {
    if (dragFrame !== null && typeof cancelAnimationFrame !== "undefined") {
      cancelAnimationFrame(dragFrame);
    }
    dragFrame = null;
    queuedDragPoint = null;
    drag = null;
    dropTarget = null;
  }

  function queueDrag(x: number, y: number) {
    queuedDragPoint = { x, y };
    if (dragFrame !== null) return;
    dragFrame = requestAnimationFrame(updateDragFrame);
  }

  function updateDragFrame() {
    dragFrame = null;
    const point = queuedDragPoint;
    queuedDragPoint = null;
    const session = drag;
    if (!point || !session?.active) return;

    drag = { ...session, x: point.x, y: point.y };
    dropTarget = targetAt(session.paths, point.x, point.y);
    if (autoScroll(point.y)) queueDrag(point.x, point.y);
  }

  function targetAt(sourcePaths: string[], x: number, y: number): DropTarget | null {
    const target = document
      .elementFromPoint(x, y)
      ?.closest<HTMLElement>("[data-library-drop-path]");
    const destination = target?.dataset.libraryDropPath;
    const kind = target?.dataset.libraryDropKind;
    if (
      destination === undefined ||
      (kind !== "folder" && kind !== "crumb") ||
      !canMoveLibraryEntries(sourcePaths, destination)
    ) {
      return null;
    }
    return { kind, path: destination };
  }

  function autoScroll(pointerY: number): boolean {
    const element = contentsElement;
    if (!element) return false;
    const rect = element.getBoundingClientRect();
    const edge = 68;
    let delta = 0;
    if (pointerY < rect.top + edge) {
      delta = -12 * Math.min(1, (rect.top + edge - pointerY) / edge);
    } else if (pointerY > rect.bottom - edge) {
      delta = 12 * Math.min(1, (pointerY - (rect.bottom - edge)) / edge);
    }
    if (delta === 0) return false;
    const before = element.scrollTop;
    element.scrollTop += delta;
    return element.scrollTop !== before;
  }

  function isPointerTarget(kind: DropTarget["kind"], destination: string): boolean {
    return dropTarget?.kind === kind && dropTarget.path === destination;
  }

  function isMoveTarget(destination: string): boolean {
    return moving !== null && canMoveLibraryEntries(moving.paths, destination);
  }

  function handleWindowKeydown(event: KeyboardEvent) {
    if (event.key !== "Escape" || (!drag && !moving)) return;
    event.preventDefault();
    clearDrag();
    moving = null;
  }

  function dragPreviewStyle(session: DragSession): string {
    const left = Math.min(session.x + 16, window.innerWidth - 236);
    const top = Math.min(session.y + 16, window.innerHeight - 54);
    return `transform: translate3d(${Math.max(8, left)}px, ${Math.max(8, top)}px, 0)`;
  }

  function pathName(entryPath: string): string {
    return entryPath.split("/").at(-1) ?? entryPath;
  }

  function togglePicked(entryPath: string) {
    const current = picked ?? [];
    picked = current.includes(entryPath)
      ? current.filter((each) => each !== entryPath)
      : [...current, entryPath];
  }

  function activate(entry: LibraryEntry) {
    if (suppressClickPath === entry.path) return;
    if (moving) {
      if (entry.kind === "folder") chooseDestination(entry.path);
      return;
    }
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
        id: "move",
        label: "Verschieben…",
        onSelect: () => beginMove(entry),
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
      checked: order === "name",
      onSelect: () => (order = "name"),
    },
    {
      id: "modified",
      label: "Zuletzt geändert",
      checked: order === "modified",
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
      const chosen = await pickNotebookRoot();
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

<svelte:window onkeydown={handleWindowKeydown} onblur={() => clearDrag()} />

<div class="library" class:dragging={drag?.active}>
  <aside class="rail">
    <div class="brand">
      <BrandMark size={20} title="" />
      <span>goodtype</span>
    </div>

    {#if onReturn}
      <button type="button" class="return-to-notebook" onclick={onReturn}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m14 6-6 6 6 6" /></svg>
        <span>{returnLabel ?? "Offenes Notizbuch"}</span>
      </button>
    {/if}

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
          aria-current={view === "library" ? "page" : undefined}
          onclick={() => void show("")}
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="M4 5.5h6.5v13H4zM13.5 5.5H20v13h-6.5z" />
            <path d="M7.25 9h0M16.75 9h0" />
          </svg>
          <span class="grow">Bibliothek</span>
        </button>
        <button
          type="button"
          class="view"
          class:current={view === "favourites"}
          aria-current={view === "favourites" ? "page" : undefined}
          onclick={() => void showFavourites()}
        >
          <svg viewBox="0 0 24 24" aria-hidden="true">
            <path d="m12 4 2.4 5 5.6.7-4 3.9 1 5.4-5-2.7-5 2.7 1-5.4-4-3.9 5.6-.7L12 4Z" />
          </svg>
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

  <section class="shelf" aria-busy={busy}>
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
              class:drop={isPointerTarget("crumb", crumb.path)}
              class:moveTarget={isMoveTarget(crumb.path)}
              data-library-drop-path={crumb.path}
              data-library-drop-kind="crumb"
              aria-current={index === crumbs.length - 1 ? "page" : undefined}
              aria-label={moving && isMoveTarget(crumb.path)
                ? `${crumb.name}, Auswahl hierher verschieben`
                : crumb.name}
              disabled={busy || (moving !== null && !isMoveTarget(crumb.path))}
              onclick={() => moving ? chooseDestination(crumb.path) : void show(crumb.path)}
              >{crumb.name}</button
            >
          {/each}
        </nav>
      {/if}

      <div class="spacer"></div>

      {#if libraryRoot}
        <div class="anchor">
          <button
            type="button"
            class="control menu-control"
            aria-haspopup="menu"
            aria-expanded={menu === "sort"}
            disabled={busy || moving !== null}
            onclick={() => (menu = menu === "sort" ? null : "sort")}
          >
            <span>Sortieren: {order === "name" ? "Name" : "Datum"}</span>
            <svg class:open={menu === "sort"} viewBox="0 0 24 24" aria-hidden="true">
              <path d="m7 9.5 5 5 5-5" />
            </svg>
          </button>
          {#if menu === "sort"}
            <ShelfMenu label="Sortieren" items={sortItems} onClose={() => (menu = null)} />
          {/if}
        </div>

        <button
          type="button"
          class="control"
          aria-pressed={selecting}
          disabled={busy || moving !== null}
          onclick={() => (picked = selecting ? null : [])}
        >
          {selecting ? "Fertig" : "Auswählen"}
        </button>

        {#if view === "library"}
          <div class="anchor">
            <button
              type="button"
              class="primary menu-control"
              aria-haspopup="menu"
              aria-expanded={menu === "new"}
              disabled={busy || moving !== null}
              onclick={() => (menu = menu === "new" ? null : "new")}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
              <span>Neu</span>
              <svg class="menu-caret" class:open={menu === "new"} viewBox="0 0 24 24" aria-hidden="true">
                <path d="m7 9.5 5 5 5-5" />
              </svg>
            </button>
            {#if menu === "new"}
              <ShelfMenu label="Neu" items={newItems} onClose={() => (menu = null)} />
            {/if}
          </div>
        {/if}
      {/if}
    </header>

    {#if moving}
      <div class="action-bar move-bar" role="status">
        <span class="move-title">
          {moving.paths.length === 1 ? `„${moving.lead.name}“` : `${moving.paths.length} Elemente`} verschieben
        </span>
        <span class="move-help">Zielordner oder Pfad wählen</span>
        <div class="spacer"></div>
        <button type="button" class="control" onclick={() => (moving = null)}>Abbrechen</button>
      </div>
    {:else if selecting && picked && picked.length > 0}
      <div class="action-bar">
        <span>{picked.length} ausgewählt</span>
        <div class="spacer"></div>
        <button type="button" class="control" disabled={busy} onclick={beginPickedMove}>
          Verschieben…
        </button>
        <button type="button" class="control destructive" disabled={busy} onclick={() => void remove(picked ?? [])}>
          In den Papierkorb
        </button>
      </div>
    {/if}

    {#if mutationFailure}
      <div class="operation-error" role="alert">
        <span>{mutationFailure}</span>
        <div class="spacer"></div>
        <button
          type="button"
          class="error-dismiss"
          aria-label="Fehlermeldung schließen"
          onclick={() => (mutationFailure = null)}
        >
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18" /></svg>
        </button>
      </div>
    {/if}

    <div class="contents" bind:this={contentsElement}>
      {#if !tauriAvailable}
        <p class="notice">Die Bibliothek benötigt die Desktop-App, damit sie deinen Ordner lesen kann.</p>
      {:else if !libraryRoot}
        <div class="first-run">
          <div class="first-run-mark" aria-hidden="true">
            <svg viewBox="0 0 24 24">
              <path d="M3.5 7.5A1.5 1.5 0 0 1 5 6h4l2 2.5h8A1.5 1.5 0 0 1 20.5 10v8A1.5 1.5 0 0 1 19 19.5H5A1.5 1.5 0 0 1 3.5 18V7.5Z" />
            </svg>
          </div>
          <h1>Wo sollen deine Notizbücher liegen?</h1>
          <p>
            Wähle den Ordner, in dem Goodtype deine Notizbücher zeigt. Unterordner werden direkt
            übernommen; Goodtype legt keine zusätzliche Datenbank an.
          </p>
          <button type="button" class="primary" disabled={busy} onclick={chooseLibrary}>
            Bibliotheksordner wählen
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
                class:drop={isPointerTarget("folder", folder.path)}
                class:moveTarget={isMoveTarget(folder.path)}
                class:draggingSource={drag?.active && drag.paths.includes(folder.path)}
                data-library-drop-path={folder.path}
                data-library-drop-kind="folder"
                role="presentation"
              >
                <button
                  type="button"
                  class="hit"
                  aria-label={moving && isMoveTarget(folder.path)
                    ? `${folder.name}, Auswahl hierher verschieben`
                    : folder.name}
                  aria-disabled={moving !== null && !isMoveTarget(folder.path)}
                  aria-pressed={selecting ? picked?.includes(folder.path) : undefined}
                  disabled={busy}
                  onpointerdown={(event) => beginEntryDrag(event, folder)}
                  onpointermove={continueEntryDrag}
                  onpointerup={finishEntryDrag}
                  onpointercancel={cancelEntryDrag}
                  onlostpointercapture={cancelEntryDrag}
                  onclick={() => activate(folder)}
                >
                  <svg width="19" height="19" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                    <path
                      d="M3 7.5A1.5 1.5 0 0 1 4.5 6h4l2 2.5h7A1.5 1.5 0 0 1 19 10v7.5A1.5 1.5 0 0 1 17.5 19h-13A1.5 1.5 0 0 1 3 17.5v-10z"
                      stroke="currentColor"
                      stroke-width="1.5"
                    />
                  </svg>
                  <span class="grow"></span>
                  <span class="tile-name">{folder.name}</span>
                  <span class="tile-meta">
                    {isPointerTarget("folder", folder.path) ? "Hier ablegen" : items(folder.childCount)}
                  </span>
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
                class:draggingSource={drag?.active && drag.paths.includes(notebook.path)}
                role="presentation"
              >
                <button
                  type="button"
                  class="hit"
                  aria-label={`${notebook.name}, ${pages(notebook.pageCount)}, zuletzt geändert ${whenModified(notebook.modifiedMs)}`}
                  aria-pressed={selecting ? picked?.includes(notebook.path) : undefined}
                  disabled={busy || moving !== null}
                  onpointerdown={(event) => beginEntryDrag(event, notebook)}
                  onpointermove={continueEntryDrag}
                  onpointerup={finishEntryDrag}
                  onpointercancel={cancelEntryDrag}
                  onlostpointercapture={cancelEntryDrag}
                  onclick={() => activate(notebook)}
                >
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

{#if drag?.active}
  <div class="drag-preview" style={dragPreviewStyle(drag)} aria-hidden="true">
    {#if drag.lead.kind === "folder"}
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none">
        <path
          d="M3 7.5A1.5 1.5 0 0 1 4.5 6h4l2 2.5h7A1.5 1.5 0 0 1 19 10v7.5A1.5 1.5 0 0 1 17.5 19h-13A1.5 1.5 0 0 1 3 17.5v-10z"
          stroke="currentColor"
          stroke-width="1.5"
        />
      </svg>
    {:else}
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none">
        <path d="M6 3.5h8l4 4v13H6zM14 3.5v4h4" stroke="currentColor" stroke-width="1.5" />
      </svg>
    {/if}
    <span>
      {drag.paths.length === 1 ? drag.lead.name : `${drag.paths.length} Elemente`}
    </span>
    {#if drag.paths.length > 1}<span class="drag-count">{drag.paths.length}</span>{/if}
  </div>
{/if}

{#snippet tileMarks(entry: LibraryEntry)}
  {#if starred.has(entry.path)}
    <span class="star" aria-label="Favorit">
      <svg width="13" height="13" viewBox="0 0 24 24" aria-hidden="true">
        <path d="M12 4l2.4 5 5.6.7-4 3.9 1 5.4-5-2.7-5 2.7 1-5.4-4-3.9 5.6-.7L12 4z" />
      </svg>
    </span>
  {/if}
  {#if selecting}
    <span class="check" class:on={picked?.includes(entry.path)} aria-hidden="true"></span>
  {:else if !moving}
    <div class="anchor tile-menu">
      <button
        type="button"
        class="chevron"
        aria-label={`Aktionen für ${entry.name}`}
        title="Weitere Aktionen"
        aria-haspopup="menu"
        aria-expanded={entryMenu === entry.path}
        disabled={busy}
        onclick={() => (entryMenu = entryMenu === entry.path ? null : entry.path)}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <circle cx="5" cy="12" r="1.35" />
          <circle cx="12" cy="12" r="1.35" />
          <circle cx="19" cy="12" r="1.35" />
        </svg>
      </button>
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

{#if prompt?.kind === "notebook"}
  <NotebookSetup
    {busy}
    onConfirm={(setup) => void createNotebook(setup)}
    onCancel={() => (prompt = null)}
  />
{:else if prompt}
  <NamePrompt
    heading={prompt.kind === "folder"
      ? "Neuer Ordner"
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
    isolation: isolate;
  }

  .rail {
    display: flex;
    flex: none;
    flex-direction: column;
    width: 236px;
    padding: 12px;
    border-right: 1px solid var(--edge-soft, rgb(255 255 255 / 7%));
    background: var(--charcoal, #16181d);
  }

  .brand {
    display: flex;
    gap: 9px;
    align-items: center;
    min-height: var(--control, 36px);
    padding: 0 6px 12px;
    font-size: var(--text-lg, 15px);
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .return-to-notebook {
    display: flex;
    width: 100%;
    min-height: var(--control, 36px);
    align-items: center;
    gap: 7px;
    margin-bottom: 10px;
    padding: 6px 8px;
    border: 0;
    border-radius: var(--radius, 6px);
    background: transparent;
    color: var(--text, #e9ebee);
    font-size: var(--text-md, 13px);
    cursor: pointer;
    text-align: left;
    touch-action: manipulation;
    transition: background 120ms ease, color 120ms ease;
  }
  .return-to-notebook:hover { background: var(--wash, rgb(255 255 255 / 8%)); }

  .return-to-notebook svg,
  .view svg {
    width: var(--icon-dense, 16px);
    height: var(--icon-dense, 16px);
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: var(--stroke-dense, 2);
  }

  .where {
    display: flex;
    gap: 8px;
    align-items: center;
    min-height: var(--control, 36px);
    padding: 7px 8px;
    border: 0;
    border-radius: var(--radius, 6px);
    background: rgb(255 255 255 / 4%);
  }

  .where-text {
    flex: 1;
    min-width: 0;
  }

  .overline {
    color: var(--quiet, #6a727c);
    font-size: var(--text-xs, 10px);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .where-path {
    overflow: hidden;
    margin-top: 2px;
    color: var(--muted, #aeb5be);
    direction: rtl;
    font-size: var(--text-sm, 11px);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .views {
    display: flex;
    flex-direction: column;
    gap: 3px;
    margin-top: 10px;
  }

  .view {
    display: flex;
    gap: 10px;
    align-items: center;
    min-height: var(--control, 36px);
    padding: 6px 8px;
    border: 1px solid transparent;
    border-radius: var(--radius, 6px);
    background: transparent;
    color: var(--muted, #aeb5be);
    font: inherit;
    font-size: var(--text-md, 13px);
    text-align: left;
    cursor: pointer;
    touch-action: manipulation;
    transition: background 120ms ease, border-color 120ms ease, color 120ms ease;
  }

  .view:hover:not(.current) {
    background: var(--wash, rgb(255 255 255 / 8%));
  }

  .view.current {
    border-color: rgb(76 141 240 / 52%);
    background: rgb(76 141 240 / 16%);
    color: var(--text, #e9ebee);
  }

  .tally {
    color: var(--quiet, #6a727c);
    font-size: var(--text-sm, 11px);
    font-variant-numeric: tabular-nums;
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
    gap: 7px;
    align-items: center;
    height: 58px;
    padding: 0 24px;
    border-bottom: 1px solid var(--edge-soft, rgb(255 255 255 / 7%));
    background: var(--charcoal, #16181d);
  }

  .crumbs {
    display: flex;
    gap: 2px;
    align-items: center;
    min-width: 0;
    overflow: hidden;
  }

  .crumb,
  .here-label {
    flex: none;
    min-height: var(--control-dense, 28px);
    padding: 4px 7px;
    border: 0;
    border-radius: var(--radius, 6px);
    background: transparent;
    color: var(--muted, #aeb5be);
    font: inherit;
    font-size: var(--text-md, 13px);
    cursor: pointer;
    transition:
      color 120ms ease,
      background 120ms ease,
      box-shadow 140ms ease,
      transform 140ms cubic-bezier(0.16, 1, 0.3, 1);
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
    box-shadow: 0 0 0 1px var(--blueprint, #4c8df0), 0 5px 14px rgb(0 0 0 / 18%);
    background: rgb(76 141 240 / 20%);
    color: var(--text, #e9ebee);
    transform: translateY(-1px);
  }

  .crumb.moveTarget:not(.drop) {
    box-shadow: inset 0 0 0 1px rgb(76 141 240 / 42%);
    color: var(--text, #e9ebee);
  }

  .separator {
    color: var(--quiet, #6a727c);
    font-size: var(--text-sm, 11px);
  }

  .spacer {
    flex: 1;
  }

  .anchor {
    position: relative;
  }

  .control {
    display: inline-flex;
    min-height: var(--control, 36px);
    align-items: center;
    gap: 7px;
    padding: 0 11px;
    border: 0;
    border-radius: var(--radius, 6px);
    background: rgb(255 255 255 / 5%);
    color: var(--muted, #aeb5be);
    font: inherit;
    font-size: var(--text-md, 13px);
    white-space: nowrap;
    cursor: pointer;
    touch-action: manipulation;
    transition: background 120ms ease, color 120ms ease;
  }

  .control:hover:not(:disabled) {
    background: var(--wash, rgb(255 255 255 / 8%));
    color: var(--text, #e9ebee);
  }

  .control[aria-pressed="true"] {
    box-shadow: inset 0 0 0 1px rgb(76 141 240 / 52%);
    background: rgb(76 141 240 / 16%);
    color: var(--text, #e9ebee);
  }

  .control.destructive {
    color: var(--oxide, #e5645e);
  }

  .control.destructive:hover:not(:disabled) {
    background: rgb(229 100 94 / 12%);
  }

  .primary {
    display: inline-flex;
    min-height: var(--control, 36px);
    align-items: center;
    justify-content: center;
    gap: 7px;
    padding: 0 13px;
    border: 0;
    border-radius: var(--radius, 6px);
    background: var(--blueprint, #4c8df0);
    color: #0e1b31;
    font: inherit;
    font-size: var(--text-md, 13px);
    font-weight: 600;
    white-space: nowrap;
    cursor: pointer;
    touch-action: manipulation;
    transition: background 120ms ease, transform 120ms cubic-bezier(0.16, 1, 0.3, 1);
  }

  .primary:hover:not(:disabled) {
    background: var(--blueprint-light, #7fb0f7);
  }

  .primary:active:not(:disabled) {
    transform: translateY(1px);
  }

  .menu-control > svg {
    width: var(--icon-dense, 16px);
    height: var(--icon-dense, 16px);
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: var(--stroke-dense, 2);
  }

  .menu-control > svg:not(:first-child),
  .menu-control > .menu-caret {
    transition: transform 140ms cubic-bezier(0.16, 1, 0.3, 1);
  }

  .menu-control > svg.open {
    transform: rotate(180deg);
  }

  .action-bar {
    display: flex;
    flex: none;
    gap: 10px;
    align-items: center;
    min-height: 48px;
    padding: 6px 24px;
    border-bottom: 1px solid var(--edge-soft, rgb(255 255 255 / 7%));
    background: rgb(76 141 240 / 10%);
    font-size: var(--text-md, 13px);
  }

  .move-title {
    color: var(--text, #e9ebee);
    font-weight: 600;
  }

  .move-help {
    color: var(--muted, #aeb5be);
  }

  .operation-error {
    display: flex;
    flex: none;
    gap: 12px;
    align-items: center;
    min-height: 44px;
    padding: 8px 24px;
    border-bottom: 1px solid rgb(229 100 94 / 32%);
    background: rgb(229 100 94 / 10%);
    color: #f0aaa6;
    font-size: var(--text-md, 13px);
  }

  .error-dismiss {
    display: grid;
    flex: none;
    width: var(--control-dense, 28px);
    height: var(--control-dense, 28px);
    padding: 0;
    border: 0;
    border-radius: var(--radius, 6px);
    background: transparent;
    color: currentColor;
    font: inherit;
    cursor: pointer;
    place-items: center;
  }

  .error-dismiss:hover {
    background: var(--wash, rgb(255 255 255 / 8%));
  }

  .error-dismiss svg {
    width: var(--icon-dense, 16px);
    height: var(--icon-dense, 16px);
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-width: var(--stroke-dense, 2);
  }

  .contents {
    flex: 1;
    padding: 22px 24px 32px;
    overflow-y: auto;
    scrollbar-color: var(--quiet, #6a727c) transparent;
    scrollbar-width: thin;
    transition: opacity 120ms ease;
  }

  .contents::selection,
  .contents :global(::selection) {
    background: rgb(76 141 240 / 34%);
    color: var(--text, #e9ebee);
  }

  .contents::-webkit-scrollbar {
    width: 10px;
  }

  .contents::-webkit-scrollbar-thumb {
    border: 3px solid transparent;
    border-radius: var(--radius-pill, 999px);
    background: var(--quiet, #6a727c);
    background-clip: padding-box;
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
    font-size: var(--text-xs, 10px);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .band-count,
  .tile-meta {
    color: var(--quiet, #6a727c);
    font-size: var(--text-sm, 11px);
    font-variant-numeric: tabular-nums;
  }

  .band-rule {
    flex: 1;
    height: 1px;
    background: var(--edge-soft, rgb(255 255 255 / 7%));
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
    transition:
      opacity 150ms ease,
      transform 160ms cubic-bezier(0.16, 1, 0.3, 1);
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
    touch-action: manipulation;
    user-select: none;
    -webkit-user-drag: none;
  }

  /* Folders are short, wide and opaque; notebooks are tall paper. The silhouettes differ before
     any label is read, which is what lets the two bands be scanned rather than parsed. */
  .folder {
    width: 172px;
    height: 98px;
    border: 1px solid var(--edge, rgb(255 255 255 / 12%));
    border-radius: var(--radius, 6px);
    background: var(--panel, #23272f);
    color: var(--muted, #aeb5be);
    transition:
      border-color 120ms ease,
      background 120ms ease,
      box-shadow 140ms ease,
      opacity 150ms ease,
      transform 160ms cubic-bezier(0.16, 1, 0.3, 1);
  }

  .folder .hit {
    padding: 11px 13px;
  }

  .folder:hover:not(.draggingSource):not(.drop) {
    background: rgb(255 255 255 / 6%);
    border-color: rgb(255 255 255 / 18%);
  }

  .folder.drop {
    border-color: var(--blueprint, #4c8df0);
    outline: 1px solid var(--blueprint, #4c8df0);
    background: rgb(76 141 240 / 18%);
    box-shadow:
      0 10px 25px rgb(0 0 0 / 24%),
      0 0 0 5px rgb(76 141 240 / 12%);
    color: var(--text, #e9ebee);
    transform: translateY(-3px) scale(1.02);
  }

  .folder.moveTarget:not(.drop) {
    border-color: rgb(76 141 240 / 52%);
    background: rgb(76 141 240 / 8%);
  }

  .notebook {
    width: 152px;
  }

  @media (hover: hover) and (pointer: fine) {
    .tile:hover:not(.draggingSource) {
      transform: translateY(-1px);
    }

    .folder.drop {
      transform: translateY(-3px) scale(1.02);
    }
  }

  .tile.draggingSource {
    opacity: 0.32;
    transform: scale(0.985);
  }

  .library.dragging,
  .library.dragging .hit {
    cursor: grabbing;
  }

  .notebook .hit {
    gap: 8px;
  }

  .tile.chosen::after {
    position: absolute;
    border-radius: var(--radius-lg, 10px);
    content: "";
    inset: -4px;
    outline: 2px solid var(--blueprint-light, #7fb0f7);
    pointer-events: none;
  }

  .grow {
    flex: 1;
  }

  .tile-name {
    overflow: hidden;
    color: var(--text, #e9ebee);
    font-size: var(--text-md, 13px);
    font-weight: 500;
    line-height: 1.3;
    text-overflow: ellipsis;
    white-space: nowrap;
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
    color: var(--amber, #e0912b);
    place-items: center;
    pointer-events: none;
  }

  .notebook .star {
    top: 7px;
    left: 7px;
    right: auto;
  }

  .star path {
    fill: currentColor;
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
    pointer-events: none;
    transition: border-color 120ms ease, background 120ms ease, transform 120ms ease;
  }

  .check.on {
    border-color: var(--blueprint, #4c8df0);
    background: var(--blueprint, #4c8df0);
  }

  .check.on::after {
    position: absolute;
    top: 3px;
    left: 5px;
    width: 4px;
    height: 8px;
    border-right: 1.7px solid #10141a;
    border-bottom: 1.7px solid #10141a;
    content: "";
    transform: rotate(45deg);
  }

  .tile-menu {
    position: absolute;
    right: 4px;
    bottom: 4px;
  }

  .notebook .tile-menu {
    top: 7px;
    right: 7px;
    bottom: auto;
  }

  .chevron {
    display: grid;
    width: var(--control-dense, 28px);
    height: var(--control-dense, 28px);
    padding: 0;
    border: 0;
    border-radius: var(--radius, 6px);
    background: rgb(22 24 29 / 76%);
    color: var(--muted, #aeb5be);
    cursor: pointer;
    place-items: center;
    touch-action: manipulation;
    transition: background 120ms ease, color 120ms ease;
  }

  .tile:hover .chevron,
  .chevron[aria-expanded="true"],
  .chevron:focus-visible {
    background: var(--wash, rgb(255 255 255 / 8%));
    color: var(--text, #e9ebee);
  }

  .chevron svg {
    width: var(--icon-dense, 16px);
    height: var(--icon-dense, 16px);
    fill: currentColor;
  }

  .hit:focus-visible,
  .crumb:focus-visible,
  .control:focus-visible,
  .primary:focus-visible,
  .view:focus-visible,
  .link:focus-visible,
  .chevron:focus-visible {
    outline: 2px solid var(--blueprint-light, #7fb0f7);
    outline-offset: 1px;
  }

  .error-dismiss:focus-visible {
    outline: 2px solid var(--blueprint-light, #7fb0f7);
    outline-offset: 1px;
  }

  .link {
    flex: none;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--blueprint, #4c8df0);
    font: inherit;
    min-height: var(--control-dense, 28px);
    padding: 0 3px;
    border-radius: var(--radius, 6px);
    font-size: var(--text-sm, 11px);
    cursor: pointer;
    touch-action: manipulation;
  }

  .link:hover:not(:disabled) {
    background: var(--wash, rgb(255 255 255 / 8%));
  }

  .first-run {
    max-width: 460px;
    margin: 12vh auto 0;
    text-align: center;
  }

  .first-run-mark {
    display: grid;
    width: 46px;
    height: 46px;
    margin: 0 auto 16px;
    border: 1px solid var(--edge, rgb(255 255 255 / 12%));
    border-radius: var(--radius-lg, 10px);
    background: var(--panel, #23272f);
    color: var(--blueprint-light, #7fb0f7);
    place-items: center;
  }

  .first-run-mark svg {
    width: 24px;
    height: 24px;
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: var(--stroke, 1.7);
  }

  .first-run h1 {
    margin: 0 0 12px;
    font-size: 22px;
    font-weight: 600;
    letter-spacing: -0.02em;
    text-wrap: balance;
  }

  .first-run p {
    margin: 0 0 22px;
    color: var(--muted, #aeb5be);
    font-size: var(--text-md, 13px);
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
    font-size: var(--text-md, 13px);
  }

  .notice {
    margin-top: 14vh;
    color: var(--muted, #aeb5be);
    text-align: center;
  }

  .notice.failure {
    color: var(--oxide, #e5645e);
  }

  .drag-preview {
    position: fixed;
    z-index: 1000;
    top: 0;
    left: 0;
    display: flex;
    gap: 9px;
    align-items: center;
    width: max-content;
    max-width: 220px;
    height: 40px;
    padding: 0 11px;
    border: 1px solid var(--edge, rgb(255 255 255 / 12%));
    border-radius: var(--radius-lg, 10px);
    background: rgb(35 39 47 / 96%);
    box-shadow: 0 18px 44px rgb(0 0 0 / 55%);
    color: var(--text, #e9ebee);
    font-family: var(--font-ui, "Bahnschrift", system-ui, sans-serif);
    font-size: var(--text-md, 13px);
    font-weight: 600;
    pointer-events: none;
  }

  .drag-preview > span:not(.drag-count) {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .drag-count {
    display: grid;
    flex: none;
    min-width: 20px;
    height: 20px;
    padding: 0 5px;
    border-radius: var(--radius-pill, 999px);
    background: var(--blueprint, #4c8df0);
    color: #0e1b31;
    font-size: var(--text-xs, 10px);
    place-items: center;
  }

  button:disabled {
    cursor: default;
    opacity: 0.45;
  }

  @media (max-width: 760px) {
    .library {
      flex-direction: column;
    }

    .rail {
      width: 100%;
      flex-direction: row;
      flex-wrap: wrap;
      align-items: center;
      gap: 7px;
      padding: 8px 12px;
      border-right: 0;
      border-bottom: 1px solid var(--edge-soft, rgb(255 255 255 / 7%));
    }

    .brand {
      min-height: var(--control, 36px);
      padding: 0 6px 0 0;
    }

    .return-to-notebook {
      width: auto;
      margin: 0;
    }

    .where {
      min-width: 180px;
      flex: 1;
    }

    .views {
      flex: none;
      flex-direction: row;
      margin: 0;
    }

    .bar {
      padding: 0 14px;
      overflow-x: auto;
    }

    .contents {
      padding: 18px 16px 28px;
    }
  }

  @media (max-width: 520px) {
    .brand span,
    .where-text {
      display: none;
    }

    .where {
      min-width: auto;
      flex: none;
      background: transparent;
    }

    .link {
      padding: 0 8px;
    }

    .crumbs {
      overflow-x: auto;
    }

    .bar {
      gap: 5px;
    }
  }

  @media (pointer: coarse) {
    .return-to-notebook,
    .where,
    .view,
    .control,
    .primary {
      min-height: var(--control-touch, 44px);
    }

    .link,
    .error-dismiss,
    .chevron {
      min-width: var(--control-touch, 44px);
      min-height: var(--control-touch, 44px);
    }

    .bar {
      height: 64px;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .tile,
    .folder,
    .crumb,
    .primary,
    .menu-control > svg,
    .check {
      transition: none;
    }

    .tile:hover:not(.draggingSource),
    .folder.drop,
    .crumb.drop,
    .tile.draggingSource {
      transform: none;
    }
  }
</style>
