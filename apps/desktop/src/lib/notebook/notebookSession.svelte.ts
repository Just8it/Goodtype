import type { createSelection } from "./selectionEditing.svelte";
import type { createNavigation } from "./viewportNavigation.svelte";
import { shapeMessage } from "../i18n/labels";
import { translate, notice, type UiText } from "../i18n";
import { notebookFailureCode } from "../ipc/errors";
import { tick } from "svelte";

import { commitNotebook, createNotebook, createPage, deletePage, duplicatePage, focusPage, importPdfPages, openNotebook, openPage, pickPdfReference, reorderPages, runHistory, runStructureHistory } from "../ipc/notebook";
import { discardRecoveryCandidate, listRecoveryCandidates, restoreRecoveryCandidate } from "../ipc/recovery";
import { moveLibraryEntries } from "../ipc/library";
import { type TypstCompileRequest } from "../ipc/typst";
import type { createNotebookPreviews } from "../editor/notebookPreviews";

import type { NotebookSnapshot } from "../ipc/types";
import { closeNotebookSession, exportNotebookPdf, listRecentNotebooks, recordNotebookOpened, recordNotebookPage, recordNotebookSession, writeNotebookCover } from "../ipc/workspace";
import { pageAtViewportCenter } from "../geometry/coordinates";

import { DEFAULT_INK_Z_INDEX, SCHEMA_VERSION, type NotebookManifest, type PageBackground, type PageGeometry, type PageObject, type PagePosition, type ShapeStyle, type Stroke } from "../model";
import { type TypstCompileResult } from "../editor/typst";
import { clearTypstCache } from "../editor/typstCache";
import { listTypstPresets } from "../ipc/presets";
import { type NotebookSetup, type PresetSummary } from "../page/presets";

import { type InkTool } from "../ink/pipeline";
import { moveSelected, scaleSelected, transformedPoint } from "../ink/selection";

import { pdfPageGeometries, releasePdfDocuments } from "../pdf/document";

import { withBlockFont } from "../editor/blockFont";

import { AssetUrlCache, blockViewsFromSnapshot, imageViewsFromSnapshot, pageTypstViewFromSnapshot, shapeViewsFromSnapshot, strokesFromSnapshot, inkLayersWithStrokes, reuseStrokeObjects, type TypstBlockState, type ImageView, type PageTypstView, type ShapeEditCommit, type ShapeView, type TypstTransform } from "./pageView";
import type { ShapeDraft } from "../shape/geometry";
import { recognizeHeldShape } from "../shape/recognize";
import { createCommitTimer } from "./commitTimer";

import { blankNotebookSnapshot } from "./newNotebook";
import { createInkCommitter, type InkCommitter } from "./inkCommitter";
import { createPageLoader, retainedPageIds } from "./pageCache";
import { projectSnapshot } from "./snapshot";

import { closedTab, openedTab, type NotebookTab } from "./tabs";

import { notebookCover } from "../library/cover";
import { type AppSettings, type RecoveryCandidate, type SearchHit } from "../settings";
type PageEntry = {
    id: string;
    path: string;
    /** From the manifest's hint until the page loads, then from the page itself. */
    geometry: PageGeometry;
    snapshot: NotebookSnapshot | null;
  };

type PageTypstBlockState = PageTypstView & {
    result: TypstCompileResult | null;
  };

type NotebookAction =
    | { kind: "page"; pageId: string }
    | { kind: "structure" };

export type SessionEnvironment = {
  addPageOpen: boolean;
  readonly collectMetrics: boolean;
  readonly followPageText: (id: string | null) => void;
  readonly message: (error: unknown) => string;
  moreOpen: boolean;
  readonly navigation: Pick<ReturnType<typeof createNavigation>, "announceTouchHistory" | "automaticPageFocusLocked" | "pagePointers" | "panFrom" | "pinchStart" | "stopTouchInertia" | "touchInertiaFrame" | "touchPanStart">;
  pageViewport: HTMLElement | undefined;
  readonly paletteDragging: boolean;
  readonly previews: Pick<ReturnType<typeof createNotebookPreviews>, "compile" | "reset">;
  readonly resetWorkspaceEditors: () => void;
  readonly saveTemplate: () => Promise<boolean>;
  searchOpen: boolean;
  readonly selection: Pick<ReturnType<typeof createSelection>, "editingShapeId" | "groupedStrokeIds" | "lassoObjectIds" | "mixedGroup" | "selectedImageId" | "selectedShapeId" | "selectedStrokeIds" | "selectedTypstId" | "updateInkSelection">;
  readonly settings: AppSettings;
  readonly shapeView: (draft: ShapeDraft, overrides?: Partial<ShapeView>) => ShapeView;
  readonly sideEditor: { focus: () => void } | undefined;
  readonly sideEditorOpen: boolean;
  tauriAvailable: boolean;
  tool: InkTool;
  windowCloseInProgress: boolean;
};

/** Instance-owned session state and operations. Callers supply live values at the boundary. */
export function createSession(env: SessionEnvironment) {
  let disposed = false;
  let focusRetryTimer: ReturnType<typeof setTimeout> | undefined;

  // A4 exactly: 210mm x 297mm at 72pt/inch. It was rounded to 595x842, which is a tenth of a
  // millimetre narrow — invisible on its own, and wrong the moment a template promises 5mm
  // squares measured against it.
  const PAGE_WIDTH_PT = 595.2756;

  const PAGE_HEIGHT_PT = 841.8898;

  const MAIN_TYPST_ID = "typst-001";

  const BLOCK_PATH = "blocks/equation.typ";

  const INK_LAYER_ID = "ink-layer-001";

  const TYPST_SAVE_DEBOUNCE_MS = 250;

  let root = $state("");

  let notebookTitle = $state("Goodtype");

  let notebookManifest = $state<NotebookManifest | null>(null);

  let openTabs = $state<NotebookTab[]>([]);

  let switchingRoot = $state<string | null>(null);

  let notebookGeneration = 0;

  let pageSession = 0;

  function editContext() {
    const requestedRoot = root;
    const pageId = activePageId;
    const generation = notebookGeneration;
    const session = pageSession;
    return { root: requestedRoot, pageId, current: () => root === requestedRoot &&
      activePageId === pageId && notebookGeneration === generation && pageSession === session &&
      !disposed && notebookChosen && pageOpen && !busy && !switchingRoot && !env.windowCloseInProgress };
  }

  let sharedStyleSource = $state("");

  let presets = $state<PresetSummary[]>([]);

  let presetRevision = $state(0);

  let activeSnapshot = $state.raw<NotebookSnapshot | null>(null);

  let activePageId = $state("page-001");

  let activeInkLayerId = $state(INK_LAYER_ID);

  let activeInkLayerPath = $state("ink/page-001-layer-001.ink");

  /** The paper under the active page, kept so committing does not overwrite it. */
  let activeBackground = $state<PageBackground>({ kind: "plain", color: "#ffffff" });

  /** The active page's own size. Same reason: committing used to write one fixed geometry back. */
  let activeGeometry = $state<PageGeometry>({ widthPt: PAGE_WIDTH_PT, heightPt: PAGE_HEIGHT_PT });

  let pageEntries = $state.raw<PageEntry[]>([]);

  let pageOpen = $state(false);

  let busy = $state(true);

  /** Operations in flight whose reply replaces the active page; see `replacingPage`. */
  let replacingPageCount = $state(0);

  const pageInputLocked = $derived(replacingPageCount > 0);

  /**
   * Bumped by every local edit, so an operation that awaited Rust can tell whether the writer
   * changed anything meanwhile. Plain, not reactive: nothing renders from it.
   */
  let editVersion = 0;

  let status = $state<UiText>(notice("workspace.openingTheNotebook"));

  let revision = $state(1);

  let createdAt = $state(new Date().toISOString());

  let strokes = $state.raw<Stroke[]>([]);

  let typstBlocks = $state.raw<TypstBlockState[]>([
    {
      id: MAIN_TYPST_ID,
      path: BLOCK_PATH,
      source: "= Newton's second law\n\n$ F = m a $",
      transform: { x: 96, y: 120, layoutWidthPt: 230, scale: 1 },
      result: null,
      zIndex: 1,
      readingOrder: 0,
    },
  ]);

  let pageTypst = $state<PageTypstBlockState | null>(null);

  let images = $state.raw<ImageView[]>([]);

  let shapes = $state.raw<ShapeView[]>([]);

  let saveMs = $state<number | null>(null);

  let reopenMs = $state<number | null>(null);

  let exportMs = $state<number | null>(null);

  let canUndo = $state(false);

  let canRedo = $state(false);

  let pendingTransactions = $state(0);

  let transactionFailed = $state(false);

  let transactionQueue: Promise<void> = Promise.resolve();

  let typstCommitTimer: ReturnType<typeof setTimeout> | undefined;

  let inkCommitLabel: UiText = notice("workspace.updatedInk");

  // The active page shares its timing with neighbouring pages but not its payload: the snapshot
  // is built when the timer fires, so it picks up whatever else changed in the meantime.
  const inkCommitTimer = createCommitTimer(() => queueCommit(inkCommitLabel));

  // Debounced ink is real unsaved work; the save indicator must not read "Saved" while it waits.
  let inkPending = $state(false);

  let typstDirty = $state(false);

  let pendingNeighborPages = $state(new Set<string>());

  // Per-page state for the pages that are rendered but not being edited. It lives here rather
  // than inside the renderer so a page keeps its component instance — and its painted Typst
  // SVGs — when edit focus moves onto it.
  let neighborStrokes = $state.raw<Record<string, Stroke[]>>({});

  let neighborResults = $state.raw<
    Record<string, Record<string, TypstCompileResult | null>>
  >({});

  const neighborUrls = new Map<string, AssetUrlCache>();

  const neighborCommitters = new Map<string, InkCommitter>();

  let activating = false;

  let focusTimer: ReturnType<typeof setTimeout> | undefined;

  /// When the page stack last moved. Making a page active loads and re-renders it, which stalls
  /// the main thread that finger scrolling and the glide run on; doing that mid-scroll was felt
  /// as a hitch at every page boundary. So edit focus waits until the view has settled.
  let lastScrollAt = 0;

  const FOCUS_SETTLE_MS = 300;

  const loadPage = createPageLoader(openPage);

  let pageLoadGeneration = 0;

  const nearPageIds = new Set<string>();

  const pageNodes = new Map<string, HTMLElement>();

  let residencyFrame = 0;

  let conflictDetail = $state<string | null>(null);

  let recoveryCandidates = $state<RecoveryCandidate[]>([]);

  let recoveryOpen = $state(false);

  let recoveryBusy = $state(false);

  let notebookChosen = $state(false);

  let coverRevision = $state(0);

  // Session-local order of committed changes across pages, so notebook-scoped undo can route
  // Ctrl+Z to the page that changed most recently.
  let notebookUndoOrder: NotebookAction[] = [];

  let notebookRedoOrder: NotebookAction[] = [];

  function titleFromRoot(path: string) {
    return path.split(/[\\/]/).filter(Boolean).at(-1) ?? translate("workspace.goodtypeNotebook");
  }

  function clearNotebookCaches() {
    env.resetWorkspaceEditors();
    env.navigation.stopTouchInertia();
    clearTypstCache();
    env.previews.reset();
    env.navigation.pagePointers.clear();
    const releasedRoot = root;
    void tick().then(() => {
      if (releasedRoot && releasedRoot !== root) void releasePdfDocuments(releasedRoot);
    });
    for (const urls of neighborUrls.values()) urls.dispose();
    neighborUrls.clear();
    for (const committer of neighborCommitters.values()) committer.dispose();
    neighborCommitters.clear();
    neighborStrokes = {};
    neighborResults = {};
    pendingNeighborPages = new Set();
    recoveryCandidates = [];
    recoveryOpen = false;
  }

  async function rememberNotebookSession(): Promise<boolean> {
    if (!env.tauriAvailable) return true;
    try {
      await recordNotebookSession(openTabs.map((tab) => tab.root), root || null);
      return true;
    } catch {
      return false;
    }
  }

  async function openNotebookAt(
    nextRoot: string,
    options: {
      createIfMissing?: boolean;
      skipCurrentPersist?: boolean;
      stayInLibrary?: boolean;
      setup?: NotebookSetup;
    } = {},
  ): Promise<boolean> {
    if (disposed) return false;
    if (nextRoot === root && activeSnapshot) {
      notebookChosen = !options.stayInLibrary;
      pageOpen = true;
      void writeCover();
      if (!options.stayInLibrary) {
        await tick();
        scrollToPage(activePageId, "auto");
      }
      status = notice("workspace.notebookReady");
      return true;
    }
    busy = true;
    replacingPageCount += 1;
    switchingRoot = nextRoot;
    try {
      if (activeSnapshot && root && !options.skipCurrentPersist) {
        if (!(await persist())) return false;
        await recordNotebookPage(root, activePageId).catch(() => {});
      }
      let snapshot: NotebookSnapshot;
      let createdNew = false;
      try {
        snapshot = await openNotebook(nextRoot);
      } catch (error) {
        if (!options.createIfMissing) throw error;
        const setup: NotebookSetup = options.setup ?? {
          name: titleFromRoot(nextRoot),
          geometry: { widthPt: PAGE_WIDTH_PT, heightPt: PAGE_HEIGHT_PT },
          background: { kind: "plain", color: "#ffffff" },
          preset: { kind: "none" },
        };
        snapshot = blankNotebookSnapshot(setup);
        await createNotebook(nextRoot, snapshot, setup.preset);
        createdNew = true;
      }
      const resume = (await listRecentNotebooks().catch(() => [])).find(
        (entry) => entry.root === nextRoot,
      )?.lastPageId;
      if (
        resume &&
        resume !== snapshot.page.id &&
        snapshot.manifest.pages.some((page) => page.id === resume)
      ) {
        snapshot = await openPage(nextRoot, resume);
      }
      if (disposed) return false;
      notebookGeneration += 1;
      clearNotebookCaches();
      root = nextRoot;
      transactionFailed = false;
      conflictDetail = null;
      notebookUndoOrder = [];
      notebookRedoOrder = [];
      resetPageEntries();
      applySnapshot(snapshot);
      if (createdNew) env.tool = "pen";
      await refreshPresets();
      notebookTitle = snapshot.manifest.title || titleFromRoot(nextRoot);
      openTabs = openedTab(openTabs, { root: nextRoot, title: notebookTitle });
      let sessionRemembered = true;
      try {
        await recordNotebookOpened(
          root,
          snapshot.manifest.title || translate("workspace.goodtypeNotebook"),
          new Date().toISOString(),
        );
        await recordNotebookPage(root, snapshot.page.id);
        if (!(await rememberNotebookSession())) sessionRemembered = false;
      } catch {
        sessionRemembered = false;
      }
      notebookChosen = !options.stayInLibrary;
      pageOpen = true;
      if (!options.stayInLibrary) {
        await tick();
        scrollToPage(snapshot.page.id, "auto");
      }
      status = sessionRemembered
        ? notice("workspace.notebookReady")
        : notice("workspace.notebookReadyButItsReopeningPositionCould");
      await refreshRecoveryCandidates();
      void writeCover();
      return true;
    } catch (error) {
      status = notice("workspace.couldNotOpenTheNotebook", { "value1": env.message(error) });
      return false;
    } finally {
      busy = false;
      replacingPageCount -= 1;
      switchingRoot = null;
    }
  }

  async function refreshPresets() {
    if (!root || !env.tauriAvailable) {
      presets = [];
      return;
    }
    try {
      presets = await listTypstPresets(root);
    } catch {
      // An externally damaged optional style must not prevent the canonical notebook opening.
      presets = [];
    }

  }

  async function refreshRecoveryCandidates() {
    if (!env.tauriAvailable || !root) return;
    try {
      recoveryCandidates = await listRecoveryCandidates(root);
      recoveryOpen = recoveryCandidates.length > 0;
    } catch {
      // A recovery listing failure must not block opening; candidates stay on disk.
    }
  }

  function buildSnapshot(): NotebookSnapshot {
    const now = new Date().toISOString();
    const manifest = notebookManifest ?? {
      schemaVersion: SCHEMA_VERSION,
      id: `notebook-${Date.now().toString(36)}`,
      title: notebookTitle,
      pages: [{ id: activePageId, path: "pages/page-001.json", geometry: activeGeometry }],
      defaultPage: {
        geometry: activeGeometry,
        background: activeBackground,
      },
      sharedStylePath: null,
      createdAt,
      modifiedAt: now,
    };

    return projectSnapshot({
      base: activeSnapshot,
      manifest,
      pageId: activePageId,
      revision,
      geometry: activeGeometry,
      background: activeBackground,
      inkLayerId: activeInkLayerId,
      inkLayerPath: activeInkLayerPath,
      strokes,
      typst: typstBlocks.map((block) => {
        // Without a compile result (not compiled yet, or the source is invalid) keep the last
        // measured size rather than recording a guess as a measurement.
        const stored = activeSnapshot?.page.objects.find((object) => object.id === block.id);
        const measured = stored?.type === "typst" ? stored : null;
        return {
          id: block.id,
          path: block.path,
          source: block.source,
          x: block.transform.x,
          y: block.transform.y,
          layoutWidthPt: block.transform.layoutWidthPt,
          scale: block.transform.scale,
          rotation: block.transform.rotation,
          measuredWidthPt: block.result?.widthPt ?? measured?.measuredWidthPt ?? block.transform.layoutWidthPt,
          measuredHeightPt: block.result?.heightPt ?? measured?.measuredHeightPt ?? 48,
          zIndex: block.zIndex,
          readingOrder: block.readingOrder,
        };
      }),
      pageTypst: pageTypst
        ? {
            id: pageTypst.id,
            path: pageTypst.path,
            source: pageTypst.source,
            zIndex: pageTypst.zIndex,
            readingOrder: pageTypst.readingOrder,
          }
        : null,
      images,
      shapes,
      sharedStyle: manifest.sharedStylePath
        ? { path: manifest.sharedStylePath, source: sharedStyleSource }
        : null,
      mixedGroup: env.selection.mixedGroup,
      groupedStrokeIds: env.selection.groupedStrokeIds,
      now,
    });
  }

  function applySnapshot(snapshot: NotebookSnapshot) {
    pageSession += 1;
    const previousEntry = pageEntries.find(entry => entry.id === snapshot.page.id);
    const previousStrokes = snapshot.page.id === activePageId ? strokes : previousEntry ? neighborStrokesFor(previousEntry) : [];
    env.selection.lassoObjectIds = [];
    // Whatever was scheduled describes the page being replaced, so it must not fire against the
    // one arriving.
    inkCommitTimer.cancel();
    inkPending = false;
    if (typstCommitTimer) clearTimeout(typstCommitTimer);
    typstCommitTimer = undefined;
    typstDirty = false;
    revokeImageUrl();
    notebookManifest = snapshot.manifest;
    notebookTitle = snapshot.manifest.title || notebookTitle;
    const style = snapshot.manifest.sharedStylePath
      ? snapshot.blocks.find((file) => file.path === snapshot.manifest.sharedStylePath)
      : undefined;
    sharedStyleSource = style
      ? new TextDecoder().decode(new Uint8Array(style.bytes))
      : "";
    activeSnapshot = snapshot;
    activePageId = snapshot.page.id;
    // The active page's state below is now the single source of truth for this page; drop the
    // copy it carried while it was a neighbour so the two can never disagree.
    dropNeighborStrokes(snapshot.page.id);
    const activeInk = snapshot.page.inkLayers[0];
    activeInkLayerId = activeInk?.id ?? `${activePageId}-ink-001`;
    activeInkLayerPath = activeInk?.path ?? `ink/${activePageId}-layer-001.ink`;
    // Carried through so `buildSnapshot` can put them back. It used to write a hardcoded white
    // A4 page, which meant the first stroke on a template erased the paper it was drawn on and
    // flattened any page that was not A4.
    activeBackground = snapshot.page.background;
    activeGeometry = snapshot.page.geometry;
    const previousPages = new Map(pageEntries.map((entry) => [entry.id, entry]));
    pageEntries = snapshot.manifest.pages.map((page) => ({
      ...page,
      snapshot:
        page.id === snapshot.page.id
          ? snapshot
          : previousPages.get(page.id)?.snapshot ?? null,
    }));
    revision = snapshot.page.revision;
    createdAt = snapshot.manifest.createdAt;
    strokes = reuseStrokeObjects(strokesFromSnapshot(snapshot), previousStrokes);
    env.selection.lassoObjectIds = [];
    env.selection.selectedStrokeIds = [];
    shapes = shapeViewsFromSnapshot(snapshot);
    env.selection.selectedShapeId = null;
    env.selection.editingShapeId = null;

    typstBlocks = blockViewsFromSnapshot(snapshot).map(({ x, y, layoutWidthPt, scale, rotation, ...block }) => ({
      ...block, transform: { x, y, layoutWidthPt, scale, rotation }, result: null,
    }));
    const storedPageTypst = pageTypstViewFromSnapshot(snapshot);
    pageTypst = storedPageTypst ? { ...storedPageTypst, result: null } : null;

    // A movable block stays pinned when the writer scrolls away. Page text is different: it is
    // the current page's writing surface, so an already-open Page text editor follows page focus.
    // Keeping the flag true when there is no surface lets the next page retarget automatically
    // and gives this page the dedicated "Write Page text" empty state.
    env.followPageText(pageTypst?.id ?? null);

    const inkGroup = snapshot.page.objects.find(
      (object): object is Extract<PageObject, { type: "ink_group" }> =>
        object.type === "ink_group",
    );
    const group = inkGroup
      ? snapshot.page.objects.find(
          (object): object is Extract<PageObject, { type: "group" }> =>
            object.type === "group" &&
            object.childIds.length === 2 &&
            object.childIds.includes(inkGroup.id) &&
            inkGroup.groupId === object.id,
        )
      : undefined;
    const groupedTypstId = group?.childIds.find((id) =>
      snapshot.page.objects.some(
        (object) =>
          object.type === "typst" &&
          object.id === id &&
          object.groupId === group.id,
      ),
    );
    env.selection.mixedGroup =
      inkGroup && group && groupedTypstId
        ? {
            inkGroupId: inkGroup.id,
            groupId: group.id,
            typstId: groupedTypstId,
            active: true,
          }
        : null;
    env.selection.groupedStrokeIds = env.selection.mixedGroup ? inkGroup!.strokeIds : [];
    if (env.selection.mixedGroup && inkGroup) {
      const layer = snapshot.page.inkLayers.find(layer => layer.id === inkGroup.inkLayerId);
      if (layer) { activeInkLayerId = layer.id; activeInkLayerPath = layer.path; }
    }

    images = imageViewsFromSnapshot(snapshot, new AssetUrlCache());
    env.selection.selectedImageId = null;
    env.selection.selectedTypstId = null;
  }

  async function ensurePageLoaded(pageId: string, prefetch = false) {
    const entry = pageEntries.find((page) => page.id === pageId);
    if (!entry || entry.snapshot) return entry?.snapshot ?? null;
    const requestRoot = root;
    const requestGeneration = notebookGeneration;
    const loadGeneration = pageLoadGeneration;
    try {
      const snapshot = await loadPage(requestRoot, loadGeneration, pageId);
      if (requestRoot !== root || requestGeneration !== notebookGeneration || loadGeneration !== pageLoadGeneration) return null;
      if (prefetch && !pagesToRetain().has(pageId)) return null;
      const current = pageEntries.find((page) => page.id === pageId);
      if (!current) return null;
      if (!current.snapshot) replacePageSnapshot(pageId, snapshot);
      scheduleResidency();
      return current.snapshot ?? snapshot;
    } catch (error) {
      if (requestRoot !== root || requestGeneration !== notebookGeneration || loadGeneration !== pageLoadGeneration) return null;
      status = notice("workspace.couldNotLoadPage", { "value1": env.message(error) });
      return null;
    }
  }

  function resetPageEntries() {
    pageLoadGeneration += 1;
    pageEntries = [];
  }

  function replacePageSnapshot(pageId: string, snapshot: NotebookSnapshot | null) {
    pageEntries = pageEntries.map((entry) => entry.id === pageId ? { ...entry, snapshot } : entry);
  }

  function dropNeighborStrokes(pageId: string) {
    const next = { ...neighborStrokes };
    delete next[pageId];
    neighborStrokes = next;
  }

  function scheduleResidency() {
    if (disposed) return;
    if (!residencyFrame) residencyFrame = requestAnimationFrame(() => {
      residencyFrame = 0;
      void evictDistantPages();
    });
  }

  function observePage(node: HTMLElement, pageId: string) {
    pageNodes.set(pageId, node);
    // One loose observer preloads neighbors well before they enter view.
    const preload = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          nearPageIds.add(pageId);
          const index = pageEntries.findIndex((page) => page.id === pageId);
          for (const page of pageEntries.slice(Math.max(0, index - 1), index + 2)) {
            void ensurePageLoaded(page.id, true);
          }
        } else {
          nearPageIds.delete(pageId);
        }
        scheduleResidency();
      },
      { root: env.pageViewport, rootMargin: "100% 0px" },
    );
    // A second, tight observer tracks how much of each page is actually visible so the most
    // centered page can take edit focus — this is what makes undo, New Typst, and paste act on
    // the page you are looking at rather than a page pinned at open.
    const focus = new IntersectionObserver(
      () => {
        scheduleFocus();
      },
      { root: env.pageViewport, threshold: [0, 0.25, 0.5, 0.75, 1] },
    );
    preload.observe(node);
    focus.observe(node);
    return {
      destroy: () => {
        preload.disconnect();
        focus.disconnect();
        nearPageIds.delete(pageId);
        if (pageNodes.get(pageId) === node) pageNodes.delete(pageId);
      },
    };
  }

  function scheduleFocus(delay = 150) {
    if (disposed) return;
    if (focusTimer) clearTimeout(focusTimer);
    focusTimer = setTimeout(() => {
      if (env.navigation.automaticPageFocusLocked || env.navigation.pinchStart) return;
      const still = performance.now() - lastScrollAt;
      if (env.navigation.panFrom || env.navigation.touchPanStart || env.navigation.touchInertiaFrame || env.navigation.pagePointers.size > 0 || still < FOCUS_SETTLE_MS) {
        scheduleFocus(Math.max(FOCUS_SETTLE_MS - still, 50));
        return;
      }
      if (!env.pageViewport) return;
      const bestId = pageAtViewportCenter(
        [...pageNodes].filter(([id]) => nearPageIds.has(id)).map(([id, node]) => ({ id, bounds: node.getBoundingClientRect() })),
        env.pageViewport.getBoundingClientRect(),
      );
      if (bestId && bestId !== activePageId) {
        if (activating || busy || pendingTransactions > 0) scheduleFocus();
        else void activatePage(bestId, true);
      }
    }, delay);
  }

  async function activatePage(pageId: string, automatic = false) {
    if (
      !env.tauriAvailable ||
      disposed ||
      pageId === activePageId ||
      activating ||
      busy ||
      pendingTransactions > 0 ||
      env.paletteDragging ||
      transactionFailed
    ) {
      return;
    }
    activating = true;
    const outgoing = activePageId;
    const requestRoot = root;
    const generation = notebookGeneration;
    const loadGeneration = pageLoadGeneration;
    const current = () => requestRoot === root && generation === notebookGeneration && loadGeneration === pageLoadGeneration;
    let retry = false;
    try {
      // Land every pending edit on the outgoing page, then flush the incoming page's neighbor
      // renderer, so focus_page reads a current revision instead of racing a debounced commit.
      flushInkCommit();
      flushTypstCommit();
      // Writing continues while Rust reads the pages. Anything edited from here on is not in the
      // snapshot focus_page returns, and applying that snapshot would discard it, so the switch
      // backs off and tries again once the edit has saved.
      const editsAtStart = editVersion;
      await transactionQueue;
      if (transactionFailed || !current()) return;
      await flushNeighbor(pageId);
      if (transactionFailed || !current()) return;

      // Refresh the outgoing page's cached bundle so it renders its just-committed state once it
      // becomes a neighbor rather than the stale bundle it was activated with.
      const outgoingEntry = pageEntries.find((page) => page.id === outgoing);
      if (outgoingEntry) {
        try {
          const snapshot = await openPage(requestRoot, outgoing);
          if (!current()) return;
          replacePageSnapshot(outgoing, snapshot);
          // It renders as a neighbour from here on, so let it derive from the fresh bundle.
          dropNeighborStrokes(outgoing);
        } catch {
          // A neighbor that fails to reload simply shows a loading state; it is not fatal.
        }
      }

      const result = await focusPage(requestRoot, pageId);
      if (!current()) return;
      if (editVersion !== editsAtStart) {
        retry = true;
        return;
      }
      // A new contact or scroll may begin while Rust reads the page. Never retarget that gesture.
      if (env.navigation.pagePointers.size || env.navigation.panFrom || env.navigation.touchPanStart || env.navigation.pinchStart || env.navigation.touchInertiaFrame ||
        (automatic && (env.navigation.automaticPageFocusLocked || performance.now() - lastScrollAt < FOCUS_SETTLE_MS))) {
        scheduleFocus();
        return;
      }
      applySnapshot(result.snapshot);
      void recordNotebookPage(root, pageId).catch(() => {});
      canUndo = result.canUndo;
      canRedo = result.canRedo;
      await evictDistantPages();
      status = notice("workspace.editingPage", { "value1": activePageNumber() });
    } catch (error) {
      if (current()) status = notice("workspace.couldNotSwitchToThatPage", { "value1": env.message(error) });
    } finally {
      activating = false;
      if (retry) {
        if (automatic) scheduleFocus(FOCUS_SETTLE_MS);
        else focusRetryTimer = setTimeout(() => {
          if (generation === notebookGeneration) void activatePage(pageId);
        }, FOCUS_SETTLE_MS);
      }
    }
  }

  function updateLoadedPage(pageId: string, snapshot: NotebookSnapshot) {
    replacePageSnapshot(pageId, snapshot);
    notebookManifest = snapshot.manifest;
    recordNotebookAction(pageId);
  }

  function assetUrls(pageId: string): AssetUrlCache {
    let urls = neighborUrls.get(pageId);
    if (!urls) {
      urls = new AssetUrlCache();
      neighborUrls.set(pageId, urls);
    }
    return urls;
  }

  /// Strokes a non-active page currently shows: local edits while the writer is drawing on it,
  /// otherwise whatever its last loaded bundle holds.
  function neighborStrokesFor(entry: PageEntry): Stroke[] {
    const local = neighborStrokes[entry.id];
    if (local) return local;
    return entry.snapshot ? strokesFromSnapshot(entry.snapshot) : [];
  }

  function neighborCommitter(pageId: string): InkCommitter {
    let committer = neighborCommitters.get(pageId);
    if (committer) return committer;
    const requestRoot = root;
    const generation = notebookGeneration;
    committer = createInkCommitter({
      save: async (strokes, label) => {
        if (root !== requestRoot || notebookGeneration !== generation || transactionFailed) return;
        // A page list reload drops loaded bundles; reload this one instead of dropping the ink.
        const loaded = pageEntries.find((page) => page.id === pageId)?.snapshot ?? await ensurePageLoaded(pageId);
        const entry = loaded && root === requestRoot && notebookGeneration === generation
          ? pageEntries.find((page) => page.id === pageId)
          : undefined;
        if (!entry?.snapshot) {
          pendingNeighborPages = new Set(
            [...pendingNeighborPages].filter((id) => id !== pageId),
          );
          if (root === requestRoot && notebookGeneration === generation) {
            status = notice("workspace.wasNotSavedThatPageIsNo", { "label": label });
          }
          return;
        }
        const drawingLayerId = entry.snapshot.inkLayers[0]?.id ?? `${pageId}-ink-001`;
        const references = entry.snapshot.page.inkLayers.length ? entry.snapshot.page.inkLayers
          : [{ id: drawingLayerId, path: `ink/${pageId}-layer-001.ink` }];
        const originalLayers = entry.snapshot.inkLayers.length ? entry.snapshot.inkLayers
          : [{ id: drawingLayerId, pageId, schemaVersion: entry.snapshot.page.schemaVersion, strokes: [] }];
        const inkLayers = inkLayersWithStrokes(originalLayers, strokes, drawingLayerId);
        const number =
          (notebookManifest?.pages.findIndex((page) => page.id === pageId) ?? 0) + 1;
        try {
          // Originals already exist on disk; a commit references them by path.
          const result = await commitNotebook(requestRoot, {
            ...entry.snapshot,
            page: { ...entry.snapshot.page, inkLayers: references },
            assets: [],
            inkLayers,
          }, entry.snapshot);
          if (root !== requestRoot || notebookGeneration !== generation) return;
          updateLoadedPage(pageId, result.snapshot);
          if (neighborStrokes[pageId] === strokes) {
            neighborStrokes = { ...neighborStrokes, [pageId]: strokesFromSnapshot(result.snapshot) };
          }
          status = notice("workspace.onPage", { "label": label, "number": number });
        } catch (error) {
          reportCommitFailure(notice("workspace.onPage", { "label": label, "number": number }), error);
        } finally {
          if (!transactionFailed && !committer?.pending()) {
            pendingNeighborPages = new Set(
              [...pendingNeighborPages].filter((id) => id !== pageId),
            );
          }
          scheduleResidency();
        }
      },
    });
    neighborCommitters.set(pageId, committer);
    return committer;
  }

  /** Refresh the first-page shelf cover on entry and close, without blocking notebook entry. */
  async function writeCover() {
    if (!env.tauriAvailable || !root) return;
    const coverRoot = root;
    const generation = notebookGeneration;
    if (!notebookManifest?.pages[0]) return;
    try {
      const png = await notebookCover(coverRoot, env.settings.pageTextBaselineGrid);
      if (!png || coverRoot !== root || generation !== notebookGeneration) return;
      await writeNotebookCover(coverRoot, png);
      coverRevision += 1;
    } catch {
      // A cover is a nicety. A notebook that saved correctly must never report a failure
      // because its thumbnail could not be drawn.
    }
  }

  function commitNeighborInk(pageId: string, strokes: Stroke[], label: UiText) {
    editVersion += 1;
    neighborStrokes = { ...neighborStrokes, [pageId]: strokes };
    const committer = neighborCommitter(pageId);
    pendingNeighborPages = new Set(pendingNeighborPages).add(pageId);
    committer.commit(strokes, label);
  }

  async function compileNeighborTypst(pageId: string, blockId: string, request: TypstCompileRequest) {
    const requestRoot = root, requestGeneration = notebookGeneration;
    const result = await env.previews.compile({ root: requestRoot, pageId, blockId,
      current: () => root === requestRoot && notebookGeneration === requestGeneration && !!pageEntries.find(page => page.id === pageId)?.snapshot }, request, false);
    if (result) neighborResults = { ...neighborResults, [pageId]: { ...neighborResults[pageId], [blockId]: result } };
  }

  /// Land a non-active page's debounced ink before it is promoted to the active editor, so
  /// focus_page reads a current revision instead of racing the pending commit.
  function flushNeighbor(pageId: string): Promise<void> {
    return neighborCommitters.get(pageId)?.flush() ?? Promise.resolve();
  }

  async function releaseNeighbor(pageId: string): Promise<boolean> {
    const generation = notebookGeneration;
    const loadGeneration = pageLoadGeneration;
    await flushNeighbor(pageId);
    if (transactionFailed || generation !== notebookGeneration || loadGeneration !== pageLoadGeneration || pagesToRetain().has(pageId)) return false;
    neighborUrls.get(pageId)?.dispose();
    neighborUrls.delete(pageId);
    neighborCommitters.get(pageId)?.dispose();
    neighborCommitters.delete(pageId);
    dropNeighborStrokes(pageId);
    const results = { ...neighborResults };
    delete results[pageId];
    neighborResults = results;
    replacePageSnapshot(pageId, null);
    return true;
  }

  function pagesToRetain() {
    const keep = retainedPageIds(pageEntries.map((page) => page.id), nearPageIds, activePageId);
    for (const id of env.navigation.pagePointers.values()) keep.add(id);
    for (const id of pendingNeighborPages) keep.add(id);
    return keep;
  }

  /// Viewport residency must not wait for edit focus, which is deferred throughout a pan.
  async function evictDistantPages() {
    const generation = notebookGeneration;
    const loadGeneration = pageLoadGeneration;
    const keep = pagesToRetain();
    for (const entry of pageEntries) {
      if (generation !== notebookGeneration || loadGeneration !== pageLoadGeneration) return;
      if (!keep.has(entry.id) && entry.snapshot) {
        await releaseNeighbor(entry.id);
      }
    }
  }

  /**
   * Run an operation whose reply replaces the active page wholesale (structure changes, history,
   * recovery, reopen). `applySnapshot` discards whatever the page holds locally, so page input is
   * locked from the save that precedes the operation until its reply has landed: a touch in that
   * window visibly does nothing instead of drawing ink that silently disappears.
   */
  async function replacingPage<T>(work: () => Promise<T>): Promise<T> {
    replacingPageCount += 1;
    try {
      return await work();
    } finally {
      replacingPageCount -= 1;
    }
  }

  async function duplicateActivePage() {
    env.moreOpen = false;
    if (!env.tauriAvailable) return;
    await replacingPage(async () => {
      if (!(await persist())) return;
      busy = true;
      try {
        const result = await duplicatePage(root, activePageId, new Date().toISOString());
        resetPageEntries();
        applySnapshot(result.snapshot);
        recordStructureAction();
        await tick();
        scrollToPage(result.snapshot.page.id);
        status = notice("workspace.duplicatedIntoPage", { "value1": activePageNumber() });
      } catch (error) {
        status = notice("workspace.couldNotDuplicateThePage", { "value1": env.message(error) });
      } finally {
        busy = false;
      }
    });
  }

  /** The page number a pending delete asks about; null while nothing is being asked. */
  let confirmingPageDelete = $state<number | null>(null);

  async function deleteActivePage() {
    env.moreOpen = false;
    if (!env.tauriAvailable || !(await persist())) return;
    confirmingPageDelete = activePageNumber();
  }

  async function confirmDeleteActivePage() {
    const deletedNumber = confirmingPageDelete;
    confirmingPageDelete = null;
    await replacingPage(async () => {
      // Edits made while the question was open belong to the page's history too, so undoing the
      // delete brings them back.
      if (!(await persist())) return;
      busy = true;
      try {
        const result = await deletePage(root, activePageId, new Date().toISOString());
        resetPageEntries();
        applySnapshot(result.snapshot);
        recordStructureAction();
        await tick();
        scrollToPage(result.snapshot.page.id);
        status = notice("workspace.deletedPage", { "deletedNumber": deletedNumber });
      } catch (error) {
        status = notice("workspace.couldNotDeleteThePage", { "value1": env.message(error) });
      } finally {
        busy = false;
      }
    });
  }

  /**
   * Empty the page without removing it.
   *
   * A page always carries at least one Typst block — `buildSnapshot` groups against
   * `MAIN_TYPST_ID` and the reading order references it — so clearing resets that block to empty
   * rather than deleting every object. Removing it outright would produce a snapshot the storage
   * layer rejects.
   *
   * This goes through the ordinary commit path, so it lands in history and undo covers it. That
   * is the whole reason there is no trash: the recovery mechanism already exists.
   */
  function clearActivePage() {
    env.moreOpen = false;
    if (!pageOpen) return;
    typstBlocks = [
      {
        id: MAIN_TYPST_ID,
        path: BLOCK_PATH,
        source: "",
        transform: { x: 96, y: 120, layoutWidthPt: 230, scale: 1 },
        result: null,
        zIndex: 1,
        readingOrder: 0,
      },
    ];
    pageTypst = null;
    strokes = [];
    shapes = [];
    env.selection.lassoObjectIds = [];
    env.selection.selectedStrokeIds = [];
    env.selection.groupedStrokeIds = [];
    revokeImageUrl();
    images = [];
    activeSnapshot = activeSnapshot
      ? {
          ...activeSnapshot,
          page: { ...activeSnapshot.page, objects: [], readingOrder: [] },
          blocks: [],
        }
      : null;
    env.selection.selectedTypstId = null;
    env.selection.selectedImageId = null;
    env.selection.selectedShapeId = null;
    env.selection.editingShapeId = null;
    env.selection.mixedGroup = null;
    queueCommit(notice("workspace.clearedPage"));
  }

  /** Jump straight to a page number, one-based as the writer sees it. */
  async function goToPageNumber(number: number) {
    const pages = notebookManifest?.pages ?? [];
    const target = pages[number - 1];
    if (target && target.id !== activePageId) await activatePage(target.id);
  }

  const pageCount = $derived(notebookManifest?.pages.length ?? 1);

  const pageNumber = $derived(
    (notebookManifest?.pages.findIndex((page) => page.id === activePageId) ?? 0) + 1,
  );

  async function moveActivePage(direction: -1 | 1) {
    env.moreOpen = false;
    const manifest = notebookManifest;
    if (busy || !env.tauriAvailable || !manifest) return;
    const order = manifest.pages.map((page) => page.id);
    const index = order.indexOf(activePageId);
    const target = index + direction;
    if (index < 0 || target < 0 || target >= order.length) return;
    [order[index], order[target]] = [order[target], order[index]];
    busy = true;
    await replacingPage(async () => {
      try {
        if (!(await persist())) return;
        const result = await reorderPages(
          root,
          order,
          new Date().toISOString(),
          activePageId,
        );
        resetPageEntries();
        applySnapshot(result.snapshot);
        recordStructureAction();
        await tick();
        scrollToPage(activePageId);
        status = notice("workspace.movedPageToPosition", { "value1": target + 1 });
      } catch (error) {
        status = notice("workspace.couldNotReorderPages", { "value1": env.message(error) });
      } finally {
        busy = false;
      }
    });
  }

  async function navigateToSearchHit(hit: SearchHit) {
    env.searchOpen = false;
    const entry = pageEntries.find((page) => page.id === hit.pageId);
    if (!entry) return;
    await ensurePageLoaded(hit.pageId);
    scrollToPage(hit.pageId);
    await activatePage(hit.pageId);
    if (activePageId === hit.pageId) {
      env.selection.selectedTypstId = typstBlocks.some((block) => block.id === hit.objectId)
        ? hit.objectId
        : null;
      env.selection.selectedImageId = null;
      env.selection.selectedShapeId = null;
      env.selection.editingShapeId = null;
      status = notice("workspace.foundOnPage", { "pageNumber": hit.pageNumber });
    }
  }

  async function restoreRecovery(fileName: string) {
    recoveryBusy = true;
    await replacingPage(async () => {
      try {
        // Pending edits land first, so restoring replaces confirmed state that undo can return to.
        // A save blocked by unresolved recovery is exactly what restoring resolves; skip it then.
        if (env.tauriAvailable && activeSnapshot && !transactionFailed && !(await persist())) return;
        const result = await restoreRecoveryCandidate(root, fileName);
        resetPageEntries();
        applySnapshot(result.snapshot);
        canUndo = result.canUndo;
        canRedo = result.canRedo;
        status = notice("workspace.restoredRecoveredWorkOnPage", { "value1": activePageNumber() });
        recoveryCandidates = recoveryCandidates.filter(
          (candidate) => candidate.fileName !== fileName,
        );
        recoveryOpen = recoveryCandidates.length > 0;
      } catch (error) {
        status = notice("workspace.couldNotRestoreTheRecoveredWork", { "value1": env.message(error) });
      } finally {
        recoveryBusy = false;
      }
    });
  }

  async function discardRecovery(fileName: string) {
    recoveryBusy = true;
    try {
      await discardRecoveryCandidate(root, fileName);
      recoveryCandidates = recoveryCandidates.filter(
        (candidate) => candidate.fileName !== fileName,
      );
      recoveryOpen = recoveryCandidates.length > 0;
      status = notice("workspace.discardedTheRecoveredCopy");
    } catch (error) {
      status = notice("workspace.couldNotDiscardTheRecoveredCopy", { "value1": env.message(error) });
    } finally {
      recoveryBusy = false;
    }
  }

  /**
   * Add one page, or a run of them.
   *
   * A run is written one page at a time, each placed after the one before it rather than at the
   * original position: asking for three pages "before page 4" has to leave them in the order they
   * were asked for, not stacked in reverse. Each is its own transaction, so an interrupted run
   * leaves the pages it managed rather than nothing.
   */
  async function addPage(
    position: PagePosition,
    background: PageBackground | null = null,
    geometry: PageGeometry | null = null,
    count = 1,
  ) {
    env.moreOpen = false;
    await replacingPage(async () => {
      if (!(await persist())) return;
      busy = true;
      try {
        let at = position;
        let last = "";
        for (let made = 0; made < Math.max(1, count); made += 1) {
          const result = await createPage(
            root,
            new Date().toISOString(),
            at,
            background,
            geometry,
            activePageId,
          );
          resetPageEntries();
          applySnapshot(result.snapshot);
          recordStructureAction();
          last = result.snapshot.page.id;
          at = { kind: "after", pageId: last };
        }
        // Load whatever now sits above the last page so the scroll lands with context above it
        // rather than against the top of an otherwise empty run.
        const pages = notebookManifest?.pages ?? [];
        const index = pages.findIndex((page) => page.id === last);
        const above = index > 0 ? pages[index - 1] : undefined;
        if (above) await ensurePageLoaded(above.id);
        await tick();
        if (last) scrollToPage(last);
        status =
          count > 1 ? notice("workspace.addedPages", { "count": count }) : notice("workspace.addedPage", { "value1": activePageNumber() });
      } catch (error) {
        status = notice("workspace.couldNotAddPage", { "value1": env.message(error) });
      } finally {
        busy = false;
      }
    });
  }

  async function importPdf(position: PagePosition) {
    env.moreOpen = false;
    if (!env.tauriAvailable) return;
    await replacingPage(async () => {
      if (!(await persist())) return;
      busy = true;
      try {
        const sourcePath = await pickPdfReference(root);
        if (!sourcePath) return;
        status = notice("workspace.readingPDFPages");
        const geometries = await pdfPageGeometries(root, sourcePath);
        const result = await importPdfPages(
          root,
          new Date().toISOString(),
          position,
          sourcePath,
          geometries,
          activePageId,
        );
        resetPageEntries();
        applySnapshot(result.snapshot);
        recordStructureAction();
        const index = result.snapshot.manifest.pages.findIndex(
          (page) => page.id === result.snapshot.page.id,
        );
        const above = index > 0 ? result.snapshot.manifest.pages[index - 1] : undefined;
        if (above) await ensurePageLoaded(above.id);
        await tick();
        scrollToPage(result.snapshot.page.id);
        status = notice("counts.importedPdf", { count: geometries.length });
      } catch (error) {
        status = notice("workspace.couldNotImportPDF", { "value1": env.message(error) });
      } finally {
        busy = false;
      }
    });
  }

  function activePageNumber() {
    const index = notebookManifest?.pages.findIndex((page) => page.id === activePageId) ?? 0;
    return Math.max(0, index) + 1;
  }

  function updateTypstTransform(id: string, next: TypstTransform) {
    const previous = typstBlocks.find((block) => block.id === id)?.transform;
    if (!previous) return;
    if (env.selection.mixedGroup?.active && id === env.selection.mixedGroup.typstId && env.selection.groupedStrokeIds.length > 0) {
      let nextStrokes = strokes;
      const scaleRatio = next.scale / Math.max(previous.scale, 0.05);
      if (scaleRatio !== 1) {
        nextStrokes = scaleSelected(
          nextStrokes,
          env.selection.groupedStrokeIds,
          { x: previous.x, y: previous.y },
          scaleRatio,
        );
      }
      const delta = {
        x: next.x - previous.x,
        y: next.y - previous.y,
      };
      if (delta.x !== 0 || delta.y !== 0) {
        nextStrokes = moveSelected(nextStrokes, env.selection.groupedStrokeIds, delta);
      }
      strokes = nextStrokes;
    }
    typstBlocks = typstBlocks.map((block) =>
      block.id === id ? { ...block, transform: next } : block,
    );
    queueCommit(notice("workspace.updatedTypstBlock"));
  }

  function updateTypstSource(id: string, source: string) {
    if (pageTypst?.id === id) pageTypst = { ...pageTypst, source };
    else {
      typstBlocks = typstBlocks.map((block) =>
        block.id === id ? { ...block, source } : block,
      );
    }
    editVersion += 1;
    typstDirty = true;
    if (typstCommitTimer) clearTimeout(typstCommitTimer);
    typstCommitTimer = setTimeout(flushTypstCommit, TYPST_SAVE_DEBOUNCE_MS);
  }

  function updateSharedStyle(source: string) {
    sharedStyleSource = source;
    editVersion += 1;
    typstDirty = true;
    if (typstCommitTimer) clearTimeout(typstCommitTimer);
    typstCommitTimer = setTimeout(flushTypstCommit, TYPST_SAVE_DEBOUNCE_MS);
  }

  function addTypstBlock() {
    let number = typstBlocks.length + 1;
    while (typstBlocks.some((block) => block.id === `typst-${String(number).padStart(3, "0")}`)) number += 1;
    const id = `typst-${String(number).padStart(3, "0")}`;
    typstBlocks = [
      ...typstBlocks,
      {
        id,
        path: `blocks/${activePageId}-${id}.typ`,
        source: withBlockFont("= New block\n\nType here", env.settings.inlineTextFont),
        transform: {
          x: 150 + (typstBlocks.length % 4) * 20,
          y: 220 + (typstBlocks.length % 6) * 28,
          layoutWidthPt: 230,
          scale: 1,
        },
        result: null,
        zIndex: nextVisualZIndex(),
        readingOrder: nextReadingOrder(),
      },
    ];
    env.selection.selectedTypstId = id;
    env.selection.selectedImageId = null;
    env.selection.selectedShapeId = null;
    env.selection.editingShapeId = null;
    status = notice("workspace.createdANewTypstBlock");
    queueCommit(notice("workspace.createdTypstBlock"));
  }

  function nextVisualZIndex(): number {
    return Math.max(
      0,
      ...(activeSnapshot?.page.objects.map((object) => object.zIndex) ?? []),
      ...typstBlocks.map((block) => block.zIndex),
      ...images.map((image) => image.zIndex),
      ...shapes.map((shape) => shape.zIndex),
      ...strokes.map((stroke) => stroke.zIndex ?? DEFAULT_INK_Z_INDEX),
    ) + 1;
  }

  function nextReadingOrder(): number {
    return Math.max(
      -1,
      ...(pageTypst ? [pageTypst.readingOrder] : []),
      ...typstBlocks.map((block) => block.readingOrder),
      ...images.map((image) => image.readingOrder),
      ...shapes.map((shape) => shape.readingOrder),
    ) + 1;
  }

  function flushTypstCommit() {
    if (typstCommitTimer) clearTimeout(typstCommitTimer);
    typstCommitTimer = undefined;
    if (!typstDirty) return;
    typstDirty = false;
    queueCommit(notice("workspace.updatedTypstSource"));
  }

  function scheduleInkCommit(label: UiText) {
    editVersion += 1;
    inkCommitLabel = label;
    inkPending = true;
    inkCommitTimer.arm();
  }

  function flushInkCommit() {
    inkCommitTimer.flush();
  }

  function queueCommit(label: UiText) {
    // Any commit builds a snapshot from current state, so it already carries pending ink.
    editVersion += 1;
    inkCommitTimer.cancel();
    inkPending = false;
    if (!env.tauriAvailable || !root || transactionFailed) return;
    enqueueSnapshot(label, buildSnapshot());
  }

  function enqueueSnapshot(label: UiText, snapshot: NotebookSnapshot) {
    const requestRoot = root;
    const generation = notebookGeneration;
    pendingTransactions += 1;
    transactionQueue = transactionQueue
      .then(async () => {
        if (root !== requestRoot || notebookGeneration !== generation || transactionFailed) return;
        snapshot.page.revision = revision;
        try {
          const result = await commitNotebook(requestRoot, snapshot, activeSnapshot);
          if (root !== requestRoot || notebookGeneration !== generation) return;
          activeSnapshot = result.snapshot;
          notebookManifest = result.snapshot.manifest;
          replacePageSnapshot(result.snapshot.page.id, result.snapshot);
          revision = result.snapshot.page.revision;
          canUndo = result.canUndo;
          canRedo = result.canRedo;
          recordNotebookAction(snapshot.page.id);
          status = notice("workspace.savedRevision", { "label": label, "revision": revision });
        } catch (error) {
          reportCommitFailure(label, error);
        }
      })
      .finally(() => {
        pendingTransactions -= 1;
      });
  }

  /// A refused commit is either an external change/conflict — the user chooses reload or
  /// cancel — or an ordinary failure that blocks saving until reopen. Never a silent latch.
  function reportCommitFailure(label: UiText, error: unknown) {
    transactionFailed = true;
    const detail = env.message(error);
    const code = notebookFailureCode(error);
    if (code === "external_change" || code === "revision_conflict") {
      conflictDetail = detail;
      status = notice("workspace.wasNotSavedTheNotebookChangedOutside", { "label": label });
    } else if (code === "recovery_limit") {
      status = notice("workspace.wasNotSavedUnresolvedRecoveredWorkMust", { "label": label });
      void refreshRecoveryCandidates();
    } else {
      status = notice("workspace.couldNotBeSavedReopenToRestore", { "label": label, "detail": detail });
    }
  }

  function recordNotebookAction(pageId: string) {
    notebookUndoOrder.push({ kind: "page", pageId });
    if (notebookUndoOrder.length > 200) notebookUndoOrder.shift();
    notebookRedoOrder = [];
    canUndo = true;
    canRedo = false;
  }

  function recordStructureAction() {
    // A manifest change invalidates page snapshots retained by Rust, but older manifest states
    // remain replayable because page files are kept. Drop only the page entries from the route.
    notebookUndoOrder = notebookUndoOrder.filter((action) => action.kind === "structure");
    notebookUndoOrder.push({ kind: "structure" });
    notebookRedoOrder = [];
    canUndo = true;
    canRedo = false;
    if (env.tauriAvailable && root) void recordNotebookPage(root, activePageId).catch(() => {});
  }

  /**
   * `replaced` comes from the precise eraser: each cut stroke and the fragments now standing for
   * it. A fragment keeps its stroke's group, so it takes the stroke's place in the group too.
   */
  function changeStrokes(next: Stroke[], label: UiText, replaced?: ReadonlyMap<string, string[]>) {
    if (replaced) env.selection.groupedStrokeIds = env.selection.groupedStrokeIds.flatMap((id) => replaced.get(id) ?? [id]);
    const nextIds = new Set(next.map((stroke) => stroke.id));
    const protectedGrouped = strokes.filter(
      (stroke) =>
        !nextIds.has(stroke.id) &&
        stroke.groupId &&
        stroke.groupId !== env.selection.mixedGroup?.inkGroupId,
    );
    if (protectedGrouped.length > 0) next = [...next, ...protectedGrouped];
    const remaining = new Set(next.map((stroke) => stroke.id));
    env.selection.groupedStrokeIds = env.selection.groupedStrokeIds.filter((id) => remaining.has(id));
    strokes = next;
    scheduleInkCommit(label);
  }

  function addStroke(stroke: Stroke) {
    changeStrokes([...strokes, stroke], notice("workspace.addedStroke", { "tool": stroke.tool }));
    // Annotating the page must not cost the writer their place in the source: the canvas takes
    // focus to receive the pen, so the side view takes it back once the stroke has landed.
    if (env.sideEditorOpen) env.sideEditor?.focus();
  }

  function addShape(draft: ShapeDraft) {
    const added = env.shapeView(draft);
    shapes = [...shapes, added];
    status = notice("workspace.addedShapeToolStaysActive", { "kind": shapeMessage(draft.geometry.kind) });
    queueCommit(notice("workspace.added", { "kind": shapeMessage(draft.geometry.kind) }));
  }

  /**
   * A held pen mark is committed once as raw ink and once as its promoted shape. That creates
   * an honest undo boundary: one Undo restores the exact samples instead of approximating them
   * from the fitted geometry.
   */
  function promoteHeldInk(stroke: Stroke, draft: ShapeDraft) {
    inkCommitTimer.cancel();
    inkPending = false;
    const originalStrokes = strokes;
    const originalShapes = shapes;
    strokes = [...originalStrokes, stroke];
    const rawSnapshot = buildSnapshot();
    strokes = originalStrokes;
    shapes = [
      ...originalShapes,
      env.shapeView(draft, { zIndex: stroke.zIndex ?? DEFAULT_INK_Z_INDEX }),
    ];
    const shapeSnapshot = buildSnapshot();
    if (env.tauriAvailable && root && !transactionFailed) {
      enqueueSnapshot(notice("workspace.addedStroke", { "tool": stroke.tool }), rawSnapshot);
      enqueueSnapshot(notice("workspace.convertedHeldInkTo", { "kind": shapeMessage(draft.geometry.kind) }), shapeSnapshot);
    }
    status = notice("workspace.convertedHeldInkToUndoRestoresThe", { "kind": shapeMessage(draft.geometry.kind) });
  }

  function convertSelectedInkToShape() {
    if (env.selection.selectedStrokeIds.length !== 1) return;
    const stroke = strokes.find((candidate) => candidate.id === env.selection.selectedStrokeIds[0]);
    if (!stroke || stroke.groupId) {
      status = notice("workspace.ungroupThisInkBeforeConvertingItTo");
      return;
    }
    const style: ShapeStyle = {
      strokeColor: stroke.color,
      strokeWidthPt: stroke.widthPt,
      fillColor: null,
      opacity: stroke.opacity,
      pattern: stroke.pattern ?? "solid",
      patternSpacing: stroke.patternSpacing ?? 1,
    };
    const draft = recognizeHeldShape(
      stroke.points.map((point) => ({ ...point, ...transformedPoint(stroke, point) })),
      style,
      { snapDegrees: env.settings.shapeConstrain ? 15 : 0 },
    )?.draft;
    if (!draft) {
      status = notice("workspace.thatStrokeIsNotCloseEnoughTo");
      return;
    }
    strokes = strokes.filter((candidate) => candidate.id !== stroke.id);
    shapes = [
      ...shapes,
      env.shapeView(draft, {
        zIndex: stroke.zIndex ?? DEFAULT_INK_Z_INDEX,
        readingOrder: nextReadingOrder(),
      }),
    ];
    env.selection.updateInkSelection([]);
    queueCommit(notice("workspace.convertedInkTo", { "kind": shapeMessage(draft.geometry.kind) }));
    status = notice("workspace.convertedInkTo", { "kind": shapeMessage(draft.geometry.kind) });
  }

  /// A finished drag is one decision and is written straight away. A held arrow or bracket key
  /// is one continuous adjustment, and settles first so a nudge across the page is a single undo
  /// step rather than one transaction per repeat.
  /// The eraser takes a whole shape, the way it takes a whole stroke.
  function eraseShape(id: string) {
    if (!shapes.some((shape) => shape.id === id)) return;
    shapes = shapes.filter((shape) => shape.id !== id);
    if (env.selection.selectedShapeId === id) {
      env.selection.selectedShapeId = null;
      env.selection.editingShapeId = null;
    }
    scheduleInkCommit(notice("workspace.erasedShape"));
    status = notice("workspace.erasedTheShape");
  }

  /**
   * The precise eraser's shape pieces, landing on pen-up. They go through the same ink commit as
   * the cut strokes from the same gesture, so the whole erase is one undo step.
   */
  function replaceErasedShapes(replaced: ReadonlyMap<string, ShapeView[]>) {
    shapes = shapes.flatMap((shape) => replaced.get(shape.id) ?? [shape]);
    if (env.selection.selectedShapeId && !shapes.some((shape) => shape.id === env.selection.selectedShapeId)) {
      env.selection.selectedShapeId = null;
      env.selection.editingShapeId = null;
    }
    scheduleInkCommit(notice("workspace.erasedInk"));
  }

  function changeShape(next: ShapeView, commit: ShapeEditCommit = "now") {
    if (!shapes.some((shape) => shape.id === next.id)) return;
    shapes = shapes.map((shape) => (shape.id === next.id ? next : shape));
    if (commit === "settled") scheduleInkCommit(notice("workspace.updatedShape"));
    else queueCommit(notice("workspace.updatedShape"));
  }

  function changeImage(id: string, next: Partial<Pick<ImageView, "x" | "y" | "scale">>) {
    if (!images.some((image) => image.id === id)) return;
    images = images.map((image) => (image.id === id ? { ...image, ...next } : image));
    queueCommit(notice("workspace.updatedImage"));
  }

  function undo() {
    void routeHistory("undo_notebook", notice("workspace.undidChange"));
  }

  function redo() {
    void routeHistory("redo_notebook", notice("workspace.redidChange"));
  }

  /// Page scope targets the page in view. Notebook scope replays the session's commit order:
  /// undo jumps to the page that changed most recently, wherever it is.
  async function routeHistory(
    command: "undo_notebook" | "redo_notebook",
    label: UiText,
    announce = false,
  ) {
    if (!(await persist())) return;
    const order = command === "undo_notebook" ? notebookUndoOrder : notebookRedoOrder;
    const target = order.at(-1);
    if (target?.kind === "structure") {
      queueStructureHistory(
        command === "undo_notebook" ? "undo_page_structure" : "redo_page_structure",
        label,
        announce,
      );
      return;
    }
    if (env.settings.undoScope === "notebook") {
      if (target?.kind === "page" && target.pageId !== activePageId) {
        await activatePage(target.pageId);
        if (activePageId !== target.pageId) return;
        scrollToPage(target.pageId);
      }
    }
    queueHistory(command, label, announce);
  }

  function lastPageActionIndex(actions: NotebookAction[], pageId: string): number {
    for (let index = actions.length - 1; index >= 0; index -= 1) {
      const action = actions[index];
      if (action.kind === "page" && action.pageId === pageId) return index;
    }
    return -1;
  }

  function lastStructureActionIndex(actions: NotebookAction[]): number {
    for (let index = actions.length - 1; index >= 0; index -= 1) {
      if (actions[index].kind === "structure") return index;
    }
    return -1;
  }

  function queueHistory(command: "undo_notebook" | "redo_notebook", label: UiText, announce = false) {
    if (!env.tauriAvailable || transactionFailed) return;
    flushInkCommit();
    flushTypstCommit();
    pendingTransactions += 1;
    // The reply replaces the page, so input waits for it rather than being discarded by it.
    replacingPageCount += 1;
    transactionQueue = transactionQueue
      .then(async () => {
        try {
          const result = await runHistory(root, command, activePageId);
          const pageId = result.snapshot.page.id;
          if (command === "undo_notebook") {
            const index = lastPageActionIndex(notebookUndoOrder, pageId);
            if (index >= 0) notebookUndoOrder.splice(index, 1);
            notebookRedoOrder.push({ kind: "page", pageId });
          } else {
            const index = lastPageActionIndex(notebookRedoOrder, pageId);
            if (index >= 0) notebookRedoOrder.splice(index, 1);
            notebookUndoOrder.push({ kind: "page", pageId });
          }
          applySnapshot(result.snapshot);
          canUndo = result.canUndo || notebookUndoOrder.length > 0;
          canRedo = result.canRedo || notebookRedoOrder.length > 0;
          status = notice("workspace.savedRevision", { "label": label, "revision": revision });
          if (announce) env.navigation.announceTouchHistory(command === "undo_notebook" ? notice("workspace.undo") : notice("workspace.redo"));
        } catch (error) {
          reportCommitFailure(label, error);
        }
      })
      .finally(() => {
        pendingTransactions -= 1;
        replacingPageCount -= 1;
      });
  }

  function queueStructureHistory(
    command: "undo_page_structure" | "redo_page_structure",
    label: UiText,
    announce = false,
  ) {
    if (!env.tauriAvailable || transactionFailed) return;
    flushInkCommit();
    flushTypstCommit();
    pendingTransactions += 1;
    replacingPageCount += 1;
    transactionQueue = transactionQueue
      .then(async () => {
        try {
          // A page-list change invalidates every page's save baseline in Rust, so neighbor saves
          // must land before it rather than be refused after it.
          await Promise.all([...neighborCommitters.values()].map((committer) => committer.flush()));
          if (transactionFailed) return;
          const result = await runStructureHistory(
            root,
            command,
            new Date().toISOString(),
          );
          if (command === "undo_page_structure") {
            const index = lastStructureActionIndex(notebookUndoOrder);
            if (index >= 0) notebookUndoOrder.splice(index, 1);
            notebookRedoOrder.push({ kind: "structure" });
          } else {
            const index = lastStructureActionIndex(notebookRedoOrder);
            if (index >= 0) notebookRedoOrder.splice(index, 1);
            notebookUndoOrder.push({ kind: "structure" });
          }
          resetPageEntries();
          applySnapshot(result.snapshot);
          void recordNotebookPage(root, result.snapshot.page.id).catch(() => {});
          canUndo = result.canUndo || notebookUndoOrder.length > 0;
          canRedo = result.canRedo || notebookRedoOrder.length > 0;
          await tick();
          scrollToPage(result.snapshot.page.id);
          status = notice("workspace.pageListChange", { "label": label });
          if (announce) env.navigation.announceTouchHistory(command === "undo_page_structure" ? notice("workspace.undo") : notice("workspace.redo"));
        } catch (error) {
          reportCommitFailure(label, error);
        }
      })
      .finally(() => {
        pendingTransactions -= 1;
        replacingPageCount -= 1;
      });
  }

  function scrollToPage(pageId: string, behavior?: ScrollBehavior) {
    document
      .querySelector<HTMLElement>(`[data-page-id="${pageId}"]`)
      ?.scrollIntoView({
        behavior: behavior ?? (env.settings.reducedMotion ? "auto" : "smooth"),
        block: "center",
        inline: "center",
      });
  }

  async function persist(): Promise<boolean> {
    if (!env.tauriAvailable) {
      status = notice("workspace.browserPreviewCannotWriteFilesLaunchThe");
      return false;
    }
    const startedAt = performance.now();
    if (!(await env.saveTemplate())) return false;
    flushInkCommit();
    flushTypstCommit();
    await transactionQueue;
    await Promise.all([...neighborCommitters.values()].map((committer) => committer.flush()));
    if (env.collectMetrics) saveMs = performance.now() - startedAt;
    if (transactionFailed) return false;
    status = notice("workspace.allChangesSavedAtRevision", { "revision": revision });
    return true;
  }

  async function exportPdf() {
    if (!(await persist())) return;
    busy = true;
    const startedAt = performance.now();
    try {
      // Rust builds the ordered multi-page PDF from the canonical files, so the export
      // matches what is saved, not what this view holds.
      const path = await exportNotebookPdf(
        root,
        "notebook.pdf",
        env.settings.pageTextBaselineGrid,
      );
      if (env.collectMetrics) exportMs = performance.now() - startedAt;
      status = notice("workspace.exportedPDFTo", { "path": path });
    } catch (error) {
      status = notice("workspace.pdfExportFailed", { "value1": env.message(error) });
    } finally {
      busy = false;
    }
  }

  async function closePage() {
    await closeNotebookTab(root);
  }

  async function showLibrary() {
    if (env.tauriAvailable && activeSnapshot && !(await persist())) return;
    if (env.tauriAvailable && root) await recordNotebookPage(root, activePageId).catch(() => {});
    env.moreOpen = false;
    env.addPageOpen = false;
    notebookChosen = false;
    status = notice("workspace.chooseANotebookOrReturnToAn");
  }

  /**
   * Save the live notebook, let Rust move the filesystem entries, then retarget every affected
   * tab and reload the active one from its new root without leaving the library surface.
   */
  async function moveLibrarySelection(
    paths: string[],
    destination: string,
  ): Promise<string | null> {
    if (activeSnapshot && !(await persist())) {
      throw new Error(translate("workspace.theOpenNotebookCouldNotBeSaved"));
    }
    if (root) await recordNotebookPage(root, activePageId).catch(() => {});

    const previousRoot = root;
    const previousTabs = openTabs;
    const result = await moveLibraryEntries(paths, destination);
    if (result.rootMoves.length === 0) return result.warning;

    const relocated = new Map(result.rootMoves.map((move) => [move.from, move.to]));
    openTabs = openTabs.map((tab) => ({
      ...tab,
      root: relocated.get(tab.root) ?? tab.root,
    }));

    const nextRoot = relocated.get(previousRoot);
    if (nextRoot) {
      const history = { undo: notebookUndoOrder, redo: notebookRedoOrder, canUndo, canRedo };
      const reopened = await openNotebookAt(nextRoot, {
        skipCurrentPersist: true,
        stayInLibrary: true,
      });
      // Relocation keeps Rust's history; reloading the same notebook must keep its routing too.
      notebookUndoOrder = history.undo;
      notebookRedoOrder = history.redo;
      canUndo = history.canUndo;
      canRedo = history.canRedo;
      if (!reopened) {
        // The canonical data was confirmed before the move and remains the same directory at a
        // new root. Keep that in-memory snapshot usable even if a fresh read hit a transient
        // failure, and make the unusual state explicit rather than pretending the move failed.
        root = nextRoot;
        openTabs = previousTabs.map((tab) => ({
          ...tab,
          root: relocated.get(tab.root) ?? tab.root,
        }));
        notebookChosen = false;
        await rememberNotebookSession();
        return translate("workspace.movedButTheOpenNotebookCouldNot");
      }
    } else if (!(await rememberNotebookSession())) {
      return translate("workspace.movedButTheUpdatedTabListCould");
    }

    return result.warning;
  }

  function returnToNotebook() {
    if (!activeSnapshot || !root) return;
    void openNotebookAt(root);
  }

  async function closeNotebookTab(tabRoot: string) {
    const closed = closedTab(openTabs, tabRoot);
    if (closed.tabs === openTabs) return;
    if (tabRoot !== root) {
      openTabs = closed.tabs;
      status = (await rememberNotebookSession())
        ? notice("workspace.closedNotebookTab")
        : notice("workspace.closedNotebookTabButTheRestoredTab");
      return;
    }
    if (env.tauriAvailable && !(await persist())) return;
    if (env.tauriAvailable) await recordNotebookPage(root, activePageId).catch(() => {});
    await writeCover();

    const previousTabs = openTabs;
    openTabs = closed.tabs;
    if (closed.nextRoot) {
      const opened = await openNotebookAt(closed.nextRoot, { skipCurrentPersist: true });
      if (!opened) {
        openTabs = previousTabs;
        notebookChosen = true;
        pageOpen = true;
      }
      return;
    }

    if (env.tauriAvailable && root) {
      try {
        await closeNotebookSession(root);
      } catch (error) {
        openTabs = previousTabs;
        status = notice("workspace.theNotebookWasSavedButCouldNot", { "value1": env.message(error) });
        return;
      }
    }
    notebookGeneration += 1;
    clearNotebookCaches();
    revokeImageUrl();
    root = "";
    notebookManifest = null;
    activeSnapshot = null;
    resetPageEntries();
    pageOpen = false;
    notebookChosen = false;
    status = notice("workspace.notebookClosedTheConfirmedLocalFilesAre");
  }

  async function reopen() {
    if (!env.tauriAvailable) {
      pageOpen = true;
      status = notice("workspace.browserPreviewReopenedItsInMemoryPage");
      return;
    }
    busy = true;
    replacingPageCount += 1;
    const startedAt = performance.now();
    try {
      await transactionQueue;
      const snapshot = await openNotebook(root);
      applySnapshot(snapshot);
      if (transactionFailed) {
        canUndo = false;
        canRedo = false;
      }
      transactionFailed = false;
      if (env.collectMetrics) reopenMs = performance.now() - startedAt;
      pageOpen = true;
      status = notice("workspace.reopenedSavedRevision", { "revision": revision });
      void writeCover();
    } catch (error) {
      status = notice("workspace.reopenFailed", { "value1": env.message(error) });
    } finally {
      busy = false;
      replacingPageCount -= 1;
    }
  }

  function revokeImageUrl() {
    for (const image of images) {
      if (image.url.startsWith("blob:")) URL.revokeObjectURL(image.url);
    }
  }
  /** The close path flushes first. Unmount only releases this instance's resources. */
  function dispose() {
    disposed = true;
    notebookGeneration += 1;
    pageLoadGeneration += 1;
    inkCommitTimer.cancel();
    clearTimeout(typstCommitTimer);
    clearTimeout(focusTimer);
    clearTimeout(focusRetryTimer);
    if (residencyFrame) cancelAnimationFrame(residencyFrame);
    for (const committer of neighborCommitters.values()) committer.dispose();
    for (const urls of neighborUrls.values()) urls.dispose();
    neighborCommitters.clear();
    neighborUrls.clear();
    revokeImageUrl();
    if (root) void releasePdfDocuments(root);
  }
  return {
    dispose,
    editContext,
    titleFromRoot,
    openNotebookAt,
    refreshPresets,
    buildSnapshot,
    applySnapshot,
    scheduleResidency,
    observePage,
    scheduleFocus,
    activatePage,
    assetUrls,
    neighborStrokesFor,
    commitNeighborInk,
    compileNeighborTypst,
    duplicateActivePage,
    deleteActivePage,
    confirmDeleteActivePage,
    clearActivePage,
    goToPageNumber,
    moveActivePage,
    navigateToSearchHit,
    restoreRecovery,
    discardRecovery,
    addPage,
    importPdf,
    activePageNumber,
    updateTypstTransform,
    updateTypstSource,
    updateSharedStyle,
    addTypstBlock,
    nextVisualZIndex,
    nextReadingOrder,
    scheduleInkCommit,
    queueCommit,
    changeStrokes,
    addStroke,
    addShape,
    promoteHeldInk,
    convertSelectedInkToShape,
    eraseShape,
    replaceErasedShapes,
    changeShape,
    changeImage,
    undo,
    redo,
    routeHistory,
    scrollToPage,
    persist,
    exportPdf,
    closePage,
    showLibrary,
    moveLibrarySelection,
    returnToNotebook,
    closeNotebookTab,
    reopen,
    get root() { return root; },
    set root(value: typeof root) { root = value; },
    get notebookTitle() { return notebookTitle; },
    get notebookManifest() { return notebookManifest; },
    set notebookManifest(value: typeof notebookManifest) { notebookManifest = value; },
    get openTabs() { return openTabs; },
    set openTabs(value: typeof openTabs) { openTabs = value; },
    get switchingRoot() { return switchingRoot; },
    get notebookGeneration() { return notebookGeneration; },
    get sharedStyleSource() { return sharedStyleSource; },
    get presets() { return presets; },
    set presets(value: typeof presets) { presets = value; },
    get presetRevision() { return presetRevision; },
    set presetRevision(value: typeof presetRevision) { presetRevision = value; },
    get activeSnapshot() { return activeSnapshot; },
    set activeSnapshot(value: typeof activeSnapshot) { activeSnapshot = value; },
    get activePageId() { return activePageId; },
    get activeInkLayerId() { return activeInkLayerId; },
    set activeInkLayerId(value: typeof activeInkLayerId) { activeInkLayerId = value; },
    get activeInkLayerPath() { return activeInkLayerPath; },
    set activeInkLayerPath(value: typeof activeInkLayerPath) { activeInkLayerPath = value; },
    get activeBackground() { return activeBackground; },
    get activeGeometry() { return activeGeometry; },
    get pageEntries() { return pageEntries; },
    get pageOpen() { return pageOpen; },
    set pageOpen(value: typeof pageOpen) { pageOpen = value; },
    get busy() { return busy; },
    set busy(value: typeof busy) { busy = value; },
    get pageInputLocked() { return pageInputLocked; },
    get status() { return status; },
    set status(value: typeof status) { status = value; },
    get strokes() { return strokes; },
    set strokes(value: typeof strokes) { strokes = value; },
    get typstBlocks() { return typstBlocks; },
    set typstBlocks(value: typeof typstBlocks) { typstBlocks = value; },
    get pageTypst() { return pageTypst; },
    set pageTypst(value: typeof pageTypst) { pageTypst = value; },
    get images() { return images; },
    set images(value: typeof images) { images = value; },
    get shapes() { return shapes; },
    set shapes(value: typeof shapes) { shapes = value; },
    get canUndo() { return canUndo; },
    get canRedo() { return canRedo; },
    get pendingTransactions() { return pendingTransactions; },
    get transactionFailed() { return transactionFailed; },
    get inkPending() { return inkPending; },
    get typstDirty() { return typstDirty; },
    get pendingNeighborPages() { return pendingNeighborPages; },
    get neighborResults() { return neighborResults; },
    get lastScrollAt() { return lastScrollAt; },
    set lastScrollAt(value: typeof lastScrollAt) { lastScrollAt = value; },
    get FOCUS_SETTLE_MS() { return FOCUS_SETTLE_MS; },
    get conflictDetail() { return conflictDetail; },
    set conflictDetail(value: typeof conflictDetail) { conflictDetail = value; },
    get recoveryCandidates() { return recoveryCandidates; },
    get recoveryOpen() { return recoveryOpen; },
    set recoveryOpen(value: typeof recoveryOpen) { recoveryOpen = value; },
    get recoveryBusy() { return recoveryBusy; },
    get notebookChosen() { return notebookChosen; },
    set notebookChosen(value: typeof notebookChosen) { notebookChosen = value; },
    get coverRevision() { return coverRevision; },
    get confirmingPageDelete() { return confirmingPageDelete; },
    set confirmingPageDelete(value: typeof confirmingPageDelete) { confirmingPageDelete = value; },
    get pageCount() { return pageCount; },
    get pageNumber() { return pageNumber; },
    get saveMs() { return saveMs; },
    get reopenMs() { return reopenMs; },
    get exportMs() { return exportMs; },
  };
}
