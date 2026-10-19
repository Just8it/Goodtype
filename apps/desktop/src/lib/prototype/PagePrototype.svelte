<script lang="ts">
  import { flushSync, onDestroy, onMount, tick } from "svelte";
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
  import { moveLibraryEntries } from "../ipc/library";
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
  import { dismissable } from "../workspace/dismiss";
  import {
    DEFAULT_INK_Z_INDEX,
    SCHEMA_VERSION,
    type NotebookManifest,
    type PageBackground,
    type PageDefaults,
    type PageGeometry,
    type PageObject,
    type PagePosition,
    type ShapeStyle,
    type PageTemplate,
    type Stroke,
    type StrokePattern,
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
  import { DEFAULT_POINTER_MAPPING, pointerRole, type InkTool, type PointerMapping } from "../ink/pipeline";
  import {
    keepsSelection,
    moveSelected,
    restyleStrokes,
    scaleSelected,
    selectionBounds,
    shared,
    transformedPoint,
    toolAfterSelection,
    type StylePatch,
  } from "../ink/selection";
  import ColorPanel from "./ColorPanel.svelte";
  import WidthPanel from "./WidthPanel.svelte";
  import PaletteEditor from "./PaletteEditor.svelte";
  import EraserPanel from "./EraserPanel.svelte";
  import ToolCard from "./ToolCard.svelte";
  import ShapeCard from "./ShapeCard.svelte";
  import PageSurface from "./PageSurface.svelte";
  import OverflowMenu from "../workspace/OverflowMenu.svelte";
  import { populated, type MenuSection } from "../workspace/menu";
  import {
    chromeLayout,
    nearestHistoryCorner,
    type ChromeArea,
    type HistoryCorner,
  } from "./chromeLayout";
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
  import PalettePocket from "./PalettePocket.svelte";
  import {
    AssetUrlCache,
    blockViewsFromSnapshot,
    imageViewsFromSnapshot,
    mimeForPath,
    pageTypstViewFromSnapshot,
    shapeViewsFromSnapshot,
    strokesFromSnapshot,
    type BlockView,
    type ImageView,
    type PageTypstView,
    type ShapeEditCommit,
    type ShapeView,
    type TypstTransform,
  } from "./pageView";
  import type { ShapeDraft } from "../shape/geometry";
  import { recognizeHeldShape } from "../shape/recognize";
  import { createCommitTimer } from "./commitTimer";
  import { createTouchTaps, type TouchHistoryAction } from "./touchTaps";
  import { blankNotebookSnapshot } from "./newNotebook";
  import { createInkCommitter, type InkCommitter } from "./inkCommitter";
  import {
    projectSnapshot,
    type ManagedMixedGroup,
  } from "./snapshot";
  import {
    nearestPaletteDock,
    paletteTransportPosition,
    type PaletteCommand,
    type PaletteDock,
  } from "./palette";
  import {
    moveReadingObject,
    moveVisualItems,
    newInkZIndex,
    type VisualMove,
  } from "./objectOrder";
  import { closedTab, cycledTab, openedTab, type NotebookTab } from "./tabs";
  import {
    clipboardMarker,
    isEmptyBundle,
    pasteOffset,
    pastedShapes,
    pastedStrokes,
    shiftTo,
    type ClipboardBundle,
  } from "./clipboard";
  import {
    atScrollEnd,
    PULL_WHEEL_IDLE_MS,
    pullProgress,
    rubberBand,
    wheelPixels,
    type PullSource,
  } from "./pullToAdd";
  import {
    addWidth as addRowWidth,
    canRemoveWidth,
    editWidth as editRowWidth,
    movePattern,
    moveSpacing,
    patternForWidth,
    prunePatterns,
    pruneSpacings,
    removeWidth as removeRowWidth,
    spacingForWidth,
    withWidthPattern,
    withWidthSpacing,
    WIDTH_BOUNDS_MM,
  } from "./widths";
  import ConfirmDialog from "../workspace/ConfirmDialog.svelte";
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
    HIGHLIGHTER_TAPER,
    MAX_SWATCHES,
    MAX_WIDTHS,
    MM_PER_PT,
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
  type ShapeState = ShapeView;
  type PaletteDrag = {
    pointerId: number;
    size: number;
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
  let activeInkLayerPath = $state("ink/page-001-layer-001.ink");
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
  let shapes = $state.raw<ShapeState[]>([]);
  let selectedImageId = $state<string | null>(null);
  let selectedShapeId = $state<string | null>(null);
  let editingShapeId = $state<string | null>(null);
  let mixedGroup = $state<ManagedMixedGroup | null>(null);
  let selectedTypstId = $state<string | null>(null);
  let directObjectInput = $state(false);
  /** True while the selection tool is only active because the lasso handed over to it. */
  /** The tool a selection hand-over gives back once the selection empties. */
  let selectionReturnTo = $state<InkTool | null>(null);
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
  let workspaceChrome = $state<HTMLElement>();
  let paletteElement = $state<HTMLElement>();
  let historyElement = $state<HTMLElement>();
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
  /// The page a pinned movable block belongs to. Page text instead follows the active page.
  let sideEditorPageId = $state<string | null>(null);
  let sideEditor = $state<{ focus: () => void }>();

  const sideEditorBlock = $derived(
    typstBlocks.find((block) => block.id === sideEditorBlockId) ??
      (pageTypst?.id === sideEditorBlockId ? pageTypst : null),
  );
  const editingPageText = $derived(
    sideEditorOpen && sideEditorPageText && pageTypst?.id === sideEditorBlockId,
  );
  /// `edit` when the target is on the page in view, `away` when a movable block is held on
  /// another page, and `none` when the active page has no Page text or no target was picked.
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
        selectedShapeId = null;
        editingShapeId = null;
      } else if (pageTypst?.id === target) {
        selectedTypstId = null;
        selectedImageId = null;
        selectedShapeId = null;
        editingShapeId = null;
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
  let historyCorner = $state<HistoryCorner>("auto");
  let historyDrag = $state<{ pointerId: number; size: number } | null>(null);
  let historyX = $state(0);
  let historyY = $state(0);
  /// Where each piece of canvas chrome sits. Derived from the dock in one place, so the palette
  /// and the controls around it cannot be given the same corner.
  let chromeCrowded = $state(false);
  const chrome = $derived(chromeLayout(paletteDock, historyCorner, chromeCrowded));
  let paletteDrag = $state<PaletteDrag | null>(null);
  let penPreset = $state<1 | 2>(1);
  // Tool-specific behaviour, anchored to the selected tool.
  let toolCard = $state<{
    kind: "pen" | "highlighter";
    slot: number;
    anchor: number;
  } | null>(null);
  let shapeCard = $state<{ anchor: number } | null>(null);
  let eraserEditor = $state<HTMLElement | null>(null);
  let paletteEditor = $state<{ kind: "width" | "color"; index: number; anchor: HTMLElement; adding?: boolean } | null>(null);

  function closePaletteEditor() {
    const anchor = paletteEditor?.anchor;
    const kind = paletteEditor?.kind;
    paletteEditor = null;
    void tick().then(() => {
      const fallback = paletteElement?.querySelector<HTMLElement>(kind === "width" ? ".widths button.current" : ".colors button.current");
      (anchor?.isConnected ? anchor : fallback)?.focus();
    });
  }

  function openPaletteEditor(kind: "width" | "color", index: number, anchor: HTMLElement, adding = false) {
    const same = paletteEditor?.kind === kind && paletteEditor.anchor === anchor;
    closeToolCard();
    if (!same) paletteEditor = { kind, index, anchor, adding };
  }

  function closeToolCard() {
    toolCard = null;
    shapeCard = null;
    paletteEditor = null;
    eraserEditor = null;
  }

  function closeEraserEditor() {
    const anchor = eraserEditor;
    closeToolCard();
    void tick().then(() => anchor?.isConnected && anchor.focus());
  }

  /// First press selects the tool; pressing the one already held opens its card — the same
  /// select-then-edit gesture the swatches use. Nothing opens by accident mid-sentence.
  function selectOrOpenTool(kind: "pen" | "highlighter", slot: number, tile: HTMLElement) {
    const alreadyActive =
      kind === "highlighter" ? tool === "highlighter" : tool === "pen" && penPreset === slot;
    if (!alreadyActive) {
      closeToolCard();
      if (kind === "pen") activateTool("pen", slot as 1 | 2);
      else activateTool("highlighter");
      return;
    }
    paletteEditor = null;
    toolCard = toolCard ? null : { kind, slot, anchor: swatchAnchor(tile) };
  }

  function selectEraser(tile: HTMLElement) {
    if (tool !== "eraser") {
      closeToolCard();
      activateTool("eraser");
      return;
    }
    const alreadyOpen = eraserEditor === tile;
    closeToolCard();
    if (!alreadyOpen) eraserEditor = tile;
  }

  function selectOrOpenShape(tile: HTMLElement) {
    paletteEditor = null;
    if (tool !== "shape") {
      closeToolCard();
      activateTool("shape");
      return;
    }
    shapeCard = shapeCard ? null : { anchor: swatchAnchor(tile) };
    toolCard = null;
  }

  function selectPlainTool(activate: () => void) {
    closeToolCard();
    activate();
  }

  const paletteCommands: Record<PaletteCommand, (tile: HTMLElement) => void> = {
    "pen-1": (tile) => selectOrOpenTool("pen", 1, tile),
    "pen-2": (tile) => selectOrOpenTool("pen", 2, tile),
    highlighter: (tile) => selectOrOpenTool("highlighter", 1, tile),
    shape: selectOrOpenShape,
    eraser: selectEraser,
    lasso: () => selectPlainTool(() => activateTool("lasso")),
    "page-text": () => selectPlainTool(() => editingPageText ? closeSideEditor() : openPageText()),
    "typst-block": () => selectPlainTool(addTypstBlock),
  };

  function activePaletteCommand(): PaletteCommand {
    if (tool === "pen") return penPreset === 1 ? "pen-1" : "pen-2";
    if (tool === "highlighter" || tool === "shape" || tool === "eraser") return tool;
    return "lasso";
  }

  const activePaletteCommands = $derived<PaletteCommand[]>([
    activePaletteCommand(),
    ...(editingPageText ? (["page-text"] as const) : []),
  ]);
  const expandedPaletteCommand = $derived<PaletteCommand | null>(
    toolCard || shapeCard || eraserEditor ? activePaletteCommand() : null,
  );

  /// Which quick controls the pocket carries for the tool in hand. Absent for tools that have
  /// nothing to set, so the rail shrinks back to bare tiles rather than showing an empty tray.
  /// Whether the pocket edits a selection rather than the pen (specs/qol-wave.md section 2).
  const selectedInk = $derived(strokes.filter((stroke) => selectedStrokeIds.includes(stroke.id)));
  const selectedShape = $derived(shapes.find((shape) => shape.id === selectedShapeId) ?? null);
  const restyling = $derived(selectedInk.length > 0 || selectedShape !== null);
  const paletteContext = $derived<"ink" | "eraser" | null>(
    restyling || tool === "pen" || tool === "highlighter" || tool === "shape"
      ? "ink"
      : tool === "eraser"
        ? "eraser"
        : null,
  );

  /**
   * CSS grid line pair, e.g. `1 / 2`.
   *
   * Only applied while the element is parked. An absolutely positioned grid child that still names
   * a grid area is positioned *within that area* rather than within the grid, so a puck being
   * carried would measure its offsets from whichever cell it had been assigned — and the cell
   * moves as the drop target changes, which sends it off the canvas and under the clip.
   */
  function gridSpan(start: number, end: number): string {
    return `${start} / ${end}`;
  }

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
  const touchTaps = createTouchTaps();
  let tapScrollOrigin: { left: number; top: number } | null = null;
  let touchHistoryToast = $state<{ label: "Undo" | "Redo" } | null>(null);
  let touchHistoryToastTimer: ReturnType<typeof setTimeout> | undefined;
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

  const toolCardPreset = $derived(
    toolCard?.kind === "highlighter"
      ? settings.highlighter
      : settings.penPresets[(toolCard?.slot ?? 1) - 1],
  );

  function updateToolPreset(next: PenPreset) {
    if (!toolCard) return;
    const slot = toolCard.slot;
    if (toolCard.kind === "highlighter") changeSettings({ ...settings, highlighter: next });
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
  let windowCloseInProgress = false;

  onMount(() => {
    void initialize();
    window.addEventListener("keydown", historyShortcut);
    // On the window, not on <main>: with focus on the body a paste never reaches <main>.
    window.addEventListener("paste", handlePaste);
    window.addEventListener("resize", onResizeToolbar);
    if (tauriAvailable) {
      const appWindow = getCurrentWindow();
      void appWindow
        .onCloseRequested(async (event) => {
          event.preventDefault();
          if (windowCloseInProgress) return;
          windowCloseInProgress = true;
          try {
            if (!(await persist())) return;
            if (notebookChosen && root) {
              await recordNotebookPage(root, activePageId).catch(() => {});
            }
            // `close()` emits another close-request event. Awaiting it from this handler can
            // re-enter the same save path, so force the already-confirmed close instead.
            await appWindow.destroy();
          } catch (error) {
            status = `The notebook is safe, but the window could not close: ${message(error)}`;
          } finally {
            windowCloseInProgress = false;
          }
        })
        .then((remove) => (removeCloseListener = remove))
        .catch((error) => {
          status = `Could not install the window close handler: ${message(error)}`;
        });
    }
  });
  onDestroy(() => {
    inkCommitTimer.cancel();
    if (typstCommitTimer) clearTimeout(typstCommitTimer);
    if (focusTimer) clearTimeout(focusTimer);
    if (metricsTimer) clearTimeout(metricsTimer);
    if (selectionToolbarFrame) cancelAnimationFrame(selectionToolbarFrame);
    if (touchInertiaFrame) cancelAnimationFrame(touchInertiaFrame);
    if (touchHistoryToastTimer) clearTimeout(touchHistoryToastTimer);
    if (pendingZoomFrame) cancelAnimationFrame(pendingZoomFrame);
    removeCloseListener?.();
    window.removeEventListener("keydown", historyShortcut);
    window.removeEventListener("paste", handlePaste);
    window.removeEventListener("resize", onResizeToolbar);
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
    void shapes;
    void strokes;
    void zoom;
    void activePageId;
    void sideEditorOpen;
    void settings.sideEditorWidth;
    selectionToolbarPosition = { left: 0, top: 0, ready: false };
    if (selected) void tick().then(() => scheduleSelectionToolbar());
  });

  async function initialize() {
    settings = await loadSettings(tauriAvailable);
    paletteDock = settings.paletteDock;
    historyCorner = settings.historyCorner;
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
    options: {
      createIfMissing?: boolean;
      skipCurrentPersist?: boolean;
      stayInLibrary?: boolean;
      setup?: NotebookSetup;
    } = {},
  ): Promise<boolean> {
    if (nextRoot === root && activeSnapshot) {
      notebookChosen = !options.stayInLibrary;
      pageOpen = true;
      if (!options.stayInLibrary) {
        await tick();
        scrollToPage(activePageId, "auto");
      }
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
      notebookChosen = !options.stayInLibrary;
      pageOpen = true;
      if (!options.stayInLibrary) {
        await tick();
        scrollToPage(snapshot.page.id, "auto");
      }
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
      shapes,
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
    activeInkLayerPath = activeInk?.path ?? `ink/${activePageId}-layer-001.ink`;
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
    shapes = shapeViewsFromSnapshot(snapshot);
    selectedShapeId = null;
    editingShapeId = null;

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

    // A movable block stays pinned when the writer scrolls away. Page text is different: it is
    // the current page's writing surface, so an already-open Page text editor follows page focus.
    // Keeping the flag true when there is no surface lets the next page retarget automatically
    // and gives this page the dedicated "Write Page text" empty state.
    if (sideEditorOpen && sideEditorPageText && !sideEditorStyle) {
      sideEditorBlockId = pageTypst?.id ?? null;
      sideEditorPageId = activePageId;
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
    historyCorner = next.historyCorner;
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

  /** The page number a pending delete asks about; null while nothing is being asked. */
  let confirmingPageDelete = $state<number | null>(null);

  async function deleteActivePage() {
    moreOpen = false;
    if (!tauriAvailable || !(await persist())) return;
    confirmingPageDelete = activePageNumber();
  }

  async function confirmDeleteActivePage() {
    const deletedNumber = confirmingPageDelete;
    confirmingPageDelete = null;
    busy = true;
    try {
      const result = await deletePage(root, activePageId, new Date().toISOString());
      pageEntries = [];
      applySnapshot(result.snapshot);
      recordStructureAction();
      await tick();
      scrollToPage(result.snapshot.page.id);
      status = `Deleted page ${deletedNumber}`;
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
    shapes = [];
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
    selectedShapeId = null;
    editingShapeId = null;
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
            { kind: "action", id: "reopen", label: "Reopen notebook", icon: "M4 12a8 8 0 1 1 2.3 5.6M4 12V6M4 12h6", onSelect: reopen },
            { kind: "action", id: "settings", label: "Settings", icon: "M4 8h8M17 8h3M4 16h3M12 16h8M14.5 5.8v4.4M9.5 13.8v4.4", hint: "Ctrl ,", onSelect: () => (settingsOpen = true) },
          ],
        },
      ];
    }
    return populated([
      ...(selected
        ? [
            {
              title: selectedImageId
                ? "Selected image"
                : selectedShapeId
                  ? "Selected shape"
                  : "Selected Typst",
              entries: [
                { kind: "action" as const, id: "object-back", label: "Send behind everything", icon: "M4 12l8 4 8-4M4 8l8-4 8 4-8 4z", disabled: !canMoveVisual(-1), onSelect: () => changeVisualOrder("back") },
                { kind: "action" as const, id: "object-front", label: "Bring in front of everything", icon: "M4 12l8-4 8 4-8 4zM4 16l8 4 8-4", disabled: !canMoveVisual(1), onSelect: () => changeVisualOrder("front") },
                { kind: "action" as const, id: "read-earlier", label: "Read earlier by screen readers", icon: "M12 19V5m0 0-5 5m5-5 5 5", disabled: !canMoveReading(-1), onSelect: () => changeReadingOrder(-1) },
                { kind: "action" as const, id: "read-later", label: "Read later by screen readers", icon: "M12 5v14m0 0 5-5m-5 5-5-5", disabled: !canMoveReading(1), onSelect: () => changeReadingOrder(1) },
                { kind: "action" as const, id: "remove-object", label: "Remove from page", icon: "M6 6l12 12M18 6 6 18", hint: "Delete", onSelect: () => void deleteSelection() },
              ],
            },
          ]
        : []),
      {
        entries: [
          // Adding a page is not here: it has its own header button, because choosing where the
          // page goes and what it is made of needs more than one row.
          { kind: "action", id: "duplicate", label: "Duplicate Page", icon: "M9 9h10v12H9zM5 15V3h10v2", onSelect: duplicateActivePage },
          { kind: "action", id: "up", label: "Move Page Up", icon: "M12 19V5m0 0-5 5m5-5 5 5", hint: "Ctrl Shift PgUp", disabled: !many || pageNumber === 1, onSelect: () => void moveActivePage(-1) },
          { kind: "action", id: "down", label: "Move Page Down", icon: "M12 5v14m0 0 5-5m-5 5-5-5", hint: "Ctrl Shift PgDn", disabled: !many || pageNumber === pageCount, onSelect: () => void moveActivePage(1) },
          { kind: "number", id: "goto", label: "Go to Page", icon: "M5 4.5h14v15H5zM9 9h6M9 13h4", value: pageNumber, min: 1, max: pageCount, hint: `of ${pageCount}`, disabled: !many, onCommit: (number) => void goToPageNumber(number) },
        ],
      },
      {
        title: "Clear or remove page",
        entries: [
          { kind: "action", id: "clear", label: "Clear Page", icon: "M6 6.5h12M9.5 6.5V4h5v2.5M8 10v7M12 10v7M16 10v7M6.5 6.5 7.5 20h9l1-13.5", destructive: true, onSelect: clearActivePage },
          // Undo covers this: deleting drops the manifest reference and the page's own files stay
          // on disk. That is why there is no trash bin.
          { kind: "action", id: "delete", label: "Delete Page", icon: "M5 4.5h14v15H5zM9 9l6 6M15 9l-6 6", destructive: true, disabled: !many, onSelect: deleteActivePage },
        ],
      },
      {
        title: "Notebook",
        entries: [
          { kind: "action", id: "search", label: "Search notebook", icon: "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM20 20l-3.6-3.6", hint: "Ctrl F", onSelect: () => (searchOpen = true) },
          { kind: "action", id: "style", label: "Edit shared Typst style", icon: "M5 4.5h14M12 4.5v15M8 19.5h8", onSelect: openSharedStyle },
          { kind: "action", id: "settings", label: "Settings", hint: "Ctrl ,", icon: "M4 8h8M17 8h3M4 16h3M12 16h8M14.5 5.8v4.4M9.5 13.8v4.4", onSelect: () => (settingsOpen = true) },
          { kind: "action", id: "save", label: "Confirm saved", icon: "M5 12.5l4.5 4.5L19 7.5", onSelect: () => void persist() },
          { kind: "action", id: "close", label: "Close notebook", icon: "M14 5h5v14h-5M13 12H4m0 0 3.5-3.5M4 12l3.5 3.5", onSelect: closePage },
        ],
      },
    ]);
  }

  async function moveActivePage(direction: -1 | 1) {
    moreOpen = false;
    const manifest = notebookManifest;
    if (busy || !tauriAvailable || !manifest) return;
    const order = manifest.pages.map((page) => page.id);
    const index = order.indexOf(activePageId);
    const target = index + direction;
    if (index < 0 || target < 0 || target >= order.length) return;
    [order[index], order[target]] = [order[target], order[index]];
    busy = true;
    try {
      if (!(await persist())) return;
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
      selectedShapeId = null;
      editingShapeId = null;
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
    moreOpen = false;
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
        pageEntries = [];
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
        count > 1 ? `Added ${count} pages` : `Added page ${activePageNumber()}`;
    } catch (error) {
      status = `Could not add page: ${message(error)}`;
    } finally {
      busy = false;
    }
  }

  async function importPdf(position: PagePosition) {
    moreOpen = false;
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
      onSelect: (position, count) =>
        void addPage(position, { kind: "template", template: source }, geometry, count),
    });
    // The notebook's paper for new pages: what it was created with, then whatever was added last
    // (Rust moves it on every add with a chosen paper). The panel opens on it.
    const next = notebookManifest?.defaultPage ?? null;
    const nextUsable = next && next.background.kind !== "pdf" ? next : null;
    const paperPreview = (paper: PageDefaults) =>
      paper.background.kind === "template"
        ? templatePreviewSvg(paper.background.template, paper.geometry)
        : paper.background.kind === "plain"
          ? templatePreviewSvg(
              { id: "next-plain", name: "Plain", backgroundColor: paper.background.color, elements: [] },
              paper.geometry,
            )
          : undefined;
    const paperName = (paper: PageDefaults) =>
      paper.background.kind === "template" ? paper.background.template.name : "Plain";
    return [
      {
        id: "import",
        title: "Import",
        lane: "import",
        sources: [
          {
            id: "pdf",
            label: "Import PDF",
            detail: "Brings in every page",
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
        title: "Notebook paper",
        lane: "current",
        sources: [
          ...(nextUsable
            ? [
                {
                  id: "next",
                  label: "Notebook paper",
                  detail: `${paperName(nextUsable)}, ${describeGeometry(nextUsable.geometry)}`,
                  preview: paperPreview(nextUsable),
                  onSelect: (position: PagePosition, count: number) =>
                    void addPage(position, nextUsable.background, nextUsable.geometry, count),
                } satisfies AddPageSource,
              ]
            : []),
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
        readingOrder: nextReadingOrder(),
      },
    ];
    selectedTypstId = id;
    selectedImageId = null;
    selectedShapeId = null;
    editingShapeId = null;
    status = "Created a new Typst block";
    queueCommit("Created Typst block");
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
    const next = toolAfterSelection(tool, ids.length, selectionReturnTo);
    tool = next.tool;
    selectionReturnTo = next.returnTo;
  }

  const TOOL_NAMES: Record<InkTool, string> = {
    pen: "Pen",
    highlighter: "Highlighter",
    shape: "Shape",
    eraser: "Eraser",
    lasso: "Lasso",
    select: "Ink selection",
  };

  /** Where "Eraser goes back to your pen" returns to. Only an eraser picked by hand sets it. */
  let toolBeforeEraser: { tool: InkTool; preset: 1 | 2 } | null = null;

  const pointerMapping = $derived<PointerMapping>({
    ...DEFAULT_POINTER_MAPPING,
    sideButton: settings.penSideButton,
    fingerDraws: settings.drawWithFinger,
  });

  /// The pen's eraser end and side button switch to the eraser like the tile does, so all three
  /// come back here: with the setting on, each erase hands the page back to the tool before it.
  function returnFromEraser() {
    if (!settings.eraserReturns || tool !== "eraser" || !toolBeforeEraser) return;
    const { tool: previous, preset } = toolBeforeEraser;
    activateTool(previous, previous === "pen" ? preset : undefined);
  }

  function activateTool(next: InkTool, preset?: 1 | 2) {
    if (next === "eraser" && tool !== "eraser") toolBeforeEraser = { tool, preset: penPreset };
    if (next !== tool || (next === "pen" && preset !== undefined && preset !== penPreset)) {
      closeToolCard();
    }
    if (preset) penPreset = preset;
    tool = next;
    selectionReturnTo = null;
    // A brush owns the page, so anything still selected would sit there showing handles and
    // refusing to move. Dropping it is the honest half of that: the selection is gone because
    // the tool that acted on it is.
    if (!keepsSelection(next)) {
      updateInkSelection([]);
      selectedTypstId = null;
      selectedImageId = null;
      selectedShapeId = null;
      editingShapeId = null;
    }
    status = `${next === "pen" ? `Pen ${penPreset}` : TOOL_NAMES[next]} active`;
  }

  // The palette carries inline stroke sizes and colors, contextual to the active tool, so the
  // everyday change is a single tap on the bar rather than a submenu. Pressure/calibration
  // stay in the Settings window.
  // Chips now read from the writer's curated rows rather than frozen constants, which is what
  // makes a colour or width added here actually appear — and stay — on the bar.
  /**
   * Restyling a selection (specs/qol-wave.md section 2). While ink or a shape is selected the
   * pocket edits that selection instead of the pen: it shows what the selection shares, and every
   * pick changes the selection while the pen keeps its own settings.
   */
  const selectionStyle = $derived.by(() => {
    const ink = selectedInk;
    const shape = selectedShape;
    const widths = [...ink.map((stroke) => stroke.widthPt), ...(shape ? [shape.style.strokeWidthPt] : [])];
    const colors = [...ink.map((stroke) => stroke.color.toLowerCase()), ...(shape ? [shape.style.strokeColor.toLowerCase()] : [])];
    const pens = ink.filter((stroke) => stroke.tool === "pen");
    const patterns = [...pens.map((stroke) => stroke.pattern ?? "solid"), ...(shape ? [shape.style.pattern ?? "solid"] : [])];
    const spacings = [...pens.map((stroke) => stroke.patternSpacing ?? 1), ...(shape ? [shape.style.patternSpacing ?? 1] : [])];
    return {
      widthPt: shared(widths),
      color: shared(colors),
      pattern: shared(patterns) ?? "solid",
      spacing: shared(spacings) ?? 1,
    };
  });

  function restyleSelection(patch: StylePatch) {
    if (selectedInk.length > 0) {
      changeStrokes(restyleStrokes(strokes, selectedStrokeIds, patch), "Restyled selection");
    }
    const shape = selectedShape;
    if (shape) {
      const style = { ...shape.style };
      if (patch.color !== undefined) {
        style.strokeColor = patch.color;
        if (style.fillColor) style.fillColor = `${patch.color}24`;
      }
      if (patch.widthPt !== undefined) style.strokeWidthPt = patch.widthPt;
      if (patch.pattern !== undefined) style.pattern = patch.pattern;
      if (patch.patternSpacing !== undefined) style.patternSpacing = patch.patternSpacing;
      changeShape({ ...shape, style });
    }
    const count = selectedInk.length + (shape ? 1 : 0);
    status = `Restyled ${count} selected item${count === 1 ? "" : "s"}`;
  }

  const activeWidthChips = $derived(
    tool === "highlighter" && !restyling ? settings.highlighterWidths : settings.penWidths,
  );
  const activeColorChips = $derived(
    tool === "highlighter" && !restyling ? settings.highlighterSwatches : settings.penSwatches,
  );
  // A mixed selection shares no width or colour, so nothing on the row reads as selected.
  const activeWidth = $derived(
    restyling
      ? (selectionStyle.widthPt ?? Number.NaN)
      : tool === "highlighter"
        ? settings.highlighter.widthPt
        : settings.penPresets[penPreset - 1].widthPt,
  );
  const activeInkColor = $derived(
    restyling
      ? (selectionStyle.color ?? "")
      : tool === "highlighter"
        ? settings.highlighter.color
        : settings.penPresets[penPreset - 1].color,
  );

  function nearestChip(chips: readonly number[], value: number): number {
    return chips.reduce((best, chip) =>
      Math.abs(chip - value) < Math.abs(best - value) ? chip : best,
    );
  }

  /// Picking a pen chip takes its pattern too: the pattern belongs to that width, not the pen.
  function setActiveWidth(widthPt: number) {
    if (restyling) {
      // A chip is a whole stroke — width, pattern and spacing — so picking it hands all three to
      // the selection, as it does to the pen.
      restyleSelection({
        widthPt,
        pattern: patternForWidth(settings.penWidthPatterns, widthPt),
        patternSpacing: spacingForWidth(settings.penWidthSpacings, widthPt),
      });
      return;
    }
    if (tool === "highlighter") {
      changeSettings({ ...settings, highlighter: { ...settings.highlighter, widthPt } });
    } else {
      const pattern = patternForWidth(settings.penWidthPatterns, widthPt);
      changeSettings({
        ...settings,
        penPresets: settings.penPresets.map((preset, index) =>
          index === penPreset - 1 ? { ...preset, widthPt, pattern } : preset,
        ),
      });
    }
  }

  /// The gap factor of the chip in hand. A custom width off the row has no chip, so it uses 1.
  function inkPatternSpacing(): number {
    return tool === "pen" ? spacingForWidth(settings.penWidthSpacings, activeWidth) : 1;
  }

  function setActiveSpacing(spacing: number) {
    if (restyling) {
      restyleSelection({ patternSpacing: spacing });
      return;
    }
    if (!settings.penWidths.some((width) => Math.abs(width - activeWidth) < 0.001)) return;
    changeSettings({
      ...settings,
      penWidthSpacings: withWidthSpacing(settings.penWidthSpacings, activeWidth, spacing),
    });
  }

  /// Sets the pattern of the chip being edited and draws with it, leaving the other widths alone.
  function setActivePattern(pattern: StrokePattern) {
    if (restyling) {
      restyleSelection({ pattern });
      return;
    }
    const onRow = settings.penWidths.some((width) => Math.abs(width - activeWidth) < 0.001);
    changeSettings({
      ...settings,
      penWidthPatterns: onRow
        ? withWidthPattern(settings.penWidthPatterns, activeWidth, pattern)
        : settings.penWidthPatterns,
      penPresets: settings.penPresets.map((preset, index) =>
        index === penPreset - 1 ? { ...preset, pattern } : preset,
      ),
    });
  }

  /// Point the active tool at a colour, and remember it as recently used. This changes which
  /// colour the pen writes with; it does not touch the swatch row.
  function setActiveColor(color: string) {
    if (restyling) {
      changeSettings({ ...settings, recentColors: withRecentColor(settings.recentColors, color) });
      restyleSelection({ color });
      return;
    }
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

  function putWidths(
    widths: number[],
    select: number,
    patterns = settings.penWidthPatterns,
    spacings = settings.penWidthSpacings,
  ) {
    const key = widthKey();
    if (tool === "highlighter") {
      changeSettings({
        ...settings,
        [key]: widths,
        highlighter: { ...settings.highlighter, widthPt: select },
      } as AppSettings);
    } else {
      const penWidthPatterns = prunePatterns(patterns, widths);
      const pattern = patternForWidth(penWidthPatterns, select);
      changeSettings({
        ...settings,
        [key]: widths,
        penWidthPatterns,
        penWidthSpacings: pruneSpacings(spacings, widths),
        penPresets: settings.penPresets.map((preset, index) =>
          index === penPreset - 1 ? { ...preset, widthPt: select, pattern } : preset,
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
    const previous = settings[widthKey()][index];
    const moves = tool !== "highlighter" && previous !== undefined;
    const patterns = moves
      ? movePattern(settings.penWidthPatterns, previous, widthPt)
      : settings.penWidthPatterns;
    const spacings = moves
      ? moveSpacing(settings.penWidthSpacings, previous, widthPt)
      : settings.penWidthSpacings;
    putWidths(
      editRowWidth(settings[widthKey()], index, widthPt, MAX_WIDTHS),
      widthPt,
      patterns,
      spacings,
    );
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
    // Explicit handles own contact, even with a brush or the Shape tool in hand.
    if (event.target instanceof Element && event.target.closest(".shape-controls .control-hit")) {
      directObjectInput = true;
      const role = pointerRole(event, tool, pointerMapping);
      if (event.type === "pointerdown" && (role === "erase" || role === "lasso" || role === "ignore" && event.pointerType === "pen")) {
        // The eraser end and barrel mappings still take precedence over a handle.
        const ink = event.target.closest(".page")?.querySelector(".ink-surface");
        directObjectInput = false;
        event.preventDefault();
        event.stopPropagation();
        ink?.dispatchEvent(new PointerEvent(event.type, event));
      }
      return;
    }
    // A stylus is for writing, so ink takes it and you can write straight over a block. That was
    // unconditional, which meant a block could never be picked up with the pen that wrote it —
    // there was no tool that gave the stylus the object layer. A selection tool now does.
    //
    // Mouse and touch keep reaching objects under any tool: clicking a block to grab it is how
    // this has always worked, and a brush is no reason to take it away.
    // The bar and its editors restyle whatever is selected, so pressing them must not re-route
    // the press to the page and deselect the very shape about to be restyled.
    if (
      event.target instanceof Element &&
      event.target.closest(".selection-actions, .instrument-palette, .palette-editor")
    ) {
      return;
    }
    // The shape tool builds on the ink surface, and the eraser has to reach it to rub anything
    // out, so neither hands the pointer to an object under the cursor.
    if (tool === "shape" || tool === "eraser") {
      directObjectInput = false;
      return;
    }
    if ((event.pointerType === "pen" || fingerDraws(event)) && !keepsSelection(tool)) {
      directObjectInput = false;
      return;
    }
    const object = document
      .elementsFromPoint(event.clientX, event.clientY)
      .map((element) => element.closest<HTMLElement>(".typst-block, .image-object, .shape-object"))
      .find((element) => element && workspace?.contains(element));
    directObjectInput = Boolean(object);
    if (event.type !== "pointerdown") return;
    const objectId =
      object?.dataset.objectId ??
      object?.querySelector<HTMLElement>("[data-object-id]")?.dataset.objectId ??
      null;
    selectedTypstId = object?.classList.contains("typst-block")
      ? objectId
      : null;
    selectedImageId = object?.classList.contains("image-object")
      ? objectId
      : null;
    selectedShapeId = object?.classList.contains("shape-object")
      ? objectId
      : null;
    if (selectedImageId || selectedShapeId) updateInkSelection([]);
    if (selectedShapeId !== editingShapeId) editingShapeId = null;
    if (object && event.target instanceof Element && event.target.closest(".ink-surface")) {
      event.preventDefault();
      event.stopPropagation();
      status = selectedTypstId
        ? "Typst block selected"
        : selectedImageId
          ? "Image selected"
          : "Shape selected";
    }
  }

  function closeObjectSelection(event: PointerEvent) {
    // The card lives inside the bar, so a press anywhere outside it dismisses the card.
    if (!(event.target instanceof Element) || !event.target.closest(".instrument-palette")) {
      closeToolCard();
    }
    // A stylus press mid-stroke is writing, not "deselect" — unless a selection tool is active,
    // where a press on empty page means exactly that.
    if (
      ((event.pointerType === "pen" || fingerDraws(event)) && !keepsSelection(tool)) ||
      (event.target instanceof Element &&
        event.target.closest(
          ".typst-block, .image-object, .shape-object, .typst-size-control, .selection-actions, .overflow-menu, .instrument-palette, .palette-editor, [data-preserve-selection]",
        ))
    ) {
      return;
    }
    selectedTypstId = null;
    selectedImageId = null;
    selectedShapeId = null;
    editingShapeId = null;
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

  /**
   * Palm rejection for finger drawing. A hand resting on the glass while the pen hovers or writes
   * is a palm, not a finger, so for a moment after any pen activity touch goes back to only
   * navigating. Hover counts: Windows reports a pen in range as pointer moves with no buttons.
   */
  const PALM_REJECTION_MS = 500;
  let lastPenSeenAt = -Infinity;

  function notePen(event: PointerEvent) {
    if (event.pointerType === "pen") {
      lastPenSeenAt = event.timeStamp;
      touchTaps.cancel();
    }
  }

  $effect(() => {
    settings.touchHistory;
    root;
    activePageId;
    touchTaps.reset();
    tapScrollOrigin = null;
  });

  function touchHistoryAllowed(event: PointerEvent): boolean {
    return settings.touchHistory && event.timeStamp - lastPenSeenAt > PALM_REJECTION_MS;
  }

  function applyTouchHistory(action: TouchHistoryAction) {
    if (busy || transactionFailed || !(action === "undo" ? canUndo : canRedo)) return;
    void routeHistory(action === "undo" ? "undo_notebook" : "redo_notebook",
      action === "undo" ? "Undid change" : "Redid change", true);
  }

  function announceTouchHistory(label: "Undo" | "Redo") {
    if (touchHistoryToastTimer) clearTimeout(touchHistoryToastTimer);
    touchHistoryToast = { label };
    touchHistoryToastTimer = setTimeout(() => { touchHistoryToast = null; }, 1000);
  }

  /** Whether this press is a finger the writer has asked to draw with. */
  function fingerDraws(event: PointerEvent): boolean {
    return (
      settings.drawWithFinger &&
      event.pointerType === "touch" &&
      event.timeStamp - lastPenSeenAt > PALM_REJECTION_MS
    );
  }

  function workspacePointerDown(event: PointerEvent) {
    notePen(event);
    stopTouchInertia();
    if (beginPan(event)) return;
    // Fixed controls inside the scroll field own their touch. Without this exception the
    // one-finger canvas pan captures the pointer before a reorder button can receive its click.
    if (
      event.target instanceof Element &&
      event.target.closest(".selection-actions, .page-number button")
    ) {
      if (event.pointerType === "touch") touchTaps.cancel();
      return;
    }
    routeObjectPointer(event);
    if (event.pointerType !== "touch") return;
    const onInk = event.target instanceof Element && Boolean(event.target.closest(".ink-surface"));
    touchTaps.start(event, touchHistoryAllowed(event) && onInk && !directObjectInput);
    if (touchPoints.size === 0 && pageViewport) {
      tapScrollOrigin = { left: pageViewport.scrollLeft, top: pageViewport.scrollTop };
    }
    touchPoints.set(event.pointerId, { x: event.clientX, y: event.clientY });
    if (tool === "shape" && onInk && touchPoints.size === 1) return;
    // A still finger held on the page opens the context menu. Not while a finger draws: there a
    // held finger is a dot being written.
    cancelLongPress();
    if (touchPoints.size === 1 && !fingerDraws(event)) {
      const { pointerId, clientX, clientY } = event;
      longPress = {
        pointerId,
        x: clientX,
        y: clientY,
        timer: setTimeout(() => {
          longPress = null;
          touchPanStart = null;
          openContextMenu(clientX, clientY);
        }, LONG_PRESS_MS),
      };
    }
    // A drawing finger is left to the ink surface. Only a second finger is claimed here, and
    // capturing it for the pinch takes the first away from the surface, which drops that stroke.
    if (
      touchPoints.size === 1 &&
      fingerDraws(event) &&
      !directObjectInput &&
      event.target instanceof Element &&
      event.target.closest(".ink-surface")
    ) return;
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
    if (touchPoints.size > 2) {
      workspace?.setPointerCapture(event.pointerId);
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    if (touchPoints.size !== 2 || !workspace) return;
    // Undo a tap's tiny first-finger pan; larger motion has already rejected the tap candidate.
    if (touchTaps.isMultiTapCandidate(event.timeStamp) && tapScrollOrigin && pageViewport) {
      pageViewport.scrollLeft = tapScrollOrigin.left;
      pageViewport.scrollTop = tapScrollOrigin.top;
    }
    automaticPageFocusLocked = true;
    touchPanStart = null;
    if (pull?.source === "touch") releasePull(false);
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
    notePen(event);
    if (event.pointerType === "touch") touchTaps.move(event, touchHistoryAllowed(event));
    if (
      longPress?.pointerId === event.pointerId &&
      Math.hypot(event.clientX - longPress.x, event.clientY - longPress.y) > LONG_PRESS_SLOP_PX
    ) {
      cancelLongPress();
    }
    if (continuePan(event)) return;
    routeObjectPointer(event);
    if (!touchPoints.has(event.pointerId)) return;
    touchPoints.set(event.pointerId, { x: event.clientX, y: event.clientY });
    // Jitter during a possible history tap must not zoom. Once the tap limits are exceeded,
    // the existing pan/pinch uses the complete movement from its original anchor.
    if (touchTaps.isMultiTapCandidate(event.timeStamp)) {
      event.preventDefault();
      event.stopPropagation();
      return;
    }
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
      // Past the end the finger pulls against resistance instead of scrolling into nothing.
      const end = pageViewport.scrollHeight - pageViewport.clientHeight;
      if (canPull() && (next.y > end || pull?.source === "touch")) {
        const raw = next.y - end;
        pull = raw > 0 ? { raw, source: "touch" } : null;
        next.y = Math.min(next.y, end);
      }
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
    notePen(event);
    if (longPress?.pointerId === event.pointerId) cancelLongPress();
    if (endPan(event)) return;
    if (event.pointerType !== "touch") return;
    const tapCandidate = touchTaps.isMultiTapCandidate(event.timeStamp);
    const tap = touchTaps.end(event, event.type !== "pointerup", touchHistoryAllowed(event));
    touchPoints.delete(event.pointerId);
    if (touchPoints.size === 0) tapScrollOrigin = null;
    if (tapCandidate || tap) {
      touchPanStart = null;
      pinchStart = null;
      if (pull?.source === "touch") releasePull(false);
      event.preventDefault();
      event.stopPropagation();
      if (tap?.action) applyTouchHistory(tap.action);
      return;
    }
    if (touchPanStart?.pointerId === event.pointerId) {
      const finishedPan = touchPanStart;
      touchPanStart = null;
      if (pull?.source === "touch") {
        // A pull ends in the page or in nothing; it never hands off to a glide.
        releasePull(event.type === "pointerup");
        event.preventDefault();
        event.stopPropagation();
        return;
      }
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
    if (event.button !== 0 || !event.isPrimary || !workspace) return;
    const target = event.currentTarget as HTMLElement;
    const workspaceBounds = workspace.getBoundingClientRect();
    const size = transportSize();
    const position = paletteTransportPosition(
      event.clientX - workspaceBounds.left,
      event.clientY - workspaceBounds.top,
      workspaceBounds.width,
      workspaceBounds.height,
      size,
    );
    const offset = transportOffset();
    paletteX = position.x - offset.x;
    paletteY = position.y - offset.y;
    closeToolCard();
    target.setPointerCapture(event.pointerId);
    paletteDrag = { pointerId: event.pointerId, size };
    status = "Moving tool palette — release at an edge to dock";
    event.preventDefault();
  }

  /**
   * Which edge the palette would take if the drag ended here.
   *
   * Applied while dragging, not only on release. The compact transport puck keeps one stable
   * geometry; its blue edge mark previews which full-palette orientation will return on release.
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
    const position = paletteTransportPosition(
      event.clientX - bounds.left,
      event.clientY - bounds.top,
      bounds.width,
      bounds.height,
      paletteDrag.size,
    );
    const offset = transportOffset();
    paletteX = position.x - offset.x;
    paletteY = position.y - offset.y;
    const next = dockUnderPointer(event);
    // Not written to settings yet: a drag that wanders across three edges should leave one
    // preference behind, on release, not three.
    if (next && next !== paletteDock) paletteDock = next;
  }

  function finishPaletteDrag(event: PointerEvent) {
    if (!paletteDrag || event.pointerId !== paletteDrag.pointerId) return;
    const next = dockUnderPointer(event) ?? paletteDock;
    paletteDrag = null;
    paletteDock = next;
    status = `Tool palette docked to the ${next} edge`;
    if (next !== settings.paletteDock) {
      changeSettings({ ...settings, paletteDock: next });
    }
  }

  /**
   * The pill is its own handle.
   *
   * There is nowhere to put a grip on a control this small without it becoming mostly grip, so the
   * strip between the two buttons is the grip — press anywhere on the pill that is not a button
   * and it moves. The buttons keep the press for themselves, so undo stays a single tap.
   */
  function beginHistoryDrag(event: PointerEvent) {
    if (event.button !== 0 || !event.isPrimary || !workspace) return;
    if (event.target instanceof Element && event.target.closest("button")) return;
    const size = transportSize();
    carryHistoryTo(event, size);
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    historyDrag = { pointerId: event.pointerId, size };
    status = "Moving the undo buttons — release in a corner";
    event.preventDefault();
  }

  function moveHistory(event: PointerEvent) {
    if (!historyDrag || event.pointerId !== historyDrag.pointerId) return;
    carryHistoryTo(event, historyDrag.size);
  }

  /// Carry the puck under the pointer and preview the corner it would land in. The corner is
  /// shown live but not written to settings until release, so one drag leaves one preference.
  function carryHistoryTo(event: PointerEvent, size: number) {
    if (!workspace) return;
    const bounds = workspace.getBoundingClientRect();
    const x = event.clientX - bounds.left;
    const y = event.clientY - bounds.top;
    const position = paletteTransportPosition(x, y, bounds.width, bounds.height, size);
    const offset = transportOffset();
    historyX = position.x - offset.x;
    historyY = position.y - offset.y;
    historyCorner = nearestHistoryCorner(x, y, bounds.width, bounds.height);
  }

  /**
   * Whether an end-docked rail and the history pill can share a row.
   *
   * Measured rather than guessed at, because it depends on how wide the side editor has been
   * pulled out and on how many tools the rail is carrying. `scrollWidth` is the rail's own width
   * even when the row has squeezed it, so this does not read back a width the pill caused — which
   * is what would make the pill move away, free up room, and be invited straight back.
   */
  $effect(() => {
    const chromeBox = workspaceChrome;
    const rail = paletteElement;
    const pill = historyElement;
    if (!chromeBox || !rail || !pill) return;
    const measure = () => {
      const style = getComputedStyle(chromeBox);
      const available =
        chromeBox.clientWidth -
        (Number.parseFloat(style.paddingLeft) || 0) -
        (Number.parseFloat(style.paddingRight) || 0);
      const gap = Number.parseFloat(style.columnGap) || 0;
      chromeCrowded = rail.scrollWidth + pill.scrollWidth + gap * 2 > available;
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(chromeBox);
    observer.observe(rail);
    return () => observer.disconnect();
  });

  /**
   * From workspace coordinates to the ones a carried puck is actually positioned in.
   *
   * Drop targets are decided across the whole workspace, but the puck is an absolutely positioned
   * child of the chrome, so its offsets are measured from that element's padding box. Without the
   * conversion the puck sits a padding's width down and to the right of the pointer carrying it.
   */
  function transportOffset(): { x: number; y: number } {
    if (!workspaceChrome) return { x: 0, y: 0 };
    const style = getComputedStyle(workspaceChrome);
    return {
      x: Number.parseFloat(style.paddingLeft) || 0,
      y: Number.parseFloat(style.paddingTop) || 0,
    };
  }

  /// The touch-sized puck both transports shrink into, read from the same token they are built on.
  function transportSize(): number {
    if (!workspace) return 44;
    const token = Number.parseFloat(
      getComputedStyle(workspace).getPropertyValue("--control-touch"),
    );
    return Number.isFinite(token) && token > 0 ? token : 44;
  }

  function finishHistoryDrag(event: PointerEvent) {
    if (!historyDrag || event.pointerId !== historyDrag.pointerId) return;
    historyDrag = null;
    status = `Undo buttons moved to the ${chrome.historyCorner.replace("-", " ")}`;
    if (historyCorner !== settings.historyCorner) {
      changeSettings({ ...settings, historyCorner });
    }
  }

  /// Arrow keys park the pill in a corner, and Backspace hands the decision back to the layout.
  function moveHistoryWithKeyboard(event: KeyboardEvent) {
    const vertical = event.key === "ArrowUp" ? "top" : event.key === "ArrowDown" ? "bottom" : null;
    const horizontal =
      event.key === "ArrowLeft" ? "left" : event.key === "ArrowRight" ? "right" : null;
    const reset = event.key === "Backspace" || event.key === "Delete";
    if (!vertical && !horizontal && !reset) return;
    event.preventDefault();
    const current = historyCorner === "auto" ? "top-left" : historyCorner;
    const [row, column] = current.split("-") as ["top" | "bottom", "left" | "right"];
    const next: HistoryCorner = reset
      ? "auto"
      : (`${vertical ?? row}-${horizontal ?? column}` as HistoryCorner);
    historyCorner = next;
    status =
      next === "auto"
        ? "Undo buttons follow the palette again"
        : `Undo buttons moved to the ${next.replace("-", " ")}`;
    if (next !== settings.historyCorner) changeSettings({ ...settings, historyCorner: next });
  }

  function dockPaletteWithKeyboard(event: KeyboardEvent) {
    const next: PaletteDock | null =
      event.key === "ArrowUp"
        ? "top"
        : event.key === "ArrowRight"
          ? "right"
          : event.key === "ArrowDown"
            ? "bottom"
            : event.key === "ArrowLeft"
              ? "left"
              : null;
    if (!next) return;
    event.preventDefault();
    paletteDock = next;
    status = `Tool palette docked to the ${next} edge`;
    if (next !== settings.paletteDock) changeSettings({ ...settings, paletteDock: next });
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
    if (selectedShapeId) {
      shapes = shapes.filter((shape) => shape.id !== selectedShapeId);
      selectedShapeId = null;
      editingShapeId = null;
      queueCommit("Deleted shape");
      status = "Deleted the shape";
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
    if (selectedShapeId) {
      shapes = shapes.map((shape) =>
        shape.id === selectedShapeId
          ? { ...shape, x: shape.x + dx, y: shape.y + dy }
          : shape,
      );
      scheduleInkCommit("Moved shape");
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

  /**
   * Copy, cut, paste and duplicate (specs/qol-wave.md section 3). The bundle lives in the app so it
   * survives page and notebook switches; the system clipboard only carries a marker, so a paste
   * after copying something in another program does not bring back a stale Goodtype copy.
   */
  // Raw: a bundle is replaced whole, never edited in place, and may carry image bytes.
  let clipboard = $state.raw<ClipboardBundle | null>(null);
  let pasteCount = 0;

  function hasCopyableSelection(): boolean {
    return (
      selectedStrokeIds.length > 0 ||
      selectedShapeId !== null ||
      selectedTypstId !== null ||
      selectedImageId !== null
    );
  }

  async function imageBytes(url: string): Promise<{ bytes: Uint8Array; mime: string } | null> {
    try {
      const response = await fetch(url);
      const blob = await response.blob();
      return { bytes: new Uint8Array(await blob.arrayBuffer()), mime: blob.type || "image/png" };
    } catch {
      return null;
    }
  }

  async function bundleSelection(): Promise<ClipboardBundle | null> {
    const ids = new Set(selectedStrokeIds);
    const block = selectedTypstId ? typstBlocks.find((entry) => entry.id === selectedTypstId) : undefined;
    const shape = selectedShapeId ? shapes.find((entry) => entry.id === selectedShapeId) : undefined;
    const image = selectedImageId ? images.find((entry) => entry.id === selectedImageId) : undefined;
    const imageData = image ? await imageBytes(image.url) : null;
    const bundle: ClipboardBundle = {
      token: crypto.randomUUID(),
      pageId: activePageId,
      strokes: strokes.filter((stroke) => ids.has(stroke.id)),
      shapes: shape ? [shape] : [],
      blocks: block
        ? [{
            source: block.source,
            x: block.transform.x,
            y: block.transform.y,
            layoutWidthPt: block.transform.layoutWidthPt,
            heightPt:
              workspace?.querySelector<HTMLElement>(`.active-page [data-object-id="${CSS.escape(block.id)}"]`)
                ?.offsetHeight ?? 0,
            scale: block.transform.scale,
          }]
        : [],
      images:
        image && imageData
          ? [{
              ...imageData,
              alt: image.alt,
              x: image.x,
              y: image.y,
              widthPt: image.widthPt,
              heightPt: image.heightPt,
              scale: image.scale,
            }]
          : [],
    };
    return isEmptyBundle(bundle) ? null : bundle;
  }

  async function copySelection(cut = false) {
    const bundle = await bundleSelection();
    if (!bundle) {
      status = "Nothing to copy";
      return;
    }
    clipboard = bundle;
    pasteCount = 0;
    // Not awaited: without window focus the write can stay pending forever, and the copy itself
    // is already done. The marker only guards a later paste against stale content.
    void navigator.clipboard?.writeText(clipboardMarker(bundle)).catch(() => {});
    if (cut) {
      deleteSelection();
      status = "Cut to the clipboard";
    } else {
      status = "Copied";
    }
  }

  /// `at` centres the copy on a page point (the spot the context bar was opened at).
  async function duplicateSelection(at: Point | null = null) {
    const bundle = await bundleSelection();
    if (!bundle) return;
    clipboard = bundle;
    pasteCount = 0;
    await pasteBundle(bundle, at);
  }

  function freshObjectId(prefix: string): string {
    const taken = new Set([
      ...typstBlocks.map((entry) => entry.id),
      ...images.map((entry) => entry.id),
      ...shapes.map((entry) => entry.id),
      ...(activeSnapshot?.page.objects.map((object) => object.id) ?? []),
    ]);
    let number = 1;
    while (taken.has(`${prefix}-${String(number).padStart(3, "0")}`)) number += 1;
    return `${prefix}-${String(number).padStart(3, "0")}`;
  }

  /// `at` is a page point to centre the paste on (from the context menu); without it a paste
  /// steps away from its original on the same page and keeps its place on any other.
  async function pasteBundle(bundle: ClipboardBundle, at: Point | null = null) {
    pasteCount += 1;
    const shift = at ? shiftTo(bundle, at) : pasteOffset(bundle, activePageId, pasteCount);
    let zIndex = nextVisualZIndex();
    let readingOrder = nextReadingOrder();

    const newStrokes = pastedStrokes(bundle.strokes, shift, zIndex, () => crypto.randomUUID());
    zIndex += newStrokes.length;
    const newShapes = pastedShapes(bundle.shapes, shift, zIndex, readingOrder, () => crypto.randomUUID());
    zIndex += newShapes.length;
    readingOrder += newShapes.length;

    let lastObject: { kind: "typst" | "image" | "shape"; id: string } | null =
      newShapes.length > 0 ? { kind: "shape", id: newShapes.at(-1)!.id } : null;
    if (newShapes.length > 0) shapes = [...shapes, ...newShapes];

    for (const block of bundle.blocks) {
      const id = freshObjectId("typst");
      typstBlocks = [
        ...typstBlocks,
        {
          id,
          path: `blocks/${activePageId}-${id}.typ`,
          source: block.source,
          transform: {
            x: block.x + shift.dx,
            y: block.y + shift.dy,
            layoutWidthPt: block.layoutWidthPt,
            scale: block.scale,
          },
          result: null,
          zIndex: zIndex++,
          readingOrder: readingOrder++,
        },
      ];
      lastObject = { kind: "typst", id };
    }

    for (const image of bundle.images) {
      const filename = `pasted-${Date.now()}.${extensionForMime(image.mime)}`;
      let path = `assets/${filename}`;
      if (tauriAvailable && root) {
        try {
          path = await storePastedImage(root, filename, image.bytes);
        } catch (error) {
          status = `The image could not be pasted: ${message(error)}`;
          continue;
        }
      }
      const id = freshObjectId("image");
      images = [
        ...images,
        {
          id,
          path,
          url: URL.createObjectURL(new Blob([image.bytes.slice()], { type: image.mime })),
          alt: image.alt,
          x: image.x + shift.dx,
          y: image.y + shift.dy,
          widthPt: image.widthPt,
          heightPt: image.heightPt,
          scale: image.scale,
          zIndex: zIndex++,
          readingOrder: readingOrder++,
        },
      ];
      lastObject = { kind: "image", id };
    }

    if (newStrokes.length > 0) changeStrokes([...strokes, ...newStrokes], "Pasted ink");
    if (lastObject) queueCommit("Pasted");

    // The paste stays selected, ready to be dragged into place.
    selectedTypstId = lastObject?.kind === "typst" ? lastObject.id : null;
    selectedImageId = lastObject?.kind === "image" ? lastObject.id : null;
    selectedShapeId = lastObject?.kind === "shape" ? lastObject.id : null;
    editingShapeId = null;
    updateInkSelection(lastObject ? [] : newStrokes.map((stroke) => stroke.id));
    const count =
      newStrokes.length + newShapes.length + bundle.blocks.length + bundle.images.length;
    status = `Pasted ${count} item${count === 1 ? "" : "s"}`;
  }

  /**
   * The canvas context menu: right-click, or a long press with a finger. Paste lands where it was
   * opened; with something selected it also offers the selection's own actions.
   */
  let contextMenu = $state<{ x: number; y: number; at: Point | null } | null>(null);
  let longPress: { pointerId: number; x: number; y: number; timer: ReturnType<typeof setTimeout> } | null =
    null;
  const LONG_PRESS_MS = 500;
  const LONG_PRESS_SLOP_PX = 10;

  /**
   * The point on the active page nearest a viewport point, in page points: under it on the page,
   * pulled to the edge when it is in the margin, so a paste there still lands by the pointer.
   */
  function pagePointAt(clientX: number, clientY: number): Point | null {
    const frame = pageFrame?.getBoundingClientRect();
    if (!frame) return null;
    const x = Math.min(Math.max(clientX, frame.left), frame.right);
    const y = Math.min(Math.max(clientY, frame.top), frame.bottom);
    return { x: (x - frame.left) / zoom, y: (y - frame.top) / zoom };
  }

  function openContextMenu(clientX: number, clientY: number) {
    closeToolCard();
    contextMenu = { x: clientX, y: clientY, at: pagePointAt(clientX, clientY) };
  }

  function canvasContextMenu(event: MouseEvent) {
    // A pen's barrel press is a tool, handled on the ink surface, which cancels the menu there.
    if (event.defaultPrevented) return;
    const target = event.target instanceof Element ? event.target : null;
    if (!target?.closest(".page-scroll-content")) return;
    if (target.closest(".cm-editor, input, textarea, [contenteditable=true]")) return;
    event.preventDefault();
    openContextMenu(event.clientX, event.clientY);
  }

  function cancelLongPress() {
    if (longPress) clearTimeout(longPress.timer);
    longPress = null;
  }

  let contextBarElement = $state<HTMLElement>();
  let contextBarPosition = $state({ left: 0, top: 0, ready: false });

  /// The context bar is the selection toolbar, opened at the pointer instead of over the
  /// selection, so right-click and the lasso offer the same controls in the same place style.
  $effect(() => {
    const menu = contextMenu;
    const bar = contextBarElement;
    if (!menu || !bar || !workspace) {
      contextBarPosition = { left: 0, top: 0, ready: false };
      return;
    }
    const placed = placeFloatingToolbar(
      { left: menu.x, top: menu.y, right: menu.x, bottom: menu.y, width: 0, height: 0 },
      bar.getBoundingClientRect(),
      workspace.getBoundingClientRect(),
    );
    contextBarPosition = { left: placed.left, top: placed.top, ready: true };
  });

  /** Closes the bar and runs a context-bar action, handing it the page point the bar opened at. */
  function fromContext(action: (at: Point | null) => unknown) {
    return () => {
      const at = contextMenu?.at ?? null;
      contextMenu = null;
      void action(at);
    };
  }

  async function handlePaste(event: ClipboardEvent) {
    // Text fields and editors paste their own way.
    if (
      event.target instanceof Element &&
      event.target.closest(".cm-editor, input, textarea, [contenteditable=true]")
    ) {
      return;
    }
    const hasImageFile = Array.from(event.clipboardData?.files ?? []).some((item) =>
      item.type.startsWith("image/"),
    );
    if (hasImageFile) {
      await pasteImage(event);
      return;
    }
    const text = event.clipboardData?.getData("text/plain") ?? "";
    if (clipboard && (text === clipboardMarker(clipboard) || text === "")) {
      event.preventDefault();
      await pasteBundle(clipboard);
    }
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
      ...shapes.map((shape) => shape.id),
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
      readingOrder: nextReadingOrder(),
    }];
    selectedImageId = id;
    selectedTypstId = null;
    selectedShapeId = null;
    editingShapeId = null;
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
    enqueueSnapshot(label, buildSnapshot());
  }

  function enqueueSnapshot(label: string, snapshot: NotebookSnapshot) {
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

  /// Used by the Shapes tool and by draw-and-hold. A held figure keeps the pattern of the pen that
  /// drew it, so a dotted mark becomes a dotted shape; the Shapes tool has its own choice.
  function currentShapeStyle(): ShapeStyle {
    const preset = settings.penPresets[penPreset - 1];
    const strokeColor = preset?.color ?? "#1e232b";
    return {
      strokeColor,
      strokeWidthPt: preset?.widthPt ?? 1.6,
      fillColor: settings.shapeFill ? `${strokeColor}24` : null,
      opacity: 1,
      pattern: tool === "shape" ? settings.shapePattern : tool === "pen" ? inkPattern() : "solid",
      patternSpacing: tool === "shape" ? settings.shapePatternSpacing : inkPatternSpacing(),
    };
  }

  function shapeView(draft: ShapeDraft, overrides: Partial<ShapeView> = {}): ShapeView {
    return {
      id: crypto.randomUUID(),
      x: draft.x,
      y: draft.y,
      rotation: draft.rotation,
      scale: 1,
      zIndex: nextVisualZIndex(),
      readingOrder: nextReadingOrder(),
      geometry: draft.geometry,
      style: draft.style,
      ...overrides,
    };
  }

  function addShape(draft: ShapeDraft) {
    const added = shapeView(draft);
    shapes = [...shapes, added];
    status = `Added ${draft.geometry.kind}; shape tool stays active`;
    queueCommit(`Added ${draft.geometry.kind}`);
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
      shapeView(draft, { zIndex: stroke.zIndex ?? DEFAULT_INK_Z_INDEX }),
    ];
    const shapeSnapshot = buildSnapshot();
    if (tauriAvailable && root && !transactionFailed) {
      enqueueSnapshot(`Added ${stroke.tool} stroke`, rawSnapshot);
      enqueueSnapshot(`Converted held ink to ${draft.geometry.kind}`, shapeSnapshot);
    }
    status = `Converted held ink to ${draft.geometry.kind}; Undo restores the original stroke`;
  }

  function convertSelectedInkToShape() {
    if (selectedStrokeIds.length !== 1) return;
    const stroke = strokes.find((candidate) => candidate.id === selectedStrokeIds[0]);
    if (!stroke || stroke.groupId) {
      status = "Ungroup this ink before converting it to a shape";
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
      { snapDegrees: settings.shapeConstrain ? 15 : 0 },
    )?.draft;
    if (!draft) {
      status = "That stroke is not close enough to a line, rectangle, ellipse, or simple curve";
      return;
    }
    strokes = strokes.filter((candidate) => candidate.id !== stroke.id);
    shapes = [
      ...shapes,
      shapeView(draft, {
        zIndex: stroke.zIndex ?? DEFAULT_INK_Z_INDEX,
        readingOrder: nextReadingOrder(),
      }),
    ];
    updateInkSelection([]);
    queueCommit(`Converted ink to ${draft.geometry.kind}`);
    status = `Converted ink to ${draft.geometry.kind}`;
  }

  /// A finished drag is one decision and is written straight away. A held arrow or bracket key
  /// is one continuous adjustment, and settles first so a nudge across the page is a single undo
  /// step rather than one transaction per repeat.
  /// The eraser takes a whole shape, the way it takes a whole stroke.
  function eraseShape(id: string) {
    if (!shapes.some((shape) => shape.id === id)) return;
    shapes = shapes.filter((shape) => shape.id !== id);
    if (selectedShapeId === id) {
      selectedShapeId = null;
      editingShapeId = null;
    }
    scheduleInkCommit("Erased shape");
    status = "Erased the shape";
  }

  function changeShape(next: ShapeView, commit: ShapeEditCommit = "now") {
    if (!shapes.some((shape) => shape.id === next.id)) return;
    shapes = shapes.map((shape) => (shape.id === next.id ? next : shape));
    if (commit === "settled") scheduleInkCommit("Updated shape");
    else queueCommit("Updated shape");
  }

  function changeImage(id: string, next: Partial<Pick<ImageState, "x" | "y" | "scale">>) {
    if (!images.some((image) => image.id === id)) return;
    images = images.map((image) => (image.id === id ? { ...image, ...next } : image));
    queueCommit("Updated image");
  }

  function selectedObjectId(): string | null {
    return selectedTypstId ?? selectedImageId ?? selectedShapeId;
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

  function scheduleSelectionToolbar(attempt = 0) {
    if (selectionToolbarFrame) cancelAnimationFrame(selectionToolbarFrame);
    selectionToolbarFrame = requestAnimationFrame(() => {
      selectionToolbarFrame = undefined;
      // A block just created or pasted is selected before it is on the page: its element appears
      // a few frames later. Keep looking briefly instead of leaving the toolbar hidden for good.
      if (!positionSelectionToolbar() && attempt < 30) scheduleSelectionToolbar(attempt + 1);
    });
  }

  const onResizeToolbar = () => scheduleSelectionToolbar();

  /** Places the toolbar; false when the selection's element is not on the page yet. */
  function positionSelectionToolbar(): boolean {
    if (!workspace || !selectionToolbarElement) return false;
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
    if (!anchor) return false;
    const boundary = workspace.getBoundingClientRect();
    if (
      anchor.right < boundary.left ||
      anchor.left > boundary.right ||
      anchor.bottom < boundary.top ||
      anchor.top > boundary.bottom
    ) {
      selectionToolbarPosition = { left: 0, top: 0, ready: false };
      return true;
    }
    const placed = placeFloatingToolbar(
      anchor,
      selectionToolbarElement.getBoundingClientRect(),
      boundary,
    );
    selectionToolbarPosition = { left: placed.left, top: placed.top, ready: true };
    return true;
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
    shapes = shapes.map((shape) => {
      const object = fields.get(shape.id);
      return object
        ? { ...shape, zIndex: object.zIndex, readingOrder: object.readingOrder }
        : shape;
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
    if (settings.undoScope === "notebook") {
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

  function queueHistory(command: "undo_notebook" | "redo_notebook", label: string, announce = false) {
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
          if (announce) announceTouchHistory(command === "undo_notebook" ? "Undo" : "Redo");
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
    announce = false,
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
          if (announce) announceTouchHistory(command === "undo_page_structure" ? "Undo" : "Redo");
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
    if (event.key === "Escape" && (toolCard || shapeCard)) {
      event.preventDefault();
      closeToolCard();
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
      selectedShapeId = null;
      editingShapeId = null;
      directObjectInput = false;
      return;
    }
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") {
      event.preventDefault();
      searchOpen = true;
      return;
    }
    if ((event.ctrlKey || event.metaKey) && !event.shiftKey && !event.altKey) {
      const key = event.key.toLowerCase();
      if ((key === "c" || key === "x" || key === "d") && hasCopyableSelection()) {
        event.preventDefault();
        if (key === "d") void duplicateSelection();
        else void copySelection(key === "x");
        return;
      }
    }
    if (
      (event.ctrlKey || event.metaKey) &&
      event.shiftKey &&
      (event.key === "PageUp" || event.key === "PageDown")
    ) {
      event.preventDefault();
      void moveActivePage(event.key === "PageUp" ? -1 : 1);
      return;
    }
    if (!event.ctrlKey && !event.metaKey && !event.altKey) {
      if (event.key === "Enter" && selectedShapeId) {
        event.preventDefault();
        editingShapeId = editingShapeId === selectedShapeId ? null : selectedShapeId;
        status = editingShapeId ? "Editing shape anchor points" : "Shape selected";
        return;
      }
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
        "6": () => activateTool("shape"),
        t: addTypstBlock,
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

  /**
   * Save the live notebook, let Rust move the filesystem entries, then retarget every affected
   * tab and reload the active one from its new root without leaving the library surface.
   */
  async function moveLibrarySelection(
    paths: string[],
    destination: string,
  ): Promise<string | null> {
    if (activeSnapshot && !(await persist())) {
      throw new Error("The open notebook could not be saved, so nothing was moved");
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
      const reopened = await openNotebookAt(nextRoot, {
        skipCurrentPersist: true,
        stayInLibrary: true,
      });
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
        return "Verschoben; der aktive Tab konnte am neuen Ort nicht neu geladen werden";
      }
    } else if (!(await rememberNotebookSession())) {
      return "Verschoben; die neue Tab-Position konnte nicht gespeichert werden";
    }

    return result.warning;
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

  /**
   * The zoom the next frame will show, with the point that must stay under the cursor or fingers.
   *
   * A trackpad delivers several wheel events per frame, and each used to set the zoom, lay out the
   * whole page stack, and correct the scroll only on the *following* frame. The page was drawn
   * once at the new size in the old scroll position, which is the jump. Now events only record a
   * target; one frame applies it, lays out once, and corrects the scroll before anything paints.
   */
  let pendingZoom: { zoom: number; clientX: number; clientY: number; fixedPagePoint?: Point } | null =
    null;
  let pendingZoomFrame = 0;

  /** Where zoom is heading, so events arriving within one frame build on each other. */
  function targetZoom() {
    return pendingZoom?.zoom ?? zoom;
  }

  function zoomAt(next: number, clientX: number, clientY: number, fixedPagePoint?: Point) {
    if (!pageViewport || !pageFrame) return;
    automaticPageFocusLocked = true;
    pendingZoom = { zoom: clampZoom(next), clientX, clientY, fixedPagePoint };
    if (!pendingZoomFrame) pendingZoomFrame = requestAnimationFrame(applyPendingZoom);
  }

  function applyPendingZoom() {
    pendingZoomFrame = 0;
    const pending = pendingZoom;
    pendingZoom = null;
    if (!pending || !pageViewport || !pageFrame) return;
    const viewport = pageViewport;
    const frame = pageFrame;
    const startedAt = performance.now();
    const before = frame.getBoundingClientRect();
    const pagePoint = pending.fixedPagePoint ?? {
      x: (pending.clientX - before.left) / zoom,
      y: (pending.clientY - before.top) / zoom,
    };
    // Apply synchronously so the layout below already has the new size; the scroll is then fixed
    // in the same frame, before the browser paints.
    flushSync(() => {
      zoom = pending.zoom;
    });
    const after = frame.getBoundingClientRect();
    viewport.scrollLeft += after.left + pagePoint.x * zoom - pending.clientX;
    viewport.scrollTop += after.top + pagePoint.y * zoom - pending.clientY;
    if (collectMetrics) zoomFrameMs = performance.now() - startedAt;
  }

  /**
   * Pull past the last page to add one (specs/qol-wave.md section 1).
   *
   * A finger pull is measured from the pan and decided on release; a wheel or trackpad pull fills
   * as it scrolls and adds the moment the ring is full, because a wheel has no release. After a
   * wheel add the pull stays disarmed until the wheel goes quiet, so a trackpad's momentum tail
   * cannot add a second page behind the first.
   */
  let pull = $state<{ raw: number; source: PullSource } | null>(null);
  let pullIdleTimer: ReturnType<typeof setTimeout> | undefined;
  let pullDisarmed = false;
  const pullFill = $derived(pull ? pullProgress(pull.raw, pull.source) : 0);
  const pullShift = $derived(pull && !settings.reducedMotion ? rubberBand(pull.raw) : 0);
  const pullPaper = $derived.by(() => {
    const paper = notebookManifest?.defaultPage;
    return paper && paper.background.kind !== "pdf" ? paper : null;
  });

  function canPull(): boolean {
    return settings.pullToAddPage && tauriAvailable && !busy && notebookManifest !== null && pageOpen;
  }

  function pullFromWheel(event: WheelEvent) {
    const viewport = pageViewport;
    if (!viewport) return;
    const delta = wheelPixels(event.deltaY, event.deltaMode, viewport.clientHeight);
    clearTimeout(pullIdleTimer);
    pullIdleTimer = setTimeout(() => {
      pullDisarmed = false;
      if (pull?.source === "wheel") pull = null;
    }, PULL_WHEEL_IDLE_MS);
    if (pullDisarmed || !canPull()) return;
    if (pull?.source === "wheel") {
      const raw = pull.raw + delta;
      pull = raw > 0 ? { raw, source: "wheel" } : null;
    } else if (
      delta > 0 &&
      atScrollEnd(viewport.scrollTop, viewport.clientHeight, viewport.scrollHeight)
    ) {
      pull = { raw: delta, source: "wheel" };
    } else {
      return;
    }
    event.preventDefault();
    if (pull && pullProgress(pull.raw, "wheel") >= 1) {
      pull = null;
      pullDisarmed = true;
      void addPageFromPull();
    }
  }

  function releasePull(add: boolean) {
    const ready = pull !== null && pullProgress(pull.raw, pull.source) >= 1;
    pull = null;
    if (add && ready) void addPageFromPull();
  }

  async function addPageFromPull() {
    await addPage({ kind: "last" }, pullPaper?.background ?? null, pullPaper?.geometry ?? null);
  }

  function wheelZoom(event: WheelEvent) {
    stopTouchInertia();
    if (!event.ctrlKey) {
      automaticPageFocusLocked = false;
      pullFromWheel(event);
      return;
    }
    event.preventDefault();
    zoomAt(targetZoom() * Math.exp(-event.deltaY * 0.01), event.clientX, event.clientY);
  }

  function currentToolLabel() {
    if (tool === "pen") return `Pen ${penPreset}`;
    if (tool === "select") return "Ink selection";
    return tool[0].toUpperCase() + tool.slice(1);
  }

  /// Read from the live preset, never written by hand. These used to be three fixed strings, so
  /// the strip kept claiming "0.35 mm · graphite" after the writer had changed both, and the
  /// widths it named did not match the defaults either.
  function nibDetail(preset: PenPreset) {
    const pattern = preset.pattern && preset.pattern !== "solid" ? ` · ${preset.pattern}` : "";
    return `${(preset.widthPt * MM_PER_PT).toFixed(2)} mm · ${colorName(preset.color).toLowerCase()}${pattern}`;
  }

  function currentToolDetail() {
    if (tool === "pen") return nibDetail(settings.penPresets[penPreset - 1]);
    if (tool === "highlighter") return nibDetail(settings.highlighter);
    if (tool === "eraser") return "whole strokes";
    if (tool === "shape") return settings.shapeKind.replace("ellipse", "circle · ellipse");
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

  /// The highlighter has no nib picker, so it must not inherit a taper from whichever nib type
  /// its stored preset happens to carry; whether its ends narrow is its own explicit switch.
  function inkTaper() {
    if (tool === "highlighter") return settings.highlighterTaper ? HIGHLIGHTER_TAPER : 0;
    return penType(activePreset().type).taper;
  }

  function inkPattern(): StrokePattern {
    return tool === "pen" ? (activePreset().pattern ?? "solid") : "solid";
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
    return `<svg xmlns="http://www.w3.org/2000/svg" width="${widthPt}pt" height="64pt"><text x="8" y="32" font-family="sans-serif" font-size="14">${escaped}</text></svg>`;
  }
</script>

<main class="workspace-app" onpointerdowncapture={closeObjectSelection} onwheel={wheelZoom}>
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
        onMove={moveLibrarySelection}
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
      >
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
      </button>
    </div>
    <div class="command-actions">
      {#if pageOpen}
        <button class="icon-button" type="button" aria-label="Export PDF" title="Export PDF" onclick={exportPdf} disabled={busy}>
          <!-- Arrow up and out of the tray: the page leaves Goodtype. Down read as "download",
               which is the one thing this button never does. -->
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 15V4m0 0-4 4m4-4 4 4M5 17v2a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-2" /></svg>
        </button>
      {:else}
        <button class="text-action" type="button" onclick={reopen} disabled={busy}>Reopen notebook</button>
      {/if}
      {#if pageOpen}
        <div class="add-page-anchor">
          <button class="icon-button" class:active={addPageOpen} type="button" aria-label="Add page" aria-expanded={addPageOpen} title="Add page" disabled={busy} onclick={() => (addPageOpen = !addPageOpen)}>
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M13 3.5H7a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h6"></path><path d="M18 9.5v9"></path><path d="M13.5 14h9"></path></svg>
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
              geometry={addPageGeometry}
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
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="7"></circle><path d="m20 20-3.6-3.6"></path></svg>
      </button>
      <button class="icon-button" class:active={sideEditorOpen} type="button" aria-label="Page text and Typst source view" aria-pressed={sideEditorOpen} title="Editor view (Ctrl+Shift+E)" onclick={toggleSideEditor}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3.5" y="4.5" width="17" height="15" rx="2"></rect><path d="M10 4.5v15"></path></svg>
      </button>
      <button class="icon-button" class:active={moreOpen} type="button" aria-label="More notebook actions" aria-expanded={moreOpen} data-preserve-selection onclick={() => (moreOpen = !moreOpen)}>
        <svg class="solid-icon" viewBox="0 0 24 24" aria-hidden="true"><circle cx="5" cy="12" r="1.7"></circle><circle cx="12" cy="12" r="1.7"></circle><circle cx="19" cy="12" r="1.7"></circle></svg>
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
        pageNumber={sideEditorPageNumber}
        hasAnyBlock={Boolean(pageTypst) || typstBlocks.length > 0}
        {root}
        dock={settings.sideEditorDock}
        width={settings.sideEditorWidth}
        diagnostics={sideEditorBlock?.result?.diagnostics ?? []}
        pageText={sideEditorPageText}
        pageTextLineWrap={settings.pageTextLineWrap}
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
        onPageTextLineWrapChange={(pageTextLineWrap) =>
          changeSettings({ ...settings, pageTextLineWrap })}
      />
    {/if}
    <section
      class="workspace-surround"
      bind:this={workspace}
      aria-label="Fixed page workspace"
      onpointerdowncapture={workspacePointerDown}
      oncontextmenu={canvasContextMenu}
      onpointermovecapture={workspacePointerMove}
      onpointerupcapture={workspacePointerEnd}
      onpointercancelcapture={workspacePointerEnd}
    >
      <div class="page-scroll-content" bind:this={pageViewport} onscroll={() => scheduleSelectionToolbar()}>
        <div
          class="page-pan-field"
          class:pulling={pull !== null}
          style:transform={pullShift > 0 ? `translateY(${-pullShift}px)` : undefined}
        >
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
            <!-- The arrows sit on every page rather than only the one in view. They used to be
                 mounted when a page became active, which made them pop in mid-scroll and — because
                 this row is anchored by its right edge — shove the page number leftwards as they
                 arrived. Two inert buttons cost less than that, and only the page you are on can
                 be moved, so elsewhere they are disabled rather than absent. -->
            <div class="page-number" class:idle={!active}>
              <span>Page {index + 1}</span>
              {#if pageCount > 1}
                <button
                  type="button"
                  aria-label={`Move page ${index + 1} up`}
                  title={active ? "Move page up (Ctrl+Shift+Page Up)" : "Scroll to this page to move it"}
                  disabled={!active || index === 0 || busy}
                  onclick={() => void moveActivePage(-1)}
                >&uarr;</button>
                <button
                  type="button"
                  aria-label={`Move page ${index + 1} down`}
                  title={active ? "Move page down (Ctrl+Shift+Page Down)" : "Scroll to this page to move it"}
                  disabled={!active || index === pageCount - 1 || busy}
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
                    shapes={active ? shapes : shapeViewsFromSnapshot(entry.snapshot!)}
                    results={active ? activeResults : (neighborResults[entry.id] ?? {})}
                    strokes={active ? strokes : neighborStrokesFor(entry)}
                    newStrokeZIndex={newInkZIndex(
                      active ? [...typstBlocks, ...images, ...shapes].map((object) => object.zIndex)
                        : entry.snapshot!.page.objects.map((object) => object.zIndex),
                      active ? strokes : neighborStrokesFor(entry),
                      tool === "highlighter" && settings.highlighterBehindInk,
                    )}
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
                    taperLengthWidths={activePreset().taperLengthWidths ?? 3}
                    pattern={inkPattern()}
                    patternSpacing={inkPatternSpacing()}
                    opacity={inkOpacity()}
                    straighten={inkStraighten()}
                    shapeKind={settings.shapeKind}
                    shapeStyle={currentShapeStyle()}
                    shapeConstrain={settings.shapeConstrain}
                    drawAndHoldShapes={settings.drawAndHoldShapes}
                    heldLineMinimumPt={settings.heldLineMinimumPt}
                    eraseRadiusPt={ERASER_RADIUS_PT[settings.eraserSize]}
                    eraserReach={settings.eraserReach}
                    calibration={settings.calibration}
                    {pointerMapping}
                    onEraseEnd={returnFromEraser}
                    onEraserEngaged={() => activateTool("eraser")}
                    directObjectInput={active && directObjectInput}
                    selectedBlockId={active ? selectedTypstId : null}
                    selectedImageId={active ? selectedImageId : null}
                    selectedShapeId={active ? selectedShapeId : null}
                    shapesGrabbable={tool !== "eraser"}
                    editingShapeId={active ? editingShapeId : null}
                    onCompile={(id, request) =>
                      active
                        ? compileTypst(id, request)
                        : compileNeighborTypst(entry.id, id, request)}
                    onSourceChange={(id, source) => updateTypstSource(id, source)}
                    onTransform={(id, transform) => updateTypstTransform(id, transform)}
                    onSelectBlock={(id) => {
                      selectedTypstId = id;
                      selectedImageId = null;
                      selectedShapeId = null;
                      editingShapeId = null;
                    }}
                    onDeselectBlock={() => (selectedTypstId = null)}
                    onSelectImage={(id) => {
                      selectedImageId = id;
                      selectedTypstId = null;
                      selectedShapeId = null;
                      editingShapeId = null;
                    }}
                    onMoveImage={(id, position) => changeImage(id, position)}
                    onScaleImage={(id, scale) => changeImage(id, { scale })}
                    onSelectShape={(id) => {
                      selectedShapeId = id;
                      selectedImageId = null;
                      selectedTypstId = null;
                      updateInkSelection([]);
                    }}
                    onChangeShape={active ? changeShape : undefined}
                    onEditShape={(id, editing) => {
                      selectedShapeId = id;
                      editingShapeId = editing ? id : null;
                      status = editing ? "Editing shape anchor points" : "Shape selected";
                    }}
                    onStrokeFinalized={(stroke) =>
                      active
                        ? addStroke(stroke)
                        : commitNeighborInk(entry.id, [...neighborStrokesFor(entry), stroke], "Added ink")}
                    onEraseShape={active ? eraseShape : undefined}
                    onShapeFinalized={active ? addShape : undefined}
                    onInkShapeRecognized={active ? promoteHeldInk : undefined}
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

      {#if touchHistoryToast}
        {#key touchHistoryToast}
          <div class="touch-history-toast" class:below-palette={paletteDock === "top"}
            class:reduced-motion={settings.reducedMotion} role="status" aria-live="polite">
            {touchHistoryToast.label}
          </div>
        {/key}
      {/if}

      {#if pull}
        <!-- Above the palette when it is docked at the bottom, so the bar never hides the ring. -->
        <div
          class="pull-badge"
          class:ready={pullFill >= 1}
          class:over-palette={paletteDock === "bottom"}
          style:opacity={Math.min(1, pullFill * 3)}
          role="status"
          aria-live="polite"
        >
          <svg viewBox="0 0 40 40" aria-hidden="true">
            <circle class="track" cx="20" cy="20" r="17" />
            <circle
              class="fill"
              cx="20"
              cy="20"
              r="17"
              pathLength="100"
              stroke-dasharray={`${pullFill * 100} 100`}
              transform="rotate(-90 20 20)"
            />
            {#if pullFill >= 1}
              <path class="mark" d="M13 20.5l5 5 9-10" />
            {:else}
              <path class="mark" d="M20 13v14M13 20h14" />
            {/if}
          </svg>
          <span>
            {pullFill >= 1
              ? `Release to add · ${pullPaper ? (pullPaper.background.kind === "template" ? pullPaper.background.template.name : "Plain") : "new page"}`
              : "Pull to add a page"}
          </span>
        </div>
      {/if}

      <!-- One grid owns the canvas chrome. Corner controls reserve their intrinsic row/column,
           so the palette can never be positioned through them by an unrelated fixed offset. -->
      <div bind:this={workspaceChrome} class="workspace-chrome">
      <div
        bind:this={historyElement}
        class:dragging={historyDrag !== null}
        class:reduced-motion={settings.reducedMotion}
        class="history-pill"
        data-corner={chrome.historyCorner}
        style:left={historyDrag ? `${historyX}px` : null}
        style:top={historyDrag ? `${historyY}px` : null}
        style:grid-column={historyDrag ? null : gridSpan(chrome.history.columnStart, chrome.history.columnEnd)}
        style:grid-row={historyDrag ? null : gridSpan(chrome.history.rowStart, chrome.history.rowEnd)}
        style:justify-self={historyDrag ? null : chrome.history.justify}
        style:align-self={historyDrag ? null : chrome.history.align}
        role="toolbar"
        tabindex="0"
        aria-label="Page history — drag between the buttons to move, or use the arrow keys"
        onpointerdown={beginHistoryDrag}
        onpointermove={moveHistory}
        onpointerup={finishHistoryDrag}
        onpointercancel={finishHistoryDrag}
        onkeydown={moveHistoryWithKeyboard}
      >
        <button type="button" aria-label="Undo" title="Undo (Ctrl+Z)" onclick={undo} disabled={busy || pendingTransactions > 0 || !canUndo}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m9 7-5 5 5 5M4 12h10a6 6 0 0 1 0 12" /></svg>
        </button>
        <span class="grip" aria-hidden="true"></span>
        <span class="transport" aria-hidden="true">
          <svg viewBox="0 0 24 24"><path d="m9 7-5 5 5 5M4 12h10a6 6 0 0 1 0 12" /></svg>
        </span>
        <button type="button" aria-label="Redo" title="Redo (Ctrl+Y)" onclick={redo} disabled={busy || pendingTransactions > 0 || !canRedo}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m15 7 5 5-5 5M20 12H10a6 6 0 0 0 0 12" /></svg>
        </button>
      </div>

      <nav
        bind:this={paletteElement}
        class:dragging={paletteDrag !== null}
        class:reduced-motion={settings.reducedMotion}
        class:horizontal={paletteDrag === null && (paletteDock === "top" || paletteDock === "bottom")}
        class:dock-top={paletteDock === "top" && paletteDrag === null}
        class:dock-right={paletteDock === "right" && paletteDrag === null}
        class:dock-bottom={paletteDock === "bottom" && paletteDrag === null}
        class:dock-left={paletteDock === "left" && paletteDrag === null}
        class="instrument-palette"
        data-dock={paletteDock}
        style:grid-column={paletteDrag ? null : gridSpan(chrome.palette.columnStart, chrome.palette.columnEnd)}
        style:grid-row={paletteDrag ? null : gridSpan(chrome.palette.rowStart, chrome.palette.rowEnd)}
        style:justify-self={paletteDrag ? null : chrome.palette.justify}
        style:align-self={paletteDrag ? null : chrome.palette.align}
        style:left={paletteDrag ? `${paletteX}px` : null}
        style:top={paletteDrag ? `${paletteY}px` : null}
        aria-label="Canvas tools"
      >
        {#if eraserEditor}
          <PaletteEditor title="Eraser" anchor={eraserEditor} dock={paletteDock}
            onClose={closeEraserEditor} onDismiss={closeToolCard}>
            <EraserPanel highlightsOnly={settings.eraserReach === "highlighter"} returns={settings.eraserReturns}
              onHighlightsChange={(enabled) => changeSettings({ ...settings, eraserReach: enabled ? "highlighter" : "everything" })}
              onReturnsChange={(eraserReturns) => changeSettings({ ...settings, eraserReturns })} />
          </PaletteEditor>
        {/if}
        {#if paletteEditor}
          {#key paletteEditor.anchor}
            <PaletteEditor title={paletteEditor.kind === "width" ? "Stroke" : paletteEditor.adding ? "Add colour" : "Colour"}
              anchor={paletteEditor.anchor} dock={paletteDock} onClose={closePaletteEditor} onDismiss={closeToolCard}>
              {#if paletteEditor.kind === "width"}
                {@const penLike = tool === "pen" || restyling}
                <WidthPanel embedded widthPt={Number.isFinite(activeWidth) ? activeWidth : settings.penWidths[0]} kind={tool === "highlighter" && !restyling ? "highlighter" : "pen"}
                  minimumMm={WIDTH_BOUNDS_MM[tool === "highlighter" && !restyling ? "highlighter" : "pen"].minimum}
                  maximumMm={WIDTH_BOUNDS_MM[tool === "highlighter" && !restyling ? "highlighter" : "pen"].maximum}
                  pattern={restyling ? selectionStyle.pattern : inkPattern()}
                  onPatternChange={penLike ? setActivePattern : undefined}
                  spacing={restyling ? selectionStyle.spacing : inkPatternSpacing()}
                  onSpacingChange={penLike ? setActiveSpacing : undefined}
                  canRemove={!restyling && paletteEditor.index >= 0 && canRemoveWidth(activeWidthChips)}
                  onCommit={(width) => { if (!paletteEditor) return; if (restyling) setActiveWidth(width); else if (paletteEditor.index < 0) addWidth(width); else editWidth(paletteEditor.index, width); closePaletteEditor(); }}
                  onAdd={!restyling && activeWidthChips.length < MAX_WIDTHS ? (width) => { addWidth(width); closePaletteEditor(); } : undefined}
                  onRemove={() => { if (paletteEditor) removeWidth(paletteEditor.index); closePaletteEditor(); }}
                  onClose={closePaletteEditor} />
              {:else}
                <ColorPanel embedded value={activeInkColor} recent={settings.recentColors}
                  mode={paletteEditor.adding ? "add" : "edit"}
                  canRemove={paletteEditor.index >= 0 && activeColorChips.length > 1}
                  onPick={(color) => { if (!paletteEditor) return; if (paletteEditor.adding) addSwatch(color); else if (paletteEditor.index >= 0) editSwatch(paletteEditor.index, color); else setActiveColor(color); closePaletteEditor(); }}
                  onChange={paletteEditor.adding ? undefined : (color) => { if (!paletteEditor) return; if (paletteEditor.index >= 0) editSwatch(paletteEditor.index, color); else setActiveColor(color); }}
                  onRemove={() => { if (!paletteEditor) return; const index = paletteEditor.index; removeSwatch(index); setActiveColor(activeColorChips[Math.min(index, activeColorChips.length - 1)]); closePaletteEditor(); }}
                  onClose={closePaletteEditor} />
              {/if}
            </PaletteEditor>
          {/key}
        {/if}
        {#if shapeCard}
          <div class="palette-panel-anchor card" style:--anchor={`${shapeCard.anchor}px`}>
            <ShapeCard
              dock={paletteDock}
              kind={settings.shapeKind}
              fill={settings.shapeFill}
              constrain={settings.shapeConstrain}
              drawAndHold={settings.drawAndHoldShapes}
              minimumLineLengthPt={settings.heldLineMinimumPt}
              onMinimumLineLengthChange={(heldLineMinimumPt) => changeSettings({ ...settings, heldLineMinimumPt })}
              strokeColor={currentShapeStyle().strokeColor}
              strokeWidthPt={currentShapeStyle().strokeWidthPt}
              pattern={settings.shapePattern}
              onPatternChange={(shapePattern) => changeSettings({ ...settings, shapePattern })}
              spacing={settings.shapePatternSpacing}
              onSpacingChange={(shapePatternSpacing) =>
                changeSettings({ ...settings, shapePatternSpacing })}
              onKindChange={(shapeKind) => changeSettings({ ...settings, shapeKind })}
              onFillChange={(shapeFill) => changeSettings({ ...settings, shapeFill })}
              onConstrainChange={(shapeConstrain) =>
                changeSettings({ ...settings, shapeConstrain })}
              onDrawAndHoldChange={(drawAndHoldShapes) =>
                changeSettings({ ...settings, drawAndHoldShapes })}
              onClose={closeToolCard}
            />
          </div>
        {/if}
        {#if toolCard && toolCardPreset}
          <div class="palette-panel-anchor card" style:--anchor={`${toolCard.anchor}px`}>
            <ToolCard
              dock={paletteDock}
              tool={toolCard.kind}
              label={toolCard.kind === "highlighter" ? "Highlighter" : `Pen ${toolCard.slot}`}
              preset={toolCardPreset}
              smoothing={settings.calibration.smoothing}
              opacity={settings.highlighterOpacity}
              straighten={settings.highlighterStraighten}
              behindInk={settings.highlighterBehindInk}
              taperEnds={settings.highlighterTaper}
              onChange={updateToolPreset}
              onSmoothingChange={(smoothing) =>
                changeSettings({ ...settings, calibration: { ...settings.calibration, smoothing } })}
              onOpacityChange={(highlighterOpacity) =>
                changeSettings({ ...settings, highlighterOpacity })}
              onStraightenChange={(highlighterStraighten) =>
                changeSettings({ ...settings, highlighterStraighten })}
              onBehindInkChange={(highlighterBehindInk) =>
                changeSettings({ ...settings, highlighterBehindInk })}
              onTaperEndsChange={(highlighterTaper) => changeSettings({ ...settings, highlighterTaper })}
              onClose={closeToolCard}
            />
          </div>
        {/if}
        <button
          class="palette-grip"
          type="button"
          aria-label="Move tool palette"
          aria-keyshortcuts="ArrowUp ArrowRight ArrowDown ArrowLeft"
          title="Drag to move · Arrow keys dock"
          onpointerdown={beginPaletteDrag}
          onpointermove={movePalette}
          onpointerup={finishPaletteDrag}
          onpointercancel={finishPaletteDrag}
          onlostpointercapture={finishPaletteDrag}
          onkeydown={dockPaletteWithKeyboard}
        >
          <span class="palette-grip-dots" aria-hidden="true">
            <i></i><i></i><i></i><i></i><i></i><i></i>
          </span>
          <svg class="palette-transport-icon" viewBox="0 0 24 24" aria-hidden="true">
            <path d="M12 3.5a8.5 8.5 0 1 0 0 17h1.2a1.8 1.8 0 0 0 1.1-3.2 1.8 1.8 0 0 1 1.1-3.2h1.7A3.4 3.4 0 0 0 20.5 10 8.5 8.5 0 0 0 12 3.5Z" />
            <circle cx="7.8" cy="9" r="1" />
            <circle cx="11.4" cy="6.9" r="1" />
            <circle cx="15.4" cy="8.1" r="1" />
          </svg>
        </button>
        <div class="palette-primary">
          <PaletteTools
            {settings}
            activeCommands={activePaletteCommands}
            expandedCommand={expandedPaletteCommand}
            horizontal={paletteDock === "top" || paletteDock === "bottom"}
            onActivate={(command, tile) => paletteCommands[command](tile)}
          />

          {#if paletteContext}
            <PalettePocket
              kind={paletteContext}
              horizontal={paletteDock === "top" || paletteDock === "bottom"}
              widths={activeWidthChips}
              {activeWidth}
              colors={activeColorChips}
              activeColor={activeInkColor}
              opacity={tool === "highlighter" ? settings.highlighterOpacity : 1}
              eraserSizes={ERASER_SIZE_OPTIONS}
              eraserSize={settings.eraserSize}
              pattern={restyling ? selectionStyle.pattern : inkPattern()}
              patternOf={tool === "pen" || restyling ? (width) => patternForWidth(settings.penWidthPatterns, width) : undefined}
              tag={restyling ? "Selection" : undefined}
              matchPattern={restyling}
              expandedEditor={paletteEditor?.adding ? null : paletteEditor?.kind ?? null}
              addingColor={paletteEditor?.adding ?? false}
              onPickWidth={(width) => { closeToolCard(); setActiveWidth(width); }}
              onPickColor={(color) => { closeToolCard(); setActiveColor(color); }}
              onEditWidth={(index, tile) => openPaletteEditor("width", index, tile)}
              onEditColor={(index, tile) => openPaletteEditor("color", index, tile)}
              onPickEraserSize={setEraserSize}
              canAddColor={activeColorChips.length < MAX_SWATCHES}
              onAddColor={(tile) => openPaletteEditor("color", -1, tile, true)}
            />
          {/if}
        </div>

      </nav>

      {#if contextMenu}
        <div class="context-actions" use:dismissable={{ onClose: () => (contextMenu = null), swallow: true }}>
          {#if selectedStrokeIds.length > 0 || groupedStrokeIds.length > 0}
            <SelectionActions
              bind:element={contextBarElement}
              subject="ink"
              left={contextBarPosition.left}
              top={contextBarPosition.top}
              ready={contextBarPosition.ready}
              canMoveBack={canMoveInkVisual(-1)}
              canMoveForward={canMoveInkVisual(1)}
              onMove={(direction) => fromContext(() => changeInkVisualOrder(direction))()}
              onCut={fromContext(() => copySelection(true))}
              onCopy={fromContext(() => copySelection())}
              onPaste={fromContext((at) => clipboard && pasteBundle(clipboard, at))}
              pasteDisabled={!clipboard}
              onDuplicate={fromContext(duplicateSelection)}
              onDelete={fromContext(deleteSelection)}
            />
          {:else if selectedImageId || selectedTypstId || selectedShapeId}
            <SelectionActions
              bind:element={contextBarElement}
              subject={selectedImageId ? "image" : selectedShapeId ? "shape" : "Typst block"}
              left={contextBarPosition.left}
              top={contextBarPosition.top}
              ready={contextBarPosition.ready}
              canMoveBack={canMoveVisual(-1)}
              canMoveForward={canMoveVisual(1)}
              onMove={(direction) => fromContext(() => changeVisualOrder(direction < 0 ? "backward" : "forward"))()}
              onCut={fromContext(() => copySelection(true))}
              onCopy={fromContext(() => copySelection())}
              onPaste={fromContext((at) => clipboard && pasteBundle(clipboard, at))}
              pasteDisabled={!clipboard}
              onDuplicate={fromContext(duplicateSelection)}
              onDelete={fromContext(deleteSelection)}
            />
          {:else}
            <SelectionActions
              bind:element={contextBarElement}
              subject="page"
              left={contextBarPosition.left}
              top={contextBarPosition.top}
              ready={contextBarPosition.ready}
              onPaste={fromContext((at) => clipboard && pasteBundle(clipboard, at))}
              pasteDisabled={!clipboard}
            />
          {/if}
        </div>
      {:else if selectedStrokeIds.length > 0 || groupedStrokeIds.length > 0}
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
          onConvert={selectedStrokeIds.length === 1 ? convertSelectedInkToShape : undefined}
          onCopy={() => void copySelection()}
          onDuplicate={() => void duplicateSelection()}
          onDelete={selectedStrokeIds.length > 0 ? () => void deleteSelection() : undefined}
        />
      {:else if selectedImageId || selectedTypstId || selectedShapeId}
        <SelectionActions
          bind:element={selectionToolbarElement}
          subject={selectedImageId ? "image" : selectedShapeId ? "shape" : "Typst block"}
          left={selectionToolbarPosition.left}
          top={selectionToolbarPosition.top}
          ready={selectionToolbarPosition.ready}
          canMoveBack={canMoveVisual(-1)}
          canMoveForward={canMoveVisual(1)}
          onMove={(direction) => changeVisualOrder(direction < 0 ? "backward" : "forward")}
          onCopy={() => void copySelection()}
          onDuplicate={() => void duplicateSelection()}
          onDelete={() => void deleteSelection()}
        />
      {/if}

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
    <!-- Zoom reads and is changed in one place. It used to be a floating pill over the canvas
         *and* a number down here, so neither looked like the answer. -->
    <span class="zoom-group">
      <button type="button" aria-label="Zoom out" onclick={() => changeZoom(zoom / ZOOM_STEP)}>−</button>
      <button type="button" class="zoom-value" aria-label="Reset zoom to 100%" title="Reset zoom" onclick={() => changeZoom(1)}>{Math.round(zoom * 100)}%</button>
      <button type="button" aria-label="Zoom in" onclick={() => changeZoom(zoom * ZOOM_STEP)}>+</button>
    </span>
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
      actions={{ openMetrics: () => (metricsOpen = true) }}
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
  {#if confirmingPageDelete !== null}
    <ConfirmDialog
      title={`Delete page ${confirmingPageDelete}?`}
      detail="Its files are kept for recovery, but it leaves this notebook."
      confirmLabel="Delete page"
      onConfirm={() => void confirmDeleteActivePage()}
      onCancel={() => (confirmingPageDelete = null)}
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

    /* Two control sizes, and only two. `chrome` is the window's own furniture — the command
       strip, the instrument palette, undo/redo. `dense` is anything sitting inside a working
       surface, where a row of tools has to stay out of the way of the work. A control that is
       neither is a control that has not been thought about; there is no third size to reach for. */
    --control: 36px;
    --control-dense: 28px;
    /* Direct manipulation and coarse-pointer targets. The instrument palette always uses the
       touch-target minimum because it is hit with a stylus mid-sentence; other chrome may claim
       it only inside a coarse-pointer media query. */
    --control-touch: 44px;

    /* Icon box and its stroke travel together: apparent stroke weight is `stroke × size / 24`,
       so a smaller icon needs a heavier number to look the same weight. Pairing them here is
       what stops 16px icons reading as faint next to 20px ones. */
    --icon: 20px;
    --icon-dense: 16px;
    --stroke: 1.7;
    --stroke-dense: 2;

    /* Controls take the small radius, the things that contain them take the large one. */
    --radius: 6px;
    --radius-lg: 10px;
    --radius-pill: 999px;

    /* Three edges. `wash` is a hover fill, `edge` outlines a container, `edge-soft` divides two
       regions of one container. Controls get no border at all — a bordered button inside a
       bordered panel is the box-in-a-box that reads as clutter. */
    --wash: rgb(255 255 255 / 8%);
    --edge: rgb(255 255 255 / 12%);
    --edge-soft: rgb(255 255 255 / 7%);

    /* Four sizes: counters and eyebrows, secondary text, everything you click, titles. */
    --text-xs: 10px;
    --text-sm: 11px;
    --text-md: 13px;
    --text-lg: 15px;

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

  /* The gap matches the one between tabs, so the trailing "+" reads as the next seat in the row
     rather than a separate control that happens to sit nearby. */
  .notebook-identity { flex: 1; min-width: 0; gap: 4px; }
  .notebook-identity .home-button { margin-right: 6px; }

  /* Sized like the strip's other icon controls so the row reads as one set, and it replaces the
     decorative square that used to sit here — the corner was already the eye's first stop. */
  .home-button {
    display: grid;
    width: var(--control);
    height: var(--control);
    flex: none;
    padding: 0;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--text);
    cursor: pointer;
    place-items: center;
  }

  .home-button:hover:not(:disabled) { background: var(--wash); }
  .new-tab-button {
    display: grid;
    width: var(--control);
    height: var(--control);
    flex: none;
    padding: 0;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    place-items: center;
  }
  .new-tab-button:hover:not(:disabled) { background: var(--wash); color: var(--text); }

  .blue-dot { width: 6px; height: 6px; flex: none; border-radius: 50%; background: var(--blueprint); }
  .page-count {
    color: var(--quiet);
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
    letter-spacing: .02em;
  }

  .command-actions { flex: none; gap: 4px; }

  /* Only shown when no notebook is open, so it never sits in the icon row and can stay worded. */
  .text-action {
    height: var(--control);
    padding: 0 12px;
    border: 1px solid var(--edge);
    border-radius: var(--radius);
    background: transparent;
    color: var(--text);
    font-size: var(--text-md);
    font-weight: 600;
    cursor: pointer;
  }

  .icon-button {
    display: grid;
    width: var(--control);
    height: var(--control);
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--text);
    cursor: pointer;
    place-items: center;
  }

  .command-strip svg { width: var(--icon); height: var(--icon); }
  /* Filled glyphs — the overflow dots, the pen nib — are shapes, not line drawings; everything
     else is a stroke at the one weight. */
  .command-strip svg:not(.solid-icon) {
    fill: none;
    stroke: currentColor;
    stroke-width: var(--stroke);
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .command-strip svg.solid-icon { fill: currentColor; }
  .text-action:hover, .icon-button:hover, .icon-button.active { background: var(--wash); }
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
  /* Springs back when a pull is let go; follows the finger without lag while it lasts. */
  .page-pan-field { transition: transform 220ms cubic-bezier(0.2, 0.9, 0.3, 1.2); }
  .page-pan-field.pulling { transition: none; }
  .touch-history-toast {
    position: absolute;
    top: 16px;
    left: 50%;
    z-index: 20;
    transform: translateX(-50%);
    padding: 8px 18px;
    border-radius: var(--radius-pill);
    background: var(--panel);
    color: var(--text);
    box-shadow: 0 4px 16px rgb(0 0 0 / 28%);
    font-size: var(--text-md);
    pointer-events: none;
    animation: touch-history-fade 1s ease-out;
  }
  .touch-history-toast.below-palette { top: 90px; }
  .touch-history-toast.reduced-motion { animation: none; }
  @keyframes touch-history-fade { 0%, 75% { opacity: 1; } 100% { opacity: 0; } }
  .pull-badge {
    position: absolute;
    left: 50%;
    bottom: 28px;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 14px 6px 6px;
    border: 1px solid var(--edge);
    border-radius: var(--radius-pill);
    background: var(--panel);
    box-shadow: 0 10px 28px rgb(0 0 0 / 45%);
    color: var(--text);
    font-size: var(--text-sm);
    pointer-events: none;
    transform: translateX(-50%);
  }
  .pull-badge.over-palette { bottom: 104px; }
  .pull-badge svg { width: 30px; height: 30px; fill: none; stroke-linecap: round; stroke-linejoin: round; }
  .pull-badge .track { stroke: rgb(255 255 255 / 14%); stroke-width: 3; }
  .pull-badge .fill { stroke: var(--blueprint); stroke-width: 3; }
  .pull-badge .mark { stroke: var(--text); stroke-width: 2; }
  .pull-badge.ready { border-color: var(--blueprint); }
  .pull-badge.ready .mark { stroke: var(--blueprint-light); }
  .page-stack-item.active-page .page-number { color: var(--blueprint-light); }
  .page-number {
    position: absolute;
    top: 0;
    right: calc(100% + 12px);
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--quiet);
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .page-number button {
    width: 26px;
    height: 26px;
    border: 1px solid rgb(255 255 255 / 10%);
    border-radius: var(--radius);
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
  .page-number button { cursor: pointer; }
  .page-number button:disabled { cursor: default; }
  /* The arrows look the same on every page. Dimming them on the pages you are not on was the
     same disappearance the mounting fix was meant to remove — a fade instead of a pop, but still
     something arriving and leaving as the canvas scrolls. Which page is live is already said by
     the number beside them turning blue, so the buttons need not say it a second time.
     The one real dimming left is a page that genuinely cannot move that way: the first page has
     nowhere to go up, and that is a fact about the page rather than about where you are. */
  .page-number:not(.idle) button:disabled { opacity: 0.35; }
  @media (pointer: coarse) {
    .page-number button { width: 40px; height: 40px; touch-action: manipulation; }
  }
  .page-frame { position: relative; flex: none; }
  .page {
    position: relative;
    overflow: hidden;
    background: var(--paper);
    box-shadow: 0 2px 6px rgb(0 0 0 / 30%), 0 24px 60px rgb(0 0 0 / 45%);
    transform-origin: top left;
  }
  .page-loading { display: grid; height: 100%; color: #8d949d; font-size: var(--text-md); place-items: center; }

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

  .history-pill {
    position: relative;
    z-index: 15;
    display: flex;
    align-items: center;
    padding: 4px;
    border: 1px solid var(--edge);
    border-radius: var(--radius-lg);
    background: var(--panel);
    box-shadow: 0 12px 30px rgb(0 0 0 / 45%);
    gap: 0;
    cursor: grab;
    touch-action: none;
    transition: border-color 120ms ease, box-shadow 120ms ease;
  }
  .history-pill:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }
  .history-pill.dragging { border-color: rgb(76 141 240 / 72%); cursor: grabbing; }
  /* The gap between the buttons is the handle, so it has to be wide enough to be one. The dots
     only appear on approach, so the control reads as two buttons until it needs to read as more. */
  /* The pill travels the way the rail does: it becomes one touch-sized puck under the pointer,
     carrying the icon of what is being moved, with a bracket marking the corner it will land in.
     The bracket follows the settled corner, not the one under the pointer, so a drag over a
     corner the palette owns shows the pill stepping aside instead of promising a place it
     cannot have. */
  .history-pill.dragging {
    position: absolute;
    z-index: 30;
    width: var(--control-touch);
    height: var(--control-touch);
    padding: 0;
    border-color: rgb(76 141 240 / 72%);
    box-shadow: 0 14px 34px rgb(0 0 0 / 62%);
    animation: palette-pickup 120ms cubic-bezier(0.16, 1, 0.3, 1);
    will-change: left, top;
  }
  .history-pill.dragging button,
  .history-pill.dragging .grip { display: none; }

  .history-pill .transport { display: none; }
  .history-pill.dragging .transport {
    display: grid;
    width: 100%;
    height: 100%;
    color: var(--text);
    place-items: center;
  }
  .history-pill .transport svg {
    width: var(--icon);
    height: var(--icon);
    fill: none;
    stroke: currentColor;
    stroke-width: var(--stroke);
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .history-pill.dragging::after {
    position: absolute;
    width: 9px;
    height: 9px;
    border: 2px solid var(--blueprint-light);
    content: "";
    pointer-events: none;
  }
  .history-pill.dragging[data-corner="top-left"]::after {
    top: -6px; left: -6px; border-right: 0; border-bottom: 0;
  }
  .history-pill.dragging[data-corner="top-right"]::after {
    top: -6px; right: -6px; border-bottom: 0; border-left: 0;
  }
  .history-pill.dragging[data-corner="bottom-left"]::after {
    bottom: -6px; left: -6px; border-top: 0; border-right: 0;
  }
  .history-pill.dragging[data-corner="bottom-right"]::after {
    bottom: -6px; right: -6px; border-top: 0; border-left: 0;
  }

  .history-pill.reduced-motion { transition: none; }
  .history-pill.reduced-motion.dragging { animation: none; }

  .history-pill .grip {
    width: 14px;
    height: var(--control);
    flex: none;
    background-image: radial-gradient(circle, var(--muted) 1.1px, transparent 1.2px);
    background-position: center;
    background-size: 4px 6px;
    background-repeat: repeat-y;
    opacity: 0;
    transition: opacity 120ms ease;
  }
  .history-pill:hover .grip,
  .history-pill:focus-visible .grip,
  .history-pill.dragging .grip { opacity: 0.8; }
  @media (prefers-reduced-motion: reduce) {
    .history-pill, .history-pill .grip { transition: none; }
    .history-pill.dragging { animation: none; }
  }
  .history-pill button {
    display: grid;
    width: var(--control);
    height: var(--control);
    border-radius: var(--radius);
    background: transparent;
    color: var(--text);
    cursor: pointer;
    place-items: center;
  }
  .history-pill button:hover:not(:disabled) { background: var(--wash); }
  .history-pill svg {
    width: var(--icon);
    height: var(--icon);
    fill: none;
    stroke: currentColor;
    stroke-width: var(--stroke);
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .instrument-palette {
    position: relative;
    z-index: 20;
    display: grid;
    /* One control wide plus its breathing room. Stated once and reused by the columns inside:
       when the grid track and the column boxes disagree, the tiles centre on the wider box and
       drift off the panel's own centre line. */
    --rail: calc(var(--control-touch) + 6px);
    grid-template-columns: var(--rail);
    grid-template-rows: auto 1fr;
    gap: 3px 0;
    padding: 6px;
    border: 1px solid var(--edge);
    border-radius: var(--radius-lg);
    background: var(--panel);
    box-shadow: 0 20px 50px rgb(0 0 0 / 52%);
    touch-action: none;
    transition: border-color 120ms ease, box-shadow 120ms ease;
  }

  /* Placement comes from `chromeLayout`, which is the only thing that sees every piece of chrome
     at once and so the only thing able to keep them off each other. The dock classes that remain
     carry appearance: which way a settings card opens, and where its arrow points. */
  .instrument-palette.horizontal {
    grid-template-columns: auto 1fr;
    grid-template-rows: var(--rail);
  }

  /* The palette overlays the page. Keep its intrinsic size rather than squeezing its
     controls into a scroll container when the page or available space changes. */
  .instrument-palette:not(.horizontal):not(.dragging) { min-width: max-content; }

  /* One column, always. The quick controls sit at the foot of this same column rather than in a
     second one, which is what lets the rail keep a single fixed width in every state. */
  .palette-primary {
    display: flex;
    width: var(--rail);
    flex-direction: column;
    align-items: center;
    justify-content: center;
    grid-column: 1;
    grid-row: 2;
    gap: 3px;
    opacity: 1;
    transition: opacity 120ms ease;
  }

  .palette-grip {
    grid-column: 1 / -1;
    grid-row: 1;
    justify-self: center;
    touch-action: none;
  }

  /* Never narrower than the tiles it holds. With `min-width: 0` the row squeezed below its own
     contents and the tiles carried on past the panel, so the background stopped short of the last
     of them — the rail looked cut off at one end rather than full. */
  .horizontal .palette-primary {
    width: auto;
    min-width: max-content;
    flex-direction: row;
    grid-column: 2;
    grid-row: 1;
  }

  .horizontal .palette-grip {
    grid-column: 1;
    grid-row: 1 / -1;
    align-self: center;
  }

  /* The full rail is too large to move accurately near the opposite edge. During transport it
     becomes one stable touch-sized puck, centred under the pointer, then restores at the dock. */
  .instrument-palette.dragging {
    position: absolute;
    display: grid;
    width: var(--control-touch);
    height: var(--control-touch);
    padding: 0;
    border-color: rgb(76 141 240 / 72%);
    grid-template: 1fr / 1fr;
    gap: 0;
    box-shadow: 0 14px 34px rgb(0 0 0 / 62%);
    animation: palette-pickup 120ms cubic-bezier(0.16, 1, 0.3, 1);
    will-change: left, top;
  }

  .instrument-palette.dragging::after {
    position: absolute;
    border-radius: var(--radius-pill);
    background: var(--blueprint-light);
    content: "";
    pointer-events: none;
  }

  .instrument-palette.dragging[data-dock="top"]::after,
  .instrument-palette.dragging[data-dock="bottom"]::after {
    left: 12px;
    width: 20px;
    height: 2px;
  }

  .instrument-palette.dragging[data-dock="top"]::after { top: -5px; }
  .instrument-palette.dragging[data-dock="bottom"]::after { bottom: -5px; }

  .instrument-palette.dragging[data-dock="left"]::after,
  .instrument-palette.dragging[data-dock="right"]::after {
    top: 12px;
    width: 2px;
    height: 20px;
  }

  .instrument-palette.dragging[data-dock="left"]::after { left: -5px; }
  .instrument-palette.dragging[data-dock="right"]::after { right: -5px; }

  .instrument-palette.dragging .palette-primary {
    position: absolute;
    visibility: hidden;
    opacity: 0;
    pointer-events: none;
  }

  /* Grip: two columns of three dots, drag handle at the leading edge. */
  .palette-grip {
    display: grid;
    min-width: var(--control-touch);
    min-height: var(--control-touch);
    flex: none;
    place-content: center;
    padding: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    cursor: grab;
  }

  .palette-grip:hover { background: var(--wash); }
  .palette-grip:focus-visible { outline: 2px solid var(--blueprint); outline-offset: 1px; }

  .palette-grip-dots {
    display: grid;
    grid-template-columns: repeat(2, 3.5px);
    grid-template-rows: repeat(3, 3.5px);
    gap: 3px;
  }

  .palette-grip i {
    width: 3.5px;
    height: 3.5px;
    border-radius: 50%;
    background: rgb(255 255 255 / 28%);
  }

  .palette-transport-icon {
    display: none;
    width: var(--icon);
    height: var(--icon);
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: var(--stroke);
  }

  .palette-transport-icon circle {
    fill: currentColor;
    stroke: none;
  }

  .dragging .palette-grip {
    width: 100%;
    height: 100%;
    color: var(--text);
    cursor: grabbing;
    grid-column: 1;
    grid-row: 1;
  }

  .dragging .palette-grip:hover { background: transparent; }
  .dragging .palette-grip-dots { display: none; }
  .dragging .palette-transport-icon { display: block; }

  @keyframes palette-pickup {
    from {
      opacity: 0.82;
      transform: scale(0.9);
    }
  }

  .instrument-palette.reduced-motion,
  .instrument-palette.reduced-motion .palette-primary {
    transition: none;
  }

  .instrument-palette.reduced-motion.dragging {
    animation: none;
  }

  @media (prefers-reduced-motion: reduce) {
    .instrument-palette,
    .palette-primary {
      transition: none;
    }

    .instrument-palette.dragging {
      animation: none;
    }
  }


  /* Where a popout from the bar hangs. `--anchor` is the tapped tile's centre within the bar;
     this knows only the four docks, and the thing it holds knows only its own contents. */
  .palette-panel-anchor {
    position: absolute;
    bottom: calc(100% + 10px);
    left: clamp(0px, calc(var(--anchor) - 108px), calc(100vw - 240px));
    z-index: 60;
  }

  /* The card is 296px rather than 216px, so it centres on its own half-width. `--arrow` is the
     tapped tile's centre expressed inside the card, so the pointer still lands on the tile after
     the clamp has slid the card back inside the window. */
  .palette-panel-anchor.card {
    --card-left: clamp(0px, calc(var(--anchor) - 148px), calc(100vw - 320px));
    --arrow: calc(var(--anchor) - var(--card-left));
    left: var(--card-left);
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
  /* Side docks: the card cannot track the tile vertically. The rail is nearly as tall as the
     workspace, so a card anchored to a tile near its foot would hang off the bottom, and CSS
     cannot know how far down the page the rail actually starts. The rail is centred in the
     chrome, so centring the card on the rail centres it in the workspace — the one placement
     that always fits. The pointer sits at the card's middle, and the lit tile says which tool. */
  .instrument-palette.dock-left .palette-panel-anchor.card,
  .instrument-palette.dock-right .palette-panel-anchor.card {
    --arrow: 50%;
    top: 50%;
    bottom: auto;
    transform: translateY(-50%);
  }
  .instrument-palette.dock-left .palette-panel-anchor { left: calc(100% + 10px); }
  .instrument-palette.dock-right .palette-panel-anchor { right: calc(100% + 10px); }

  .zoom-group { display: flex; align-items: center; gap: 1px; }
  .zoom-group .zoom-value { min-width: 42px; font-variant-numeric: tabular-nums; text-align: center; }

  .closed-state { display: grid; align-content: center; justify-items: center; padding: 2rem; background: var(--surround); text-align: center; }
  .closed-mark { width: 34px; height: 42px; border: 1px solid var(--quiet); border-radius: 3px; box-shadow: inset 0 5px var(--panel); }
  .closed-state h1 { margin: 18px 0 5px; font-size: 20px; font-weight: 600; }
  .closed-state p { margin: 0; color: var(--muted); font-size: var(--text-md); }
  .closed-state button { margin-top: 20px; padding: 10px 14px; border-radius: var(--radius); background: var(--blueprint); color: #10141a; font-size: var(--text-md); font-weight: 700; cursor: pointer; }

  .status-strip {
    z-index: 30;
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 12px;
    padding: 0 16px;
    border-top: 1px solid var(--edge-soft);
    background: var(--charcoal);
    color: var(--muted);
    font-size: var(--text-sm);
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
  .footer-divider { width: 1px; height: 15px; flex: none; background: var(--edge); }
  .status-strip button { padding: 3px 5px; border-radius: var(--radius); background: transparent; color: var(--muted); cursor: pointer; }
  .status-strip button:hover { background: rgb(255 255 255 / 8%); }
  .local-state { flex: none; }

  .panel-scrim { position: absolute; z-index: 70; inset: 58px 0 34px; display: grid; padding: 24px; background: rgb(10 12 16 / 62%); place-items: start end; }
  .diagnostics-panel {
    width: min(620px, 100%);
    max-height: 100%;
    overflow: auto;
    padding: 22px 24px;
    border: 1px solid var(--edge);
    border-radius: var(--radius-lg);
    background: var(--panel);
    box-shadow: 0 24px 60px rgb(0 0 0 / 55%);
  }

  .panel-heading { display: flex; align-items: flex-start; justify-content: space-between; }
  .panel-heading span { color: var(--quiet); font: 10px "Cascadia Mono", Consolas, monospace; letter-spacing: .1em; text-transform: uppercase; }
  .panel-heading h2 { margin: 4px 0 0; font-size: 20px; font-weight: 600; }
  .diagnostic-path { overflow-wrap: anywhere; color: var(--quiet); font: 10px "Cascadia Mono", Consolas, monospace; }
  .diagnostics-panel dl { display: grid; grid-template-columns: minmax(210px, 1fr) 1fr; gap: 8px 18px; margin: 18px 0 0; padding-top: 16px; border-top: 1px solid rgb(255 255 255 / 8%); font-size: var(--text-md); }
  .diagnostics-panel dt { color: var(--muted); }
  .diagnostics-panel dd { margin: 0; color: var(--text); font-family: "Cascadia Mono", Consolas, monospace; font-variant-numeric: tabular-nums; }

  .screen-reader-status { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); clip-path: inset(50%); white-space: nowrap; }
  @media (max-width: 800px) {
    .operation-status, .page-count { display: none; }
  }
</style>
