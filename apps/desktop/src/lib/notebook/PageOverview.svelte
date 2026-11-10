<script module lang="ts">
  /**
   * Thumbnails outlive the panel: closing and reopening it must not redraw every page. Each entry
   * remembers how many saves its page had when it was drawn; a later save makes it stale.
   */
  const thumbnailCache = new Map<string, { saves: number; url: string }>();

  export type OverviewPage = { id: string; widthPt: number; heightPt: number; calculation: boolean };
</script>

<script lang="ts">
  import { onDestroy, onMount, tick, untrack } from "svelte";
  import { t } from "../i18n";
  import { pageThumbnail } from "../library/cover";
  import { focusTarget, moveByOne, moveIntoGap, sameOrder, selectRange, toggleSelection } from "./pageOverview";

  /**
   * Every page of the notebook as a tile, docked at the right edge. It is not modal: the page
   * stays usable beside it, and clicking a tile scrolls there without closing the panel.
   */
  let {
    id,
    pages,
    activePageId,
    root,
    thumbnails,
    pageTextBaselineGrid,
    pageSaves,
    reducedMotion = false,
    onOpenPage,
    onGoToNumber,
    onReorder,
    onDelete,
    onClose,
  }: {
    /** Element id, so the toggle that opens the panel can point at it. */
    id: string;
    pages: OverviewPage[];
    activePageId: string;
    root: string;
    /** False without the desktop app: tiles stay plain paper. */
    thumbnails: boolean;
    pageTextBaselineGrid: boolean;
    /** Save count per page this session; a change redraws that page's tile. */
    pageSaves: Record<string, number>;
    reducedMotion?: boolean;
    onOpenPage: (pageId: string) => void;
    onGoToNumber: (pageNumber: number) => void;
    onReorder: (order: string[]) => void;
    onDelete: (pageIds: string[]) => void;
    onClose: () => void;
  } = $props();

  const COLUMNS = 2;
  const MAX_RENDERS = 2;
  const MOUSE_DRAG_PX = 4;
  const HOLD_MS = 350;
  /** A pen or finger that moves this far before the hold completes is scrolling, not dragging. */
  const HOLD_SLOP_PX = 8;
  const EDGE_SCROLL_PX = 40;

  const order = $derived(pages.map((page) => page.id));
  const currentNumber = $derived(Math.max(1, order.indexOf(activePageId) + 1));

  let grid = $state<HTMLElement>();
  let selected = $state<string[]>([]);
  let anchor = $state<string | null>(null);
  let focusId = $state<string | null>(null);
  let jump = $state<number | null>(null);
  let urls = $state<Record<string, string>>({});
  let dragIds = $state<string[]>([]);
  let dragGap = $state<number | null>(null);

  const rovingId = $derived(order.includes(focusId ?? "") ? focusId : order.includes(activePageId) ? activePageId : order[0]);
  const everySelected = $derived(selected.length >= pages.length);

  // Selected pages that no longer exist (deleted, or the notebook changed) drop out.
  $effect(() => {
    const present = new Set(order);
    untrack(() => {
      if (selected.some((pageId) => !present.has(pageId))) selected = selected.filter((pageId) => present.has(pageId));
    });
  });

  $effect(() => {
    const number = currentNumber;
    untrack(() => { jump = number; });
  });

  // ---- Thumbnails: only tiles in view, two at a time, cached per page for the session. ----

  const visible = new Set<string>();
  const inFlight = new Set<string>();
  /** Pages whose render failed, with their save count then: not retried until they save again. */
  const failedAt = new Map<string, number>();
  const queue: string[] = [];
  let running = 0;
  let destroyed = false;
  let observer: IntersectionObserver | undefined;

  const cacheKey = (pageId: string) => `${root}\n${pageId}`;

  function want(pageId: string) {
    const page = pages.find((candidate) => candidate.id === pageId);
    if (!thumbnails || !page || page.calculation) return;
    const saves = pageSaves[pageId] ?? 0;
    const cached = thumbnailCache.get(cacheKey(pageId));
    if (cached && urls[pageId] !== cached.url) urls = { ...urls, [pageId]: cached.url };
    if (cached?.saves === saves || failedAt.get(pageId) === saves) return;
    if (inFlight.has(pageId) || queue.includes(pageId)) return;
    queue.push(pageId);
    pump();
  }

  function pump() {
    while (running < MAX_RENDERS && queue.length > 0) {
      const pageId = queue.shift()!;
      if (visible.has(pageId)) void draw(pageId);
    }
  }

  async function draw(pageId: string) {
    running += 1;
    inFlight.add(pageId);
    const saves = pageSaves[pageId] ?? 0;
    const requestRoot = root;
    try {
      const blob = await pageThumbnail(requestRoot, pageId, pageTextBaselineGrid);
      if (destroyed || requestRoot !== root) return;
      if (!blob) {
        failedAt.set(pageId, saves);
        return;
      }
      const url = URL.createObjectURL(blob);
      const key = cacheKey(pageId);
      const previous = thumbnailCache.get(key);
      thumbnailCache.set(key, { saves, url });
      urls = { ...urls, [pageId]: url };
      if (previous) URL.revokeObjectURL(previous.url);
      failedAt.delete(pageId);
    } catch {
      // A missing thumbnail is a plain paper tile, never an error the writer has to act on.
      failedAt.set(pageId, saves);
    } finally {
      running -= 1;
      inFlight.delete(pageId);
      if (!destroyed) {
        // Saved again while drawing: draw once more.
        if ((pageSaves[pageId] ?? 0) !== saves) want(pageId);
        pump();
      }
    }
  }

  // A page that saves redraws, if it is in view; the old picture stays until the new one is ready.
  $effect(() => {
    void pageSaves;
    untrack(() => { for (const pageId of visible) want(pageId); });
  });

  function tile(node: HTMLElement, pageId: string) {
    node.dataset.overviewPage = pageId;
    observer?.observe(node);
    return {
      update(next: string) { node.dataset.overviewPage = next; },
      destroy() {
        observer?.unobserve(node);
        visible.delete(node.dataset.overviewPage ?? "");
      },
    };
  }

  onMount(() => {
    observer = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        const pageId = (entry.target as HTMLElement).dataset.overviewPage;
        if (!pageId) continue;
        if (entry.isIntersecting) {
          visible.add(pageId);
          want(pageId);
        } else visible.delete(pageId);
      }
    }, { root: grid, rootMargin: "120px 0px" });
    grid?.querySelectorAll<HTMLElement>("[data-overview-page]").forEach((node) => observer?.observe(node));
    grid?.addEventListener("touchmove", holdTouch, { passive: false });
    void tick().then(() => {
      const current = grid?.querySelector<HTMLElement>(".tile.current");
      current?.scrollIntoView({ block: "center" });
      current?.focus({ preventScroll: true });
    });
  });

  onDestroy(() => {
    destroyed = true;
    observer?.disconnect();
    grid?.removeEventListener("touchmove", holdTouch);
    stopPress();
  });

  // ---- Selection and keyboard ----

  function tileClick(event: MouseEvent, pageId: string) {
    if (suppressClick) {
      suppressClick = false;
      return;
    }
    focusId = pageId;
    if (event.ctrlKey || event.metaKey) {
      selected = toggleSelection(selected, pageId);
      anchor = pageId;
    } else if (event.shiftKey) {
      selected = selectRange(order, anchor ?? activePageId, pageId);
    } else {
      selected = [];
      anchor = pageId;
      onOpenPage(pageId);
    }
  }

  function focusTile(pageId: string | undefined) {
    if (!pageId) return;
    focusId = pageId;
    void tick().then(() => {
      grid?.querySelector<HTMLElement>(`[data-overview-page="${CSS.escape(pageId)}"]`)?.focus();
    });
  }

  function tileKeydown(event: KeyboardEvent, pageId: string) {
    const index = order.indexOf(pageId);
    if (event.altKey && event.key.startsWith("Arrow")) {
      event.preventDefault();
      const moving = selected.includes(pageId) ? selected : [pageId];
      const next = moveByOne(order, moving, event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 1);
      if (!sameOrder(order, next)) onReorder(next);
      focusTile(pageId);
      return;
    }
    if (event.key === " ") {
      event.preventDefault();
      selected = toggleSelection(selected, pageId);
      anchor = pageId;
      return;
    }
    const target = focusTarget(event.key, index, order.length, COLUMNS);
    if (target === null) return;
    event.preventDefault();
    focusTile(order[target]);
  }

  function panelKeydown(event: KeyboardEvent) {
    // Keys typed here belong to the panel. Without this, Delete or a digit would also reach the
    // workspace shortcuts and edit the page behind it. Ctrl/Cmd shortcuts such as undo still go.
    if (event.key !== "Escape") {
      if (!event.ctrlKey && !event.metaKey) event.stopPropagation();
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    if (press?.dragging) {
      stopPress();
      return;
    }
    onClose();
  }

  function goToTyped(event: KeyboardEvent) {
    if (event.key !== "Enter" || jump === null || !Number.isFinite(jump)) return;
    event.preventDefault();
    onGoToNumber(Math.min(pages.length, Math.max(1, Math.round(jump))));
  }

  function deleteSelected() {
    if (everySelected || selected.length === 0) return;
    const doomed = order.filter((pageId) => selected.includes(pageId));
    selected = [];
    onDelete(doomed);
  }

  // ---- Drag to reorder ----

  type Press = {
    pointerId: number;
    pageId: string;
    node: HTMLElement;
    x: number;
    y: number;
    mouse: boolean;
    dragging: boolean;
    timer?: ReturnType<typeof setTimeout>;
  };
  let press: Press | null = null;
  let suppressClick = false;
  let pointer = { x: 0, y: 0 };
  let scrollFrame = 0;

  function pressStart(event: PointerEvent, pageId: string) {
    if (event.button !== 0 || press) return;
    // A drag's click may never arrive (it can land on another element); it must not eat this one.
    suppressClick = false;
    const mouse = event.pointerType === "mouse";
    press = { pointerId: event.pointerId, pageId, node: event.currentTarget as HTMLElement, x: event.clientX, y: event.clientY, mouse, dragging: false };
    pointer = { x: event.clientX, y: event.clientY };
    // A pen or finger has to hold still first; otherwise the same gesture scrolls the grid.
    if (!mouse) press.timer = setTimeout(beginDrag, HOLD_MS);
  }

  function pressMove(event: PointerEvent) {
    if (!press || event.pointerId !== press.pointerId) return;
    pointer = { x: event.clientX, y: event.clientY };
    const moved = Math.hypot(event.clientX - press.x, event.clientY - press.y);
    if (!press.dragging) {
      if (press.mouse && moved > MOUSE_DRAG_PX) beginDrag();
      else if (!press.mouse && moved > HOLD_SLOP_PX) stopPress();
      return;
    }
    event.preventDefault();
    updateGap();
  }

  function pressEnd(event: PointerEvent) {
    if (!press || event.pointerId !== press.pointerId) return;
    if (press.dragging) {
      const gap = dragGap;
      const moving = dragIds;
      suppressClick = true;
      stopPress();
      if (gap !== null) {
        const next = moveIntoGap(order, moving, gap);
        if (!sameOrder(order, next)) onReorder(next);
      }
      return;
    }
    stopPress();
  }

  function beginDrag() {
    if (!press) return;
    press.dragging = true;
    if (press.timer) clearTimeout(press.timer);
    press.node.setPointerCapture(press.pointerId);
    dragIds = selected.includes(press.pageId) ? order.filter((pageId) => selected.includes(pageId)) : [press.pageId];
    updateGap();
    if (!scrollFrame) scrollFrame = requestAnimationFrame(edgeScroll);
  }

  function stopPress() {
    if (press?.timer) clearTimeout(press.timer);
    if (press?.node.hasPointerCapture(press.pointerId)) press.node.releasePointerCapture(press.pointerId);
    press = null;
    dragIds = [];
    dragGap = null;
    if (scrollFrame) cancelAnimationFrame(scrollFrame);
    scrollFrame = 0;
  }

  /** Which gap the pointer is over: before a tile on its left half, after it on its right half. */
  function updateGap() {
    if (!grid) return;
    const over = document.elementFromPoint(pointer.x, pointer.y)?.closest<HTMLElement>("[data-overview-page]");
    if (over && grid.contains(over)) {
      const index = order.indexOf(over.dataset.overviewPage ?? "");
      if (index < 0) return;
      const bounds = over.getBoundingClientRect();
      dragGap = pointer.x < bounds.left + bounds.width / 2 ? index : index + 1;
      return;
    }
    const bounds = grid.getBoundingClientRect();
    const last = grid.querySelector<HTMLElement>("[data-overview-page]:last-of-type")?.getBoundingClientRect();
    if (last && pointer.y > last.bottom && pointer.y < bounds.bottom) dragGap = order.length;
  }

  /** Near the top or bottom edge, the grid scrolls on its own so a drag can reach every page. */
  function edgeScroll() {
    scrollFrame = 0;
    if (!press?.dragging || !grid) return;
    const bounds = grid.getBoundingClientRect();
    const speed =
      pointer.y < bounds.top + EDGE_SCROLL_PX ? -Math.ceil((bounds.top + EDGE_SCROLL_PX - pointer.y) / 4)
      : pointer.y > bounds.bottom - EDGE_SCROLL_PX ? Math.ceil((pointer.y - bounds.bottom + EDGE_SCROLL_PX) / 4)
      : 0;
    if (speed !== 0) {
      grid.scrollTop += speed;
      updateGap();
    }
    scrollFrame = requestAnimationFrame(edgeScroll);
  }

  /** Once a held finger is dragging, its moves must not scroll the grid out from under it. */
  function holdTouch(event: TouchEvent) {
    if (press?.dragging && event.cancelable) event.preventDefault();
  }

  function insertion(index: number): "before" | "after" | null {
    if (dragGap === null) return null;
    if (dragGap === index) return "before";
    if (dragGap === order.length && index === order.length - 1) return "after";
    return null;
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<aside
  {id}
  class="page-overview"
  class:reduced-motion={reducedMotion}
  aria-label={$t("pageOverview.label")}
  onkeydown={panelKeydown}
>
  <header>
    <div class="title-row">
      <h2>{$t("pageOverview.title")}</h2>
      <button type="button" class="icon" aria-label={$t("pageOverview.close")} title={$t("pageOverview.close")} onclick={onClose}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>
    </div>
    <label class="jump">
      <span>{$t("workspace.goToPage")}</span>
      <input
        type="number"
        min="1"
        max={pages.length}
        bind:value={jump}
        onkeydown={goToTyped}
      />
      <span class="of">{$t("workspace.of", { pageCount: pages.length })}</span>
    </label>
    <p class="hint" id={`${id}-hint`}>{$t("pageOverview.hint")}</p>
  </header>

  <div
    bind:this={grid}
    class="grid"
    role="listbox"
    tabindex="-1"
    aria-multiselectable="true"
    aria-label={$t("pageOverview.title")}
    aria-describedby={`${id}-hint`}
  >
    {#each pages as page, index (page.id)}
      {@const place = insertion(index)}
      <button
        type="button"
        role="option"
        class="tile"
        class:current={page.id === activePageId}
        class:picked={selected.includes(page.id)}
        class:moving={dragIds.includes(page.id)}
        class:insert-before={place === "before"}
        class:insert-after={place === "after"}
        aria-selected={selected.includes(page.id)}
        aria-current={page.id === activePageId ? "page" : undefined}
        aria-label={$t("pageOverview.tile", { number: index + 1 })}
        tabindex={page.id === rovingId ? 0 : -1}
        use:tile={page.id}
        onclick={(event) => tileClick(event, page.id)}
        onkeydown={(event) => tileKeydown(event, page.id)}
        onkeyup={(event) => { if (event.key === " ") event.preventDefault(); }}
        onfocus={() => (focusId = page.id)}
        onpointerdown={(event) => pressStart(event, page.id)}
        onpointermove={pressMove}
        onpointerup={pressEnd}
        onpointercancel={() => stopPress()}
        oncontextmenu={(event) => event.preventDefault()}
      >
        <span class="paper" style:aspect-ratio={`${Math.max(page.widthPt, 1)} / ${Math.max(page.heightPt, 1)}`}>
          {#if page.calculation}
            <span class="kind">{$t("pageOverview.python")}</span>
          {:else if urls[page.id]}
            <img src={urls[page.id]} alt="" draggable="false" />
          {/if}
          {#if selected.includes(page.id)}
            <span class="check" aria-hidden="true">
              <svg viewBox="0 0 24 24"><path d="M6 12.5l4 4 8-9" /></svg>
            </span>
          {/if}
        </span>
        <span class="number">{index + 1}</span>
      </button>
    {/each}
  </div>

  {#if selected.length > 0}
    <footer class="selection-bar">
      <span class="count">{$t("pageOverview.selected", { count: selected.length })}</span>
      <button type="button" class="secondary" onclick={() => { selected = []; focusTile(rovingId ?? undefined); }}>{$t("pageOverview.clear")}</button>
      <button
        type="button"
        class="danger"
        aria-disabled={everySelected}
        aria-describedby={everySelected ? `${id}-keep-one` : undefined}
        title={everySelected ? $t("pageOverview.keepOnePage") : undefined}
        onclick={deleteSelected}
      >{$t("pageOverview.deletePages", { count: selected.length })}</button>
      {#if everySelected}<span id={`${id}-keep-one`} class="visually-hidden">{$t("pageOverview.keepOnePage")}</span>{/if}
    </footer>
  {/if}
</aside>

<style>
  .page-overview {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    z-index: 25;
    display: flex;
    width: min(304px, calc(100vw - 48px));
    flex-direction: column;
    border-left: 1px solid var(--edge);
    background: var(--panel);
    box-shadow: var(--shadow-panel);
    color: var(--text);
    animation: overview-in var(--enter) ease-out;
  }

  @keyframes overview-in {
    from { opacity: 0; transform: translateX(6px); }
  }

  @keyframes overview-fade {
    from { opacity: 0; }
  }

  .page-overview.reduced-motion { animation-name: overview-fade; }

  @media (prefers-reduced-motion: reduce) {
    .page-overview { animation-name: overview-fade; }
  }

  header {
    display: flex;
    flex: none;
    flex-direction: column;
    padding: 12px;
    border-bottom: 1px solid var(--edge-soft);
    gap: 8px;
  }

  .title-row { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  h2 { margin: 0; font-size: var(--text-lg); font-weight: 600; }

  button { border: 0; font: inherit; cursor: pointer; }
  button:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }

  .icon {
    display: grid;
    width: var(--control-dense);
    height: var(--control-dense);
    padding: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    place-items: center;
    transition: background var(--fast) ease, color var(--fast) ease;
  }

  .icon:hover { background: var(--wash); color: var(--text); }
  .icon svg {
    width: var(--icon-dense);
    height: var(--icon-dense);
    fill: none;
    stroke: currentColor;
    stroke-width: var(--stroke-dense);
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .jump { display: flex; align-items: center; color: var(--muted); font-size: var(--text-sm); gap: 8px; }
  .jump input {
    box-sizing: border-box;
    width: 56px;
    height: var(--control-dense);
    padding: 0 6px;
    border: 1px solid var(--edge);
    border-radius: var(--radius);
    background: var(--charcoal);
    caret-color: var(--blueprint-light);
    color: var(--text);
    font: inherit;
    font-size: var(--text-md);
    font-variant-numeric: tabular-nums;
    text-align: center;
    transition: border-color var(--fast) ease;
  }

  .jump input:hover { border-color: var(--edge-strong); }
  .jump input:focus-visible { border-color: var(--blueprint); outline: none; }
  .of { color: var(--quiet); font-variant-numeric: tabular-nums; }
  .hint { margin: 0; color: var(--quiet); font-size: var(--text-sm); }

  .grid {
    display: grid;
    min-height: 0;
    flex: 1;
    align-content: start;
    grid-template-columns: 1fr 1fr;
    padding: 12px;
    gap: 12px;
    overflow-y: auto;
    outline: none;
  }

  .tile {
    position: relative;
    display: flex;
    min-width: 0;
    flex-direction: column;
    align-items: center;
    padding: 8px;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    touch-action: pan-y;
    user-select: none;
    gap: 6px;
    transition: background var(--fast) ease;
  }

  .tile:hover { background: var(--wash); }
  .tile.current { background: var(--accent-fill); box-shadow: inset 0 0 0 1px var(--accent-edge); color: var(--text); }
  .tile.moving { background: var(--accent-tint); }

  /* The insertion line sits in the middle of the 12px gap beside the tile. */
  .tile.insert-before::before,
  .tile.insert-after::after {
    position: absolute;
    top: 4px;
    bottom: 4px;
    width: 2px;
    border-radius: 1px;
    background: var(--blueprint);
    content: "";
  }

  .tile.insert-before::before { left: -7px; }
  .tile.insert-after::after { right: -7px; }

  .paper {
    position: relative;
    display: grid;
    width: 100%;
    max-height: 220px;
    overflow: hidden;
    border-radius: 2px;
    background: var(--paper);
    box-shadow: var(--shadow-page);
    place-items: center;
  }

  .tile.picked .paper { outline: 2px solid var(--blueprint); outline-offset: 1px; }

  .paper img { display: block; width: 100%; height: 100%; object-fit: contain; pointer-events: none; }
  .kind { color: var(--quiet); font-size: var(--text-sm); }

  .check {
    position: absolute;
    top: 4px;
    right: 4px;
    display: grid;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    /* The ring around the paper is the tile's one blue; the badge only confirms it. */
    background: var(--panel);
    color: var(--text);
    place-items: center;
  }

  .check svg { width: 14px; height: 14px; fill: none; stroke: currentColor; stroke-width: var(--stroke); stroke-linecap: round; stroke-linejoin: round; }

  .number { font-size: var(--text-sm); font-variant-numeric: tabular-nums; }

  .selection-bar {
    display: flex;
    flex: none;
    align-items: center;
    padding: 10px 12px;
    border-top: 1px solid var(--edge-soft);
    gap: 8px;
  }

  .count { min-width: 0; flex: 1; color: var(--muted); font-size: var(--text-sm); font-variant-numeric: tabular-nums; }

  .secondary,
  .danger {
    height: var(--control);
    padding: 0 12px;
    border-radius: var(--radius);
    font-size: var(--text-md);
    font-weight: 600;
    transition: background var(--fast) ease;
  }

  .secondary { background: var(--wash); color: var(--text); }
  .secondary:hover { background: var(--wash-strong); }
  .danger { background: var(--oxide); color: var(--on-accent); }
  .danger:hover:not([aria-disabled="true"]) { background: var(--oxide-hover); }
  .danger[aria-disabled="true"] { opacity: var(--disabled); cursor: not-allowed; }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }

  @media (prefers-reduced-motion: reduce) {
    .tile, .icon, .secondary, .danger, .jump input { transition: none; }
  }

  @media (pointer: coarse) {
    .secondary, .danger { min-height: var(--control-touch); }
  }
</style>
