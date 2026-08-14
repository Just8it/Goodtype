<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import {
    commitNotebook,
    createNotebook,
    createPage,
    deletePage,
    duplicatePage,
    focusPage,
    importPdfPages,
    openNotebook,
    openPage,
    pickPdfReference,
    reorderPages,
    runHistory,
    runStructureHistory,
    storePastedImage,
  } from "../ipc/notebook";
  import {
    discardRecoveryCandidate,
    listRecoveryCandidates,
    restoreRecoveryCandidate,
  } from "../ipc/recovery";
  // The local `compileTypst` owns the cache check, the browser fallback and the block's preview
  // state; this is only the round trip it makes when none of those answer.
  import { compileTypst as requestTypstCompile } from "../ipc/typst";
  import type { HistoryResult, NotebookSnapshot } from "../ipc/types";
  import {
    closeNotebookSession,
    exportNotebookPdf,
    listRecentNotebooks,
    recordNotebookOpened,
    recordNotebookPage,
    recordNotebookSession,
    resumeNotebookSession,
    writeMetrics,
    writeNotebookCover,
  } from "../ipc/workspace";
  import { clampZoom, dampedVelocity, pannedScroll, type Point } from "../geometry/coordinates";
  import { placeFloatingToolbar, type ViewRect } from "../geometry/placement";
  import {
    DEFAULT_INK_Z_INDEX,
    type NotebookManifest,
    type PageBackground,
    type PageGeometry,
    type PageObject,
    type PagePosition,
    type PageTemplate,
    type Stroke,
  } from "../model";
  import {
    TYPST_IDLE_DEBOUNCE_MS,
    type TypstCompileResult,
  } from "../editor/typst";
  import { clearTypstCache, getCachedTypst, setCachedTypst } from "../editor/typstCache";
  import {
    installPageTypstPreset,
    listTypstPresets,
    pickTypstPreset,
    setDefaultTypstPreset,
  } from "../ipc/presets";
  import {
    presetHeader,
    DEFAULT_PRESET_PATH,
    withPagePreset,
    type NotebookSetup,
    type PresetChoice,
    type PresetSummary,
  } from "../page/presets";
  import {
    summarizeMetric,
    type StrokePerformance,
  } from "../ink/metrics";
  import type { InkTool } from "../ink/pipeline";
  import {
    keepsSelection,
    moveSelected,
    scaleSelected,
    selectionBounds,
    toolAfterSelection,
  } from "../ink/selection";
  import ColorPanel from "./ColorPanel.svelte";
  import WidthPanel from "./WidthPanel.svelte";
  import ToolPanel from "./ToolPanel.svelte";
  import PageSurface from "./PageSurface.svelte";
  import OverflowMenu from "../workspace/OverflowMenu.svelte";
  import { populated, type MenuSection } from "../workspace/menu";
  import AddPageMenu from "../workspace/AddPageMenu.svelte";
  import type { AddPageGroup, AddPageSource, AddPageWhere } from "../workspace/addPage";
  import { templatePreviewSvg } from "../page/template";
  import { pdfPageGeometries } from "../pdf/document";
  import { PAPER_TONES, templateGroups } from "../page/templates";
  import {
    DEFAULT_PAGE_SIZE,
    PAGE_SIZES,
    describeGeometry,
    geometryOf,
    type Orientation,
  } from "../page/sizes";
  import SideEditor from "./SideEditor.svelte";
  import SelectionActions from "./SelectionActions.svelte";
  import NotebookTabs from "./NotebookTabs.svelte";
  import PaletteTools from "./PaletteTools.svelte";
  import {
    AssetUrlCache,
    blockViewsFromSnapshot,
    imageViewsFromSnapshot,
    mimeForPath,
    pageTypstViewFromSnapshot,
    strokesFromSnapshot,
    type BlockView,
    type ImageView,
    type PageTypstView,
    type TypstTransform,
  } from "./pageView";
  import { createCommitTimer } from "./commitTimer";
  import { blankNotebookSnapshot } from "./newNotebook";
  import { createInkCommitter, type InkCommitter } from "./inkCommitter";
  import {
    projectSnapshot,
    type ManagedMixedGroup,
  } from "./snapshot";
  import {
    nearestPaletteDock,
    type PaletteCommand,
    type PaletteDock,
  } from "./palette";
  import {
    moveReadingObject,
    moveVisualItems,
    type VisualMove,
  } from "./objectOrder";
  import { closedTab, cycledTab, openedTab, type NotebookTab } from "./tabs";
  import {
    addWidth as addRowWidth,
    canRemoveWidth,
    editWidth as editRowWidth,
    removeWidth as removeRowWidth,
    WIDTH_BOUNDS_MM,
  } from "./widths";
  import ConflictDialog from "../workspace/ConflictDialog.svelte";
  import RecoveryDialog from "../workspace/RecoveryDialog.svelte";
  import SearchOverlay from "../workspace/SearchOverlay.svelte";
  import SettingsPanel from "../workspace/SettingsPanel.svelte";
  import LibrarySurface from "../library/LibrarySurface.svelte";
  import { SHELF_ROOT, type ShelfLocation } from "../library/location";
  import { coverSvg, rasteriseCover } from "../library/cover";
  import {
    DEFAULT_SETTINGS,
    loadSettings,
    saveSettings,
    ERASER_RADIUS_PT,
    MAX_SWATCHES,
    MAX_WIDTHS,
    colorName,
    penType,
    withRecentColor,
    type AppSettings,
    type PenPreset,
    type RecoveryCandidate,
    type SearchHit,
  } from "../settings";

  type PageEntry = {
    id: string;
    path: string;
    /** From the manifest's hint until the page loads, then from the page itself. */
    geometry: PageGeometry;
    snapshot: NotebookSnapshot | null;
  };
  type TypstState = Omit<BlockView, keyof TypstTransform> & {
    transform: TypstTransform;
    result: TypstCompileResult | null;
  };
  type PageTypstState = PageTypstView & {
    result: TypstCompileResult | null;
  };
  type ImageState = ImageView;
  type PaletteDrag = {
    pointerId: number;
    clientX: number;
    clientY: number;
    startX: number;
    startY: number;
    width: number;
    height: number;
  };
  type PinchStart = { distance: number; zoom: number; center: Point; pagePoint: Point };
  type TouchPanStart = {
    pointerId: number;
    pointer: Point;
    scroll: Point;
    lastPointer: Point;
    lastTime: number;
    velocity: Point;
  };
  type TypstScaleEdit = { id: string; transform: TypstTransform };
  type NotebookAction =
    | { kind: "page"; pageId: string }
    | { kind: "structure" };

  // A4 exactly: 210mm x 297mm at 72pt/inch. It was rounded to 595x842, which is a tenth of a
  // millimetre narrow — invisible on its own, and wrong the moment a template promises 5mm
  // squares measured against it.
  const PAGE_WIDTH_PT = 595.2756;
  const PAGE_HEIGHT_PT = 841.8898;
  const MAIN_TYPST_ID = "typst-001";
  const BLOCK_PATH = "blocks/equation.typ";
  const INK_LAYER_ID = "ink-layer-001";
  const TYPST_SAVE_DEBOUNCE_MS = 250;
  const ERASER_SIZE_OPTIONS = [
    { id: "small" as const, label: "Small", diameter: 12 },
    { id: "medium" as const, label: "Medium", diameter: 18 },
    { id: "large" as const, label: "Large", diameter: 26 },
  ];
  const collectMetrics = import.meta.env.DEV;
  const tauriAvailable =
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

  let root = $state("");
  let notebookTitle = $state("Goodtype");
  let notebookManifest = $state<NotebookManifest | null>(null);
  let openTabs = $state<NotebookTab[]>([]);
  let switchingRoot = $state<string | null>(null);
  let notebookGeneration = 0;
  let sharedStyleSource = $state("");
  let presets = $state<PresetSummary[]>([]);
  let presetBusy = $state(false);
  let presetRevision = $state(0);
  let activeSnapshot = $state<NotebookSnapshot | null>(null);
  let activePageId = $state("page-001");
  let activeInkLayerId = $state(INK_LAYER_ID);
  let activeInkLayerPath = $state("ink/page-001-layer-001.json");
  /** The paper under the active page, kept so committing does not overwrite it. */
  let activeBackground = $state<PageBackground>({ kind: "plain", color: "#ffffff" });
  /** The active page's own size. Same reason: committing used to write one fixed geometry back. */
  let activeGeometry = $state<PageGeometry>({ widthPt: PAGE_WIDTH_PT, heightPt: PAGE_HEIGHT_PT });
  let pageEntries = $state<PageEntry[]>([]);
  let pageOpen = $state(false);
  let busy = $state(true);
  let status = $state("Opening the notebook…");
  let revision = $state(1);
  let zoom = $state(1);
  let createdAt = $state(new Date().toISOString());
  let tool = $state<InkTool>("pen");
  let strokes = $state<Stroke[]>([]);
  let selectedStrokeIds = $state<string[]>([]);
  let groupedStrokeIds = $state<string[]>([]);
  let typstBlocks = $state<TypstState[]>([
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
  let pageTypst = $state<PageTypstState | null>(null);
  let images = $state<ImageState[]>([]);
  let selectedImageId = $state<string | null>(null);
  let mixedGroup = $state<ManagedMixedGroup | null>(null);
  let selectedTypstId = $state<string | null>(null);
  let directObjectInput = $state(false);
  /** True while the selection tool is only active because the lasso handed over to it. */
  let lassoHandedOver = $state(false);
  let strokeMetrics = $state<StrokePerformance[]>([]);
  let compileMs = $state<number | null>(null);
  let zoomFrameMs = $state<number | null>(null);
  let saveMs = $state<number | null>(null);
  let reopenMs = $state<number | null>(null);
  let exportMs = $state<number | null>(null);
  let canUndo = $state(false);
  let canRedo = $state(false);
  let pendingTransactions = $state(0);
  let transactionFailed = $state(false);
  let transactionQueue: Promise<void> = Promise.resolve();
  let typstCommitTimer: ReturnType<typeof setTimeout> | undefined;
  let inkCommitLabel = "Updated ink";
  // The active page shares its timing with neighbouring pages but not its payload: the snapshot
  // is built when the timer fires, so it picks up whatever else changed in the meantime.
  const inkCommitTimer = createCommitTimer(() => queueCommit(inkCommitLabel));
  // Debounced ink is real unsaved work; the save indicator must not read "Saved" while it waits.
  let inkPending = $state(false);
  let typstDirty = $state(false);
  let pendingNeighborPages = $state(new Set<string>());
  let workspace = $state<HTMLElement>();
  let pageViewport = $state<HTMLElement>();
  let pageFrame = $state<HTMLElement>();
  const savePending = $derived(
    pendingTransactions > 0 ||
      inkPending ||
      typstDirty ||
      pendingNeighborPages.size > 0,
  );

  // The page being edited renders through the same surface as its neighbours, so its state is
  // projected into the same view model they use.
  const activeBlockViews: BlockView[] = $derived(
    typstBlocks.map((block) => ({
      id: block.id,
      path: block.path,
      source: block.source,
      x: block.transform.x,
      y: block.transform.y,
      layoutWidthPt: block.transform.layoutWidthPt,
      scale: block.transform.scale,
      zIndex: block.zIndex,
      readingOrder: block.readingOrder,
    })),
  );
  const activePageTypstView: PageTypstView | null = $derived(
    pageTypst
      ? {
          id: pageTypst.id,
          path: pageTypst.path,
          source: pageTypst.source,
          zIndex: pageTypst.zIndex,
          readingOrder: pageTypst.readingOrder,
        }
      : null,
  );
  const activeResults: Record<string, TypstCompileResult | null> = $derived(
    Object.fromEntries([
      ...typstBlocks.map((block) => [block.id, block.result] as const),
      ...(pageTypst ? [[pageTypst.id, pageTypst.result] as const] : []),
    ]),
  );
  const activeImageViews: ImageView[] = $derived(
    images.map((image) => ({
      id: image.id,
      path: image.path,
      url: image.url,
      alt: image.alt,
      x: image.x,
      y: image.y,
      widthPt: image.widthPt,
      heightPt: image.heightPt,
      scale: image.scale,
      zIndex: image.zIndex,
      readingOrder: image.readingOrder,
    })),
  );

  // Full-height source view beside the canvas. It is a sibling of the canvas region rather than
  // an overlay, so the canvas genuinely narrows — and the palette, which is positioned inside
  // that region, follows the paper instead of colliding with the panel.
  let sideEditorOpen = $state(false);
  let sideEditorStyle = $state(false);
  let sideEditorBlockId = $state<string | null>(null);
  let sideEditorPageText = $state(false);
  /// The page the target block belongs to, so scrolling elsewhere does not lose it.
  let sideEditorPageId = $state<string | null>(null);
  let sideEditor = $state<{ focus: () => void }>();

  const sideEditorBlock = $derived(
    typstBlocks.find((block) => block.id === sideEditorBlockId) ??
      (pageTypst?.id === sideEditorBlockId ? pageTypst : null),
  );
  const editingPageText = $derived(
    sideEditorOpen && sideEditorPageText && pageTypst?.id === sideEditorBlockId,
  );
  /// `edit` when the target is on the page in view, `away` when it is held on another page, and
  /// `none` when nothing has been picked yet. The target only changes when the writer picks one.
  const sideEditorMode = $derived<"edit" | "style" | "away" | "none">(
    sideEditorStyle ? "style" : sideEditorBlock ? "edit" : sideEditorBlockId ? "away" : "none",
  );
  const sideEditorPageNumber = $derived(
    sideEditorPageId
      ? (notebookManifest?.pages.findIndex((page) => page.id === sideEditorPageId) ?? 0) + 1
      : null,
  );

  function openSideEditor(blockId?: string) {
    sideEditorStyle = false;
    // An explicit canvas edit still wins. The generic button/shortcut starts with the sustained
    // Page text surface, then falls back to the same movable-block choices as before.
    const remembered = typstBlocks.some((block) => block.id === sideEditorBlockId)
      ? sideEditorBlockId
      : null;
    const target = blockId ?? pageTypst?.id ?? remembered ?? selectedTypstId ?? typstBlocks[0]?.id ?? null;
    sideEditorOpen = true;
    if (target) {
      if (target !== sideEditorBlockId) sideEditorPageId = activePageId;
      sideEditorBlockId = target;
      sideEditorPageText = pageTypst?.id === target;
      if (typstBlocks.some((block) => block.id === target)) {
        selectedTypstId = target;
        selectedImageId = null;
      } else if (pageTypst?.id === target) {
        selectedTypstId = null;
        selectedImageId = null;
      }
    } else if (sideEditorPageText) {
      sideEditorBlockId = null;
      sideEditorPageId = null;
      sideEditorPageText = false;
    }
    // The panel mounts this tick; take the caret once it exists.
    void tick().then(() => sideEditor?.focus());
  }

  function openPageText() {
    if (!pageTypst) {
      const hasDefault = presets.some((preset) => preset.kind === "default");
      pageTypst = {
        id: `${activePageId}-page-text`,
        path: `blocks/${activePageId}-page.typ`,
        source: hasDefault ? presetHeader() : "",
        result: null,
        zIndex: 0,
        readingOrder: 0,
      };
      queueCommit("Created page text");
      status = "Created page text";
    }
    openSideEditor(pageTypst.id);
  }

  async function changePagePreset(action: string) {
    if (!root || !pageTypst || presetBusy) return;
    presetBusy = true;
    try {
      if (action === "none" || action.startsWith("path:")) {
        const path = action === "none" ? null : action.slice(5);
        updateTypstSource(pageTypst.id, withPagePreset(pageTypst.source, path));
      } else if (action === "page:import" || action.startsWith("page:")) {
        const choice = action === "page:import"
          ? await pickTypstPreset()
          : { kind: "builtin", id: action.slice(5) } as PresetChoice;
        if (choice) {
          const installed = await installPageTypstPreset(root, choice);
          if (installed?.importPath) {
            updateTypstSource(pageTypst.id, withPagePreset(pageTypst.source, installed.importPath));
            await refreshPresets();
          }
        }
      } else if (action === "default:import" || action.startsWith("default:")) {
        const choice = action === "default:import"
          ? await pickTypstPreset()
          : { kind: "builtin", id: action.slice(8) } as PresetChoice;
        if (choice) {
          await setDefaultTypstPreset(root, choice);
          clearTypstCache(DEFAULT_PRESET_PATH);
          presetRevision += 1;
          await refreshPresets();
          status = "Updated the notebook Typst preset";
        }
      }
    } catch (error) {
      status = `Could not change the Typst preset: ${message(error)}`;
    } finally {
      presetBusy = false;
      void tick().then(() => sideEditor?.focus());
    }
  }

  function closeSideEditor() {
    sideEditorOpen = false;
  }

  function toggleSideEditor() {
    if (sideEditorOpen) closeSideEditor();
    else openSideEditor();
  }

  function openSharedStyle() {
    if (!notebookManifest) return;
    if (!notebookManifest.sharedStylePath) {
      notebookManifest = { ...notebookManifest, sharedStylePath: "style.typ" };
      queueCommit("Created shared Typst style");
    }
    sideEditorStyle = true;
    sideEditorOpen = true;
    moreOpen = false;
    void tick().then(() => sideEditor?.focus());
  }

  /// Tracks the frame of whichever page currently has edit focus, so zoom-to-point keeps working
  /// now that every page renders through one structure.
  function trackActiveFrame(node: HTMLElement, isActive: boolean) {
    if (isActive) pageFrame = node;
    return {
      update(nextActive: boolean) {
        if (nextActive) pageFrame = node;
        else if (pageFrame === node) pageFrame = undefined;
      },
      destroy() {
        if (pageFrame === node) pageFrame = undefined;
      },
    };
  }
  // The instrument bar snaps magnetically to the nearest workspace edge: horizontal along the
  // top/bottom, vertical along the left/right. Its inline sizes and colors follow that axis.
  let paletteX = $state(24);
  let paletteY = $state(96);
  let paletteDock = $state<PaletteDock>("bottom");
  let paletteDrag = $state<PaletteDrag | null>(null);
  let penPreset = $state<1 | 2>(1);
  /// Open colour editor: `index` is the swatch being edited, or -1 when adding a new one.
  /// `anchor` is that chip's centre within the palette, so the panel opens where you tapped.
  let colorPanel = $state<{ index: number; anchor: number } | null>(null);
  let widthPanel = $state<{ index: number; anchor: number } | null>(null);
  let paletteContextOpen = $state(false);
  /// Quick settings for a tool slot, opened by double-pressing its tile.
  let toolPanel = $state<{ kind: "pen" | "highlighter"; slot: number; anchor: number } | null>(
    null,
  );

  /// First press selects the tool; pressing the one already selected opens its settings — the
  /// same select-then-edit gesture the colour swatches use.
  function closePaletteContext() {
    paletteContextOpen = false;
    colorPanel = null;
    widthPanel = null;
    toolPanel = null;
  }

  function selectOrOpenTool(kind: "pen" | "highlighter", slot: number) {
    const alreadyActive =
      kind === "highlighter" ? tool === "highlighter" : tool === "pen" && penPreset === slot;
    if (!alreadyActive) {
      if (kind === "pen") activateTool("pen", slot as 1 | 2);
      else activateTool("highlighter");
      return;
    }
    if (paletteContextOpen) closePaletteContext();
    else paletteContextOpen = true;
  }

  function selectOrOpenEraser() {
    if (tool !== "eraser") activateTool("eraser");
    else if (paletteContextOpen) closePaletteContext();
    else paletteContextOpen = true;
  }

  const paletteCommands: Record<PaletteCommand, () => void> = {
    "pen-1": () => selectOrOpenTool("pen", 1),
    "pen-2": () => selectOrOpenTool("pen", 2),
    highlighter: () => selectOrOpenTool("highlighter", 1),
    eraser: selectOrOpenEraser,
    lasso: () => activateTool("lasso"),
    "page-text": openPageText,
    "typst-block": addTypstBlock,
  };

  function activePaletteCommand(): PaletteCommand {
    if (tool === "pen") return penPreset === 1 ? "pen-1" : "pen-2";
    if (tool === "highlighter" || tool === "eraser") return tool;
    return "lasso";
  }

  const activePaletteCommands = $derived<PaletteCommand[]>([
    activePaletteCommand(),
    ...(editingPageText ? (["page-text"] as const) : []),
  ]);
  const expandedPaletteCommand = $derived<PaletteCommand | null>(
    paletteContextOpen && (tool === "pen" || tool === "highlighter" || tool === "eraser")
      ? activePaletteCommand()
      : null,
  );

  function swatchAnchor(chip: HTMLElement): number {
    const bar = chip.closest(".instrument-palette");
    if (!bar) return 0;
    const chipBox = chip.getBoundingClientRect();
    const barBox = bar.getBoundingClientRect();
    const vertical = paletteDock === "left" || paletteDock === "right";
    return vertical
      ? chipBox.top + chipBox.height / 2 - barBox.top
      : chipBox.left + chipBox.width / 2 - barBox.left;
  }
  let moreOpen = $state(false);
  let addPageOpen = $state(false);
  // Remembered across openings: inserting a run of pages before the current one should not mean
  // re-picking "Before" every single time.
  let addPageWhere = $state<AddPageWhere>("after");
  /** Paper colour the picker is showing. Remembered for the same reason the position is. */
  let addPageToneId = $state(PAPER_TONES[0].id);
  let addPageSizeId = $state(DEFAULT_PAGE_SIZE.id);
  let addPageOrientation = $state<Orientation>("portrait");
  const addPageGeometry = $derived(
    geometryOf(
      PAGE_SIZES.find((size) => size.id === addPageSizeId) ?? DEFAULT_PAGE_SIZE,
      addPageOrientation,
    ),
  );
  let metricsOpen = $state(false);
  const touchPoints = new Map<number, Point>();
  let pinchStart: PinchStart | null = null;
  let touchPanStart: TouchPanStart | null = null;
  let automaticPageFocusLocked = false;
  let touchInertiaFrame: number | undefined;
  let typstScaleEdit: TypstScaleEdit | null = null;
  // Per-page state for the pages that are rendered but not being edited. It lives here rather
  // than inside the renderer so a page keeps its component instance — and its painted Typst
  // SVGs — when edit focus moves onto it.
  let neighborStrokes = $state<Record<string, Stroke[]>>({});
  let neighborResults = $state<
    Record<string, Record<string, TypstCompileResult | null>>
  >({});
  const neighborUrls = new Map<string, AssetUrlCache>();
  const neighborCommitters = new Map<string, InkCommitter>();
  let activating = false;
  let focusTimer: ReturnType<typeof setTimeout> | undefined;

  let settings = $state<AppSettings>(structuredClone(DEFAULT_SETTINGS));

  const toolPanelPreset = $derived(
    toolPanel?.kind === "highlighter"
      ? settings.highlighter
      : settings.penPresets[(toolPanel?.slot ?? 1) - 1],
  );

  function updateToolPreset(next: PenPreset) {
    if (!toolPanel) return;
    const slot = toolPanel.slot;
    if (toolPanel.kind === "highlighter") changeSettings({ ...settings, highlighter: next });
    else
      changeSettings({
        ...settings,
        penPresets: settings.penPresets.map((preset, index) =>
          index === slot - 1 ? next : preset,
        ),
      });
  }
  let settingsOpen = $state(false);
  let settingsSaveTimer: ReturnType<typeof setTimeout> | undefined;
  let settingsSaveQueue: Promise<void> = Promise.resolve();
  let settingsVersion = 0;
  let searchOpen = $state(false);
  let conflictDetail = $state<string | null>(null);
  let recoveryCandidates = $state<RecoveryCandidate[]>([]);
  let recoveryOpen = $state(false);
  let recoveryBusy = $state(false);
  let notebookChosen = $state(false);
  let showInitialSetup = $state(false);

  /**
   * Where the shelf was when a notebook was opened from it.
   *
   * Held here because `LibrarySurface` is unmounted for as long as a notebook is open. Closing
   * one should return you to the folder you opened it from, not to the library root three levels
   * up — the notebook you just closed is the thing you are most likely to want next to.
   */
  let shelfLocation = $state<ShelfLocation>(SHELF_ROOT);
  // Session-local order of committed changes across pages, so notebook-scoped undo can route
  // Ctrl+Z to the page that changed most recently.
  let notebookUndoOrder: NotebookAction[] = [];
  let notebookRedoOrder: NotebookAction[] = [];
  let metricsTimer: ReturnType<typeof setTimeout> | undefined;
  let removeCloseListener: (() => void) | undefined;
  let selectionToolbarElement = $state<HTMLElement>();
  let selectionToolbarFrame: number | undefined;
  let selectionToolbarPosition = $state({ left: 0, top: 0, ready: false });
  let closeConfirmed = false;

  onMount(() => {
    void initialize();
    window.addEventListener("keydown", historyShortcut);
    window.addEventListener("resize", scheduleSelectionToolbar);
    if (tauriAvailable) {
      void getCurrentWindow()
        .onCloseRequested(async (event) => {
          if (closeConfirmed) return;
          event.preventDefault();
          if (!(await persist())) return;
          if (notebookChosen && root) {
            await recordNotebookPage(root, activePageId).catch(() => {});
          }
          closeConfirmed = true;
          await getCurrentWindow().close();
        })
        .then((remove) => (removeCloseListener = remove));
    }
  });
  onDestroy(() => {
    inkCommitTimer.cancel();
    if (typstCommitTimer) clearTimeout(typstCommitTimer);
    if (focusTimer) clearTimeout(focusTimer);
    if (metricsTimer) clearTimeout(metricsTimer);
    if (selectionToolbarFrame) cancelAnimationFrame(selectionToolbarFrame);
    if (touchInertiaFrame) cancelAnimationFrame(touchInertiaFrame);
    removeCloseListener?.();
    window.removeEventListener("keydown", historyShortcut);
    window.removeEventListener("resize", scheduleSelectionToolbar);
    revokeImageUrl();
  });

  $effect(() => {
    if (!collectMetrics || !tauriAvailable || !root || !notebookChosen) return;
    const metrics = metricsPayload();
    // Metrics are dev telemetry; batching writes keeps them off the per-stroke path.
    if (metricsTimer) clearTimeout(metricsTimer);
    metricsTimer = setTimeout(() => {
      void writeMetrics(root, metrics).catch(() => {});
    }, 1000);
  });

  $effect(() => {
    const selected = selectedObjectId() || selectedStrokeIds.length > 0 || groupedStrokeIds.length > 0;
    void images;
    void strokes;
    void zoom;
    void activePageId;
    void sideEditorOpen;
    void settings.sideEditorWidth;
    selectionToolbarPosition = { left: 0, top: 0, ready: false };
    if (selected) void tick().then(scheduleSelectionToolbar);
  });

  async function initialize() {
    settings = await loadSettings(tauriAvailable);
    paletteDock = settings.paletteDock;
    if (!tauriAvailable) {
      root = "Browser preview (persistence and real Typst compilation require Tauri)";
      applySnapshot(buildSnapshot());
      openTabs = [{ root, title: notebookTitle }];
      notebookChosen = true;
      pageOpen = true;
      busy = false;
      status = "Browser preview ready";
      return;
    }

    try {
      const [session, recents] = await Promise.all([
        resumeNotebookSession(),
        listRecentNotebooks(),
      ]);
      openTabs = session.openRoots.map((knownRoot) => ({
        root: knownRoot,
        title: recents.find((entry) => entry.root === knownRoot)?.title ?? titleFromRoot(knownRoot),
      }));
      const candidates = [session.activeRoot, ...session.openRoots].filter(
        (candidate, index, roots): candidate is string =>
          Boolean(candidate) && roots.indexOf(candidate) === index,
      );
      for (const candidate of candidates) {
        if (await openNotebookAt(candidate, { skipCurrentPersist: true })) return;
        openTabs = openTabs.filter((tab) => tab.root !== candidate);
      }
      await recordNotebookSession(openTabs.map((tab) => tab.root), null);
    } catch (error) {
      status = `The last notebook could not be reopened: ${message(error)}`;
    }

    try {
      const recents = await listRecentNotebooks();
      showInitialSetup = recents.length === 0;
    } catch {
      // The recents list is a convenience; failing to read it falls through to the start surface.
    }
    busy = false;
  }

  function titleFromRoot(path: string) {
    return path.split(/[\\/]/).filter(Boolean).at(-1) ?? "Goodtype notebook";
  }

  function clearNotebookCaches() {
    stopTouchInertia();
    for (const urls of neighborUrls.values()) urls.dispose();
    neighborUrls.clear();
    for (const committer of neighborCommitters.values()) committer.dispose();
    neighborCommitters.clear();
    neighborStrokes = {};
    neighborResults = {};
    pendingNeighborPages = new Set();
    visibleRatios.clear();
    sideEditorOpen = false;
    sideEditorBlockId = null;
    sideEditorPageText = false;
    sideEditorPageId = null;
    searchOpen = false;
    recoveryCandidates = [];
    recoveryOpen = false;
  }

  async function rememberNotebookSession(): Promise<boolean> {
    if (!tauriAvailable) return true;
    try {
      await recordNotebookSession(openTabs.map((tab) => tab.root), root || null);
      return true;
    } catch {
      return false;
    }
  }

  async function openNotebookAt(
    nextRoot: string,
    options: { createIfMissing?: boolean; skipCurrentPersist?: boolean; setup?: NotebookSetup } = {},
  ): Promise<boolean> {
    if (nextRoot === root && activeSnapshot) {
      notebookChosen = true;
      pageOpen = true;
      await tick();
      scrollToPage(activePageId, "auto");
      status = "Notebook ready";
      return true;
    }
    busy = true;
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
      notebookGeneration += 1;
      clearNotebookCaches();
      root = nextRoot;
      transactionFailed = false;
      conflictDetail = null;
      notebookUndoOrder = [];
      notebookRedoOrder = [];
      pageEntries = [];
      applySnapshot(snapshot);
      if (createdNew) tool = "pen";
      await refreshPresets();
      notebookTitle = snapshot.manifest.title || titleFromRoot(nextRoot);
      openTabs = openedTab(openTabs, { root: nextRoot, title: notebookTitle });
      let sessionRemembered = true;
      try {
        await recordNotebookOpened(
          root,
          snapshot.manifest.title || "Goodtype notebook",
          new Date().toISOString(),
        );
        await recordNotebookPage(root, snapshot.page.id);
        if (!(await rememberNotebookSession())) sessionRemembered = false;
      } catch {
        sessionRemembered = false;
      }
      notebookChosen = true;
      pageOpen = true;
      await tick();
      scrollToPage(snapshot.page.id, "auto");
      status = sessionRemembered
        ? "Notebook ready"
        : "Notebook ready, but its reopening position could not be remembered";
      await refreshRecoveryCandidates();
      return true;
    } catch (error) {
      status = `Could not open the notebook: ${message(error)}`;
      return false;
    } finally {
      busy = false;
      switchingRoot = null;
    }
  }

  async function refreshPresets() {
    if (!root || !tauriAvailable) {
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
    if (!tauriAvailable || !root) return;
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
      schemaVersion: 1,
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
      typst: typstBlocks.map((block) => ({
        id: block.id,
        path: block.path,
        source: block.source,
        x: block.transform.x,
        y: block.transform.y,
        layoutWidthPt: block.transform.layoutWidthPt,
        scale: block.transform.scale,
        measuredWidthPt: block.result?.widthPt ?? block.transform.layoutWidthPt,
        measuredHeightPt: block.result?.heightPt ?? 48,
        zIndex: block.zIndex,
        readingOrder: block.readingOrder,
      })),
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
      sharedStyle: manifest.sharedStylePath
        ? { path: manifest.sharedStylePath, source: sharedStyleSource }
        : null,
      mixedGroup,
      groupedStrokeIds,
      now,
    });
  }

  function applySnapshot(snapshot: NotebookSnapshot) {
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
    delete neighborStrokes[snapshot.page.id];
    const activeInk = snapshot.page.inkLayers[0];
    activeInkLayerId = activeInk?.id ?? `${activePageId}-ink-001`;
    activeInkLayerPath = activeInk?.path ?? `ink/${activePageId}-layer-001.json`;
    // Carried through so `buildSnapshot` can put them back. It used to write a hardcoded white
    // A4 page, which meant the first stroke on a template erased the paper it was drawn on and
    // flattened any page that was not A4.
    activeBackground = snapshot.page.background;
    activeGeometry = snapshot.page.geometry;
    pageEntries = snapshot.manifest.pages.map((page) => ({
      ...page,
      snapshot:
        page.id === snapshot.page.id
          ? snapshot
          : pageEntries.find((entry) => entry.id === page.id)?.snapshot ?? null,
    }));
    revision = snapshot.page.revision;
    createdAt = snapshot.manifest.createdAt;
    strokes = snapshot.inkLayers[0]?.strokes ?? [];
    selectedStrokeIds = [];

    typstBlocks = snapshot.page.objects
      .filter(
        (object): object is Extract<PageObject, { type: "typst" }> =>
          object.type === "typst",
      )
      .map((object) => {
        const block = snapshot.blocks.find((file) => file.path === object.sourcePath);
        return {
          id: object.id,
          path: object.sourcePath,
          source: block
            ? new TextDecoder().decode(new Uint8Array(block.bytes))
            : "",
          transform: {
            x: object.x,
            y: object.y,
            layoutWidthPt: object.layoutWidthPt,
            scale: object.scale,
          },
          result: null,
          zIndex: object.zIndex,
          readingOrder: object.readingOrder,
        };
      });

    const storedPageTypst = snapshot.page.objects.find(
      (object): object is Extract<PageObject, { type: "page_typst" }> =>
        object.type === "page_typst",
    );
    if (storedPageTypst) {
      const block = snapshot.blocks.find((file) => file.path === storedPageTypst.sourcePath);
      pageTypst = {
        id: storedPageTypst.id,
        path: storedPageTypst.sourcePath,
        source: block ? new TextDecoder().decode(new Uint8Array(block.bytes)) : "",
        result: null,
        zIndex: storedPageTypst.zIndex,
        readingOrder: storedPageTypst.readingOrder,
      };
    } else {
      pageTypst = null;
    }

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
    mixedGroup =
      inkGroup && group && groupedTypstId
        ? {
            inkGroupId: inkGroup.id,
            groupId: group.id,
            typstId: groupedTypstId,
            active: true,
          }
        : null;
    groupedStrokeIds = mixedGroup ? inkGroup!.strokeIds : [];

    images = snapshot.page.objects
      .filter(
        (object): object is Extract<PageObject, { type: "image" }> =>
          object.type === "image",
      )
      .flatMap((object) => {
        const asset = snapshot.assets.find((file) => file.path === object.sourcePath);
        if (!asset) return [];
        const blob = new Blob([new Uint8Array(asset.bytes)], {
          type: mimeForPath(asset.path),
        });
        return [{
          id: object.id,
          path: asset.path,
          url: URL.createObjectURL(blob),
          alt: object.altText,
          x: object.x,
          y: object.y,
          widthPt: object.widthPt,
          heightPt: object.heightPt,
          scale: object.scale,
          zIndex: object.zIndex,
          readingOrder: object.readingOrder,
        }];
      });
    selectedImageId = null;
    selectedTypstId = null;
  }

  async function ensurePageLoaded(pageId: string) {
    const entry = pageEntries.find((page) => page.id === pageId);
    if (!entry || entry.snapshot) return entry?.snapshot ?? null;
    const requestRoot = root;
    const requestGeneration = notebookGeneration;
    try {
      const snapshot = await openPage(requestRoot, pageId);
      if (requestRoot !== root || requestGeneration !== notebookGeneration) return null;
      entry.snapshot = snapshot;
      return snapshot;
    } catch (error) {
      if (requestRoot !== root || requestGeneration !== notebookGeneration) return null;
      status = `Could not load page: ${message(error)}`;
      return null;
    }
  }

  const visibleRatios = new Map<string, number>();

  function observePage(node: HTMLElement, pageId: string) {
    // One loose observer preloads neighbors well before they enter view.
    const preload = new IntersectionObserver(
      (entries) => {
        if (!entries.some((entry) => entry.isIntersecting)) return;
        const index = pageEntries.findIndex((page) => page.id === pageId);
        for (const page of pageEntries.slice(Math.max(0, index - 1), index + 2)) {
          void ensurePageLoaded(page.id);
        }
      },
      { root: pageViewport, rootMargin: "100% 0px" },
    );
    // A second, tight observer tracks how much of each page is actually visible so the most
    // centered page can take edit focus — this is what makes undo, New Typst, and paste act on
    // the page you are looking at rather than a page pinned at open.
    const focus = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) visibleRatios.set(pageId, entry.intersectionRatio);
        scheduleFocus();
      },
      { root: pageViewport, threshold: [0, 0.25, 0.5, 0.75, 1] },
    );
    preload.observe(node);
    focus.observe(node);
    return {
      destroy: () => {
        preload.disconnect();
        focus.disconnect();
        visibleRatios.delete(pageId);
      },
    };
  }

  function scheduleFocus() {
    if (focusTimer) clearTimeout(focusTimer);
    focusTimer = setTimeout(() => {
      if (automaticPageFocusLocked || pinchStart) return;
      let bestId = activePageId;
      let bestRatio = 0;
      for (const [id, ratio] of visibleRatios) {
        if (ratio > bestRatio) {
          bestRatio = ratio;
          bestId = id;
        }
      }
      if (bestRatio >= 0.5 && bestId !== activePageId) void activatePage(bestId);
    }, 150);
  }

  async function activatePage(pageId: string) {
    if (
      !tauriAvailable ||
      pageId === activePageId ||
      activating ||
      busy ||
      pendingTransactions > 0 ||
      paletteDrag ||
      transactionFailed
    ) {
      return;
    }
    activating = true;
    const outgoing = activePageId;
    try {
      // Land every pending edit on the outgoing page, then flush the incoming page's neighbor
      // renderer, so focus_page reads a current revision instead of racing a debounced commit.
      flushInkCommit();
      flushTypstCommit();
      await transactionQueue;
      if (transactionFailed) return;
      await flushNeighbor(pageId);

      // Refresh the outgoing page's cached bundle so it renders its just-committed state once it
      // becomes a neighbor rather than the stale bundle it was activated with.
      const outgoingEntry = pageEntries.find((page) => page.id === outgoing);
      if (outgoingEntry) {
        try {
          outgoingEntry.snapshot = await openPage(root, outgoing);
          // It renders as a neighbour from here on, so let it derive from the fresh bundle.
          delete neighborStrokes[outgoing];
        } catch {
          // A neighbor that fails to reload simply shows a loading state; it is not fatal.
        }
      }

      const result = await focusPage(root, pageId);
      applySnapshot(result.snapshot);
      void recordNotebookPage(root, pageId).catch(() => {});
      canUndo = result.canUndo;
      canRedo = result.canRedo;
      await evictDistantPages();
      status = `Editing page ${activePageNumber()}`;
    } catch (error) {
      status = `Could not switch to that page: ${message(error)}`;
    } finally {
      activating = false;
    }
  }

  function updateLoadedPage(pageId: string, snapshot: NotebookSnapshot) {
    const entry = pageEntries.find((page) => page.id === pageId);
    if (entry) entry.snapshot = snapshot;
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
    committer = createInkCommitter({
      save: async (strokes, label) => {
        const entry = pageEntries.find((page) => page.id === pageId);
        if (!entry?.snapshot) {
          pendingNeighborPages = new Set(
            [...pendingNeighborPages].filter((id) => id !== pageId),
          );
          return;
        }
        const inkLayers = entry.snapshot.inkLayers.map((layer, index) =>
          index === 0 ? { ...layer, strokes } : layer,
        );
        const number =
          (notebookManifest?.pages.findIndex((page) => page.id === pageId) ?? 0) + 1;
        try {
          // Originals already exist on disk; a commit references them by path.
          const result = await commitNotebook(root, {
            ...entry.snapshot,
            assets: [],
            inkLayers,
          });
          updateLoadedPage(pageId, result.snapshot);
          neighborStrokes[pageId] = strokesFromSnapshot(result.snapshot);
          status = `${label} on page ${number}`;
        } catch (error) {
          reportCommitFailure(`${label} on page ${number}`, error);
        } finally {
          if (!committer?.pending()) {
            pendingNeighborPages = new Set(
              [...pendingNeighborPages].filter((id) => id !== pageId),
            );
          }
        }
      },
    });
    neighborCommitters.set(pageId, committer);
    return committer;
  }

  /**
   * Draw this notebook's shelf cover, once, on the way out.
   *
   * Closing is the right moment for three reasons. It happens once a session rather than once per
   * idle pause, so rasterising costs nothing anyone waits for. The work is already saved and in
   * memory. And it is immediately before the shelf is looked at, so the cover a writer sees is
   * the state they just left.
   *
   * Rendering covers when a *folder* opens instead would mean reading and rasterising a page for
   * every notebook in it — cheaper than it sounds, since only the first page is needed rather
   * than the whole notebook, but still forty SVG decodes between the click and the grid. Drawing
   * on the way out spreads that cost to one notebook at a time, at a moment with nothing to
   * block. A notebook never opened since covers existed simply shows its paper until it is.
   *
   * The cover is the first page. That is the page a notebook is recognised by — except when it
   * is not: a run of problem sheets that all open with the same letterhead is exactly the case
   * where the first page distinguishes nothing, which is why this reads the manifest's order
   * rather than assuming, and why a chosen cover page belongs here next.
   */
  async function writeCover() {
    if (!tauriAvailable || !root) return;
    const coverPageId = notebookManifest?.pages[0]?.id;
    if (!coverPageId) return;
    // Loaded on demand: the cover page is often not the one that was being worked on, and one
    // page read on close is cheaper than keeping the first page resident all session.
    const snapshot = await ensurePageLoaded(coverPageId);
    if (!snapshot) return;

    const { background, geometry } = snapshot.page;
    const png = await rasteriseCover(
      coverSvg(background, geometry, strokesFromSnapshot(snapshot)),
      geometry,
    );
    if (!png) return;
    try {
      await writeNotebookCover(root, png);
    } catch {
      // A cover is a nicety. A notebook that saved correctly must never report a failure
      // because its thumbnail could not be drawn.
    }
  }

  function commitNeighborInk(pageId: string, strokes: Stroke[], label: string) {
    neighborStrokes[pageId] = strokes;
    const committer = neighborCommitter(pageId);
    pendingNeighborPages = new Set(pendingNeighborPages).add(pageId);
    committer.commit(strokes, label);
  }

  async function compileNeighborTypst(
    pageId: string,
    blockId: string,
    request: { source: string; sharedStyle?: string | null; widthPt: number; generation: number },
  ) {
    const requestRoot = root;
    const requestGeneration = notebookGeneration;
    const cacheSource = `${request.sharedStyle ?? ""}\n${request.source}`;
    const cached = getCachedTypst(cacheSource, request.widthPt);
    if (cached) {
      neighborResults[pageId] = {
        ...neighborResults[pageId],
        [blockId]: { ...cached, generation: request.generation },
      };
      return;
    }
    if (!tauriAvailable) return;
    try {
      const result = await requestTypstCompile(requestRoot, request);
      setCachedTypst(cacheSource, request.widthPt, result);
      if (requestRoot !== root || requestGeneration !== notebookGeneration) return;
      neighborResults[pageId] = {
        ...neighborResults[pageId],
        [blockId]: result,
      };
    } catch {
      // The page stays readable through its ink and images if a background preview fails.
    }
  }

  /// Land a non-active page's debounced ink before it is promoted to the active editor, so
  /// focus_page reads a current revision instead of racing the pending commit.
  function flushNeighbor(pageId: string): Promise<void> {
    return neighborCommitters.get(pageId)?.flush() ?? Promise.resolve();
  }

  async function releaseNeighbor(pageId: string): Promise<boolean> {
    await flushNeighbor(pageId);
    if (transactionFailed) return false;
    neighborUrls.get(pageId)?.dispose();
    neighborUrls.delete(pageId);
    neighborCommitters.get(pageId)?.dispose();
    neighborCommitters.delete(pageId);
    delete neighborStrokes[pageId];
    delete neighborResults[pageId];
    return true;
  }

  /// Keep only the active page's neighbors as full bundles (Phase 2 §7 residency budget).
  /// Evicted pages fall back to placeholders and reload on demand near the viewport.
  async function evictDistantPages() {
    const active = pageEntries.findIndex((page) => page.id === activePageId);
    if (active < 0) return;
    for (const [index, entry] of pageEntries.entries()) {
      if (Math.abs(index - active) > 2 && entry.snapshot) {
        if (await releaseNeighbor(entry.id)) entry.snapshot = null;
      }
    }
  }

  function changeSettings(next: AppSettings) {
    settings = next;
    paletteDock = next.paletteDock;
    const version = ++settingsVersion;
    if (settingsSaveTimer) clearTimeout(settingsSaveTimer);
    settingsSaveTimer = setTimeout(() => {
      const pending = settings;
      settingsSaveQueue = settingsSaveQueue.then(async () => {
        try {
          const sanitized = await saveSettings(tauriAvailable, pending);
          if (version === settingsVersion) settings = sanitized;
        } catch (error) {
          status = `Settings were not saved: ${message(error)}`;
        }
      });
    }, 400);
  }

  async function duplicateActivePage() {
    moreOpen = false;
    if (!tauriAvailable || !(await persist())) return;
    busy = true;
    try {
      const result = await duplicatePage(root, activePageId, new Date().toISOString());
      pageEntries = [];
      applySnapshot(result.snapshot);
      recordStructureAction();
      await tick();
      scrollToPage(result.snapshot.page.id);
      status = `Duplicated into page ${activePageNumber()}`;
    } catch (error) {
      status = `Could not duplicate the page: ${message(error)}`;
    } finally {
      busy = false;
    }
  }

  async function deleteActivePage() {
    moreOpen = false;
    if (!tauriAvailable || !(await persist())) return;
    const pageNumber = activePageNumber();
    if (
      !window.confirm(
        `Delete page ${pageNumber}? Its files are kept for recovery, but it leaves this notebook.`,
      )
    ) {
      return;
    }
    busy = true;
    try {
      const result = await deletePage(root, activePageId, new Date().toISOString());
      pageEntries = [];
      applySnapshot(result.snapshot);
      recordStructureAction();
      await tick();
      scrollToPage(result.snapshot.page.id);
      status = `Deleted page ${pageNumber}`;
    } catch (error) {
      status = `Could not delete the page: ${message(error)}`;
    } finally {
      busy = false;
    }
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
    moreOpen = false;
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
    selectedStrokeIds = [];
    groupedStrokeIds = [];
    revokeImageUrl();
    images = [];
    activeSnapshot = activeSnapshot
      ? {
          ...activeSnapshot,
          page: { ...activeSnapshot.page, objects: [], readingOrder: [] },
          blocks: [],
        }
      : null;
    selectedTypstId = null;
    selectedImageId = null;
    mixedGroup = null;
    queueCommit("Cleared page");
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

  /**
   * The overflow menu, described as data. A new page-level feature — a template picker, a
   * bookmark, rotation — is an entry in this list rather than another branch of markup, which
   * is the point: this menu is where all of them have to surface.
   */
  function menuSections(): MenuSection[] {
    const many = pageCount > 1;
    const selected = selectedObjectId();
    if (!pageOpen) {
      return [
        {
          title: "Notebook",
          entries: [
            { kind: "action", id: "reopen", label: "Reopen notebook", onSelect: reopen },
            { kind: "action", id: "settings", label: "Settings", hint: "Ctrl ,", onSelect: () => (settingsOpen = true) },
          ],
        },
      ];
    }
    return populated([
      ...(selected
        ? [
            {
              title: selectedImageId ? "Selected image" : "Selected Typst",
              entries: [
                { kind: "action" as const, id: "object-back", label: "Send behind everything", disabled: !canMoveVisual(-1), onSelect: () => changeVisualOrder("back") },
                { kind: "action" as const, id: "object-front", label: "Bring in front of everything", disabled: !canMoveVisual(1), onSelect: () => changeVisualOrder("front") },
                { kind: "action" as const, id: "read-earlier", label: "Read earlier by screen readers", disabled: !canMoveReading(-1), onSelect: () => changeReadingOrder(-1) },
                { kind: "action" as const, id: "read-later", label: "Read later by screen readers", disabled: !canMoveReading(1), onSelect: () => changeReadingOrder(1) },
                { kind: "action" as const, id: "remove-object", label: "Remove from page", hint: "Delete", onSelect: () => void deleteSelection() },
              ],
            },
          ]
        : []),
      {
        entries: [
          // Adding a page is not here: it has its own header button, because choosing where the
          // page goes and what it is made of needs more than one row.
          { kind: "action", id: "duplicate", label: "Duplicate Page", onSelect: duplicateActivePage },
          { kind: "action", id: "up", label: "Move Page Up", disabled: !many || pageNumber === 1, onSelect: () => void moveActivePage(-1) },
          { kind: "action", id: "down", label: "Move Page Down", disabled: !many || pageNumber === pageCount, onSelect: () => void moveActivePage(1) },
          { kind: "number", id: "goto", label: "Go to Page", value: pageNumber, min: 1, max: pageCount, hint: `of ${pageCount}`, disabled: !many, onCommit: (number) => void goToPageNumber(number) },
        ],
      },
      {
        title: "Clear or remove page",
        entries: [
          { kind: "action", id: "clear", label: "Clear Page", destructive: true, onSelect: clearActivePage },
          // Undo covers this: deleting drops the manifest reference and the page's own files stay
          // on disk. That is why there is no trash bin.
          { kind: "action", id: "delete", label: "Delete Page", destructive: true, disabled: !many, onSelect: deleteActivePage },
        ],
      },
      {
        title: "Notebook",
        entries: [
          { kind: "action", id: "search", label: "Search notebook", hint: "Ctrl F", onSelect: () => (searchOpen = true) },
          { kind: "action", id: "style", label: "Edit shared Typst style", onSelect: openSharedStyle },
          { kind: "action", id: "settings", label: "Settings", hint: "Ctrl ,", onSelect: () => (settingsOpen = true) },
          { kind: "action", id: "save", label: "Confirm saved", onSelect: () => void persist() },
          ...(collectMetrics
            ? [{ kind: "action" as const, id: "metrics", label: "Timing evidence", onSelect: () => (metricsOpen = true) }]
            : []),
          { kind: "action", id: "close", label: "Close notebook", onSelect: closePage },
        ],
      },
    ]);
  }

  async function moveActivePage(direction: -1 | 1) {
    moreOpen = false;
    const manifest = notebookManifest;
    if (!tauriAvailable || !manifest || !(await persist())) return;
    const order = manifest.pages.map((page) => page.id);
    const index = order.indexOf(activePageId);
    const target = index + direction;
    if (index < 0 || target < 0 || target >= order.length) return;
    [order[index], order[target]] = [order[target], order[index]];
    busy = true;
    try {
      const result = await reorderPages(
        root,
        order,
        new Date().toISOString(),
        activePageId,
      );
      pageEntries = [];
      applySnapshot(result.snapshot);
      recordStructureAction();
      await tick();
      scrollToPage(activePageId);
      status = `Moved page to position ${target + 1}`;
    } catch (error) {
      status = `Could not reorder pages: ${message(error)}`;
    } finally {
      busy = false;
    }
  }

  async function navigateToSearchHit(hit: SearchHit) {
    searchOpen = false;
    const entry = pageEntries.find((page) => page.id === hit.pageId);
    if (!entry) return;
    await ensurePageLoaded(hit.pageId);
    scrollToPage(hit.pageId);
    await activatePage(hit.pageId);
    if (activePageId === hit.pageId) {
      selectedTypstId = typstBlocks.some((block) => block.id === hit.objectId)
        ? hit.objectId
        : null;
      selectedImageId = null;
      status = `Found on page ${hit.pageNumber}`;
    }
  }

  async function restoreRecovery(fileName: string) {
    recoveryBusy = true;
    try {
      const result = await restoreRecoveryCandidate(root, fileName);
      pageEntries = [];
      applySnapshot(result.snapshot);
      canUndo = result.canUndo;
      canRedo = result.canRedo;
      status = `Restored recovered work on page ${activePageNumber()}`;
      recoveryCandidates = recoveryCandidates.filter(
        (candidate) => candidate.fileName !== fileName,
      );
      recoveryOpen = recoveryCandidates.length > 0;
    } catch (error) {
      status = `Could not restore the recovered work: ${message(error)}`;
    } finally {
      recoveryBusy = false;
    }
  }

  async function discardRecovery(fileName: string) {
    recoveryBusy = true;
    try {
      await discardRecoveryCandidate(root, fileName);
      recoveryCandidates = recoveryCandidates.filter(
        (candidate) => candidate.fileName !== fileName,
      );
      recoveryOpen = recoveryCandidates.length > 0;
      status = "Discarded the recovered copy";
    } catch (error) {
      status = `Could not discard the recovered copy: ${message(error)}`;
    } finally {
      recoveryBusy = false;
    }
  }

  async function addPage(
    position: PagePosition,
    background: PageBackground | null = null,
    geometry: PageGeometry | null = null,
  ) {
    moreOpen = false;
    addPageOpen = false;
    if (!(await persist())) return;
    busy = true;
    try {
      const result = await createPage(
        root,
        new Date().toISOString(),
        position,
        background,
        geometry,
        activePageId,
      );
      pageEntries = [];
      applySnapshot(result.snapshot);
      recordStructureAction();
      // Load whatever now sits above the new page so the scroll lands with context above it
      // rather than against the top of an otherwise empty run.
      const index = result.snapshot.manifest.pages.findIndex(
        (page) => page.id === result.snapshot.page.id,
      );
      const above = index > 0 ? result.snapshot.manifest.pages[index - 1] : undefined;
      if (above) await ensurePageLoaded(above.id);
      await tick();
      scrollToPage(result.snapshot.page.id);
      status = `Added page ${activePageNumber()}`;
    } catch (error) {
      status = `Could not add page: ${message(error)}`;
    } finally {
      busy = false;
    }
  }

  async function importPdf(position: PagePosition) {
    moreOpen = false;
    addPageOpen = false;
    if (!tauriAvailable || !(await persist())) return;
    busy = true;
    try {
      const sourcePath = await pickPdfReference(root);
      if (!sourcePath) return;
      status = "Reading PDF pages…";
      const geometries = await pdfPageGeometries(root, sourcePath);
      const result = await importPdfPages(
        root,
        new Date().toISOString(),
        position,
        sourcePath,
        geometries,
        activePageId,
      );
      pageEntries = [];
      applySnapshot(result.snapshot);
      recordStructureAction();
      const index = result.snapshot.manifest.pages.findIndex(
        (page) => page.id === result.snapshot.page.id,
      );
      const above = index > 0 ? result.snapshot.manifest.pages[index - 1] : undefined;
      if (above) await ensurePageLoaded(above.id);
      await tick();
      scrollToPage(result.snapshot.page.id);
      status = `Imported ${geometries.length} PDF ${geometries.length === 1 ? "page" : "pages"}`;
    } catch (error) {
      status = `Could not import PDF: ${message(error)}`;
    } finally {
      busy = false;
    }
  }

  /**
   * What a new page can be made from, on the shelves the picker shows. One entry per source,
   * which is the extension point: an image and PDF import arrive here rather than in the menu's
   * markup.
   *
   * A template's definition is copied onto the page rather than referenced, so the notebook
   * still looks like itself on a machine that never had this build's library — the same rule a
   * stroke follows when it stores its resolved nib parameters instead of a pen name.
   */
  function addPageGroups(): AddPageGroup[] {
    // Previews are drawn at the size the new page will be, so a swatch of A5 squared paper shows
    // the same 5mm cells at A5's proportions rather than A4's.
    const geometry = addPageGeometry;
    const template = (source: PageTemplate): AddPageSource => ({
      id: source.id,
      label: source.name,
      preview: templatePreviewSvg(source, geometry),
      onSelect: (position) =>
        void addPage(position, { kind: "template", template: source }, geometry),
    });
    return [
      {
        id: "import",
        title: "Import",
        sources: [
          {
            id: "pdf",
            label: "Import PDF",
            detail: "Add every PDF page to the notebook",
            disabled: !tauriAvailable,
            compact: true,
            preview:
              '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48"><rect x="8" y="5" width="29" height="38" rx="3" fill="#fff" stroke="#9aa3ad"/><path d="M29 5v10h8" fill="none" stroke="#9aa3ad"/><rect x="12" y="25" width="29" height="13" rx="3" fill="#d85b55"/><text x="26.5" y="34.5" fill="#fff" font-size="8" font-family="sans-serif" font-weight="700" text-anchor="middle">PDF</text></svg>',
            onSelect: (position) => void importPdf(position),
          },
        ],
      },
      {
        id: "current",
        title: "This page",
        sources: [
          {
            id: "same",
            label: "Same paper",
            detail: describeGeometry(activeGeometry),
            preview:
              activeBackground.kind === "template"
                ? templatePreviewSvg(activeBackground.template, activeGeometry)
                : undefined,
            // Matches this page outright — its paper *and* its size — rather than picking up
            // whatever size is selected above.
            onSelect: (position) => void addPage(position, activeBackground, activeGeometry),
          },
        ],
      },
      ...templateGroups(PAPER_TONES.find((paper) => paper.id === addPageToneId) ?? PAPER_TONES[0]).map((group) => ({
        id: group.id,
        title: group.title,
        sources: group.templates.map(template),
      })),
    ];
  }

  function activePageNumber() {
    const index = notebookManifest?.pages.findIndex((page) => page.id === activePageId) ?? 0;
    return Math.max(0, index) + 1;
  }

  async function compileTypst(id: string, request: {
    source: string;
    sharedStyle?: string | null;
    widthPt: number;
    generation: number;
  }) {
    const requestRoot = root;
    const requestGeneration = notebookGeneration;
    const startedAt = performance.now();
    let result: TypstCompileResult;
    if (!tauriAvailable) {
      const browserSource = `${request.sharedStyle ?? ""}\n${request.source}`;
      result = {
        generation: request.generation,
        svg: previewSvg(browserSource, request.widthPt),
        widthPt: request.widthPt,
        heightPt: 64,
        // The browser stand-in draws its own SVG at exactly the block's size, with nothing
        // outside it to make room for.
        padPt: 0,
        diagnostics: [],
      };
      if (collectMetrics) compileMs = performance.now() - startedAt;
    } else {
      const cacheSource = `${request.sharedStyle ?? ""}\n${request.source}`;
      const cached = getCachedTypst(cacheSource, request.widthPt);
      if (cached) {
        // Unchanged source: reuse the compiled SVG, stamped with this request's generation so
        // the block's preview state machine accepts it. No recompile, no IPC round trip.
        result = { ...cached, generation: request.generation };
      } else {
        try {
          result = await requestTypstCompile(requestRoot, request);
          setCachedTypst(cacheSource, request.widthPt, result);
        } catch (error) {
          result = {
            generation: request.generation,
            svg: null,
            widthPt: null,
            heightPt: null,
            padPt: 0,
            diagnostics: [{ severity: "error", message: message(error) }],
          };
        }
      }
      if (collectMetrics) compileMs = performance.now() - startedAt;
    }
    if (requestRoot !== root || requestGeneration !== notebookGeneration) return;
    if (pageTypst?.id === id) pageTypst = { ...pageTypst, result };
    else {
      typstBlocks = typstBlocks.map((block) =>
        block.id === id ? { ...block, result } : block,
      );
    }
  }

  function updateTypstTransform(id: string, next: TypstTransform) {
    const previous = typstBlocks.find((block) => block.id === id)?.transform;
    if (!previous) return;
    if (mixedGroup?.active && id === mixedGroup.typstId && groupedStrokeIds.length > 0) {
      let nextStrokes = strokes;
      const scaleRatio = next.scale / Math.max(previous.scale, 0.05);
      if (scaleRatio !== 1) {
        nextStrokes = scaleSelected(
          nextStrokes,
          groupedStrokeIds,
          { x: previous.x, y: previous.y },
          scaleRatio,
        );
      }
      const delta = {
        x: next.x - previous.x,
        y: next.y - previous.y,
      };
      if (delta.x !== 0 || delta.y !== 0) {
        nextStrokes = moveSelected(nextStrokes, groupedStrokeIds, delta);
      }
      strokes = nextStrokes;
    }
    typstBlocks = typstBlocks.map((block) =>
      block.id === id ? { ...block, transform: next } : block,
    );
    queueCommit("Updated Typst block");
  }

  function updateTypstSource(id: string, source: string) {
    if (pageTypst?.id === id) pageTypst = { ...pageTypst, source };
    else {
      typstBlocks = typstBlocks.map((block) =>
        block.id === id ? { ...block, source } : block,
      );
    }
    typstDirty = true;
    if (typstCommitTimer) clearTimeout(typstCommitTimer);
    typstCommitTimer = setTimeout(flushTypstCommit, TYPST_SAVE_DEBOUNCE_MS);
  }

  function updateSharedStyle(source: string) {
    sharedStyleSource = source;
    typstDirty = true;
    if (typstCommitTimer) clearTimeout(typstCommitTimer);
    typstCommitTimer = setTimeout(flushTypstCommit, TYPST_SAVE_DEBOUNCE_MS);
  }

  function previewTypstScale(event: Event) {
    if (!selectedTypstId) return;
    const block = typstBlocks.find((candidate) => candidate.id === selectedTypstId);
    if (!block) return;
    if (!typstScaleEdit || typstScaleEdit.id !== block.id) {
      typstScaleEdit = { id: block.id, transform: { ...block.transform } };
    }
    const scale = Number((event.currentTarget as HTMLInputElement).value);
    typstBlocks = typstBlocks.map((candidate) =>
      candidate.id === block.id
        ? { ...candidate, transform: { ...candidate.transform, scale } }
        : candidate,
    );
  }

  function commitTypstScale() {
    if (!typstScaleEdit) return;
    const edit = typstScaleEdit;
    const next = typstBlocks.find((block) => block.id === edit.id)?.transform;
    typstScaleEdit = null;
    if (!next) return;
    typstBlocks = typstBlocks.map((block) =>
      block.id === edit.id ? { ...block, transform: edit.transform } : block,
    );
    updateTypstTransform(edit.id, next);
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
        source: "= New block\n\nType here",
        transform: {
          x: 150 + (typstBlocks.length % 4) * 20,
          y: 220 + (typstBlocks.length % 6) * 28,
          layoutWidthPt: 230,
          scale: 1,
        },
        result: null,
        zIndex: nextVisualZIndex(),
        readingOrder: typstBlocks.length + images.length,
      },
    ];
    selectedTypstId = id;
    selectedImageId = null;
    status = "Created a new Typst block";
    queueCommit("Created Typst block");
  }

  function nextVisualZIndex(): number {
    return Math.max(
      0,
      ...(activeSnapshot?.page.objects.map((object) => object.zIndex) ?? []),
      ...typstBlocks.map((block) => block.zIndex),
      ...images.map((image) => image.zIndex),
      ...strokes.map((stroke) => stroke.zIndex ?? DEFAULT_INK_Z_INDEX),
    ) + 1;
  }

  function nextSnapshotVisualZIndex(snapshot: NotebookSnapshot, pageStrokes: Stroke[]): number {
    return Math.max(
      0,
      ...snapshot.page.objects.map((object) => object.zIndex),
      ...pageStrokes.map((stroke) => stroke.zIndex ?? DEFAULT_INK_Z_INDEX),
    ) + 1;
  }

  function flushTypstCommit() {
    if (typstCommitTimer) clearTimeout(typstCommitTimer);
    typstCommitTimer = undefined;
    if (!typstDirty) return;
    typstDirty = false;
    queueCommit("Updated Typst source");
  }

  function groupSelectedInk() {
    if (selectedStrokeIds.length === 0) return;
    const typstId = selectedTypstId;
    if (!typstId) {
      status = "Select a Typst block before grouping it with ink";
      return;
    }
    const typstObject = activeSnapshot?.page.objects.find((object) => object.id === typstId);
    if (
      (typstObject?.groupId && typstObject.groupId !== mixedGroup?.groupId) ||
      (mixedGroup?.active && mixedGroup.typstId !== typstId) ||
      strokes.some(
        (stroke) =>
          selectedStrokeIds.includes(stroke.id) &&
          stroke.groupId &&
          stroke.groupId !== mixedGroup?.inkGroupId,
      )
    ) {
      status = "Ungroup the existing selection before creating another mixed group";
      return;
    }
    const occupied = new Set(activeSnapshot?.page.objects.map((object) => object.id) ?? []);
    const freshId = (prefix: string) => {
      let number = 1;
      while (occupied.has(`${prefix}-${number}`)) number += 1;
      const id = `${prefix}-${number}`;
      occupied.add(id);
      return id;
    };
    mixedGroup ??= {
      inkGroupId: freshId("ink-group"),
      groupId: freshId("group"),
      typstId,
      active: true,
    };
    mixedGroup = { ...mixedGroup, typstId, active: true };
    const selected = new Set(selectedStrokeIds);
    groupedStrokeIds = [...selectedStrokeIds];
    strokes = strokes.map((stroke) => ({
      ...stroke,
      groupId: selected.has(stroke.id) ? mixedGroup!.inkGroupId : stroke.groupId,
    }));
    status = `Grouped ${groupedStrokeIds.length} ink stroke${groupedStrokeIds.length === 1 ? "" : "s"} with the Typst block`;
    queueCommit("Grouped ink with Typst");
  }

  /**
   * Every change to the ink selection goes through here, because the lasso's hand-over to the
   * selection tool has to be undone when the selection empties — and a delete that assigned
   * `selectedStrokeIds` directly would skip that and strand the writer on a tool they never
   * picked.
   */
  function updateInkSelection(ids: string[]) {
    selectedStrokeIds = ids;
    const next = toolAfterSelection(tool, ids.length, lassoHandedOver);
    tool = next.tool;
    lassoHandedOver = next.handedOver;
  }

  const TOOL_NAMES: Record<InkTool, string> = {
    pen: "Pen",
    highlighter: "Highlighter",
    eraser: "Eraser",
    lasso: "Lasso",
    select: "Ink selection",
  };

  function activateTool(next: InkTool, preset?: 1 | 2) {
    if (next !== tool || (next === "pen" && preset !== undefined && preset !== penPreset)) {
      closePaletteContext();
    }
    if (preset) penPreset = preset;
    tool = next;
    lassoHandedOver = false;
    // A brush owns the page, so anything still selected would sit there showing handles and
    // refusing to move. Dropping it is the honest half of that: the selection is gone because
    // the tool that acted on it is.
    if (!keepsSelection(next)) {
      updateInkSelection([]);
      selectedTypstId = null;
      selectedImageId = null;
    }
    status = `${next === "pen" ? `Pen ${penPreset}` : TOOL_NAMES[next]} active`;
  }

  // The palette carries inline stroke sizes and colors, contextual to the active tool, so the
  // everyday change is a single tap on the bar rather than a submenu. Pressure/calibration
  // stay in the Settings window.
  // Chips now read from the writer's curated rows rather than frozen constants, which is what
  // makes a colour or width added here actually appear — and stay — on the bar.
  const activeWidthChips = $derived(
    tool === "highlighter" ? settings.highlighterWidths : settings.penWidths,
  );
  const activeColorChips = $derived(
    tool === "highlighter" ? settings.highlighterSwatches : settings.penSwatches,
  );
  const activeWidth = $derived(
    tool === "highlighter" ? settings.highlighter.widthPt : settings.penPresets[penPreset - 1].widthPt,
  );
  const activeInkColor = $derived(
    tool === "highlighter" ? settings.highlighter.color : settings.penPresets[penPreset - 1].color,
  );

  function nearestChip(chips: readonly number[], value: number): number {
    return chips.reduce((best, chip) =>
      Math.abs(chip - value) < Math.abs(best - value) ? chip : best,
    );
  }

  function setActiveWidth(widthPt: number) {
    if (tool === "highlighter") {
      changeSettings({ ...settings, highlighter: { ...settings.highlighter, widthPt } });
    } else {
      changeSettings({
        ...settings,
        penPresets: settings.penPresets.map((preset, index) =>
          index === penPreset - 1 ? { ...preset, widthPt } : preset,
        ),
      });
    }
  }

  /// Point the active tool at a colour, and remember it as recently used. This changes which
  /// colour the pen writes with; it does not touch the swatch row.
  function setActiveColor(color: string) {
    const next = { ...settings, recentColors: withRecentColor(settings.recentColors, color) };
    if (tool === "highlighter") {
      changeSettings({ ...next, highlighter: { ...settings.highlighter, color } });
    } else {
      changeSettings({
        ...next,
        penPresets: settings.penPresets.map((preset, index) =>
          index === penPreset - 1 ? { ...preset, color } : preset,
        ),
      });
    }
  }

  /// Replace the swatch at `index` for the active tool, and follow it with the active colour so
  /// editing the chip you are drawing with changes your ink immediately.
  function editSwatch(index: number, color: string) {
    const key = tool === "highlighter" ? "highlighterSwatches" : "penSwatches";
    const swatches = settings[key].map((existing, position) =>
      position === index ? color : existing,
    );
    const wasActive = settings[key][index]?.toLowerCase() === activeInkColor.toLowerCase();
    const next = { ...settings, [key]: swatches } as AppSettings;
    changeSettings(next);
    if (wasActive) setActiveColor(color);
  }

  /// Append a colour to the active tool's swatch row and select it.
  function addSwatch(color: string) {
    const key = tool === "highlighter" ? "highlighterSwatches" : "penSwatches";
    if (settings[key].some((existing) => existing.toLowerCase() === color.toLowerCase())) {
      setActiveColor(color);
      return;
    }
    if (settings[key].length >= MAX_SWATCHES) {
      status = `The palette holds at most ${MAX_SWATCHES} colors`;
      return;
    }
    changeSettings({ ...settings, [key]: [...settings[key], color] } as AppSettings);
    setActiveColor(color);
  }

  /**
   * The width row, edited the same way the colour row is.
   *
   * Widths are stored per tool, sorted, and de-duplicated: the row is a ladder, so an entry out
   * of order or repeated makes it read as a list of numbers instead. Selecting the committed
   * width is what makes the panel feel like it did something.
   */
  function widthKey() {
    return tool === "highlighter" ? "highlighterWidths" : "penWidths";
  }

  function putWidths(widths: number[], select: number) {
    const key = widthKey();
    if (tool === "highlighter") {
      changeSettings({
        ...settings,
        [key]: widths,
        highlighter: { ...settings.highlighter, widthPt: select },
      } as AppSettings);
    } else {
      changeSettings({
        ...settings,
        [key]: widths,
        penPresets: settings.penPresets.map((preset, index) =>
          index === penPreset - 1 ? { ...preset, widthPt: select } : preset,
        ),
      } as AppSettings);
    }
  }

  /**
   * A backstop rather than a path anyone walks: the add tile is not rendered once the row is
   * full, so a refusal here is only reachable from a settings file already carrying the maximum.
   * The width is still taken for the current stroke — a full row is no reason to refuse a nib.
   */
  function addWidth(widthPt: number) {
    const widths = settings[widthKey()];
    const next = addRowWidth(widths, widthPt, MAX_WIDTHS);
    if (next === widths) status = `The palette holds at most ${MAX_WIDTHS} widths`;
    putWidths(next, widthPt);
  }

  function editWidth(index: number, widthPt: number) {
    putWidths(editRowWidth(settings[widthKey()], index, widthPt, MAX_WIDTHS), widthPt);
  }

  function removeWidth(index: number) {
    const widths = settings[widthKey()];
    const remaining = removeRowWidth(widths, index);
    if (remaining === widths) {
      status = "The palette keeps at least one width";
      return;
    }
    putWidths(remaining, nearestChip(remaining, activeWidth));
  }

  function removeSwatch(index: number) {
    const key = tool === "highlighter" ? "highlighterSwatches" : "penSwatches";
    if (settings[key].length <= 1) {
      status = "The palette keeps at least one color";
      return;
    }
    changeSettings({
      ...settings,
      [key]: settings[key].filter((_, position) => position !== index),
    } as AppSettings);
  }

  function setEraserSize(size: "small" | "medium" | "large") {
    changeSettings({ ...settings, eraserSize: size });
  }

  function routeObjectPointer(event: PointerEvent) {
    // A stylus is for writing, so ink takes it and you can write straight over a block. That was
    // unconditional, which meant a block could never be picked up with the pen that wrote it —
    // there was no tool that gave the stylus the object layer. A selection tool now does.
    //
    // Mouse and touch keep reaching objects under any tool: clicking a block to grab it is how
    // this has always worked, and a brush is no reason to take it away.
    if (
      event.target instanceof Element &&
      event.target.closest(".selection-actions")
    ) {
      return;
    }
    if (event.pointerType === "pen" && !keepsSelection(tool)) {
      directObjectInput = false;
      return;
    }
    const object = document
      .elementsFromPoint(event.clientX, event.clientY)
      .map((element) => element.closest<HTMLElement>(".typst-block, .image-object"))
      .find((element) => element && workspace?.contains(element));
    directObjectInput = Boolean(object);
    if (event.type !== "pointerdown") return;
    selectedTypstId = object?.classList.contains("typst-block")
      ? object.dataset.objectId ?? null
      : null;
    selectedImageId = object?.classList.contains("image-object")
      ? object.dataset.objectId ?? null
      : null;
    if (object && event.target instanceof Element && event.target.closest(".ink-surface")) {
      event.preventDefault();
      event.stopPropagation();
      status = selectedTypstId ? "Typst block selected" : "Image selected";
    }
  }

  function closeObjectSelection(event: PointerEvent) {
    // Both palette popovers live inside the bar, so a press anywhere outside it dismisses them.
    if (!(event.target instanceof Element) || !event.target.closest(".instrument-palette")) {
      closePaletteContext();
    }
    // A stylus press mid-stroke is writing, not "deselect" — unless a selection tool is active,
    // where a press on empty page means exactly that.
    if (
      (event.pointerType === "pen" && !keepsSelection(tool)) ||
      (event.target instanceof Element &&
        event.target.closest(
          ".typst-block, .image-object, .typst-size-control, .selection-actions, .overflow-menu, [data-preserve-selection]",
        ))
    ) {
      return;
    }
    selectedTypstId = null;
    selectedImageId = null;
  }

  /**
   * Panning with the middle button, from anywhere on the page.
   *
   * Two fingers already pan and pinch, and a trackpad scrolls, but a mouse had only the
   * scrollbars — and the one gesture every drawing tool shares is that the middle button shoves
   * the canvas around without disturbing the tool in hand. It reads the scroll container directly
   * rather than going through `zoomAt`, because a pan does not change scale and so needs none of
   * the anchoring that a zoom does.
   */
  let panFrom = $state<{ pointerId: number; x: number; y: number; left: number; top: number } | null>(
    null,
  );

  function beginPan(event: PointerEvent): boolean {
    if (event.button !== 1 || !pageViewport) return false;
    stopTouchInertia();
    automaticPageFocusLocked = false;
    panFrom = {
      pointerId: event.pointerId,
      x: event.clientX,
      y: event.clientY,
      left: pageViewport.scrollLeft,
      top: pageViewport.scrollTop,
    };
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    // Middle-press is "autoscroll" on Windows; without this the browser drops its own scroll
    // puck on the page and takes the gesture over.
    event.preventDefault();
    event.stopPropagation();
    return true;
  }

  function continuePan(event: PointerEvent): boolean {
    if (!panFrom || event.pointerId !== panFrom.pointerId || !pageViewport) return false;
    pageViewport.scrollLeft = panFrom.left - (event.clientX - panFrom.x);
    pageViewport.scrollTop = panFrom.top - (event.clientY - panFrom.y);
    event.preventDefault();
    event.stopPropagation();
    return true;
  }

  function endPan(event: PointerEvent): boolean {
    if (!panFrom || event.pointerId !== panFrom.pointerId) return false;
    panFrom = null;
    event.preventDefault();
    event.stopPropagation();
    return true;
  }

  function workspacePointerDown(event: PointerEvent) {
    stopTouchInertia();
    if (beginPan(event)) return;
    if (event.target instanceof Element && event.target.closest(".selection-actions")) return;
    routeObjectPointer(event);
    if (event.pointerType !== "touch") return;
    touchPoints.set(event.pointerId, { x: event.clientX, y: event.clientY });
    if (touchPoints.size === 1 && !directObjectInput && pageViewport && workspace) {
      automaticPageFocusLocked = false;
      touchPanStart = {
        pointerId: event.pointerId,
        pointer: { x: event.clientX, y: event.clientY },
        scroll: { x: pageViewport.scrollLeft, y: pageViewport.scrollTop },
        lastPointer: { x: event.clientX, y: event.clientY },
        lastTime: performance.now(),
        velocity: { x: 0, y: 0 },
      };
      workspace.setPointerCapture(event.pointerId);
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    if (touchPoints.size !== 2 || !workspace) return;
    automaticPageFocusLocked = true;
    touchPanStart = null;
    const points = [...touchPoints.values()];
    const center = {
      x: (points[0].x + points[1].x) / 2,
      y: (points[0].y + points[1].y) / 2,
    };
    const frame = pageFrame?.getBoundingClientRect();
    if (!frame) return;
    pinchStart = {
      distance: distance(points[0], points[1]),
      zoom,
      center,
      pagePoint: {
        x: (center.x - frame.left) / zoom,
        y: (center.y - frame.top) / zoom,
      },
    };
    for (const pointerId of touchPoints.keys()) workspace.setPointerCapture(pointerId);
    event.preventDefault();
    event.stopPropagation();
  }

  function workspacePointerMove(event: PointerEvent) {
    if (continuePan(event)) return;
    routeObjectPointer(event);
    if (!touchPoints.has(event.pointerId)) return;
    touchPoints.set(event.pointerId, { x: event.clientX, y: event.clientY });
    if (touchPanStart && event.pointerId === touchPanStart.pointerId && pageViewport) {
      const now = performance.now();
      const next = pannedScroll(touchPanStart.scroll, touchPanStart.pointer, {
        x: event.clientX,
        y: event.clientY,
      });
      const elapsed = Math.max(now - touchPanStart.lastTime, 1);
      const rawVelocity = {
        x: -(event.clientX - touchPanStart.lastPointer.x) / elapsed,
        y: -(event.clientY - touchPanStart.lastPointer.y) / elapsed,
      };
      touchPanStart.velocity = {
        x: touchPanStart.velocity.x * 0.35 + rawVelocity.x * 0.65,
        y: touchPanStart.velocity.y * 0.35 + rawVelocity.y * 0.65,
      };
      touchPanStart.lastPointer = { x: event.clientX, y: event.clientY };
      touchPanStart.lastTime = now;
      pageViewport.scrollLeft = next.x;
      pageViewport.scrollTop = next.y;
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    if (!pinchStart) return;
    const points = [...touchPoints.values()];
    if (points.length !== 2) return;
    zoomAt(
      clampZoom(pinchStart.zoom * (distance(points[0], points[1]) / pinchStart.distance)),
      pinchStart.center.x,
      pinchStart.center.y,
      pinchStart.pagePoint,
    );
    event.preventDefault();
    event.stopPropagation();
  }

  function workspacePointerEnd(event: PointerEvent) {
    if (endPan(event)) return;
    if (event.pointerType !== "touch") return;
    touchPoints.delete(event.pointerId);
    if (touchPanStart?.pointerId === event.pointerId) {
      const finishedPan = touchPanStart;
      touchPanStart = null;
      if (event.type === "pointerup" && performance.now() - finishedPan.lastTime < 80) {
        startTouchInertia(finishedPan.velocity);
      }
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    if (pinchStart) {
      touchPoints.clear();
      pinchStart = null;
      touchPanStart = null;
      event.preventDefault();
      event.stopPropagation();
    }
  }

  function distance(a: Point, b: Point) {
    return Math.max(Math.hypot(a.x - b.x, a.y - b.y), 1);
  }

  const TOUCH_INERTIA_RETENTION = 0.86;

  function stopTouchInertia() {
    if (touchInertiaFrame) cancelAnimationFrame(touchInertiaFrame);
    touchInertiaFrame = undefined;
  }

  function startTouchInertia(initialVelocity: Point) {
    stopTouchInertia();
    if (settings.reducedMotion || !pageViewport) return;
    let velocity = {
      x: Math.max(-1.5, Math.min(1.5, initialVelocity.x)) * settings.touchGlide,
      y: Math.max(-1.5, Math.min(1.5, initialVelocity.y)) * settings.touchGlide,
    };
    if (Math.hypot(velocity.x, velocity.y) < 0.04) return;
    let previous = performance.now();
    const glide = (now: number) => {
      if (!pageViewport) return;
      const elapsed = Math.min(now - previous, 32);
      previous = now;
      const before = { x: pageViewport.scrollLeft, y: pageViewport.scrollTop };
      pageViewport.scrollLeft += velocity.x * elapsed;
      pageViewport.scrollTop += velocity.y * elapsed;
      if (pageViewport.scrollLeft === before.x) velocity.x = 0;
      if (pageViewport.scrollTop === before.y) velocity.y = 0;
      velocity = dampedVelocity(velocity, elapsed, TOUCH_INERTIA_RETENTION);
      if (Math.hypot(velocity.x, velocity.y) < 0.025) {
        touchInertiaFrame = undefined;
        return;
      }
      touchInertiaFrame = requestAnimationFrame(glide);
    };
    touchInertiaFrame = requestAnimationFrame(glide);
  }

  function beginPaletteDrag(event: PointerEvent) {
    if (event.button !== 0 || !workspace) return;
    const target = event.currentTarget as HTMLElement;
    const workspaceBounds = workspace.getBoundingClientRect();
    const paletteBounds = target.parentElement!.getBoundingClientRect();
    paletteX = paletteBounds.left - workspaceBounds.left;
    paletteY = paletteBounds.top - workspaceBounds.top;
    target.setPointerCapture(event.pointerId);
    paletteDrag = {
      pointerId: event.pointerId,
      clientX: event.clientX,
      clientY: event.clientY,
      startX: paletteX,
      startY: paletteY,
      width: paletteBounds.width,
      height: paletteBounds.height,
    };
    event.preventDefault();
  }

  /**
   * Which edge the palette would take if the drag ended here.
   *
   * Applied while dragging, not only on release. The dock decides whether the bar is a row or a
   * column, and finding that out after letting go means aiming at one shape and getting another
   * — you drag to the left edge picturing a column and drop a row, then drag again. Turning
   * under the hand makes the drag a preview of its own result.
   */
  function dockUnderPointer(event: PointerEvent): PaletteDock | null {
    if (!workspace) return null;
    const bounds = workspace.getBoundingClientRect();
    return nearestPaletteDock(
      event.clientX - bounds.left,
      event.clientY - bounds.top,
      bounds.width,
      bounds.height,
    );
  }

  function movePalette(event: PointerEvent) {
    if (!paletteDrag || event.pointerId !== paletteDrag.pointerId || !workspace) return;
    const bounds = workspace.getBoundingClientRect();
    paletteX = Math.min(
      Math.max(paletteDrag.startX + event.clientX - paletteDrag.clientX, 8),
      Math.max(bounds.width - paletteDrag.width - 8, 8),
    );
    paletteY = Math.min(
      Math.max(paletteDrag.startY + event.clientY - paletteDrag.clientY, 8),
      Math.max(bounds.height - paletteDrag.height - 8, 8),
    );
    const next = dockUnderPointer(event);
    // Not written to settings yet: a drag that wanders across three edges should leave one
    // preference behind, on release, not three.
    if (next && next !== paletteDock) paletteDock = next;
  }

  function finishPaletteDrag(event: PointerEvent) {
    if (!paletteDrag || event.pointerId !== paletteDrag.pointerId || !workspace) return;
    paletteDock = dockUnderPointer(event) ?? paletteDock;
    paletteDrag = null;
    if (paletteDock !== settings.paletteDock) {
      changeSettings({ ...settings, paletteDock });
    }
  }

  /// Delete the selected object or ink as one undoable committed action. Original asset
  /// files are never removed; deleting an image only drops the page's reference.
  function deleteSelection(): boolean {
    if (selectedTypstId) {
      const id = selectedTypstId;
      const object = activeSnapshot?.page.objects.find((candidate) => candidate.id === id);
      if (object?.groupId && object.groupId !== mixedGroup?.groupId) {
        status = "Ungroup this object before deleting it";
        return false;
      }
      if (id === mixedGroup?.typstId && mixedGroup.active) ungroupInk();
      typstBlocks = typstBlocks.filter((block) => block.id !== id);
      selectedTypstId = null;
      queueCommit("Deleted Typst block");
      status = "Deleted the Typst block";
      return true;
    }
    if (selectedImageId) {
      const object = activeSnapshot?.page.objects.find(
        (candidate) => candidate.id === selectedImageId,
      );
      if (object?.groupId) {
        status = "Ungroup this object before deleting it";
        return false;
      }
      const removed = images.find((image) => image.id === selectedImageId);
      if (removed?.url.startsWith("blob:")) URL.revokeObjectURL(removed.url);
      images = images.filter((image) => image.id !== selectedImageId);
      selectedImageId = null;
      queueCommit("Deleted image");
      status = "Removed the image from this page; the original file is kept";
      return true;
    }
    if (selectedStrokeIds.length > 0) {
      const removed = new Set(selectedStrokeIds);
      updateInkSelection([]);
      changeStrokes(
        strokes.filter((stroke) => !removed.has(stroke.id)),
        "Deleted ink selection",
      );
      return true;
    }
    return false;
  }

  /// Arrow-key movement for the current selection; batched through the ink debounce so
  /// holding a key produces one committed action.
  function nudgeSelection(dx: number, dy: number): boolean {
    if (selectedTypstId) {
      typstBlocks = typstBlocks.map((block) =>
        block.id === selectedTypstId
          ? {
              ...block,
              transform: {
                ...block.transform,
                x: block.transform.x + dx,
                y: block.transform.y + dy,
              },
            }
          : block,
      );
      scheduleInkCommit("Moved Typst block");
      return true;
    }
    if (selectedImageId) {
      images = images.map((image) =>
        image.id === selectedImageId
          ? { ...image, x: image.x + dx, y: image.y + dy }
          : image,
      );
      scheduleInkCommit("Moved image");
      return true;
    }
    if (selectedStrokeIds.length > 0) {
      strokes = moveSelected(strokes, selectedStrokeIds, { x: dx, y: dy });
      scheduleInkCommit("Moved ink selection");
      return true;
    }
    return false;
  }

  function ungroupInk() {
    strokes = strokes.map((stroke) =>
      groupedStrokeIds.includes(stroke.id) &&
      (!mixedGroup || stroke.groupId === mixedGroup.inkGroupId)
        ? { ...stroke, groupId: null }
        : stroke,
    );
    groupedStrokeIds = [];
    if (mixedGroup) mixedGroup = { ...mixedGroup, active: false };
    status = "Removed the Typst and ink group";
    queueCommit("Ungrouped ink and Typst");
  }

  async function pasteImage(event: ClipboardEvent) {
    const file = Array.from(event.clipboardData?.files ?? []).find((item) =>
      item.type.startsWith("image/"),
    );
    if (!file) return;
    event.preventDefault();
    if (file.size > 20 * 1024 * 1024) {
      status = "That image is larger than the 20 MiB prototype limit";
      return;
    }
    const url = URL.createObjectURL(file);
    const dimensions = await imageDimensions(url);
    const filename = `pasted-${Date.now()}.${extensionForMime(file.type)}`;

    // Store the original once, here. Commits then reference the asset by path instead of
    // carrying its bytes, so ordinary pen strokes never marshal an image across IPC.
    let path = `assets/${filename}`;
    if (tauriAvailable && root) {
      try {
        path = await storePastedImage(root, filename, new Uint8Array(await file.arrayBuffer()));
      } catch (error) {
        URL.revokeObjectURL(url);
        status = `The image could not be stored: ${message(error)}`;
        return;
      }
    }

    const fit = Math.min(1, 220 / dimensions.width, 160 / dimensions.height);
    const existingIds = new Set([
      ...typstBlocks.map((block) => block.id),
      ...images.map((image) => image.id),
      ...(activeSnapshot?.page.objects.map((object) => object.id) ?? []),
    ]);
    let number = images.length + 1;
    while (existingIds.has(`image-${String(number).padStart(3, "0")}`)) number += 1;
    const id = `image-${String(number).padStart(3, "0")}`;
    images = [...images, {
      id,
      path,
      url,
      alt: file.name || "Pasted image",
      x: 300,
      y: 380,
      widthPt: Math.max(1, dimensions.width * fit),
      heightPt: Math.max(1, dimensions.height * fit),
      scale: 1,
      zIndex: nextVisualZIndex(),
      readingOrder: typstBlocks.length + images.length,
    }];
    selectedImageId = id;
    selectedTypstId = null;
    status = "Pasted one original image";
    queueCommit("Pasted image");
  }

  function scheduleInkCommit(label: string) {
    inkCommitLabel = label;
    inkPending = true;
    inkCommitTimer.arm();
  }

  function flushInkCommit() {
    inkCommitTimer.flush();
  }

  function queueCommit(label: string) {
    // Any commit builds a snapshot from current state, so it already carries pending ink.
    inkCommitTimer.cancel();
    inkPending = false;
    if (!tauriAvailable || !root || transactionFailed) return;
    const snapshot = buildSnapshot();
    pendingTransactions += 1;
    transactionQueue = transactionQueue
      .then(async () => {
        snapshot.page.revision = revision;
        try {
          const result = await commitNotebook(root, snapshot);
          activeSnapshot = result.snapshot;
          notebookManifest = result.snapshot.manifest;
          const entry = pageEntries.find((page) => page.id === result.snapshot.page.id);
          if (entry) entry.snapshot = result.snapshot;
          revision = result.snapshot.page.revision;
          canUndo = result.canUndo;
          canRedo = result.canRedo;
          recordNotebookAction(snapshot.page.id);
          status = `${label}; saved revision ${revision}`;
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
  function reportCommitFailure(label: string, error: unknown) {
    transactionFailed = true;
    const detail = message(error);
    if (detail.includes("external change") || detail.includes("revision conflict")) {
      conflictDetail = detail;
      status = `${label} was not saved: the notebook changed outside this window.`;
    } else if (detail.includes("recovery contains")) {
      status = `${label} was not saved: unresolved recovered work must be handled first.`;
      void refreshRecoveryCandidates();
    } else {
      status = `${label} could not be saved: ${detail}. Reopen to restore the last confirmed state.`;
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
    if (tauriAvailable && root) void recordNotebookPage(root, activePageId).catch(() => {});
  }

  function changeStrokes(next: Stroke[], label: string) {
    const nextIds = new Set(next.map((stroke) => stroke.id));
    const protectedGrouped = strokes.filter(
      (stroke) =>
        !nextIds.has(stroke.id) &&
        stroke.groupId &&
        stroke.groupId !== mixedGroup?.inkGroupId,
    );
    if (protectedGrouped.length > 0) next = [...next, ...protectedGrouped];
    const remaining = new Set(next.map((stroke) => stroke.id));
    groupedStrokeIds = groupedStrokeIds.filter((id) => remaining.has(id));
    strokes = next;
    scheduleInkCommit(label);
  }

  function addStroke(stroke: Stroke) {
    changeStrokes([...strokes, stroke], `Added ${stroke.tool} stroke`);
    // Annotating the page must not cost the writer their place in the source: the canvas takes
    // focus to receive the pen, so the side view takes it back once the stroke has landed.
    if (sideEditorOpen) sideEditor?.focus();
  }

  function changeImage(id: string, next: Partial<Pick<ImageState, "x" | "y" | "scale">>) {
    if (!images.some((image) => image.id === id)) return;
    images = images.map((image) => (image.id === id ? { ...image, ...next } : image));
    queueCommit("Updated image");
  }

  function selectedObjectId(): string | null {
    return selectedTypstId ?? selectedImageId;
  }

  function canMoveVisual(direction: -1 | 1): boolean {
    const id = selectedObjectId();
    if (!id || !activeSnapshot) return false;
    return moveVisualItems(
      activeSnapshot.page,
      strokes,
      [id],
      [],
      direction < 0 ? "backward" : "forward",
    ) !== null;
  }

  function canMoveReading(direction: -1 | 1): boolean {
    const id = selectedObjectId();
    if (!id || !activeSnapshot) return false;
    const index = activeSnapshot.page.readingOrder.indexOf(id);
    return direction < 0
      ? index > 0
      : index >= 0 && index < activeSnapshot.page.readingOrder.length - 1;
  }

  function inkActionIds(): string[] {
    return selectedStrokeIds.length > 0 ? selectedStrokeIds : groupedStrokeIds;
  }

  function canMoveInkVisual(direction: -1 | 1): boolean {
    return Boolean(
      activeSnapshot &&
        moveVisualItems(
          activeSnapshot.page,
          strokes,
          [],
          inkActionIds(),
          direction < 0 ? "backward" : "forward",
        ),
    );
  }

  function changeInkVisualOrder(direction: -1 | 1) {
    if (!activeSnapshot) return;
    const next = moveVisualItems(
      activeSnapshot.page,
      strokes,
      [],
      inkActionIds(),
      direction < 0 ? "backward" : "forward",
    );
    if (!next) return;
    strokes = next.strokes;
    applyObjectOrder(next.page, "Changed visual order");
    status = direction < 0 ? "Moved selected ink back" : "Moved selected ink forward";
  }

  function scheduleSelectionToolbar() {
    if (selectionToolbarFrame) cancelAnimationFrame(selectionToolbarFrame);
    selectionToolbarFrame = requestAnimationFrame(() => {
      selectionToolbarFrame = undefined;
      positionSelectionToolbar();
    });
  }

  function positionSelectionToolbar() {
    if (!workspace || !selectionToolbarElement) return;
    let anchor: ViewRect | null = null;
    const objectId = selectedObjectId();
    if (objectId) {
      anchor = workspace
        .querySelector<HTMLElement>(`.active-page [data-object-id="${CSS.escape(objectId)}"]`)
        ?.getBoundingClientRect() ?? null;
    } else {
      const bounds = selectionBounds(strokes, inkActionIds());
      const frame = pageFrame?.getBoundingClientRect();
      if (bounds && frame) {
        anchor = {
          left: frame.left + bounds.left * zoom,
          top: frame.top + bounds.top * zoom,
          right: frame.left + bounds.right * zoom,
          bottom: frame.top + bounds.bottom * zoom,
          width: (bounds.right - bounds.left) * zoom,
          height: (bounds.bottom - bounds.top) * zoom,
        };
      }
    }
    if (!anchor) return;
    const boundary = workspace.getBoundingClientRect();
    if (
      anchor.right < boundary.left ||
      anchor.left > boundary.right ||
      anchor.bottom < boundary.top ||
      anchor.top > boundary.bottom
    ) {
      selectionToolbarPosition = { left: 0, top: 0, ready: false };
      return;
    }
    const placed = placeFloatingToolbar(
      anchor,
      selectionToolbarElement.getBoundingClientRect(),
      boundary,
    );
    selectionToolbarPosition = { left: placed.left, top: placed.top, ready: true };
  }

  function applyObjectOrder(page: NonNullable<typeof activeSnapshot>["page"], label: string) {
    if (!activeSnapshot) return;
    activeSnapshot = { ...activeSnapshot, page };
    const fields = new Map(page.objects.map((object) => [object.id, object]));
    typstBlocks = typstBlocks.map((block) => {
      const object = fields.get(block.id);
      return object
        ? { ...block, zIndex: object.zIndex, readingOrder: object.readingOrder }
        : block;
    });
    images = images.map((image) => {
      const object = fields.get(image.id);
      return object
        ? { ...image, zIndex: object.zIndex, readingOrder: object.readingOrder }
        : image;
    });
    queueCommit(label);
  }

  function changeVisualOrder(move: VisualMove) {
    const id = selectedObjectId();
    if (!id || !activeSnapshot) return;
    const next = moveVisualItems(activeSnapshot.page, strokes, [id], [], move);
    if (!next) return;
    strokes = next.strokes;
    applyObjectOrder(next.page, "Changed visual order");
  }

  function changeReadingOrder(direction: -1 | 1) {
    const id = selectedObjectId();
    if (!id || !activeSnapshot) return;
    const next = moveReadingObject(activeSnapshot.page, id, direction);
    if (next === activeSnapshot.page) {
      status = "That object cannot move further in reading order";
      return;
    }
    applyObjectOrder(next, "Changed reading order");
  }

  function undo() {
    void routeHistory("undo_notebook", "Undid change");
  }

  function redo() {
    void routeHistory("redo_notebook", "Redid change");
  }

  /// Page scope targets the page in view. Notebook scope replays the session's commit order:
  /// undo jumps to the page that changed most recently, wherever it is.
  async function routeHistory(
    command: "undo_notebook" | "redo_notebook",
    label: string,
  ) {
    if (!(await persist())) return;
    const order = command === "undo_notebook" ? notebookUndoOrder : notebookRedoOrder;
    const target = order.at(-1);
    if (target?.kind === "structure") {
      queueStructureHistory(
        command === "undo_notebook" ? "undo_page_structure" : "redo_page_structure",
        label,
      );
      return;
    }
    if (settings.undoScope === "notebook") {
      if (target?.kind === "page" && target.pageId !== activePageId) {
        await activatePage(target.pageId);
        if (activePageId !== target.pageId) return;
        scrollToPage(target.pageId);
      }
    }
    queueHistory(command, label);
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

  function queueHistory(command: "undo_notebook" | "redo_notebook", label: string) {
    if (!tauriAvailable || transactionFailed) return;
    flushInkCommit();
    flushTypstCommit();
    pendingTransactions += 1;
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
          status = `${label}; saved revision ${revision}`;
        } catch (error) {
          reportCommitFailure(label, error);
        }
      })
      .finally(() => {
        pendingTransactions -= 1;
      });
  }

  function queueStructureHistory(
    command: "undo_page_structure" | "redo_page_structure",
    label: string,
  ) {
    if (!tauriAvailable || transactionFailed) return;
    pendingTransactions += 1;
    transactionQueue = transactionQueue
      .then(async () => {
        try {
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
          pageEntries = [];
          applySnapshot(result.snapshot);
          void recordNotebookPage(root, result.snapshot.page.id).catch(() => {});
          canUndo = result.canUndo || notebookUndoOrder.length > 0;
          canRedo = result.canRedo || notebookRedoOrder.length > 0;
          await tick();
          scrollToPage(result.snapshot.page.id);
          status = `${label} page-list change`;
        } catch (error) {
          reportCommitFailure(label, error);
        }
      })
      .finally(() => {
        pendingTransactions -= 1;
      });
  }

  function scrollToPage(pageId: string, behavior?: ScrollBehavior) {
    document
      .querySelector<HTMLElement>(`[data-page-id="${pageId}"]`)
      ?.scrollIntoView({
        behavior: behavior ?? (settings.reducedMotion ? "auto" : "smooth"),
        block: "center",
        inline: "center",
      });
  }

  function historyShortcut(event: KeyboardEvent) {
    if (event.defaultPrevented) return;
    if (event.ctrlKey || event.metaKey) {
      if (event.key === "Tab") {
        const next = cycledTab(openTabs, root, event.shiftKey ? -1 : 1);
        if (next) {
          event.preventDefault();
          void openNotebookAt(next);
        }
        return;
      }
      if (event.key.toLowerCase() === "w" && openTabs.length > 0) {
        event.preventDefault();
        void closeNotebookTab(root);
        return;
      }
      const zoomShortcuts: Record<string, () => void> = {
        "+": () => changeZoom(zoom + 0.1),
        "=": () => changeZoom(zoom + 0.1),
        "-": () => changeZoom(zoom - 0.1),
        "0": () => changeZoom(1),
      };
      if (zoomShortcuts[event.key]) {
        event.preventDefault();
        zoomShortcuts[event.key]();
        return;
      }
      if (event.key === ",") {
        event.preventDefault();
        settingsOpen = !settingsOpen;
        return;
      }
    }
    // Deliberately above the text-editing guard: promoting an in-canvas edit to the full-height
    // source view has to work while the caret is inside the editor, which is exactly when the
    // ten-line cap starts to bite.
    if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.key.toLowerCase() === "e") {
      event.preventDefault();
      toggleSideEditor();
      return;
    }
    if (event.key === "Escape" && paletteContextOpen) {
      event.preventDefault();
      closePaletteContext();
      return;
    }
    const editingText =
      event.target instanceof Element &&
      event.target.closest(".cm-editor, input, textarea, [contenteditable=true]");
    if (editingText) return;
    if (event.key === "Escape") {
      searchOpen = false;
      settingsOpen = false;
      moreOpen = false;
      addPageOpen = false;
      metricsOpen = false;
      selectedTypstId = null;
      selectedImageId = null;
      directObjectInput = false;
      return;
    }
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") {
      event.preventDefault();
      searchOpen = true;
      return;
    }
    if (!event.ctrlKey && !event.metaKey && !event.altKey) {
      if (event.key === "Delete" || event.key === "Backspace") {
        if (deleteSelection()) event.preventDefault();
        return;
      }
      const arrows: Record<string, [number, number]> = {
        ArrowLeft: [-1, 0],
        ArrowRight: [1, 0],
        ArrowUp: [0, -1],
        ArrowDown: [0, 1],
      };
      if (arrows[event.key]) {
        const step = event.shiftKey ? 10 : 1;
        if (nudgeSelection(arrows[event.key][0] * step, arrows[event.key][1] * step)) {
          event.preventDefault();
          return;
        }
      }
      const shortcuts: Record<string, () => void> = {
        "1": () => activateTool("pen", 1),
        "2": () => activateTool("pen", 2),
        "3": () => activateTool("highlighter"),
        "4": () => activateTool("eraser"),
        "5": () => activateTool("lasso"),
        "6": addTypstBlock,
      };
      if (shortcuts[event.key]) {
        event.preventDefault();
        shortcuts[event.key]();
      }
      return;
    }
    if (!(event.ctrlKey || event.metaKey)) return;
    if (event.key.toLowerCase() === "z" && event.shiftKey && canRedo) {
      event.preventDefault();
      redo();
    } else if (event.key.toLowerCase() === "z" && canUndo) {
      event.preventDefault();
      undo();
    } else if (event.key.toLowerCase() === "y" && canRedo) {
      event.preventDefault();
      redo();
    }
  }

  async function persist(): Promise<boolean> {
    if (!tauriAvailable) {
      status = "Browser preview cannot write files; launch the Tauri desktop app to save";
      return false;
    }
    const startedAt = performance.now();
    flushInkCommit();
    flushTypstCommit();
    await transactionQueue;
    await Promise.all([...neighborCommitters.values()].map((committer) => committer.flush()));
    if (collectMetrics) saveMs = performance.now() - startedAt;
    if (transactionFailed) return false;
    status = `All changes saved at revision ${revision}`;
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
        settings.pageTextBaselineGrid,
      );
      if (collectMetrics) exportMs = performance.now() - startedAt;
      status = `Exported PDF to ${path}`;
    } catch (error) {
      status = `PDF export failed: ${message(error)}`;
    } finally {
      busy = false;
    }
  }

  async function closePage() {
    await closeNotebookTab(root);
  }

  async function showLibrary() {
    if (tauriAvailable && activeSnapshot && !(await persist())) return;
    if (tauriAvailable && root) await recordNotebookPage(root, activePageId).catch(() => {});
    moreOpen = false;
    addPageOpen = false;
    notebookChosen = false;
    status = "Choose a notebook, or return to an open tab";
  }

  function returnToNotebook() {
    if (!activeSnapshot || !root) return;
    notebookChosen = true;
    pageOpen = true;
    status = "Notebook ready";
    void tick().then(() => scrollToPage(activePageId, "auto"));
  }

  async function closeNotebookTab(tabRoot: string) {
    const closed = closedTab(openTabs, tabRoot);
    if (closed.tabs === openTabs) return;
    if (tabRoot !== root) {
      openTabs = closed.tabs;
      status = (await rememberNotebookSession())
        ? "Closed notebook tab"
        : "Closed notebook tab, but the restored tab list could not be updated";
      return;
    }
    if (tauriAvailable && !(await persist())) return;
    if (tauriAvailable) await recordNotebookPage(root, activePageId).catch(() => {});
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

    if (tauriAvailable && root) {
      try {
        await closeNotebookSession(root);
      } catch (error) {
        openTabs = previousTabs;
        status = `The notebook was saved but could not be closed: ${message(error)}`;
        return;
      }
    }
    notebookGeneration += 1;
    clearNotebookCaches();
    revokeImageUrl();
    root = "";
    notebookManifest = null;
    activeSnapshot = null;
    pageEntries = [];
    pageOpen = false;
    notebookChosen = false;
    status = "Notebook closed; the confirmed local files are safe";
  }

  async function reopen() {
    if (!tauriAvailable) {
      pageOpen = true;
      status = "Browser preview reopened its in-memory page";
      return;
    }
    busy = true;
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
      if (collectMetrics) reopenMs = performance.now() - startedAt;
      pageOpen = true;
      status = `Reopened saved revision ${revision}`;
    } catch (error) {
      status = `Reopen failed: ${message(error)}`;
    } finally {
      busy = false;
    }
  }

  function revokeImageUrl() {
    for (const image of images) {
      if (image.url.startsWith("blob:")) URL.revokeObjectURL(image.url);
    }
  }

  /**
   * One press of + or −, as a ratio rather than an amount.
   *
   * Zoom is perceived multiplicatively: the step from 25% to 35% is the same visual jump as 200%
   * to 280%, while a fixed ±0.1 is imperceptible at the top of the range and a third of the page
   * at the bottom. With a ceiling of 8 a linear tenth would also want seventy presses to cross
   * the range.
   */
  const ZOOM_STEP = 1.4;

  function changeZoom(next: number) {
    const bounds = pageViewport?.getBoundingClientRect();
    if (bounds) {
      zoomAt(next, bounds.left + bounds.width / 2, bounds.top + bounds.height / 2);
      return;
    }
    zoom = clampZoom(next);
  }

  function zoomAt(next: number, clientX: number, clientY: number, fixedPagePoint?: Point) {
    if (!pageViewport || !pageFrame) return;
    automaticPageFocusLocked = true;
    const viewport = pageViewport;
    const frame = pageFrame;
    const startedAt = performance.now();
    const before = frame.getBoundingClientRect();
    const pagePoint = fixedPagePoint ?? {
      x: (clientX - before.left) / zoom,
      y: (clientY - before.top) / zoom,
    };
    zoom = clampZoom(next);
    requestAnimationFrame(() => {
      const after = frame.getBoundingClientRect();
      viewport.scrollLeft += after.left + pagePoint.x * zoom - clientX;
      viewport.scrollTop += after.top + pagePoint.y * zoom - clientY;
      if (collectMetrics) zoomFrameMs = performance.now() - startedAt;
    });
  }

  function wheelZoom(event: WheelEvent) {
    stopTouchInertia();
    if (!event.ctrlKey) {
      automaticPageFocusLocked = false;
      return;
    }
    event.preventDefault();
    zoomAt(zoom * Math.exp(-event.deltaY * 0.01), event.clientX, event.clientY);
  }

  function currentToolLabel() {
    if (tool === "pen") return `Pen ${penPreset}`;
    if (tool === "select") return "Ink selection";
    return tool[0].toUpperCase() + tool.slice(1);
  }

  function currentToolDetail() {
    if (tool === "pen") return penPreset === 1 ? "0.35 mm · graphite" : "0.70 mm · blueprint";
    if (tool === "highlighter") return "4.0 mm · amber";
    if (tool === "eraser") return "whole strokes";
    if (tool === "lasso" || tool === "select") return `${selectedStrokeIds.length} selected`;
    return "";
  }

  function inkColor() {
    if (tool === "highlighter") return settings.highlighter.color;
    return settings.penPresets[penPreset - 1]?.color ?? "#1e232b";
  }

  function inkWidthPt() {
    if (tool === "highlighter") return settings.highlighter.widthPt;
    return settings.penPresets[penPreset - 1]?.widthPt ?? 1.6;
  }

  /** The palette slot in use, whichever tool is active. */
  function activePreset(): PenPreset {
    if (tool === "highlighter") return settings.highlighter;
    return settings.penPresets[penPreset - 1] ?? DEFAULT_SETTINGS.penPresets[0];
  }

  /**
   * A stroke records the nib it was drawn with, so these are resolved once here rather than
   * re-derived at render or export time — that re-derivation is how pressure and translucency
   * used to end up different in the PDF than they were on the page.
   */
  function inkPressure() {
    return settings.pressureEnabled && activePreset().pressure;
  }

  function inkTaper() {
    return penType(activePreset().type).taper;
  }

  function inkOpacity() {
    return tool === "highlighter" ? settings.highlighterOpacity : 1;
  }

  /**
   * Highlighter only, and deliberately so: an underline is meant to be straight, whereas snapping
   * a pen stroke would quietly rewrite handwriting.
   */
  function inkStraighten() {
    return tool === "highlighter" && settings.highlighterStraighten;
  }

  function recordStrokeMetrics(metrics: StrokePerformance) {
    if (!collectMetrics) return;
    strokeMetrics = [...strokeMetrics.slice(-19), metrics];
  }

  function timingSummary(
    key: keyof Omit<StrokePerformance, "sampleCount">,
  ): string {
    const summary = summarizeMetric(strokeMetrics, key);
    return summary
      ? `${summary.median.toFixed(1)} / ${summary.p95.toFixed(1)} / ${summary.worst.toFixed(1)} ms`
      : "not measured";
  }

  function milliseconds(value: number | null): string {
    return value === null ? "not measured" : `${value.toFixed(1)} ms`;
  }

  function metricsPayload() {
    return {
      schemaVersion: 1,
      updatedAt: new Date().toISOString(),
      strokes: {
        count: strokeMetrics.length,
        activeFeedbackMs: summarizeMetric(strokeMetrics, "activeFeedbackMs"),
        maximumSampleGapMs: summarizeMetric(strokeMetrics, "maxSampleGapMs"),
        commitMs: summarizeMetric(strokeMetrics, "commitMs"),
      },
      latestMs: {
        typstSubprocess: compileMs,
        typstIdleDebounce: TYPST_IDLE_DEBOUNCE_MS,
        zoomFrame: zoomFrameMs,
        save: saveMs,
        reopen: reopenMs,
        export: exportMs,
      },
    };
  }

  function imageDimensions(url: string): Promise<{ width: number; height: number }> {
    return new Promise((resolve, reject) => {
      const candidate = new Image();
      candidate.onload = () =>
        resolve({ width: candidate.naturalWidth, height: candidate.naturalHeight });
      candidate.onerror = () => reject(new Error("The pasted image could not be decoded"));
      candidate.src = url;
    });
  }

  function extensionForMime(mime: string) {
    if (mime === "image/jpeg") return "jpg";
    if (mime === "image/svg+xml") return "svg";
    if (mime === "image/webp") return "webp";
    return "png";
  }

  function message(error: unknown) {
    return error instanceof Error ? error.message : String(error);
  }

  function previewSvg(source: string, widthPt: number) {
    const escaped = source
      .replaceAll("&", "&amp;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;");
    return `<svg xmlns="http://www.w3.org/2000/svg" width="${widthPt}pt" height="64pt"><rect width="100%" height="100%" fill="white"/><text x="8" y="32" font-family="sans-serif" font-size="14">${escaped}</text></svg>`;
  }
</script>

<main class="workspace-app" onpaste={pasteImage} onpointerdowncapture={closeObjectSelection} onwheel={wheelZoom}>
  {#if !notebookChosen}
    <div class="start-slot">
      <LibrarySurface
        {tauriAvailable}
        showNotebookSetup={showInitialSetup}
        location={shelfLocation}
        onOpen={(nextRoot) => void openNotebookAt(nextRoot)}
        onCreate={(nextRoot, setup) => {
          showInitialSetup = false;
          void openNotebookAt(nextRoot, { createIfMissing: true, setup });
        }}
        returnLabel={openTabs.find((tab) => tab.root === root)?.title}
        onReturn={openTabs.length > 0 ? returnToNotebook : undefined}
        onLocationChange={(next) => (shelfLocation = next)}
        onStatus={(next) => (status = next)}
      />
    </div>
  {:else}
  <header class="command-strip">
    <div class="notebook-identity">
      <button
        class="home-button"
        type="button"
        aria-label="Open the notebook library"
        title="Notebook library"
        disabled={busy}
        onclick={() => void showLibrary()}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="M3.5 11.2 12 4.5l8.5 6.7" />
          <path d="M5.8 10v8.2a1 1 0 0 0 1 1h3.4v-4.9h3.6v4.9h3.4a1 1 0 0 0 1-1V10" />
        </svg>
      </button>
      <NotebookTabs
        tabs={openTabs}
        activeRoot={root}
        {switchingRoot}
        {busy}
        saving={savePending}
        warning={transactionFailed}
        onSelect={openNotebookAt}
        onClose={closeNotebookTab}
      />
      <button
        class="new-tab-button"
        type="button"
        aria-label="Open another notebook"
        title="Open another notebook"
        disabled={busy}
        onclick={() => void showLibrary()}
      >+</button>
    </div>
    <div class="command-actions">
      {#if pageOpen}
        <button class="export-button" type="button" onclick={exportPdf} disabled={busy}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3v11m0 0-4-4m4 4 4-4M5 17v2a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-2" /></svg>
          Export PDF
        </button>
      {:else}
        <button class="export-button" type="button" onclick={reopen} disabled={busy}>Reopen notebook</button>
      {/if}
      {#if pageOpen}
        <div class="add-page-anchor">
          <button class="icon-button" class:active={addPageOpen} type="button" aria-label="Add page" aria-expanded={addPageOpen} title="Add page" disabled={busy} onclick={() => (addPageOpen = !addPageOpen)}>
            <svg class="stroke-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M13 3.5H7a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h6"></path><path d="M18 9.5v9"></path><path d="M13.5 14h9"></path></svg>
          </button>
          {#if addPageOpen}
            <AddPageMenu
              where={addPageWhere}
              groups={addPageGroups()}
              tones={PAPER_TONES}
              toneId={addPageToneId}
              sizes={PAGE_SIZES}
              sizeId={addPageSizeId}
              orientation={addPageOrientation}
              previewAspect={addPageGeometry.widthPt / addPageGeometry.heightPt}
              currentPageId={activePageId}
              {pageNumber}
              {pageCount}
              canPlaceRelative={pageCount > 0 && Boolean(activePageId)}
              onWhereChange={(next) => (addPageWhere = next)}
              onToneChange={(next) => (addPageToneId = next)}
              onSizeChange={(next) => (addPageSizeId = next)}
              onOrientationChange={(next) => (addPageOrientation = next)}
              onClose={() => (addPageOpen = false)}
            />
          {/if}
        </div>
      {/if}
      <button class="icon-button" class:active={searchOpen} type="button" aria-label="Search typed content" title="Search (Ctrl+F)" onclick={() => (searchOpen = !searchOpen)}>
        <svg class="stroke-icon" viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="7"></circle><path d="m20 20-3.6-3.6"></path></svg>
      </button>
      <button class="icon-button" class:active={sideEditorOpen} type="button" aria-label="Page text and Typst source view" aria-pressed={sideEditorOpen} title="Editor view (Ctrl+Shift+E)" onclick={toggleSideEditor}>
        <svg class="stroke-icon" viewBox="0 0 24 24" aria-hidden="true"><rect x="3.5" y="4.5" width="17" height="15" rx="2"></rect><path d="M10 4.5v15"></path></svg>
      </button>
      <button class="icon-button" class:active={moreOpen} type="button" aria-label="More notebook actions" aria-expanded={moreOpen} data-preserve-selection onclick={() => (moreOpen = !moreOpen)}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="5" cy="12" r="1.7"></circle><circle cx="12" cy="12" r="1.7"></circle><circle cx="19" cy="12" r="1.7"></circle></svg>
      </button>
    </div>
  </header>

  {#if moreOpen}
    <OverflowMenu
      title={pageOpen ? `Page ${pageNumber} of ${pageCount}` : "Notebook"}
      subtitle={root || "Opening..."}
      sections={menuSections()}
      onClose={() => (moreOpen = false)}
    />
  {/if}

  {#if pageOpen}
    <div class="workspace-split" class:panel-right={settings.sideEditorDock === "right"}>
    {#if sideEditorOpen}
      <SideEditor
        bind:this={sideEditor}
        mode={sideEditorMode}
        source={sideEditorStyle ? sharedStyleSource : (sideEditorBlock?.source ?? "")}
        blockLabel={sideEditorPageText
          ? "Page text"
          : sideEditorBlockId
            ? `Typst block ${sideEditorBlockId}`
            : ""}
        awayPageNumber={sideEditorPageNumber}
        hasAnyBlock={Boolean(pageTypst) || typstBlocks.length > 0}
        {root}
        dock={settings.sideEditorDock}
        width={settings.sideEditorWidth}
        diagnostics={sideEditorBlock?.result?.diagnostics ?? []}
        pageText={sideEditorPageText}
        {presets}
        {presetBusy}
        onChange={(next) =>
          sideEditorStyle
            ? updateSharedStyle(next)
            : sideEditorBlock && updateTypstSource(sideEditorBlock.id, next)}
        onClose={closeSideEditor}
        onDockChange={(dock) => changeSettings({ ...settings, sideEditorDock: dock })}
        onWidthChange={(next) => changeSettings({ ...settings, sideEditorWidth: next })}
        onGoToBlock={() => {
          if (sideEditorMode === "away" && sideEditorPageId) scrollToPage(sideEditorPageId);
          else if (typstBlocks[0]) openSideEditor(typstBlocks[0].id);
        }}
        onCreatePageText={openPageText}
        onCreateBlock={() => {
          addTypstBlock();
          void tick().then(() => {
            const added = typstBlocks.at(-1);
            if (added) openSideEditor(added.id);
          });
        }}
        onPresetAction={(action) => void changePagePreset(action)}
      />
    {/if}
    <section
      class="workspace-surround"
      bind:this={workspace}
      aria-label="Fixed page workspace"
      onpointerdowncapture={workspacePointerDown}
      onpointermovecapture={workspacePointerMove}
      onpointerupcapture={workspacePointerEnd}
      onpointercancelcapture={workspacePointerEnd}
    >
      <div class="page-scroll-content" bind:this={pageViewport} onscroll={scheduleSelectionToolbar}>
        <div class="page-pan-field">
        {#each pageEntries as entry, index (entry.id)}
          {@const active = entry.id === activePageId}
          <!-- The active page's own state wins; a neighbour that has loaded knows its real size,
               and one that has not falls back to the manifest's hint so the scroller still
               reserves the right amount of room for it. -->
          {@const box = active ? activeGeometry : (entry.snapshot?.page.geometry ?? entry.geometry)}
          <article
            class:active-page={active}
            class="page-stack-item"
            data-page-id={entry.id}
            aria-label={`Page ${index + 1}`}
            aria-current={active ? "page" : undefined}
            use:observePage={entry.id}
          >
            <div class="page-number">
              <span>Page {index + 1}</span>
              {#if active && pageCount > 1}
                <button
                  type="button"
                  aria-label={`Move page ${index + 1} up`}
                  title="Move page up"
                  disabled={index === 0 || busy}
                  onclick={() => void moveActivePage(-1)}
                >&uarr;</button>
                <button
                  type="button"
                  aria-label={`Move page ${index + 1} down`}
                  title="Move page down"
                  disabled={index === pageCount - 1 || busy}
                  onclick={() => void moveActivePage(1)}
                >&darr;</button>
              {/if}
            </div>
            <div class="page-frame" use:trackActiveFrame={active} style:width={`${box.widthPt * zoom}px`} style:height={`${box.heightPt * zoom}px`}>
              <div class="page" style:width={`${box.widthPt}px`} style:height={`${box.heightPt}px`} style:transform={`scale(${zoom})`}>
                {#if active || entry.snapshot}
                  <PageSurface
                    blocks={active ? activeBlockViews : blockViewsFromSnapshot(entry.snapshot!)}
                    pageTypst={active
                      ? activePageTypstView
                      : pageTypstViewFromSnapshot(entry.snapshot!)}
                    images={active ? activeImageViews : imageViewsFromSnapshot(entry.snapshot!, assetUrls(entry.id))}
                    results={active ? activeResults : (neighborResults[entry.id] ?? {})}
                    strokes={active ? strokes : neighborStrokesFor(entry)}
                    newStrokeZIndex={active
                      ? nextVisualZIndex()
                      : nextSnapshotVisualZIndex(entry.snapshot!, neighborStrokesFor(entry))}
                    selectedStrokeIds={active ? selectedStrokeIds : []}
                    background={active ? activeBackground : (entry.snapshot?.page.background ?? { kind: "plain", color: "#ffffff" })}
                    pageWidthPt={box.widthPt}
                    pageHeightPt={box.heightPt}
                    {zoom}
                    interactive={active}
                    {root}
                    sharedStyle={sharedStyleSource}
                    pageTextBaselineGrid={settings.pageTextBaselineGrid}
                    {presetRevision}
                    inlineEditing={!sideEditorOpen}
                    onRequestEdit={(id) => openSideEditor(id)}
                    {tool}
                    color={inkColor()}
                    widthPt={inkWidthPt()}
                    pressure={inkPressure()}
                    taper={inkTaper()}
                    opacity={inkOpacity()}
                    straighten={inkStraighten()}
                    eraseRadiusPt={ERASER_RADIUS_PT[settings.eraserSize]}
                    calibration={settings.calibration}
                    directObjectInput={active && directObjectInput}
                    selectedBlockId={active ? selectedTypstId : null}
                    selectedImageId={active ? selectedImageId : null}
                    onCompile={(id, request) =>
                      active
                        ? compileTypst(id, request)
                        : compileNeighborTypst(entry.id, id, request)}
                    onSourceChange={(id, source) => updateTypstSource(id, source)}
                    onTransform={(id, transform) => updateTypstTransform(id, transform)}
                    onSelectBlock={(id) => {
                      selectedTypstId = id;
                      selectedImageId = null;
                    }}
                    onDeselectBlock={() => (selectedTypstId = null)}
                    onSelectImage={(id) => {
                      selectedImageId = id;
                      selectedTypstId = null;
                    }}
                    onMoveImage={(id, position) => changeImage(id, position)}
                    onScaleImage={(id, scale) => changeImage(id, { scale })}
                    onStrokeFinalized={(stroke) =>
                      active
                        ? addStroke(stroke)
                        : commitNeighborInk(entry.id, [...neighborStrokesFor(entry), stroke], "Added ink")}
                    onStrokesChange={(next) =>
                      active
                        ? changeStrokes(next, "Updated ink")
                        : commitNeighborInk(entry.id, next, "Updated ink")}
                    onSelectionChange={(next) => {
                      if (active) updateInkSelection(next);
                    }}
                    onStrokeMetrics={collectMetrics
                      ? (metrics) => {
                          if (active) recordStrokeMetrics(metrics);
                        }
                      : undefined}
                  />
                {:else}
                  <span class="page-loading">Loading page…</span>
                {/if}
              </div>
            </div>
          </article>
        {/each}
        </div>
      </div>

      <!-- One grid owns the canvas chrome. Corner controls reserve their intrinsic row/column,
           so the palette can never be positioned through them by an unrelated fixed offset. -->
      <div class="workspace-chrome">
      <div class="history-pill" aria-label="Page history">
        <button type="button" aria-label="Undo" title="Undo (Ctrl+Z)" onclick={undo} disabled={busy || pendingTransactions > 0 || !canUndo}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m9 7-5 5 5 5M4 12h10a6 6 0 0 1 0 12" /></svg>
        </button>
        <button type="button" aria-label="Redo" title="Redo (Ctrl+Y)" onclick={redo} disabled={busy || pendingTransactions > 0 || !canRedo}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m15 7 5 5-5 5M20 12H10a6 6 0 0 0 0 12" /></svg>
        </button>
      </div>

      <nav
        class:dragging={paletteDrag !== null}
        class:expanded={paletteContextOpen && (tool === "pen" || tool === "highlighter" || tool === "eraser")}
        class:horizontal={paletteDock === "top" || paletteDock === "bottom"}
        class:inward-right={paletteDock === "right"}
        class:inward-bottom={paletteDock === "bottom"}
        class:dock-top={paletteDock === "top" && paletteDrag === null}
        class:dock-right={paletteDock === "right" && paletteDrag === null}
        class:dock-bottom={paletteDock === "bottom" && paletteDrag === null}
        class:dock-left={paletteDock === "left" && paletteDrag === null}
        class="instrument-palette"
        style:left={paletteDrag ? `${paletteX}px` : null}
        style:top={paletteDrag ? `${paletteY}px` : null}
        aria-label="Canvas tools"
      >
        {#if toolPanel && toolPanelPreset}
          <div class="palette-panel-anchor" style:--anchor={`${toolPanel.anchor}px`}>
            <ToolPanel
              preset={toolPanelPreset}
              kind={toolPanel.kind}
              label={toolPanel.kind === "highlighter" ? "Highlighter" : `Pen ${toolPanel.slot}`}
              smoothing={settings.calibration.smoothing}
              opacity={settings.highlighterOpacity}
              straighten={settings.highlighterStraighten}
              behindInk={settings.highlighterBehindInk}
              onChange={updateToolPreset}
              onSmoothingChange={(smoothing) =>
                changeSettings({ ...settings, calibration: { ...settings.calibration, smoothing } })}
              onOpacityChange={(highlighterOpacity) =>
                changeSettings({ ...settings, highlighterOpacity })}
              onStraightenChange={(highlighterStraighten) =>
                changeSettings({ ...settings, highlighterStraighten })}
              onBehindInkChange={(highlighterBehindInk) =>
                changeSettings({ ...settings, highlighterBehindInk })}
              onClose={() => (toolPanel = null)}
            />
          </div>
        {/if}
        <button class="palette-grip" type="button" aria-label="Move tool bar" title="Drag to move the bar" onpointerdown={beginPaletteDrag} onpointermove={movePalette} onpointerup={finishPaletteDrag} onpointercancel={finishPaletteDrag}>
          <i></i><i></i><i></i><i></i><i></i><i></i>
        </button>
        <div class="palette-primary">
          <PaletteTools
            {settings}
            activeCommands={activePaletteCommands}
            expandedCommand={expandedPaletteCommand}
            horizontal={paletteDock === "top" || paletteDock === "bottom"}
            onActivate={(command) => paletteCommands[command]()}
          />
        </div>

        {#if paletteContextOpen && (tool === "pen" || tool === "highlighter")}
          <div class="palette-context" aria-label={`${tool === "highlighter" ? "Highlighter" : `Pen ${penPreset}`} quick settings`}>
          <div class="inline-group" role="group" aria-label="Stroke size">
            {#each activeWidthChips as chip, index (chip)}
              {@const isActive = nearestChip(activeWidthChips, activeWidth) === chip}
              <button
                type="button"
                class="size-tile settings"
                class:active={isActive}
                aria-pressed={isActive}
                title={isActive
                  ? `${(chip / 2.835).toFixed(2)} mm — tap again to set exactly`
                  : `${(chip / 2.835).toFixed(2)} mm`}
                onclick={(event) => {
                  // The same select-then-edit gesture the colour swatches use.
                  if (isActive)
                    widthPanel =
                      widthPanel?.index === index
                        ? null
                        : { index, anchor: swatchAnchor(event.currentTarget) };
                  else {
                    widthPanel = null;
                    setActiveWidth(chip);
                  }
                }}
              >
                <span
                  class="size-line"
                  style:height={`${Math.max(2, Math.min(chip * (tool === "highlighter" ? 2.2 : 1.4), 9))}px`}
                  style:background={tool === "highlighter" ? `${activeInkColor}99` : "#aeb5be"}
                ></span>
              </button>
            {/each}
            <!-- The empty slot only exists while there is a slot: at four widths the row is the
                 row, and the fourth tile is standing where this was. Leaving a `+` that could
                 only fail would be a control that lies about what it does. -->
            {#if activeWidthChips.length < MAX_WIDTHS}
            <button
              type="button"
              class="size-tile custom"
              aria-label="Set a stroke width"
              aria-expanded={widthPanel?.index === -1}
              title="Set a stroke width"
              onclick={(event) =>
                (widthPanel =
                  widthPanel?.index === -1
                    ? null
                    : { index: -1, anchor: swatchAnchor(event.currentTarget) })}
            >
              <!-- Drawn rather than typed: a text `+` brings its own weight and metrics, which
                   is what made it read as a stray character. Small, because the tile's own dotted
                   outline is the shape here and the mark is only a hint. -->
              <svg width="9" height="9" viewBox="0 0 9 9" fill="none" aria-hidden="true">
                <path d="M0 4.5h9M4.5 0v9" stroke="currentColor" stroke-width="1" />
              </svg>
            </button>
            {/if}
            {#if widthPanel}
              <div class="palette-panel-anchor" style:--anchor={`${widthPanel.anchor}px`}>
                <WidthPanel
                  widthPt={widthPanel.index === -1
                    ? activeWidth
                    : activeWidthChips[widthPanel.index]}
                  kind={tool === "highlighter" ? "highlighter" : "pen"}
                  minimumMm={WIDTH_BOUNDS_MM[tool === "highlighter" ? "highlighter" : "pen"].minimum}
                  maximumMm={WIDTH_BOUNDS_MM[tool === "highlighter" ? "highlighter" : "pen"].maximum}
                  canRemove={widthPanel.index !== -1 && canRemoveWidth(activeWidthChips)}
                  onCommit={(next) => {
                    if (widthPanel?.index === -1) addWidth(next);
                    else if (widthPanel) editWidth(widthPanel.index, next);
                    widthPanel = null;
                  }}
                  onRemove={() => {
                    if (widthPanel && widthPanel.index !== -1) removeWidth(widthPanel.index);
                    widthPanel = null;
                  }}
                  onClose={() => (widthPanel = null)}
                />
              </div>
            {/if}
          </div>
          <span class="palette-divider"></span>
          <div class="inline-group colors" role="group" aria-label="Ink color">
            {#each activeColorChips as color, index (color)}
              {@const isActive = activeInkColor.toLowerCase() === color.toLowerCase()}
              <button
                type="button"
                class="color-dot"
                class:active={isActive}
                style:background={color}
                style:opacity={tool === "highlighter" ? settings.highlighterOpacity : 1}
                aria-label={isActive ? `Edit color ${colorName(color)}` : `Use color ${colorName(color)}`}
                aria-pressed={isActive}
                title={isActive ? `${colorName(color)} — tap again to edit` : colorName(color)}
                onclick={(event) => {
                  // Second tap on the colour you are already using opens its editor, anchored
                  // to that chip so the panel appears where you were looking.
                  if (isActive)
                    colorPanel =
                      colorPanel?.index === index
                        ? null
                        : { index, anchor: swatchAnchor(event.currentTarget) };
                  else {
                    colorPanel = null;
                    setActiveColor(color);
                  }
                }}
              ></button>
            {/each}
            <button
              type="button"
              class="color-dot custom"
              aria-label="Add a color"
              aria-expanded={colorPanel?.index === -1}
              title="Add a color"
              onclick={(event) =>
                (colorPanel =
                  colorPanel?.index === -1
                    ? null
                    : { index: -1, anchor: swatchAnchor(event.currentTarget) })}
            >+</button>
            {#if colorPanel}
              <div class="palette-panel-anchor" style:--anchor={`${colorPanel.anchor}px`}>
                <ColorPanel
                  value={colorPanel.index === -1 ? activeInkColor : activeColorChips[colorPanel.index]}
                  recent={settings.recentColors}
                  mode={colorPanel.index === -1 ? "add" : "edit"}
                  canRemove={colorPanel.index !== -1 && activeColorChips.length > 1}
                  onPick={(color) => {
                    if (colorPanel?.index === -1) addSwatch(color);
                    else if (colorPanel) editSwatch(colorPanel.index, color);
                    colorPanel = null;
                  }}
                  onChange={(color) => {
                    // Same commit, panel left open: the picker is adjusted by eye, so the ink
                    // has to follow before the writer decides whether to go again.
                    if (colorPanel?.index === -1) {
                      addSwatch(color);
                      // The new swatch is now the last one, and further adjustment must retarget
                      // it rather than appending a second swatch per drag.
                      colorPanel = { ...colorPanel, index: activeColorChips.length - 1 };
                    } else if (colorPanel) {
                      editSwatch(colorPanel.index, color);
                    }
                  }}
                  onRemove={() => {
                    if (colorPanel && colorPanel.index !== -1) removeSwatch(colorPanel.index);
                    colorPanel = null;
                  }}
                  onClose={() => (colorPanel = null)}
                />
              </div>
            {/if}
          </div>
          <span class="palette-divider"></span>
          <button
            type="button"
            class="size-tile advanced-tool-settings"
            aria-label={`${tool === "highlighter" ? "Highlighter" : `Pen ${penPreset}`} advanced settings`}
            aria-expanded={toolPanel !== null}
            title="Nib and smoothing settings"
            onclick={(event) => {
              colorPanel = null;
              widthPanel = null;
              const kind = tool === "highlighter" ? "highlighter" : "pen";
              const slot = tool === "highlighter" ? 1 : penPreset;
              toolPanel = toolPanel ? null : { kind, slot, anchor: swatchAnchor(event.currentTarget) };
            }}
          >
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path d="M4 7h10M18 7h2M4 17h2M10 17h10" />
              <circle cx="16" cy="7" r="2" /><circle cx="8" cy="17" r="2" />
            </svg>
          </button>
          </div>
        {:else if paletteContextOpen && tool === "eraser"}
          <div class="palette-context" role="radiogroup" aria-label="Eraser hit-area size">
            {#each ERASER_SIZE_OPTIONS as option (option.id)}
              <button
                type="button"
                class="size-tile eraser-size"
                class:active={settings.eraserSize === option.id}
                role="radio"
                aria-checked={settings.eraserSize === option.id}
                aria-label={`${option.label} eraser, ${ERASER_RADIUS_PT[option.id]} point hit radius`}
                title={`${option.label} — ${ERASER_RADIUS_PT[option.id]} pt`}
                onclick={() => setEraserSize(option.id)}
              >
                <span
                  class="eraser-ring"
                  style:width={`${option.diameter}px`}
                  style:height={`${option.diameter}px`}
                ></span>
              </button>
            {/each}
          </div>
        {/if}
      </nav>

      {#if selectedStrokeIds.length > 0 || groupedStrokeIds.length > 0}
        <SelectionActions
          bind:element={selectionToolbarElement}
          subject="ink"
          left={selectionToolbarPosition.left}
          top={selectionToolbarPosition.top}
          ready={selectionToolbarPosition.ready}
          canMoveBack={canMoveInkVisual(-1)}
          canMoveForward={canMoveInkVisual(1)}
          onMove={changeInkVisualOrder}
          grouped={groupedStrokeIds.length > 0}
          onGroup={groupedStrokeIds.length > 0
            ? ungroupInk
            : selectedTypstId
              ? groupSelectedInk
              : undefined}
          onDelete={selectedStrokeIds.length > 0 ? () => void deleteSelection() : undefined}
        />
      {:else if selectedImageId || selectedTypstId}
        <SelectionActions
          bind:element={selectionToolbarElement}
          subject={selectedImageId ? "image" : "Typst block"}
          left={selectionToolbarPosition.left}
          top={selectionToolbarPosition.top}
          ready={selectionToolbarPosition.ready}
          canMoveBack={canMoveVisual(-1)}
          canMoveForward={canMoveVisual(1)}
          onMove={(direction) => changeVisualOrder(direction < 0 ? "backward" : "forward")}
          onDelete={() => void deleteSelection()}
        />
      {/if}

      <div class="zoom-pill">
        <button type="button" aria-label="Zoom out" onclick={() => changeZoom(zoom / ZOOM_STEP)}>−</button>
        <output aria-label="Page zoom">{Math.round(zoom * 100)}%</output>
        <button type="button" aria-label="Zoom in" onclick={() => changeZoom(zoom * ZOOM_STEP)}>+</button>
      </div>
      </div>
    </section>
    </div>
  {:else}
    <section class="closed-state">
      <span class="closed-mark" aria-hidden="true"></span><h1>Notebook closed</h1>
      <p>The confirmed local files are still safe.</p>
      <button type="button" onclick={reopen} disabled={busy}>Reopen notebook</button>
    </section>
  {/if}

  <footer class="status-strip">
    <div class="tool-status"><span class="blue-dot"></span><strong>{currentToolLabel()}</strong><span>{currentToolDetail()}</span></div>
    {#if selectedTypstId}
      {@const selectedBlock = typstBlocks.find((block) => block.id === selectedTypstId)}
      {#if selectedBlock}
        <label class="typst-size-control">
          <span>Content size</span>
          <input
            type="range"
            min="0.5"
            max="2"
            step="0.05"
            value={selectedBlock.transform.scale}
            aria-label="Selected Typst content size"
            oninput={previewTypstScale}
            onchange={commitTypstScale}
          />
          <output>{Math.round(selectedBlock.transform.scale * 100)}%</output>
        </label>
      {/if}
    {/if}
    <div class:failure={transactionFailed} class="operation-status" title={status}>{status}</div>
    <span class="page-count">Page {activePageNumber()} of {notebookManifest?.pages.length ?? 1}</span><span class="footer-divider"></span>
    <button type="button" onclick={() => changeZoom(1)}>{Math.round(zoom * 100)}%</button>
    <span class="footer-divider"></span><span class:failure={transactionFailed} class="local-state">{transactionFailed ? "Needs attention" : savePending ? "Local · saving" : "Local · saved"}</span>
  </footer>

  {#if searchOpen}
    <SearchOverlay
      {root}
      {tauriAvailable}
      onNavigate={(hit) => void navigateToSearchHit(hit)}
      onClose={() => (searchOpen = false)}
    />
  {/if}

  {#if settingsOpen}
    <SettingsPanel
      {settings}
      onChange={changeSettings}
      onClose={() => (settingsOpen = false)}
    />
  {/if}

  {#if conflictDetail}
    <ConflictDialog
      detail={conflictDetail}
      onReload={() => {
        conflictDetail = null;
        void reopen();
      }}
      onCancel={() => (conflictDetail = null)}
    />
  {/if}

  {#if recoveryOpen && recoveryCandidates.length > 0}
    <RecoveryDialog
      candidates={recoveryCandidates}
      busy={recoveryBusy}
      onRestore={(fileName) => void restoreRecovery(fileName)}
      onDiscard={(fileName) => void discardRecovery(fileName)}
      onClose={() => (recoveryOpen = false)}
    />
  {/if}
  {/if}

  {#if collectMetrics && metricsOpen}
    <div class="panel-scrim" role="presentation">
      <aside class="diagnostics-panel" aria-label="Timing evidence">
        <div class="panel-heading">
          <div><span>Local diagnostics</span><h2>Timing evidence</h2></div>
          <button class="icon-button" type="button" aria-label="Close timing evidence" onclick={() => (metricsOpen = false)}>×</button>
        </div>
        <p class="diagnostic-path">{root}\.goodtype\phase0-metrics.json</p>
        <dl>
          <dt>Recorded strokes</dt><dd>{strokeMetrics.length}</dd>
          <dt>Active feedback median / p95 / worst</dt><dd>{timingSummary("activeFeedbackMs")}</dd>
          <dt>Maximum sample gap median / p95 / worst</dt><dd>{timingSummary("maxSampleGapMs")}</dd>
          <dt>Stroke commit median / p95 / worst</dt><dd>{timingSummary("commitMs")}</dd>
          <dt>Latest Typst compile</dt><dd>{milliseconds(compileMs)} + {TYPST_IDLE_DEBOUNCE_MS} ms debounce</dd>
          <dt>Latest zoom frame</dt><dd>{milliseconds(zoomFrameMs)}</dd>
          <dt>Save / reopen / export</dt><dd>{milliseconds(saveMs)} / {milliseconds(reopenMs)} / {milliseconds(exportMs)}</dd>
        </dl>
      </aside>
    </div>
  {/if}
  <p class="screen-reader-status" aria-live="polite">{status}</p>
</main>

<style>
  :global(*) { box-sizing: border-box; }
  :global(button), :global(input), :global(textarea) { font: inherit; }

  .workspace-app {
    --charcoal: #16181d;
    --surround: #1b1e24;
    --panel: #23272f;
    --paper: #fcfcfa;
    --text: #e9ebee;
    --muted: #aeb5be;
    --quiet: #6a727c;
    --blueprint: #4c8df0;
    --blueprint-light: #7fb0f7;
    --amber: #e0912b;
    --oxide: #e5645e;
    /* One UI font across the whole chrome (the header face). Monospace lives only in the
       Typst code editor. */
    --font-ui: Bahnschrift, "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
    position: relative;
    display: grid;
    grid-template-rows: 58px minmax(0, 1fr) 34px;
    /* Pin the single column to the window so no row can widen the app. */
    grid-template-columns: minmax(0, 1fr);
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: var(--surround);
    font-family: var(--font-ui);
  }

  .start-slot {
    grid-row: 1 / -1;
    min-height: 0;
    color: var(--text);
  }

  button { border: 0; }
  button:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }
  button:disabled { cursor: not-allowed; opacity: .32; }

  .command-strip {
    z-index: 30;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 0 14px 0 18px;
    border-bottom: 1px solid rgb(255 255 255 / 8%);
    background: var(--charcoal);
  }

  .notebook-identity, .command-actions, .tool-status {
    display: flex;
    align-items: center;
  }

  .notebook-identity { flex: 1; min-width: 0; gap: 8px; }

  /* Sized like the strip's other icon controls so the row reads as one set, and it replaces the
     decorative square that used to sit here — the corner was already the eye's first stop. */
  .home-button {
    display: grid;
    width: 40px;
    height: 40px;
    flex: none;
    padding: 0;
    border: 1px solid rgb(255 255 255 / 18%);
    border-radius: 7px;
    background: transparent;
    color: var(--text);
    cursor: pointer;
    place-items: center;
  }

  .home-button:hover:not(:disabled) { background: rgb(255 255 255 / 8%); }
  .home-button svg {
    width: 20px;
    height: 20px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .new-tab-button {
    width: 40px;
    height: 40px;
    flex: none;
    border: 1px solid rgb(255 255 255 / 18%);
    border-radius: 7px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    font-size: 21px;
  }
  .new-tab-button:hover:not(:disabled) { background: rgb(255 255 255 / 8%); color: var(--text); }

  .blue-dot { width: 6px; height: 6px; flex: none; border-radius: 50%; background: var(--blueprint); }
  .page-count, .zoom-pill output {
    color: var(--quiet);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
    letter-spacing: .02em;
  }

  .command-actions { flex: none; gap: 8px; }
  .export-button, .icon-button {
    height: 40px;
    border: 1px solid rgb(255 255 255 / 18%);
    border-radius: 7px;
    background: transparent;
    color: var(--text);
    cursor: pointer;
  }

  .export-button {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 14px;
    font-size: 13px;
    font-weight: 600;
  }

  .export-button svg { width: 16px; fill: none; stroke: currentColor; stroke-width: 1.7; stroke-linecap: round; stroke-linejoin: round; }
  .icon-button { display: grid; width: 40px; place-items: center; }
  .icon-button svg { width: 20px; fill: currentColor; }
  .icon-button svg.stroke-icon { fill: none; stroke: currentColor; stroke-width: 1.9; stroke-linecap: round; stroke-linejoin: round; }
  .export-button:hover, .icon-button:hover, .icon-button.active { background: rgb(255 255 255 / 8%); }
  .icon-button:disabled { opacity: 0.45; cursor: default; }

  /* The add-page popout hangs off its own button rather than the strip's right edge, so the
     button can move without the menu drifting away from it. */
  .add-page-anchor { position: relative; display: flex; }


  /* The source view is a sibling of the canvas, not an overlay on it, so opening the panel
     genuinely narrows the canvas. Everything positioned inside the canvas — the palette above
     all — then follows the paper instead of colliding with the panel. */
  .workspace-split {
    display: flex;
    /* Grid items default to `min-width: auto`, which refuses to shrink below the content's
       intrinsic width. Without this the widening panel stretches the whole app grid past the
       window, dragging the footer and the canvas chrome off screen with it. */
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }

  .workspace-split.panel-right {
    flex-direction: row-reverse;
  }

  .workspace-surround {
    position: relative;
    min-width: 0;
    min-height: 0;
    flex: 1;
    overflow: hidden;
    background: radial-gradient(circle at 50% 46%, rgb(255 255 255 / 2%), transparent 42%), var(--surround);
  }

  .page-scroll-content {
    width: 100%;
    height: 100%;
    overflow: auto;
    scrollbar-width: none;
    touch-action: none;
  }
  .page-scroll-content::-webkit-scrollbar { display: none; }
  .page-pan-field {
    --page-pan-gutter: min(40vw, 480px);
    display: flex;
    width: calc(100% + var(--page-pan-gutter) + var(--page-pan-gutter));
    min-height: 100%;
    padding: 46px var(--page-pan-gutter) 58px;
    /* Its content box is one viewport wide. Equal outer padding therefore creates real positive
       scroll range on both sides even when a zoomed-out page would otherwise fit completely. */
    align-items: safe center;
    flex-direction: column;
    gap: 46px;
  }
  .page-stack-item { position: relative; flex: none; }
  .page-stack-item.active-page .page-number { color: var(--blueprint-light); }
  .page-number {
    position: absolute;
    top: 0;
    right: calc(100% + 12px);
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--quiet);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .page-number button {
    width: 26px;
    height: 26px;
    border: 1px solid rgb(255 255 255 / 10%);
    border-radius: 6px;
    background: var(--panel);
    color: var(--muted);
    cursor: pointer;
  }
  .page-number button:hover:not(:disabled),
  .page-number button:focus-visible {
    background: var(--panel-high);
    color: var(--text);
    outline: 2px solid var(--blueprint-light);
    outline-offset: 1px;
  }
  .page-number button:disabled { opacity: 0.35; cursor: default; }
  .page-frame { position: relative; flex: none; }
  .page {
    position: relative;
    overflow: hidden;
    background: var(--paper);
    box-shadow: 0 2px 6px rgb(0 0 0 / 30%), 0 24px 60px rgb(0 0 0 / 45%);
    transform-origin: top left;
  }
  .page-loading { display: grid; height: 100%; color: #8d949d; font-size: 12px; place-items: center; }

  /* The object and ink layer rules now live with the renderer, in PageSurface.svelte. */

  .workspace-chrome {
    position: absolute;
    z-index: 15;
    inset: 0;
    display: grid;
    min-width: 0;
    min-height: 0;
    grid-template-columns: max-content minmax(0, 1fr) max-content;
    grid-template-rows: max-content minmax(0, 1fr) max-content;
    gap: 10px;
    padding: 16px 18px;
    pointer-events: none;
  }
  .workspace-chrome > * { pointer-events: auto; }

  .history-pill, .zoom-pill {
    display: flex;
    align-items: center;
    border: 1px solid rgb(255 255 255 / 10%);
    background: var(--panel);
    box-shadow: 0 12px 30px rgb(0 0 0 / 45%);
  }

  .history-pill, .zoom-pill { position: relative; z-index: 15; }
  .history-pill { grid-area: 1 / 1; justify-self: start; align-self: start; gap: 2px; padding: 5px; border-radius: 10px; }
  .history-pill button, .zoom-pill button { display: grid; border-radius: 7px; background: transparent; color: var(--text); cursor: pointer; place-items: center; }
  .history-pill button { width: 40px; height: 40px; }
  .history-pill button:hover:not(:disabled), .zoom-pill button:hover { background: rgb(255 255 255 / 8%); }
  .history-pill svg { width: 19px; fill: none; stroke: currentColor; stroke-width: 1.7; stroke-linecap: round; stroke-linejoin: round; }

  .instrument-palette {
    position: relative;
    z-index: 20;
    display: grid;
    grid-template-columns: 46px;
    grid-template-rows: auto 1fr;
    gap: 3px 0;
    padding: 6px;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 13px;
    background: var(--panel);
    box-shadow: 0 20px 50px rgb(0 0 0 / 52%);
    touch-action: none;
  }

  /* Each dock spans every grid track except the occupied corner on its own edge. */
  .instrument-palette.dock-left { grid-column: 1; grid-row: 2 / 4; justify-self: start; align-self: center; }
  .instrument-palette.dock-right { grid-column: 3; grid-row: 1 / 3; justify-self: end; align-self: center; }
  .instrument-palette.dock-top { grid-column: 2 / 4; grid-row: 1; justify-self: center; align-self: start; }
  .instrument-palette.dock-bottom { grid-column: 1 / 3; grid-row: 3; justify-self: center; align-self: end; }
  .instrument-palette.expanded:not(.horizontal) { grid-template-columns: repeat(2, 46px); }
  .instrument-palette.horizontal {
    grid-template-columns: auto 1fr;
    grid-template-rows: 46px;
  }
  .instrument-palette.horizontal.expanded { grid-template-rows: 46px 32px; }
  .instrument-palette.horizontal.expanded.inward-bottom { grid-template-rows: 32px 46px; }

  .palette-primary,
  .palette-context {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 3px;
  }

  .palette-grip {
    grid-column: 1 / -1;
    grid-row: 1;
    justify-self: center;
  }

  .palette-primary {
    grid-column: 1;
    grid-row: 2;
    width: 46px;
  }

  .palette-context {
    grid-column: 2;
    grid-row: 2;
    width: 46px;
    border-left: 1px solid rgb(255 255 255 / 12%);
  }

  .instrument-palette.expanded.inward-right .palette-primary { grid-column: 2; }
  .instrument-palette.expanded.inward-right .palette-context {
    grid-column: 1;
    border-right: 1px solid rgb(255 255 255 / 12%);
    border-left: 0;
  }

  .horizontal .palette-primary,
  .horizontal .palette-context {
    flex-direction: row;
    width: auto;
    min-width: 0;
  }

  .horizontal .palette-grip {
    grid-column: 1;
    grid-row: 1 / -1;
    align-self: center;
  }

  .horizontal .palette-primary {
    grid-column: 2;
    grid-row: 1;
  }

  .horizontal .palette-context {
    grid-column: 2;
    grid-row: 2;
    border-top: 1px solid rgb(255 255 255 / 12%);
    border-left: 0;
  }

  .horizontal.expanded.inward-bottom .palette-primary { grid-row: 2; }
  .horizontal.expanded.inward-bottom .palette-context {
    grid-row: 1;
    border-top: 0;
    border-bottom: 1px solid rgb(255 255 255 / 12%);
  }

  .instrument-palette.dragging { position: absolute; box-shadow: 0 26px 60px rgb(0 0 0 / 68%); }

  /* Grip: two columns of three dots, drag handle at the leading edge. */
  .palette-grip {
    display: grid;
    flex: none;
    grid-template-columns: repeat(2, 3.5px);
    grid-template-rows: repeat(3, 3.5px);
    gap: 3px;
    place-content: center;
    padding: 4px 6px;
    background: transparent;
    cursor: grab;
  }
  .dragging .palette-grip { cursor: grabbing; }
  .palette-grip i { width: 3.5px; height: 3.5px; border-radius: 50%; background: rgb(255 255 255 / 28%); }

  .palette-divider { width: 26px; height: 1px; margin: 1px 0; background: rgb(255 255 255 / 12%); }
  .horizontal .palette-divider { width: 1px; height: 26px; margin: 0 3px; }

  /* Inline stroke sizes and colors carried on the palette bar (contextual to the active tool). */
  .inline-group { display: flex; flex-direction: column; align-items: center; gap: 3px; }
  .horizontal .inline-group { flex-direction: row; }
  .inline-group.colors { display: flex; flex-direction: column; gap: 5px; }
  .horizontal .inline-group.colors { flex-direction: row; }

  .size-tile {
    display: grid;
    /* Buttons carry a UA border and padding; without resetting them the tile box is not the
       34px it claims to be, so the rings inside sit off-centre from one another. */
    box-sizing: border-box;
    width: 34px;
    height: 34px;
    flex: none;
    padding: 0;
    border: 0;
    place-items: center;
    border-radius: 9px;
    background: transparent;
    color: var(--text);
    cursor: pointer;
  }

  .size-tile:hover { background: rgb(255 255 255 / 6%); }
  .size-tile.active { outline: 1.5px solid var(--blueprint); background: rgb(76 141 240 / 16%); }
  .size-line { width: 20px; border-radius: 3px; }
  /* No outline: the row's own rhythm already says where the slot is, and a box drawn around an
     empty tile was louder than the widths it sits beside. */
  .size-tile.custom { color: var(--quiet); }
  .size-tile.custom:hover { color: var(--text); }
  .size-tile.active .size-line { background: var(--text) !important; }
  .eraser-ring {
    box-sizing: border-box;
    border: 1.5px solid currentColor;
    border-radius: 50%;
  }
  .eraser-size.active .eraser-ring { color: var(--text); }
  .advanced-tool-settings svg {
    width: 19px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .advanced-tool-settings[aria-expanded="true"] { background: rgb(255 255 255 / 8%); }

  .horizontal .palette-context .size-tile { width: 28px; height: 28px; border-radius: 7px; }
  .horizontal .palette-context .color-dot { width: 20px; height: 20px; }
  .horizontal .palette-context .palette-divider { height: 20px; }

  .color-dot {
    position: relative;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    border: 1.5px solid rgb(255 255 255 / 22%);
    cursor: pointer;
    padding: 0;
  }

  .color-dot.active { outline: 1.5px solid var(--blueprint); outline-offset: 2px; }

  /* Active editable chips retain a pen-visible hint that a second press opens their editor. */
  .size-tile.settings.active::after,
  .color-dot.active::after {
    position: absolute;
    right: 3px;
    bottom: 3px;
    width: 3px;
    height: 3px;
    border-radius: 50%;
    background: currentColor;
    content: "";
    opacity: 0.75;
    pointer-events: none;
  }

  .color-dot.active::after { right: 2px; bottom: 2px; background: rgb(255 255 255 / 85%); }
  /* The tiles have to be a containing block for the dot to sit in their corner. */
  .size-tile { position: relative; }
  .color-dot.custom {
    display: grid;
    place-items: center;
    border-style: dashed;
    border-color: rgb(255 255 255 / 30%);
    background: transparent;
    color: var(--quiet);
    font-size: 15px;
    line-height: 1;
  }

  /* The colour editor opens beside the chip that was tapped: `--anchor` is that chip's centre
     within the bar, and the panel is centred on it but kept inside the workspace. */
  /* Where a popout from the bar hangs: the colour editor and the width panel both use it, so
     each knows only its own contents and this knows only the four docks. */
  .palette-panel-anchor {
    position: absolute;
    bottom: calc(100% + 10px);
    left: clamp(0px, calc(var(--anchor) - 108px), calc(100vw - 240px));
    z-index: 60;
  }
  .instrument-palette.dock-top .palette-panel-anchor { top: calc(100% + 10px); bottom: auto; }
  /* Centred on the chip; the panel itself measures and nudges back inside the window, so no
     height is guessed here. */
  .instrument-palette.dock-left .palette-panel-anchor,
  .instrument-palette.dock-right .palette-panel-anchor {
    top: calc(var(--anchor) - 130px);
    bottom: auto;
    left: auto;
  }
  .instrument-palette.dock-left .palette-panel-anchor { left: calc(100% + 10px); }
  .instrument-palette.dock-right .palette-panel-anchor { right: calc(100% + 10px); }

  .zoom-pill { grid-area: 3 / 3; justify-self: end; align-self: end; gap: 2px; padding: 4px; border-radius: 9px; }
  .zoom-pill button { width: 34px; height: 34px; font-size: 19px; }
  .zoom-pill output { min-width: 48px; padding: 0 7px; color: #c4cad2; text-align: center; }

  .closed-state { display: grid; align-content: center; justify-items: center; padding: 2rem; background: var(--surround); text-align: center; }
  .closed-mark { width: 34px; height: 42px; border: 1px solid var(--quiet); border-radius: 3px; box-shadow: inset 0 5px var(--panel); }
  .closed-state h1 { margin: 18px 0 5px; font-size: 24px; font-weight: 600; }
  .closed-state p { margin: 0; color: var(--muted); }
  .closed-state button { margin-top: 20px; padding: 10px 14px; border-radius: 7px; background: var(--blueprint); color: #10141a; font-weight: 700; cursor: pointer; }

  .status-strip {
    z-index: 30;
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 12px;
    padding: 0 16px;
    border-top: 1px solid rgb(255 255 255 / 8%);
    background: var(--charcoal);
    color: var(--muted);
    font-size: 11px;
  }

  .tool-status { flex: none; gap: 6px; }
  .tool-status strong { color: var(--text); font-weight: 500; }
  .typst-size-control {
    display: flex;
    flex: none;
    align-items: center;
    gap: 7px;
    color: var(--text);
    white-space: nowrap;
  }
  .typst-size-control input { width: 108px; accent-color: var(--blueprint); }
  .typst-size-control output {
    min-width: 31px;
    color: var(--muted);
    font-family: "Cascadia Mono", Consolas, monospace;
    text-align: right;
  }
  .operation-status { overflow: hidden; flex: 1; color: var(--quiet); text-align: center; text-overflow: ellipsis; white-space: nowrap; }
  .operation-status.failure, .local-state.failure { color: var(--oxide); }
  .footer-divider { width: 1px; height: 15px; flex: none; background: rgb(255 255 255 / 12%); }
  .status-strip button { padding: 3px 5px; border-radius: 4px; background: transparent; color: var(--muted); cursor: pointer; }
  .status-strip button:hover { background: rgb(255 255 255 / 8%); }
  .local-state { flex: none; }

  .panel-scrim { position: absolute; z-index: 70; inset: 58px 0 34px; display: grid; padding: 24px; background: rgb(10 12 16 / 62%); place-items: start end; }
  .diagnostics-panel {
    width: min(620px, 100%);
    max-height: 100%;
    overflow: auto;
    padding: 22px 24px;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 14px;
    background: var(--panel);
    box-shadow: 0 24px 60px rgb(0 0 0 / 55%);
  }

  .panel-heading { display: flex; align-items: flex-start; justify-content: space-between; }
  .panel-heading span { color: var(--quiet); font: 10px "Cascadia Mono", Consolas, monospace; letter-spacing: .1em; text-transform: uppercase; }
  .panel-heading h2 { margin: 4px 0 0; font-size: 22px; font-weight: 600; }
  .diagnostic-path { overflow-wrap: anywhere; color: var(--quiet); font: 10px "Cascadia Mono", Consolas, monospace; }
  .diagnostics-panel dl { display: grid; grid-template-columns: minmax(210px, 1fr) 1fr; gap: 8px 18px; margin: 18px 0 0; padding-top: 16px; border-top: 1px solid rgb(255 255 255 / 8%); font-size: 12px; }
  .diagnostics-panel dt { color: var(--muted); }
  .diagnostics-panel dd { margin: 0; color: var(--text); font-family: "Cascadia Mono", Consolas, monospace; font-variant-numeric: tabular-nums; }

  .screen-reader-status { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); clip-path: inset(50%); white-space: nowrap; }
  @media (max-width: 800px) {
    .operation-status, .page-count { display: none; }
    .export-button { width: 40px; padding: 0; justify-content: center; font-size: 0; }
  }
</style>
