<script lang="ts">
  import { createWindowCloseHandler } from "./windowClose";
  import NotebookPalette from "./NotebookPalette.svelte";
  import { createSession } from "./notebookSession.svelte";
  import { createSelection } from "./selectionEditing.svelte";
  import { createNavigation } from "./viewportNavigation.svelte";
  import { patternLabel, shapeLabel, toolMessage, toolLabel } from "../i18n/labels";
  import { setLanguage, t, formatNumber, displayLocale, notice, renderMessage } from "../i18n";
  import { errorDetails } from "../ipc/errors";
  import { onDestroy, onMount, tick } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { type TypstCompileRequest } from "../ipc/typst";
  import { createNotebookPreviews } from "../editor/notebookPreviews";
  import { createTemplateEditing } from "../editor/templateEditing.svelte";
  import { listRecentNotebooks, recordNotebookPage, recordNotebookSession, resumeNotebookSession, writeMetrics } from "../ipc/workspace";
  import { placeFloatingToolbar } from "../geometry/placement";
  import { dismissable } from "../workspace/dismiss";
  import { type PageDefaults, type PagePosition, type ShapeStyle, type PageTemplate, type StrokePattern } from "../model";
  import { TYPST_IDLE_DEBOUNCE_MS, type TypstCompileResult } from "../editor/typst";
  import { clearTypstCache } from "../editor/typstCache";
  import { createTypstPreset, installPageTypstPreset, readTypstPreset, pickTypstPreset, setDefaultTypstPreset } from "../ipc/presets";
  import { presetHeader, DEFAULT_PRESET_PATH, withPagePreset, type PresetChoice } from "../page/presets";
  import { summarizeMetric, type StrokePerformance } from "../ink/metrics";
  import { DEFAULT_POINTER_MAPPING, type InkTool, type PointerMapping } from "../ink/pipeline";
  import { keepsSelection, shared } from "../ink/selection";
  import ColorPanel from "./ColorPanel.svelte";
  import WidthPanel from "./WidthPanel.svelte";
  import PaletteEditor from "./PaletteEditor.svelte";
  import EraserPanel from "./EraserPanel.svelte";
  import ToolCard from "./ToolCard.svelte";
  import ShapeCard from "./ShapeCard.svelte";
  import PageSurface from "./PageSurface.svelte";
  import OverflowMenu from "../workspace/OverflowMenu.svelte";
  import { populated, type MenuSection } from "../workspace/menu";
  import AddPageMenu from "../workspace/AddPageMenu.svelte";
  import type { AddPageGroup, AddPageSource, AddPageWhere } from "../workspace/addPage";
  import { templatePreviewSvg } from "../page/template";
  import { PAPER_TONES, paperTones, templateGroups } from "../page/templates";
  import { DEFAULT_PAGE_SIZE, PAGE_SIZES, pageSizes, describeGeometry, geometryOf, type Orientation } from "../page/sizes";
  import SideEditor from "./SideEditor.svelte";
  import SelectionActions from "./SelectionActions.svelte";
  import TypstSizeControls from "./TypstSizeControls.svelte";
  import { blockFont, withBlockFont } from "../editor/blockFont";
  import NotebookHeader from "./NotebookHeader.svelte";
  import WorkspaceStatus from "./WorkspaceStatus.svelte";
  import PaletteTools from "./PaletteTools.svelte";
  import PalettePocket from "./PalettePocket.svelte";
  import { blockViewsFromSnapshot, imageViewsFromSnapshot, pageTypstViewFromSnapshot, shapeViewsFromSnapshot, type BlockView, type ImageView, type PageTypstView, type ShapeView } from "./pageView";
  import type { ShapeDraft } from "../shape/geometry";
  import LassoPanel from "./LassoPanel.svelte";
  import MixedSelection from "./MixedSelection.svelte";
  import { DEFAULT_LASSO_TARGETS } from "./lasso";
  import { type PaletteCommand, type PaletteDock } from "./palette";
  import { newInkZIndex } from "./objectOrder";
  import { cycledTab } from "./tabs";
  import { addWidth as addRowWidth, canRemoveWidth, editWidth as editRowWidth, movePattern, moveSpacing, patternForWidth, prunePatterns, pruneSpacings, removeWidth as removeRowWidth, spacingForWidth, withWidthPattern, withWidthSpacing, widthTaken, WIDTH_BOUNDS_MM } from "./widths";
  import ConfirmDialog from "../workspace/ConfirmDialog.svelte";
  import ConflictDialog from "../workspace/ConflictDialog.svelte";
  import RecoveryDialog from "../workspace/RecoveryDialog.svelte";
  import SearchOverlay from "../workspace/SearchOverlay.svelte";
  import SettingsPanel from "../workspace/SettingsPanel.svelte";
  import LibrarySurface from "../library/LibrarySurface.svelte";
  import { SHELF_ROOT, type ShelfLocation } from "../library/location";
  import { DEFAULT_SETTINGS, loadSettings, saveSettings, ERASER_RADIUS_PT, HIGHLIGHTER_TAPER, MAX_SWATCHES, MAX_WIDTHS, MM_PER_PT, colorName, penType, withRecentColor, type AppSettings, type PenPreset } from "../settings";

  const session = createSession({
    get addPageOpen() { return addPageOpen; },
    set addPageOpen(value) { addPageOpen = value; },
    get collectMetrics() { return collectMetrics; },
    get followPageText() { return followPageText; },
    get message() { return message; },
    get moreOpen() { return moreOpen; },
    set moreOpen(value) { moreOpen = value; },
    get navigation() { return navigation; },
    get pageViewport() { return pageViewport; },
    set pageViewport(value) { pageViewport = value; },
    get paletteDragging() { return paletteDragging; },
    get previews() { return previews; },
    get resetWorkspaceEditors() { return resetWorkspaceEditors; },
    get saveTemplate() { return saveTemplate; },
    get searchOpen() { return searchOpen; },
    set searchOpen(value) { searchOpen = value; },
    get selection() { return selection; },
    get settings() { return settings; },
    get shapeView() { return shapeView; },
    get sideEditor() { return sideEditor; },
    get sideEditorOpen() { return sideEditorOpen; },
    get tauriAvailable() { return tauriAvailable; },
    get tool() { return tool; },
    set tool(value) { tool = value; },
    get windowCloseInProgress() { return windowClose.closing; },
  });

  const observePage = session.observePage;

  const selection = createSelection({
    get activeInkLayerId() { return session.activeInkLayerId; },
    set activeInkLayerId(value) { session.activeInkLayerId = value; },
    get activeInkLayerPath() { return session.activeInkLayerPath; },
    set activeInkLayerPath(value) { session.activeInkLayerPath = value; },
    get activePageId() { return session.activePageId; },
    get activeSnapshot() { return session.activeSnapshot; },
    set activeSnapshot(value) { session.activeSnapshot = value; },
    get changeStrokes() { return session.changeStrokes; },
    get closeToolCard() { return closeToolCard; },
    get editContext() { return session.editContext; },
    get images() { return session.images; },
    set images(value) { session.images = value; },
    get message() { return message; },
    get navigation() { return navigation; },
    get nextReadingOrder() { return session.nextReadingOrder; },
    get nextVisualZIndex() { return session.nextVisualZIndex; },
    get pageFrame() { return pageFrame; },
    get pageInputLocked() { return session.pageInputLocked; },
    get pointerMapping() { return pointerMapping; },
    get queueCommit() { return session.queueCommit; },
    get root() { return session.root; },
    get scheduleInkCommit() { return session.scheduleInkCommit; },
    get selectedInk() { return selectedInk; },
    get selectedShapes() { return selectedShapes; },
    get settings() { return settings; },
    get shapes() { return session.shapes; },
    set shapes(value) { session.shapes = value; },
    get status() { return session.status; },
    set status(value) { session.status = value; },
    get strokes() { return session.strokes; },
    set strokes(value) { session.strokes = value; },
    get tauriAvailable() { return tauriAvailable; },
    get tool() { return tool; },
    set tool(value) { tool = value; },
    get typstBlocks() { return session.typstBlocks; },
    set typstBlocks(value) { session.typstBlocks = value; },
    get workspace() { return workspace; },
    set workspace(value) { workspace = value; },
    get zoom() { return zoom; },
    set zoom(value) { zoom = value; },
  });

  const navigation = createNavigation({
    get addPage() { return session.addPage; },
    get busy() { return session.busy; },
    set busy(value) { session.busy = value; },
    get canRedo() { return session.canRedo; },
    get canUndo() { return session.canUndo; },
    get collectMetrics() { return collectMetrics; },
    get directObjectInput() { return selection.directObjectInput; },
    set directObjectInput(value) { selection.directObjectInput = value; },
    get notebookManifest() { return session.notebookManifest; },
    get openContextMenu() { return selection.openContextMenu; },
    get pageFrame() { return pageFrame; },
    set pageFrame(value) { pageFrame = value; },
    get pageOpen() { return session.pageOpen; },
    get pageViewport() { return pageViewport; },
    set pageViewport(value) { pageViewport = value; },
    get routeHistory() { return session.routeHistory; },
    get routeObjectPointer() { return selection.routeObjectPointer; },
    get scheduleFocus() { return session.scheduleFocus; },
    get scheduleResidency() { return session.scheduleResidency; },
    get settings() { return settings; },
    get tauriAvailable() { return tauriAvailable; },
    get tool() { return tool; },
    get transactionFailed() { return session.transactionFailed; },
    get workspace() { return workspace; },
    set workspace(value) { workspace = value; },
    get zoom() { return zoom; },
    set zoom(value) { zoom = value; },
    get zoomFrameMs() { return zoomFrameMs; },
    set zoomFrameMs(value) { zoomFrameMs = value; },
  });

  // Preview requests and template drafts have instance-local owners in lib/editor.

  const ERASER_SIZE_OPTIONS = $derived([
    { id: "small" as const, label: $t("workspace.small"), diameter: 12 },
    { id: "medium" as const, label: $t("workspace.medium"), diameter: 18 },
    { id: "large" as const, label: $t("workspace.large"), diameter: 26 },
  ]);
  const collectMetrics = import.meta.env.DEV;
  const tauriAvailable =
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  let presetBusy = $state(false);
  let zoom = $state(1);
  let tool = $state<InkTool>("pen");
  let strokeMetrics = $state<StrokePerformance[]>([]);
  let compileMs = $state<number | null>(null);
  let zoomFrameMs = $state<number | null>(null);
  const previews = createNotebookPreviews(tauriAvailable, ms => { if (collectMetrics) compileMs = ms; });
  let workspace = $state<HTMLElement>();
  let paletteElement = $state<HTMLElement>();
  let pageViewport = $state<HTMLElement>();
  let pageFrame = $state<HTMLElement>();

  // The page being edited renders through the same surface as its neighbours, so its state is
  // projected into the same view model they use.
  const activeBlockViews: BlockView[] = $derived(
    session.typstBlocks.map(({ transform, result: _result, ...block }) => ({ ...block, ...transform })),
  );
  const activePageTypstView: PageTypstView | null = $derived(session.pageTypst);
  const activeResults: Record<string, TypstCompileResult | null> = $derived(
    Object.fromEntries([
      ...session.typstBlocks.map((block) => [block.id, block.result] as const),
      ...(session.pageTypst ? [[session.pageTypst.id, session.pageTypst.result] as const] : []),
    ]),
  );
  const activeImageViews: ImageView[] = $derived(session.images);

  // Full-height source view beside the canvas. It is a sibling of the canvas region rather than
  // an overlay, so the canvas genuinely narrows — and the palette, which is positioned inside
  // that region, follows the paper instead of colliding with the panel.
  let sideEditorOpen = $state(false);
  let sideEditorStyle = $state(false);
  /// A page template open in the side editor (style mode), instead of the shared notebook style.
  const templateEditor = createTemplateEditing({
    context: () => ({ root: session.root, generation: session.notebookGeneration }),
    show: () => { sideEditorStyle = true; sideEditorOpen = true; void tick().then(() => sideEditor?.focus()); },
    saved: (template) => {
      session.presets = session.presets.map(preset => preset.importPath === template.path ? { ...preset, name: template.name } : preset);
      clearTypstCache(template.path); session.presetRevision += 1;
    },
    report: (message) => { session.status = message; },
  });
  const { open: openTemplate, update: updateTemplate, flush: saveTemplate, close: closeTemplate } = templateEditor;
  const saveNeedsAttention = $derived(session.transactionFailed || templateEditor.error !== "");
  const savePending = $derived(
    session.pendingTransactions > 0 ||
      session.inkPending ||
      session.typstDirty ||
      session.pendingNeighborPages.size > 0 ||
      !!templateEditor.draft?.dirty,
  );
  let sideEditorBlockId = $state<string | null>(null);
  let sideEditorPageText = $state(false);
  /// The page a pinned movable block belongs to. Page text instead follows the active page.
  let sideEditorPageId = $state<string | null>(null);
  let sideEditor = $state<{ focus: () => void }>();

  const sideEditorBlock = $derived(
    session.typstBlocks.find((block) => block.id === sideEditorBlockId) ??
      (session.pageTypst?.id === sideEditorBlockId ? session.pageTypst : null),
  );
  const editingPageText = $derived(
    sideEditorOpen && sideEditorPageText && session.pageTypst?.id === sideEditorBlockId,
  );
  /// `edit` when the target is on the page in view, `away` when a movable block is held on
  /// another page, and `none` when the active page has no Page text or no target was picked.
  const sideEditorMode = $derived<"edit" | "style" | "away" | "none">(
    sideEditorStyle ? "style" : sideEditorBlock ? "edit" : sideEditorBlockId ? "away" : "none",
  );
  const sideEditorPageNumber = $derived(
    sideEditorPageId
      ? (session.notebookManifest?.pages.findIndex((page) => page.id === sideEditorPageId) ?? 0) + 1
      : null,
  );

  async function openSideEditor(blockId?: string) {
    if (!(await closeTemplate())) return;
    sideEditorStyle = false;
    // An explicit canvas edit still wins. The generic button/shortcut starts with the sustained
    // Page text surface, then falls back to the same movable-block choices as before.
    const remembered = session.typstBlocks.some((block) => block.id === sideEditorBlockId)
      ? sideEditorBlockId
      : null;
    const target = blockId ?? session.pageTypst?.id ?? remembered ?? selection.selectedTypstId ?? session.typstBlocks[0]?.id ?? null;
    sideEditorOpen = true;
    if (target) {
      if (target !== sideEditorBlockId) sideEditorPageId = session.activePageId;
      sideEditorBlockId = target;
      sideEditorPageText = session.pageTypst?.id === target;
      if (session.typstBlocks.some((block) => block.id === target)) {
        selection.selectedTypstId = target;
        selection.selectedImageId = null;
        selection.selectedShapeId = null;
        selection.editingShapeId = null;
      } else if (session.pageTypst?.id === target) {
        selection.selectedTypstId = null;
        selection.selectedImageId = null;
        selection.selectedShapeId = null;
        selection.editingShapeId = null;
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
    if (!session.pageTypst) {
      const hasDefault = session.presets.some((preset) => preset.kind === "default");
      session.pageTypst = {
        id: `${session.activePageId}-page-text`,
        path: `blocks/${session.activePageId}-page.typ`,
        source: hasDefault ? presetHeader() : "",
        result: null,
        zIndex: 0,
        readingOrder: 0,
      };
      session.queueCommit(notice("workspace.createdPageText"));
      session.status = notice("workspace.createdPageText");
    }
    openSideEditor(session.pageTypst.id);
  }

  async function changePagePreset(action: string) {
    if (!session.root || !session.pageTypst || presetBusy) return;
    const requestedRoot = session.root;
    const generation = session.notebookGeneration;
    const current = () => session.root === requestedRoot && session.notebookGeneration === generation;
    presetBusy = true;
    try {
      if (action === "none" || action.startsWith("path:")) {
        const path = action === "none" ? null : action.slice(5);
        session.updateTypstSource(session.pageTypst.id, withPagePreset(session.pageTypst.source, path));
      } else if (action === "page:new") {
        const created = await createTypstPreset(requestedRoot);
        if (!current()) return;
        if (created.importPath) {
          session.updateTypstSource(session.pageTypst.id, withPagePreset(session.pageTypst.source, created.importPath));
          await session.refreshPresets();
          await openTemplate(created.importPath, created.name);
          session.status = notice("workspace.createdEditItInTheSidePanel", { "name": created.name });
        }
        return;
      } else if (action.startsWith("edit:")) {
        const path = action.slice(5);
        await openTemplate(path, session.presets.find((preset) => preset.importPath === path)?.name ?? $t("workspace.template"));
        return;
      } else if (action === "page:import" || action.startsWith("page:")) {
        const choice = action === "page:import"
          ? await pickTypstPreset()
          : { kind: "builtin", id: action.slice(5) } as PresetChoice;
        if (choice && current()) {
          const installed = await installPageTypstPreset(requestedRoot, choice);
          if (!current()) return;
          if (installed?.importPath) {
            session.updateTypstSource(session.pageTypst.id, withPagePreset(session.pageTypst.source, installed.importPath));
            await session.refreshPresets();
          }
        }
      } else if (action === "default:import" || action.startsWith("default:")) {
        const expectedSource = session.presets.some((preset) => preset.kind === "default")
          ? await readTypstPreset(requestedRoot, DEFAULT_PRESET_PATH)
          : null;
        const choice = action === "default:import"
          ? await pickTypstPreset()
          : { kind: "builtin", id: action.slice(8) } as PresetChoice;
        if (choice && current()) {
          await setDefaultTypstPreset(requestedRoot, choice, expectedSource);
          if (!current()) return;
          clearTypstCache(DEFAULT_PRESET_PATH);
          session.presetRevision += 1;
          await session.refreshPresets();
          session.status = notice("workspace.updatedTheNotebookTypstPreset");
        }
      }
    } catch (error) {
      if (current()) session.status = notice("workspace.couldNotChangeTheTypstPreset", { "value1": message(error) });
    } finally {
      presetBusy = false;
      void tick().then(() => sideEditor?.focus());
    }
  }

  async function closeSideEditor() {
    if (!(await closeTemplate())) return;
    sideEditorOpen = false;
  }

  /**
   * Page templates are edited in the side panel's style mode. A template is a notebook file of
   * its own (`styles/…`), shared by every page that imports it, so it is saved straight to that
   * file — debounced while typing — and every page using it re-renders. A half-written template
   * is saved too; pages keep their last good render until it compiles again.
   */

  function toggleSideEditor() {
    if (sideEditorOpen) closeSideEditor();
    else openSideEditor();
  }

  async function openSharedStyle() {
    if (!session.notebookManifest) return;
    if (!(await closeTemplate())) return;
    if (!session.notebookManifest.sharedStylePath) {
      session.notebookManifest = { ...session.notebookManifest, sharedStylePath: "style.typ" };
      session.queueCommit(notice("workspace.createdSharedTypstStyle"));
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
  let paletteDock = $state<PaletteDock>("bottom");
  let paletteDragging = $state(false);
  let penPreset = $state<1 | 2>(1);
  // Tool-specific behaviour, anchored to the selected tool.
  let toolCard = $state<{
    kind: "pen" | "highlighter";
    slot: number;
    anchor: number;
  } | null>(null);
  let shapeCard = $state<{ anchor: number } | null>(null);
  let eraserEditor = $state<HTMLElement | null>(null);
  let lassoEditor = $state<HTMLElement | null>(null);
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
    lassoEditor = null;
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

  function selectLasso(tile: HTMLElement) {
    if (!keepsSelection(tool)) { closeToolCard(); activateTool("lasso"); return; }
    const alreadyOpen = lassoEditor === tile;
    closeToolCard();
    if (!alreadyOpen) lassoEditor = tile;
  }

  function closeLassoEditor() {
    const anchor = lassoEditor;
    closeToolCard();
    void tick().then(() => anchor?.isConnected && anchor.focus());
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
    lasso: selectLasso,
    "page-text": () => selectPlainTool(() => editingPageText ? closeSideEditor() : openPageText()),
    "typst-block": () => selectPlainTool(session.addTypstBlock),
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
    toolCard || shapeCard || eraserEditor || lassoEditor ? activePaletteCommand() : null,
  );

  /// Which quick controls the pocket carries for the tool in hand. Absent for tools that have
  /// nothing to set, so the rail shrinks back to bare tiles rather than showing an empty tray.
  /// Whether the pocket edits a selection rather than the pen (specs/qol-wave.md section 2).
  const selectedInk = $derived(session.strokes.filter((stroke) => selection.selectedStrokeIds.includes(stroke.id)));
  const selectedShape = $derived(session.shapes.find((shape) => shape.id === selection.selectedShapeId) ?? null);
  const selectedShapes = $derived(session.shapes.filter(shape => shape.id === selection.selectedShapeId || selection.lassoObjectIds.includes(shape.id)));
  const restyling = $derived(selectedInk.length > 0 || selectedShapes.length > 0);
  const paletteContext = $derived<"ink" | "eraser" | null>(
    restyling || tool === "pen" || tool === "highlighter" || tool === "shape"
      ? "ink"
      : tool === "eraser"
        ? "eraser"
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
  let settingsReady = $state(false);
  let settingsSaveTimer: ReturnType<typeof setTimeout> | undefined;
  let settingsSaveQueue: Promise<void> = Promise.resolve();
  let settingsNeedSave = false;
  let settingsVersion = 0;
  let searchOpen = $state(false);
  let showInitialSetup = $state(false);

  /**
   * Where the shelf was when a notebook was opened from it.
   *
   * Held here because `LibrarySurface` is unmounted for as long as a notebook is open. Closing
   * one should return you to the folder you opened it from, not to the library root three levels
   * up — the notebook you just closed is the thing you are most likely to want next to.
   */
  let shelfLocation = $state<ShelfLocation>(SHELF_ROOT);
  let metricsTimer: ReturnType<typeof setTimeout> | undefined;
  let removeCloseListener: (() => void) | undefined;
  // Size/reflow/compile updates must not move the controls underneath repeated clicks.
  const typstToolbarAnchor = $derived.by(() => {
    const block = session.typstBlocks.find(candidate => candidate.id === selection.selectedTypstId);
    return block ? `${block.id}:${block.transform.x}:${block.transform.y}:${block.transform.rotation ?? 0}` : null;
  });
  let destroyed = false;
  const windowClose = createWindowCloseHandler({
    saveSettings: flushSettings,
    saveNotebook: session.persist,
    rememberPage: async () => {
      if (session.notebookChosen && session.root) {
        await recordNotebookPage(session.root, session.activePageId).catch(() => {});
      }
    },
    destroy: async () => { await getCurrentWindow().destroy(); },
    failed: error => { session.status = notice("workspace.theNotebookIsSafeButTheWindow", { value1: message(error) }); },
  });

  onMount(() => {
    void initialize();
    window.addEventListener("keydown", historyShortcut);
    // On the window, not on <main>: with focus on the body a paste never reaches <main>.
    window.addEventListener("paste", selection.handlePaste);
    window.addEventListener("resize", onResizeToolbar);
    if (tauriAvailable) {
      const appWindow = getCurrentWindow();
      void appWindow
        .onCloseRequested(windowClose.handle)
        .then(remove => {
          if (destroyed) remove();
          else removeCloseListener = remove;
        })
        .catch((error) => {
          session.status = notice("workspace.couldNotInstallTheWindowCloseHandler", { "value1": message(error) });
        });
    }
  });
  onDestroy(() => {
    destroyed = true;
    clearTimeout(settingsSaveTimer);
    templateEditor.dispose();
    previews.dispose();
    session.dispose();
    selection.dispose();
    navigation.dispose();
    if (metricsTimer) clearTimeout(metricsTimer);
    removeCloseListener?.();
    window.removeEventListener("keydown", historyShortcut);
    window.removeEventListener("paste", selection.handlePaste);
    window.removeEventListener("resize", onResizeToolbar);
  });

  $effect(() => {
    if (!collectMetrics || !tauriAvailable || !session.root || !session.notebookChosen) return;
    const metrics = metricsPayload();
    // Metrics are dev telemetry; batching writes keeps them off the per-stroke path.
    if (metricsTimer) clearTimeout(metricsTimer);
    metricsTimer = setTimeout(() => {
      void writeMetrics(session.root, metrics).catch(() => {});
    }, 1000);
  });

  $effect(() => {
    const selected = selection.selectedObjectId() || selection.lassoObjectIds.length > 0 || selection.selectedStrokeIds.length > 0 || selection.groupedStrokeIds.length > 0;
    void typstToolbarAnchor;
    if (!selection.selectedTypstId) {
      void session.images;
      void session.shapes;
      void session.strokes;
    }
    void $t;
    void zoom;
    void session.activePageId;
    void sideEditorOpen;
    void settings.sideEditorWidth;
    selection.selectionToolbarPosition = { left: 0, top: 0, ready: false };
    if (selected) void tick().then(() => selection.scheduleSelectionToolbar());
  });

  async function initialize() {
    settings = await loadSettings(tauriAvailable);
    setLanguage(settings.uiLanguage);
    settingsReady = true;
    paletteDock = settings.paletteDock;
    if (!tauriAvailable) {
      session.root = "Browser preview (persistence and real Typst compilation require Tauri)";
      session.applySnapshot(session.buildSnapshot());
      session.openTabs = [{ root: session.root, title: session.notebookTitle }];
      session.notebookChosen = true;
      session.pageOpen = true;
      session.busy = false;
      session.status = notice("workspace.browserPreviewReady");
      return;
    }

    try {
      const [restored, recents] = await Promise.all([
        resumeNotebookSession(),
        listRecentNotebooks(),
      ]);
      session.openTabs = restored.openRoots.map((knownRoot) => ({
        root: knownRoot,
        title: recents.find((entry) => entry.root === knownRoot)?.title ?? session.titleFromRoot(knownRoot),
      }));
      const candidates = [restored.activeRoot, ...restored.openRoots].filter(
        (candidate, index, roots): candidate is string =>
          Boolean(candidate) && roots.indexOf(candidate) === index,
      );
      for (const candidate of candidates) {
        if (await session.openNotebookAt(candidate, { skipCurrentPersist: true })) return;
        session.openTabs = session.openTabs.filter((tab) => tab.root !== candidate);
      }
      await recordNotebookSession(session.openTabs.map((tab) => tab.root), null);
    } catch (error) {
      session.status = notice("workspace.theLastNotebookCouldNotBeReopened", { "value1": message(error) });
    }

    try {
      const recents = await listRecentNotebooks();
      showInitialSetup = recents.length === 0;
    } catch {
      // The recents list is a convenience; failing to read it falls through to the start surface.
    }
    session.busy = false;
  }

  function resetWorkspaceEditors() {
    templateEditor.reset();
    sideEditorStyle = false;
    sideEditorOpen = false;
    sideEditorBlockId = null;
    sideEditorPageText = false;
    sideEditorPageId = null;
    searchOpen = false;
  }

  function followPageText(id: string | null) {
    if (sideEditorOpen && sideEditorPageText && !sideEditorStyle) {
      sideEditorBlockId = id;
      sideEditorPageId = session.activePageId;
    }
  }

  function changeSettings(next: AppSettings) {
    settings = next;
    setLanguage(next.uiLanguage);
    paletteDock = next.paletteDock;
    const version = ++settingsVersion;
    settingsNeedSave = true;
    if (settingsSaveTimer) clearTimeout(settingsSaveTimer);
    settingsSaveTimer = setTimeout(() => {
      settingsSaveTimer = undefined;
      queueSettingsSave(settings, version);
    }, 400);
  }

  function changeInlineFont(font: string) {
    const block = session.typstBlocks.find(candidate => candidate.id === selection.selectedTypstId);
    if (block) session.updateTypstSource(block.id, withBlockFont(block.source, font));
    changeSettings({ ...settings, inlineTextFont: font });
  }

  function queueSettingsSave(pending: AppSettings, version: number) {
    settingsSaveQueue = settingsSaveQueue.then(async () => {
      try {
        const sanitized = await saveSettings(tauriAvailable, pending);
        if (version === settingsVersion) {
          settings = sanitized;
          setLanguage(sanitized.uiLanguage);
          settingsNeedSave = false;
        }
      } catch (error) {
        session.status = notice("errors.settingsSession", { details: message(error) });
      }
    });
  }

  async function flushSettings(): Promise<boolean> {
    if (settingsSaveTimer) clearTimeout(settingsSaveTimer);
    settingsSaveTimer = undefined;
    if (settingsNeedSave) queueSettingsSave(settings, settingsVersion);
    await settingsSaveQueue;
    return !settingsNeedSave;
  }

  /**
   * The overflow menu, described as data. A new page-level feature — a template picker, a
   * bookmark, rotation — is an entry in this list rather than another branch of markup, which
   * is the point: this menu is where all of them have to surface.
   */
  function menuSections(): MenuSection[] {
    const many = session.pageCount > 1;
    const selected = selection.selectedObjectId();
    if (!session.pageOpen) {
      return [
        {
          title: $t("workspace.notebook"),
          entries: [
            { kind: "action", id: "reopen", label: $t("workspace.reopenNotebook"), icon: "M4 12a8 8 0 1 1 2.3 5.6M4 12V6M4 12h6", onSelect: session.reopen },
            { kind: "action", id: "settings", label: $t("library.settings"), icon: "M4 8h8M17 8h3M4 16h3M12 16h8M14.5 5.8v4.4M9.5 13.8v4.4", hint: "Ctrl ,", onSelect: () => (settingsOpen = true) },
          ],
        },
      ];
    }
    return populated([
      ...(selected
        ? [
            {
              title: selection.selectedImageId
                ? $t("workspace.selectedImage")
                : selection.selectedShapeId
                  ? $t("workspace.selectedShape")
                  : $t("workspace.selectedTypst"),
              entries: [
                { kind: "action" as const, id: "object-back", label: $t("workspace.sendBehindEverything"), icon: "M4 12l8 4 8-4M4 8l8-4 8 4-8 4z", disabled: !selection.canMoveVisual(-1), onSelect: () => selection.changeVisualOrder("back") },
                { kind: "action" as const, id: "object-front", label: $t("workspace.bringInFrontOfEverything"), icon: "M4 12l8-4 8 4-8 4zM4 16l8 4 8-4", disabled: !selection.canMoveVisual(1), onSelect: () => selection.changeVisualOrder("front") },
                { kind: "action" as const, id: "read-earlier", label: $t("workspace.readEarlierByScreenReaders"), icon: "M12 19V5m0 0-5 5m5-5 5 5", disabled: !selection.canMoveReading(-1), onSelect: () => selection.changeReadingOrder(-1) },
                { kind: "action" as const, id: "read-later", label: $t("workspace.readLaterByScreenReaders"), icon: "M12 5v14m0 0 5-5m-5 5-5-5", disabled: !selection.canMoveReading(1), onSelect: () => selection.changeReadingOrder(1) },
                { kind: "action" as const, id: "remove-object", label: $t("workspace.removeFromPage"), icon: "M6 6l12 12M18 6 6 18", hint: $t("workspace.delete"), onSelect: () => void selection.deleteSelection() },
              ],
            },
          ]
        : []),
      {
        entries: [
          // Adding a page is not here: it has its own header button, because choosing where the
          // page goes and what it is made of needs more than one row.
          { kind: "action", id: "duplicate", label: $t("workspace.duplicatePage"), icon: "M9 9h10v12H9zM5 15V3h10v2", onSelect: session.duplicateActivePage },
          { kind: "action", id: "up", label: $t("workspace.movePageUp"), icon: "M12 19V5m0 0-5 5m5-5 5 5", hint: "Ctrl Shift PgUp", disabled: !many || session.pageNumber === 1, onSelect: () => void session.moveActivePage(-1) },
          { kind: "action", id: "down", label: $t("workspace.movePageDown"), icon: "M12 5v14m0 0 5-5m-5 5-5-5", hint: "Ctrl Shift PgDn", disabled: !many || session.pageNumber === session.pageCount, onSelect: () => void session.moveActivePage(1) },
          { kind: "number", id: "goto", label: $t("workspace.goToPage"), icon: "M5 4.5h14v15H5zM9 9h6M9 13h4", value: session.pageNumber, min: 1, max: session.pageCount, hint: $t("workspace.of", { "pageCount": session.pageCount }), disabled: !many, onCommit: (number) => void session.goToPageNumber(number) },
        ],
      },
      {
        title: $t("workspace.clearOrRemovePage"),
        entries: [
          { kind: "action", id: "clear", label: $t("workspace.clearPage"), icon: "M6 6.5h12M9.5 6.5V4h5v2.5M8 10v7M12 10v7M16 10v7M6.5 6.5 7.5 20h9l1-13.5", destructive: true, onSelect: session.clearActivePage },
          // Undo covers this: deleting drops the manifest reference and the page's own files stay
          // on disk. That is why there is no trash bin.
          { kind: "action", id: "delete", label: $t("workspace.deletePage"), icon: "M5 4.5h14v15H5zM9 9l6 6M15 9l-6 6", destructive: true, disabled: !many, onSelect: session.deleteActivePage },
        ],
      },
      {
        title: $t("workspace.notebook"),
        entries: [
          { kind: "action", id: "search", label: $t("workspace.searchNotebook"), icon: "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM20 20l-3.6-3.6", hint: "Ctrl F", onSelect: () => (searchOpen = true) },
          { kind: "action", id: "style", label: $t("workspace.editSharedTypstStyle"), icon: "M5 4.5h14M12 4.5v15M8 19.5h8", onSelect: openSharedStyle },
          { kind: "action", id: "settings", label: $t("library.settings"), hint: "Ctrl ,", icon: "M4 8h8M17 8h3M4 16h3M12 16h8M14.5 5.8v4.4M9.5 13.8v4.4", onSelect: () => (settingsOpen = true) },
          { kind: "action", id: "save", label: $t("workspace.confirmSaved"), icon: "M5 12.5l4.5 4.5L19 7.5", onSelect: () => void session.persist() },
          { kind: "action", id: "close", label: $t("workspace.closeNotebook"), icon: "M14 5h5v14h-5M13 12H4m0 0 3.5-3.5M4 12l3.5 3.5", onSelect: session.closePage },
        ],
      },
    ]);
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
        void session.addPage(position, { kind: "template", template: source }, geometry, count),
    });
    // The notebook's paper for new pages: what it was created with, then whatever was added last
    // (Rust moves it on every add with a chosen paper). The panel opens on it.
    const next = session.notebookManifest?.defaultPage ?? null;
    const nextUsable = next && next.background.kind !== "pdf" ? next : null;
    const paperPreview = (paper: PageDefaults) =>
      paper.background.kind === "template"
        ? templatePreviewSvg(paper.background.template, paper.geometry)
        : paper.background.kind === "plain"
          ? templatePreviewSvg(
              { id: "next-plain", name: $t("workspace.plain"), backgroundColor: paper.background.color, elements: [] },
              paper.geometry,
            )
          : undefined;
    const paperName = (paper: PageDefaults) =>
      paper.background.kind === "template" ? paper.background.template.name : $t("workspace.plain");
    return [
      {
        id: "import",
        title: $t("library.import"),
        lane: "import",
        sources: [
          {
            id: "pdf",
            label: $t("workspace.importPDF"),
            detail: $t("workspace.bringsInEveryPage"),
            disabled: !tauriAvailable,
            compact: true,
            preview:
              '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48"><rect x="8" y="5" width="29" height="38" rx="3" fill="#fff" stroke="#9aa3ad"/><path d="M29 5v10h8" fill="none" stroke="#9aa3ad"/><rect x="12" y="25" width="29" height="13" rx="3" fill="#d85b55"/><text x="26.5" y="34.5" fill="#fff" font-size="8" font-family="sans-serif" font-weight="700" text-anchor="middle">PDF</text></svg>',
            onSelect: (position) => void session.importPdf(position),
          },
        ],
      },
      {
        id: "current",
        title: $t("workspace.notebookPaper"),
        lane: "current",
        sources: [
          ...(nextUsable
            ? [
                {
                  id: "next",
                  label: $t("workspace.notebookPaper"),
                  detail: `${paperName(nextUsable)}, ${describeGeometry(nextUsable.geometry, $t)}`,
                  preview: paperPreview(nextUsable),
                  onSelect: (position: PagePosition, count: number) =>
                    void session.addPage(position, nextUsable.background, nextUsable.geometry, count),
                } satisfies AddPageSource,
              ]
            : []),
        ],
      },
      ...templateGroups(PAPER_TONES.find((paper) => paper.id === addPageToneId) ?? PAPER_TONES[0], $t).map((group) => ({
        id: group.id,
        title: group.title,
        sources: group.templates.map(template),
      })),
    ];
  }

  async function compileTypst(id: string, request: TypstCompileRequest) {
    const requestRoot = session.root, requestGeneration = session.notebookGeneration, pageId = session.activePageId;
    const result = await previews.compile({ root: requestRoot, pageId, blockId: id,
      current: () => session.root === requestRoot && session.notebookGeneration === requestGeneration && session.activePageId === pageId }, request, true);
    if (!result) return;
    if (session.pageTypst?.id === id) session.pageTypst = { ...session.pageTypst, result };
    else session.typstBlocks = session.typstBlocks.map(block => block.id === id ? { ...block, result } : block);
  }

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
    selection.selectionReturnTo = null;
    // A brush owns the page, so anything still selected would sit there showing handles and
    // refusing to move. Dropping it is the honest half of that: the selection is gone because
    // the tool that acted on it is.
    if (!keepsSelection(next)) {
      selection.lassoObjectIds = [];
      selection.updateInkSelection([]);
      selection.selectedTypstId = null;
      selection.selectedImageId = null;
      selection.selectedShapeId = null;
      selection.editingShapeId = null;
    }
    session.status = notice("workspace.active", { "value1": next === "pen" ? notice("workspace.pen2", { "penPreset": penPreset }) : toolMessage(next) });
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
    const shape = selectedShapes;
    const widths = [...ink.map((stroke) => stroke.widthPt), ...shape.map(shape => shape.style.strokeWidthPt)];
    const colors = [...ink.map((stroke) => stroke.color.toLowerCase()), ...shape.map(shape => shape.style.strokeColor.toLowerCase())];
    const pens = ink.filter((stroke) => stroke.tool === "pen");
    const patterns = [...pens.map((stroke) => stroke.pattern ?? "solid"), ...shape.map(shape => shape.style.pattern ?? "solid")];
    const spacings = [...pens.map((stroke) => stroke.patternSpacing ?? 1), ...shape.map(shape => shape.style.patternSpacing ?? 1)];
    return {
      widthPt: shared(widths),
      color: shared(colors),
      pattern: shared(patterns) ?? "solid",
      spacing: shared(spacings) ?? 1,
    };
  });

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
      selection.restyleSelection({
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
      selection.restyleSelection({ patternSpacing: spacing });
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
      selection.restyleSelection({ pattern });
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
      selection.restyleSelection({ color });
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
      session.status = notice("workspace.thePaletteHoldsAtMostColors", { "MAX_SWATCHES": MAX_SWATCHES });
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
  /// A new chip arrives whole: its width, and for a pen its pattern and spacing, in one write.
  function addWidth(widthPt: number, pattern: StrokePattern = "solid", spacing = 1) {
    const widths = settings[widthKey()];
    const next = addRowWidth(widths, widthPt, MAX_WIDTHS);
    if (next === widths) session.status = notice("workspace.thePaletteHoldsAtMostWidths", { "MAX_WIDTHS": MAX_WIDTHS });
    putWidths(
      next,
      widthPt,
      withWidthPattern(settings.penWidthPatterns, widthPt, pattern),
      withWidthSpacing(settings.penWidthSpacings, widthPt, spacing),
    );
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
      session.status = notice("workspace.thePaletteKeepsAtLeastOneWidth");
      return;
    }
    putWidths(remaining, nearestChip(remaining, activeWidth));
  }

  function removeSwatch(index: number) {
    const key = tool === "highlighter" ? "highlighterSwatches" : "penSwatches";
    if (settings[key].length <= 1) {
      session.status = notice("workspace.thePaletteKeepsAtLeastOneColor");
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

  $effect(() => {
    settings.touchHistory;
    session.root;
    session.activePageId;
    navigation.touchTaps.reset();
    navigation.tapScrollOrigin = null;
  });

  /// The context bar is the selection toolbar, opened at the pointer instead of over the
  /// selection, so right-click and the lasso offer the same controls in the same place style.
  $effect(() => {
    const menu = selection.contextMenu;
    const bar = selection.contextBarElement;
    if (!menu || !bar || !workspace) {
      selection.contextBarPosition = { left: 0, top: 0, ready: false };
      return;
    }
    const placed = placeFloatingToolbar(
      { left: menu.x, top: menu.y, right: menu.x, bottom: menu.y, width: 0, height: 0 },
      bar.getBoundingClientRect(),
      workspace.getBoundingClientRect(),
    );
    selection.contextBarPosition = { left: placed.left, top: placed.top, ready: true };
  });

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
      zIndex: session.nextVisualZIndex(),
      readingOrder: session.nextReadingOrder(),
      geometry: draft.geometry,
      style: draft.style,
      ...overrides,
    };
  }

  const onResizeToolbar = () => selection.scheduleSelectionToolbar();

  function historyShortcut(event: KeyboardEvent) {
    if (event.defaultPrevented) return;
    // Every shortcut below edits, navigates or replaces the page; none may race a replacing reply.
    if (session.pageInputLocked) return;
    if (event.ctrlKey || event.metaKey) {
      if (event.key === "Tab") {
        const next = cycledTab(session.openTabs, session.root, event.shiftKey ? -1 : 1);
        if (next) {
          event.preventDefault();
          void session.openNotebookAt(next);
        }
        return;
      }
      if (event.key.toLowerCase() === "w" && session.openTabs.length > 0) {
        event.preventDefault();
        void session.closeNotebookTab(session.root);
        return;
      }
      const zoomShortcuts: Record<string, () => void> = {
        "+": () => navigation.changeZoom(zoom + 0.1),
        "=": () => navigation.changeZoom(zoom + 0.1),
        "-": () => navigation.changeZoom(zoom - 0.1),
        "0": () => navigation.changeZoom(1),
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
    if (event.key === "Escape" && (toolCard || shapeCard || lassoEditor)) {
      event.preventDefault();
      closeToolCard();
      return;
    }
    const editingText =
      event.target instanceof Element &&
      event.target.closest(".cm-editor, input, textarea, [contenteditable=true]");
    if (editingText) return;
    if (event.key === "Escape") {
      selection.lassoObjectIds = [];
      selection.updateInkSelection([]);
      searchOpen = false;
      settingsOpen = false;
      moreOpen = false;
      addPageOpen = false;
      metricsOpen = false;
      selection.selectedTypstId = null;
      selection.selectedImageId = null;
      selection.selectedShapeId = null;
      selection.editingShapeId = null;
      selection.directObjectInput = false;
      return;
    }
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") {
      event.preventDefault();
      searchOpen = true;
      return;
    }
    if ((event.ctrlKey || event.metaKey) && !event.shiftKey && !event.altKey) {
      const key = event.key.toLowerCase();
      if ((key === "c" || key === "x" || key === "d") && selection.hasCopyableSelection()) {
        event.preventDefault();
        if (key === "d") void selection.duplicateSelection();
        else void selection.copySelection(key === "x");
        return;
      }
    }
    if (
      (event.ctrlKey || event.metaKey) &&
      event.shiftKey &&
      (event.key === "PageUp" || event.key === "PageDown")
    ) {
      event.preventDefault();
      void session.moveActivePage(event.key === "PageUp" ? -1 : 1);
      return;
    }
    if (!event.ctrlKey && !event.metaKey && !event.altKey) {
      if (event.key === "Enter" && selection.selectedShapeId) {
        event.preventDefault();
        selection.editingShapeId = selection.editingShapeId === selection.selectedShapeId ? null : selection.selectedShapeId;
        session.status = selection.editingShapeId ? notice("workspace.editingShapeAnchorPoints") : notice("workspace.shapeSelected");
        return;
      }
      if (event.key === "Delete" || event.key === "Backspace") {
        if (selection.deleteSelection()) event.preventDefault();
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
        if (selection.nudgeSelection(arrows[event.key][0] * step, arrows[event.key][1] * step)) {
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
        t: session.addTypstBlock,
      };
      if (shortcuts[event.key]) {
        event.preventDefault();
        shortcuts[event.key]();
      }
      return;
    }
    if (!(event.ctrlKey || event.metaKey)) return;
    if (event.key.toLowerCase() === "z" && event.shiftKey && session.canRedo) {
      event.preventDefault();
      session.redo();
    } else if (event.key.toLowerCase() === "z" && session.canUndo) {
      event.preventDefault();
      session.undo();
    } else if (event.key.toLowerCase() === "y" && session.canRedo) {
      event.preventDefault();
      session.redo();
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

  function currentToolLabel() {
    if (tool === "pen") return $t("workspace.pen2", { "penPreset": penPreset });
    if (tool === "select") return $t("workspace.inkSelection");
    return toolLabel(tool, $t);
  }

  /// Read from the live preset, never written by hand. These used to be three fixed strings, so
  /// the strip kept claiming "0.35 mm · graphite" after the writer had changed both, and the
  /// widths it named did not match the defaults either.
  function nibDetail(preset: PenPreset) {
    const pattern = preset.pattern && preset.pattern !== "solid" ? ` · ${patternLabel(preset.pattern, $t)}` : "";
    return $t("workspace.mm", { "value1": formatNumber(preset.widthPt * MM_PER_PT, { minimumFractionDigits: 2, maximumFractionDigits: 2 }, $displayLocale), "value2": colorName(preset.color, $t).toLowerCase(), "pattern": pattern });
  }

  function currentToolDetail() {
    if (tool === "pen") return nibDetail(settings.penPresets[penPreset - 1]);
    if (tool === "highlighter") return nibDetail(settings.highlighter);
    if (tool === "eraser") return $t(settings.eraserMode === "precise" ? "workspace.preciseErasing" : "workspace.wholeStrokes");
    if (tool === "shape") return settings.shapeKind === "ellipse" ? $t("workspace.circleEllipse") : shapeLabel(settings.shapeKind, $t);
    if (tool === "lasso" || tool === "select") return $t("workspace.selected", { "value1": selection.selectedStrokeIds.length + selection.lassoObjectIds.length });
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
      ? $t("workspace.ms", { "value1": summary.median.toFixed(1), "value2": summary.p95.toFixed(1), "value3": summary.worst.toFixed(1) })
      : $t("workspace.notMeasured");
  }

  function milliseconds(value: number | null): string {
    return value === null ? $t("workspace.notMeasured") : $t("workspace.ms2", { "value1": value.toFixed(1) });
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
        save: session.saveMs,
        reopen: session.reopenMs,
        export: session.exportMs,
      },
    };
  }

  function message(error: unknown) {
    return errorDetails(error);
  }

</script>

{#snippet typstSizeControls()}
  {#if sideEditorBlockId}
    {@const block = session.typstBlocks.find(candidate => candidate.id === sideEditorBlockId)}
    {#if block}
      {#key block.id}
        <TypstSizeControls scale={block.transform.scale} layoutWidthPt={block.transform.layoutWidthPt}
          onChange={change => session.updateTypstTransform(block.id, { ...block.transform, ...change })} />
      {/key}
    {/if}
  {/if}
{/snippet}

<main class="workspace-app" onpointerdowncapture={selection.closeObjectSelection} onwheel={navigation.wheelZoom}>
  {#if settingsReady}
  {#if !session.notebookChosen}
    <div class="start-slot">
      <LibrarySurface
        {tauriAvailable}
        onSettings={() => (settingsOpen = true)}
        coverRevision={session.coverRevision}
        showNotebookSetup={showInitialSetup}
        location={shelfLocation}
        onOpen={(nextRoot) => void session.openNotebookAt(nextRoot)}
        onCreate={(nextRoot, setup) => {
          showInitialSetup = false;
          void session.openNotebookAt(nextRoot, { createIfMissing: true, setup });
        }}
        onMove={session.moveLibrarySelection}
        returnLabel={session.openTabs.find((tab) => tab.root === session.root)?.title}
        onReturn={session.openTabs.length > 0 ? session.returnToNotebook : undefined}
        onLocationChange={(next) => (shelfLocation = next)}
        onStatus={(next) => (session.status = next)}
      />
    </div>
  {:else}
  <NotebookHeader tabs={session.openTabs} activeRoot={session.root} switchingRoot={session.switchingRoot} busy={session.busy} saving={savePending}
    warning={saveNeedsAttention} onSelect={session.openNotebookAt} onClose={session.closeNotebookTab}
    onLibrary={() => void session.showLibrary()}>
    {#snippet actions()}
    <div class="command-actions">
      {#if session.pageOpen}
        <button class="icon-button" type="button" aria-label={$t("workspace.exportPDF")} title={$t("workspace.exportPDF")} onclick={session.exportPdf} disabled={session.busy}>
          <!-- Arrow up and out of the tray: the page leaves Goodtype. Down read as "download",
               which is the one thing this button never does. -->
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 15V4m0 0-4 4m4-4 4 4M5 17v2a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-2" /></svg>
        </button>
      {:else}
        <button class="text-action" type="button" onclick={session.reopen} disabled={session.busy}>{$t("workspace.reopenNotebook")}</button>
      {/if}
      {#if session.pageOpen}
        <div class="add-page-anchor">
          <button class="icon-button" class:active={addPageOpen} type="button" aria-label={$t("workspace.addPage")} aria-expanded={addPageOpen} title={$t("workspace.addPage")} disabled={session.busy} onclick={() => (addPageOpen = !addPageOpen)}>
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M13 3.5H7a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h6"></path><path d="M18 9.5v9"></path><path d="M13.5 14h9"></path></svg>
          </button>
          {#if addPageOpen}
            <AddPageMenu
              where={addPageWhere}
              groups={addPageGroups()}
              tones={paperTones($t)}
              toneId={addPageToneId}
              sizes={pageSizes($t)}
              sizeId={addPageSizeId}
              orientation={addPageOrientation}
              previewAspect={addPageGeometry.widthPt / addPageGeometry.heightPt}
              currentPageId={session.activePageId}
              pageNumber={session.pageNumber}
              pageCount={session.pageCount}
              geometry={addPageGeometry}
              canPlaceRelative={session.pageCount > 0 && Boolean(session.activePageId)}
              onWhereChange={(next) => (addPageWhere = next)}
              onToneChange={(next) => (addPageToneId = next)}
              onSizeChange={(next) => (addPageSizeId = next)}
              onOrientationChange={(next) => (addPageOrientation = next)}
              onClose={() => (addPageOpen = false)}
            />
          {/if}
        </div>
      {/if}
      <button class="icon-button" class:active={searchOpen} type="button" aria-label={$t("workspace.searchTypedContent")} title={$t("workspace.searchCtrlF")} onclick={() => (searchOpen = !searchOpen)}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="7"></circle><path d="m20 20-3.6-3.6"></path></svg>
      </button>
      <button class="icon-button" class:active={sideEditorOpen} type="button" aria-label={$t("workspace.pageTextAndTypstSourceView")} aria-pressed={sideEditorOpen} title={$t("workspace.editorViewCtrlShiftE")} onclick={toggleSideEditor}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3.5" y="4.5" width="17" height="15" rx="2"></rect><path d="M10 4.5v15"></path></svg>
      </button>
      <button class="icon-button" class:active={moreOpen} type="button" aria-label={$t("workspace.moreNotebookActions")} aria-expanded={moreOpen} data-preserve-selection onclick={() => (moreOpen = !moreOpen)}>
        <svg class="solid-icon" viewBox="0 0 24 24" aria-hidden="true"><circle cx="5" cy="12" r="1.7"></circle><circle cx="12" cy="12" r="1.7"></circle><circle cx="19" cy="12" r="1.7"></circle></svg>
      </button>
    </div>
    {/snippet}
  </NotebookHeader>

  {#if moreOpen}
    <OverflowMenu
      title={session.pageOpen ? $t("workspace.pageOf2", { "pageNumber": session.pageNumber, "pageCount": session.pageCount }) : $t("workspace.notebook")}
      subtitle={session.root || $t("workspace.opening")}
      sections={menuSections()}
      onClose={() => (moreOpen = false)}
    />
  {/if}

  {#if session.pageOpen}
    <div class="workspace-split" class:panel-right={settings.sideEditorDock === "right"} inert={session.pageInputLocked}>
    {#if sideEditorOpen}
      <SideEditor
        bind:this={sideEditor}
        mode={sideEditorMode}
        source={templateEditor.draft ? templateEditor.draft.source : sideEditorStyle ? session.sharedStyleSource : (sideEditorBlock?.source ?? "")}
        styleTitle={templateEditor.draft ? $t("workspace.template2", { "name": templateEditor.draft.name }) : $t("workspace.notebookStyle")}
        blockLabel={sideEditorPageText
          ? $t("workspace.pageText")
          : sideEditorBlockId
            ? $t("workspace.typstBlock", { "sideEditorBlockId": sideEditorBlockId })
            : ""}
        pageNumber={sideEditorPageNumber}
        hasAnyBlock={Boolean(session.pageTypst) || session.typstBlocks.length > 0}
        root={session.root}
        dock={settings.sideEditorDock}
        width={settings.sideEditorWidth}
        diagnostics={sideEditorBlock?.result?.diagnostics ?? []}
        pageText={sideEditorPageText}
        pageTextLineWrap={settings.pageTextLineWrap}
        presets={session.presets}
        {presetBusy}
        blockControls={typstSizeControls}
        onChange={(next) =>
          templateEditor.draft
            ? updateTemplate(next)
            : sideEditorStyle
            ? session.updateSharedStyle(next)
            : sideEditorBlock && session.updateTypstSource(sideEditorBlock.id, next)}
        onClose={closeSideEditor}
        onDockChange={(dock) => changeSettings({ ...settings, sideEditorDock: dock })}
        onWidthChange={(next) => changeSettings({ ...settings, sideEditorWidth: next })}
        onGoToBlock={() => {
          if (sideEditorMode === "away" && sideEditorPageId) session.scrollToPage(sideEditorPageId);
          else if (session.typstBlocks[0]) openSideEditor(session.typstBlocks[0].id);
        }}
        onCreatePageText={openPageText}
        onCreateBlock={() => {
          session.addTypstBlock();
          void tick().then(() => {
            const added = session.typstBlocks.at(-1);
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
      aria-label={$t("workspace.fixedPageWorkspace")}
      onpointerdowncapture={navigation.workspacePointerDown}
      oncontextmenu={selection.canvasContextMenu}
      onpointermovecapture={navigation.workspacePointerMove}
      onpointerupcapture={navigation.workspacePointerEnd}
      onpointercancelcapture={navigation.workspacePointerEnd}
      onlostpointercapture={navigation.releasePagePointer}
    >
      <div class="page-scroll-content" bind:this={pageViewport} onscroll={() => { session.lastScrollAt = performance.now(); selection.scheduleSelectionToolbar(); session.scheduleFocus(session.FOCUS_SETTLE_MS); }}>
        <div
          class="page-pan-field"
          style:--page-zoom={zoom}
          class:pulling={navigation.pull !== null}
          style:transform={navigation.pullShift > 0 ? `translateY(${-navigation.pullShift}px)` : undefined}
        >
        {#each session.pageEntries as entry, index (entry.id)}
          {@const active = entry.id === session.activePageId}
          <!-- The active page's own state wins; a neighbour that has loaded knows its real size,
               and one that has not falls back to the manifest's hint so the scroller still
               reserves the right amount of room for it. -->
          {@const box = active ? session.activeGeometry : (entry.snapshot?.page.geometry ?? entry.geometry)}
          <article
            class:active-page={active}
            class="page-stack-item"
            data-page-id={entry.id}
            aria-label={$t("workspace.page4", { "value1": index + 1 })}
            aria-current={active ? "page" : undefined}
            use:observePage={entry.id}
          >
            <!-- The arrows sit on every page rather than only the one in view. They used to be
                 mounted when a page became active, which made them pop in mid-scroll and — because
                 this row is anchored by its right edge — shove the page number leftwards as they
                 arrived. Two inert buttons cost less than that, and only the page you are on can
                 be moved, so elsewhere they are disabled rather than absent. -->
            <div class="page-number" class:idle={!active}>
              <span>{$t("workspace.page", { "value2": index + 1 })}</span>
              {#if session.pageCount > 1}
                <button
                  type="button"
                  aria-label={$t("workspace.movePageUp2", { "value1": index + 1 })}
                  title={active ? $t("workspace.movePageUpCtrlShiftPageUp") : $t("workspace.scrollToThisPageToMoveIt")}
                  disabled={!active || index === 0 || session.busy}
                  onclick={() => void session.moveActivePage(-1)}
                >&uarr;</button>
                <button
                  type="button"
                  aria-label={$t("workspace.movePageDown2", { "value1": index + 1 })}
                  title={active ? $t("workspace.movePageDownCtrlShiftPageDown") : $t("workspace.scrollToThisPageToMoveIt")}
                  disabled={!active || index === session.pageCount - 1 || session.busy}
                  onclick={() => void session.moveActivePage(1)}
                >&darr;</button>
              {/if}
            </div>
            <div class="page-frame" use:trackActiveFrame={active} style:--page-width={`${box.widthPt}px`} style:--page-height={`${box.heightPt}px`} style:width="calc(var(--page-width) * var(--page-zoom))" style:height="calc(var(--page-height) * var(--page-zoom))">
              <div class="page" style:width={`${box.widthPt}px`} style:height={`${box.heightPt}px`} style:transform="scale(var(--page-zoom))">
                {#if active || entry.snapshot}
                  <PageSurface
                    blocks={active ? activeBlockViews : blockViewsFromSnapshot(entry.snapshot!)}
                    pageTypst={active
                      ? activePageTypstView
                      : pageTypstViewFromSnapshot(entry.snapshot!)}
                    images={active ? activeImageViews : imageViewsFromSnapshot(entry.snapshot!, session.assetUrls(entry.id))}
                    shapes={active ? session.shapes : shapeViewsFromSnapshot(entry.snapshot!)}
                    results={active ? activeResults : (session.neighborResults[entry.id] ?? {})}
                    strokes={active ? session.strokes : session.neighborStrokesFor(entry)}
                    newStrokeZIndex={newInkZIndex(
                      active ? [...session.typstBlocks, ...session.images, ...session.shapes].map((object) => object.zIndex)
                        : entry.snapshot!.page.objects.map((object) => object.zIndex),
                      active ? session.strokes : session.neighborStrokesFor(entry),
                      tool === "highlighter" && settings.highlighterBehindInk,
                    )}
                    selectedStrokeIds={active ? selection.selectedStrokeIds : []}
                    strokePreview={active ? selection.mixedStrokePreview : null}
                    lassoTargets={settings.lassoTargets ?? DEFAULT_LASSO_TARGETS}
                    onLassoSelect={active ? selection.lassoSelection : undefined}
                    showSelectionBounds={!active || selection.lassoObjectIds.length === 0}
                    background={active ? session.activeBackground : (entry.snapshot?.page.background ?? { kind: "plain", color: "#ffffff" })}
                    pageWidthPt={box.widthPt}
                    pageHeightPt={box.heightPt}
                    {zoom}
                    interactive={active}
                    root={session.root}
                    sharedStyle={session.sharedStyleSource}
                    pageTextBaselineGrid={settings.pageTextBaselineGrid}
                    presetRevision={session.presetRevision}
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
                    eraserMode={settings.eraserMode}
                    erasable={(stroke) => !stroke.groupId || (active && stroke.groupId === selection.mixedGroup?.inkGroupId)}
                    calibration={settings.calibration}
                    {pointerMapping}
                    onEraseEnd={returnFromEraser}
                    onEraserEngaged={() => activateTool("eraser")}
                    directObjectInput={active && selection.directObjectInput}
                    selectedBlockId={active ? selection.selectedTypstId : null}
                    selectedImageId={active ? selection.selectedImageId : null}
                    selectedShapeId={active ? selection.selectedShapeId : null}
                    shapesGrabbable={tool !== "eraser"}
                    editingShapeId={active ? selection.editingShapeId : null}
                    onCompile={(id, request) =>
                      active
                        ? compileTypst(id, request)
                        : session.compileNeighborTypst(entry.id, id, request)}
                    onSourceChange={(id, source) => session.updateTypstSource(id, source)}
                    onTransform={(id, transform) => session.updateTypstTransform(id, transform)}
                    onSelectBlock={(id) => {
                      selection.selectedTypstId = id;
                      selection.selectedImageId = null;
                      selection.selectedShapeId = null;
                      selection.editingShapeId = null;
                    }}
                    onDeselectBlock={() => (selection.selectedTypstId = null)}
                    onSelectImage={(id) => {
                      selection.selectedImageId = id;
                      selection.selectedTypstId = null;
                      selection.selectedShapeId = null;
                      selection.editingShapeId = null;
                    }}
                    onMoveImage={(id, position) => session.changeImage(id, position)}
                    onScaleImage={(id, scale) => session.changeImage(id, { scale })}
                    onSelectShape={(id) => {
                      selection.selectedShapeId = id;
                      selection.selectedImageId = null;
                      selection.selectedTypstId = null;
                      selection.updateInkSelection([]);
                    }}
                    onChangeShape={active ? session.changeShape : undefined}
                    onEditShape={(id, editing) => {
                      selection.selectedShapeId = id;
                      selection.editingShapeId = editing ? id : null;
                      session.status = editing ? $t("workspace.editingShapeAnchorPoints") : $t("workspace.shapeSelected");
                    }}
                    onStrokeFinalized={(stroke) =>
                      active
                        ? session.addStroke(stroke)
                        : session.commitNeighborInk(entry.id, [...session.neighborStrokesFor(entry), stroke], $t("workspace.addedInk"))}
                    onEraseShape={active ? session.eraseShape : undefined}
                    onShapesErased={active ? session.replaceErasedShapes : undefined}
                    onShapeFinalized={active ? session.addShape : undefined}
                    onInkShapeRecognized={active ? session.promoteHeldInk : undefined}
                    onStrokesChange={(next, replaced) => {
                      const label = notice(replaced ? "workspace.erasedInk" : "workspace.updatedInk");
                      if (active) session.changeStrokes(next, label, replaced);
                      else session.commitNeighborInk(entry.id, next, label);
                    }}
                    onSelectionChange={(next) => {
                      if (active) selection.updateInkSelection(next);
                    }}
                    onStrokeMetrics={collectMetrics
                      ? (metrics) => {
                          if (active) recordStrokeMetrics(metrics);
                        }
                      : undefined}
                  />
                  {#if active && selection.mixedSelectionBounds}
                    <MixedSelection bounds={selection.mixedSelectionBounds} {zoom}
                      onStart={selection.beginMixedTransform} onPreview={selection.previewMixedTransform} onEnd={selection.endMixedTransform} />
                  {/if}
                {:else}
                  <span class="page-loading">{$t("workspace.loadingPage")}</span>
                {/if}
              </div>
            </div>
          </article>
        {/each}
        </div>
      </div>

      {#if navigation.touchHistoryToast}
        {#key navigation.touchHistoryToast}
          <div class="touch-history-toast" class:below-palette={paletteDock === "top"}
            class:reduced-motion={settings.reducedMotion} role="status" aria-live="polite">
            {renderMessage(navigation.touchHistoryToast.label, $t)}
          </div>
        {/key}
      {/if}

      {#if navigation.pull}
        <!-- Above the palette when it is docked at the bottom, so the bar never hides the ring. -->
        <div
          class="pull-badge"
          class:ready={navigation.pullFill >= 1}
          class:over-palette={paletteDock === "bottom"}
          style:opacity={Math.min(1, navigation.pullFill * 3)}
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
              stroke-dasharray={`${navigation.pullFill * 100} 100`}
              transform="rotate(-90 20 20)"
            />
            {#if navigation.pullFill >= 1}
              <path class="mark" d="M13 20.5l5 5 9-10" />
            {:else}
              <path class="mark" d="M20 13v14M13 20h14" />
            {/if}
          </svg>
          <span>
            {navigation.pullFill >= 1
              ? $t("workspace.releaseToAdd", { "value1": navigation.pullPaper ? (navigation.pullPaper.background.kind === "template" ? navigation.pullPaper.background.template.name : $t("workspace.plain")) : $t("workspace.newPage") })
              : $t("workspace.pullToAddAPage")}
          </span>
        </div>
      {/if}

      <NotebookPalette {workspace} {settings} {changeSettings} onStatus={next => { session.status = next; }}
        bind:paletteDock bind:paletteElement bind:paletteDragging {closeToolCard}
        onUndo={session.undo} onRedo={session.redo}
        undoDisabled={session.busy || session.pendingTransactions > 0 || !session.canUndo}
        redoDisabled={session.busy || session.pendingTransactions > 0 || !session.canRedo}>
        {#snippet cards()}
        {#if eraserEditor}
          <PaletteEditor title={$t("workspace.eraser")} anchor={eraserEditor} dock={paletteDock}
            onClose={closeEraserEditor} onDismiss={closeToolCard}>
            <EraserPanel precise={settings.eraserMode === "precise"}
              onPreciseChange={(enabled) => changeSettings({ ...settings, eraserMode: enabled ? "precise" : "stroke" })}
              highlightsOnly={settings.eraserReach === "highlighter"} returns={settings.eraserReturns}
              onHighlightsChange={(enabled) => changeSettings({ ...settings, eraserReach: enabled ? "highlighter" : "everything" })}
              onReturnsChange={(eraserReturns) => changeSettings({ ...settings, eraserReturns })} />
          </PaletteEditor>
        {/if}
        {#if lassoEditor}
          <PaletteEditor title={$t("workspace.lasso")} anchor={lassoEditor} dock={paletteDock} onClose={closeLassoEditor} onDismiss={closeToolCard}>
            <LassoPanel targets={settings.lassoTargets ?? DEFAULT_LASSO_TARGETS}
              onChange={lassoTargets => changeSettings({ ...settings, lassoTargets })} />
          </PaletteEditor>
        {/if}
        {#if paletteEditor}
          {#key paletteEditor.anchor}
            <PaletteEditor title={paletteEditor.kind === "width" ? (paletteEditor.adding ? $t("workspace.addStroke") : $t("workspace.stroke")) : paletteEditor.adding ? $t("workspace.addColour") : $t("workspace.colour")}
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
                  adding={paletteEditor.adding}
                  taken={(width) => !restyling && widthTaken(activeWidthChips, width, paletteEditor?.index ?? -1)}
                  onCommit={(width) => { if (!paletteEditor) return; if (restyling) setActiveWidth(width); else if (paletteEditor.index < 0) addWidth(width); else editWidth(paletteEditor.index, width); closePaletteEditor(); }}
                  onAdd={(width, pattern, spacing) => { addWidth(width, pattern, spacing); closePaletteEditor(); }}
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
              label={toolCard.kind === "highlighter" ? $t("workspace.highlighter") : $t("workspace.pen5", { "slot": toolCard.slot })}
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
        {/snippet}
        {#snippet tools()}

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
              tag={restyling ? $t("workspace.selection") : undefined}
              matchPattern={restyling}
              expandedEditor={paletteEditor?.adding ? null : paletteEditor?.kind ?? null}
              addingColor={paletteEditor?.kind === "color" && paletteEditor.adding}
              addingWidth={paletteEditor?.kind === "width" && paletteEditor.adding}
              onPickWidth={(width) => { closeToolCard(); setActiveWidth(width); }}
              onPickColor={(color) => { closeToolCard(); setActiveColor(color); }}
              onEditWidth={(index, tile) => openPaletteEditor("width", index, tile)}
              onEditColor={(index, tile) => openPaletteEditor("color", index, tile)}
              onPickEraserSize={setEraserSize}
              canAddColor={activeColorChips.length < MAX_SWATCHES}
              onAddColor={(tile) => openPaletteEditor("color", -1, tile, true)}
              canAddWidth={activeWidthChips.length < MAX_WIDTHS}
              onAddWidth={(tile) => openPaletteEditor("width", -1, tile, true)}
            />
          {/if}        {/snippet}

      {#if selection.contextMenu}
        <div class="context-actions" use:dismissable={{ onClose: () => (selection.contextMenu = null), swallow: true }}>
          {#if selection.selectedStrokeIds.length > 0 || selection.groupedStrokeIds.length > 0 || selection.lassoObjectIds.length > 0}
            <SelectionActions
              bind:element={selection.contextBarElement}
              subject={selection.lassoObjectIds.length ? "items" : "ink"}
              left={selection.contextBarPosition.left}
              top={selection.contextBarPosition.top}
              ready={selection.contextBarPosition.ready}
              canMoveBack={selection.canMoveInkVisual(-1)}
              canMoveForward={selection.canMoveInkVisual(1)}
              onMove={(direction) => selection.fromContext(() => selection.changeInkVisualOrder(direction))()}
              onCut={selection.fromContext(() => selection.copySelection(true))}
              onCopy={selection.fromContext(() => selection.copySelection())}
              onPaste={selection.fromContext((at) => selection.clipboard && selection.pasteBundle(selection.clipboard, at))}
              pasteDisabled={!selection.clipboard}
              onDuplicate={selection.fromContext(selection.duplicateSelection)}
              onDelete={selection.fromContext(() => selection.deleteSelection())}
            />
          {:else if selection.selectedImageId || selection.selectedTypstId || selection.selectedShapeId}
            <SelectionActions
              bind:element={selection.contextBarElement}
              subject={selection.selectedImageId ? "image" : selection.selectedShapeId ? "shape" : "Typst block"}
              left={selection.contextBarPosition.left}
              top={selection.contextBarPosition.top}
              ready={selection.contextBarPosition.ready}
              canMoveBack={selection.canMoveVisual(-1)}
              canMoveForward={selection.canMoveVisual(1)}
              onMove={(direction) => selection.fromContext(() => selection.changeVisualOrder(direction < 0 ? "backward" : "forward"))()}
              onCut={selection.fromContext(() => selection.copySelection(true))}
              onCopy={selection.fromContext(() => selection.copySelection())}
              onPaste={selection.fromContext((at) => selection.clipboard && selection.pasteBundle(selection.clipboard, at))}
              pasteDisabled={!selection.clipboard}
              onDuplicate={selection.fromContext(selection.duplicateSelection)}
              onDelete={selection.fromContext(() => selection.deleteSelection())}
            />
          {:else}
            <SelectionActions
              bind:element={selection.contextBarElement}
              subject="page"
              left={selection.contextBarPosition.left}
              top={selection.contextBarPosition.top}
              ready={selection.contextBarPosition.ready}
              onPaste={selection.fromContext((at) => selection.clipboard && selection.pasteBundle(selection.clipboard, at))}
              pasteDisabled={!selection.clipboard}
            />
          {/if}
        </div>
      {:else if selection.selectedStrokeIds.length > 0 || selection.groupedStrokeIds.length > 0 || selection.lassoObjectIds.length > 0}
        <SelectionActions
          bind:element={selection.selectionToolbarElement}
          subject={selection.lassoObjectIds.length ? "items" : "ink"}
          left={selection.selectionToolbarPosition.left}
          top={selection.selectionToolbarPosition.top}
          ready={selection.selectionToolbarPosition.ready}
          canMoveBack={selection.canMoveInkVisual(-1)}
          canMoveForward={selection.canMoveInkVisual(1)}
          onMove={selection.changeInkVisualOrder}
          grouped={selection.groupedStrokeIds.length > 0}
          onGroup={selection.lassoObjectIds.length ? undefined : selection.groupedStrokeIds.length > 0
            ? selection.ungroupInk
            : selection.selectedTypstId
              ? selection.groupSelectedInk
              : undefined}
          onConvert={!selection.lassoObjectIds.length && selection.selectedStrokeIds.length === 1 ? session.convertSelectedInkToShape : undefined}
          onCopy={() => void selection.copySelection()}
          onDuplicate={() => void selection.duplicateSelection()}
          onDelete={selection.hasCopyableSelection() ? () => void selection.deleteSelection() : undefined}
        />
      {:else if selection.selectedImageId || selection.selectedTypstId || selection.selectedShapeId}
        <SelectionActions
          bind:element={selection.selectionToolbarElement}
          subject={selection.selectedImageId ? "image" : selection.selectedShapeId ? "shape" : "Typst block"}
          left={selection.selectionToolbarPosition.left}
          top={selection.selectionToolbarPosition.top}
          ready={selection.selectionToolbarPosition.ready}
          canMoveBack={selection.canMoveVisual(-1)}
          canMoveForward={selection.canMoveVisual(1)}
          onMove={(direction) => selection.changeVisualOrder(direction < 0 ? "backward" : "forward")}
          onCopy={() => void selection.copySelection()}
          onDuplicate={() => void selection.duplicateSelection()}
          onDelete={() => void selection.deleteSelection()}
        />
      {/if}

      </NotebookPalette>
    </section>
    </div>
  {:else}
    <section class="closed-state">
      <span class="closed-mark" aria-hidden="true"></span><h1>{$t("workspace.notebookClosed")}</h1>
      <p>{$t("workspace.theConfirmedLocalFilesAreStillSafe")}</p>
      <button type="button" onclick={session.reopen} disabled={session.busy}>{$t("workspace.reopenNotebook")}</button>
    </section>
  {/if}

  <WorkspaceStatus toolLabel={currentToolLabel()} toolDetail={currentToolDetail()} status={session.status} transactionFailed={session.transactionFailed}
    pageNumber={session.activePageNumber()} pageCount={session.notebookManifest?.pages.length ?? 1} {zoom}
    {saveNeedsAttention} {savePending} onZoom={navigation.changeZoom} />

  {#if searchOpen}
    <SearchOverlay
      root={session.root}
      {tauriAvailable}
      onNavigate={(hit) => void session.navigateToSearchHit(hit)}
      onClose={() => (searchOpen = false)}
    />
  {/if}

  {#if session.conflictDetail}
    <ConflictDialog
      detail={session.conflictDetail}
      onReload={() => {
        session.conflictDetail = null;
        void session.reopen();
      }}
      onCancel={() => (session.conflictDetail = null)}
    />
  {/if}
  {#if session.confirmingPageDelete !== null}
    <ConfirmDialog
      title={$t("workspace.deletePage3", { "confirmingPageDelete": session.confirmingPageDelete })}
      detail={$t("workspace.itsFilesAreKeptForRecoveryBut")}
      confirmLabel={$t("workspace.deletePage2")}
      onConfirm={() => void session.confirmDeleteActivePage()}
      onCancel={() => (session.confirmingPageDelete = null)}
    />
  {/if}

  {#if session.recoveryOpen && session.recoveryCandidates.length > 0}
    <RecoveryDialog
      candidates={session.recoveryCandidates}
      busy={session.recoveryBusy}
      onRestore={(fileName) => void session.restoreRecovery(fileName)}
      onDiscard={(fileName) => void session.discardRecovery(fileName)}
      onClose={() => (session.recoveryOpen = false)}
    />
  {/if}
  {/if}

  {#if settingsOpen}
    <SettingsPanel
      selectedBlockFont={selection.selectedTypstId ? blockFont(session.typstBlocks.find(block => block.id === selection.selectedTypstId)?.source ?? "") : null}
      onFontChange={changeInlineFont}
      {settings}
      actions={{ openMetrics: () => (metricsOpen = true) }}
      onChange={changeSettings}
      onClose={() => (settingsOpen = false)}
    />
  {/if}

  {#if collectMetrics && metricsOpen}
    <div class="panel-scrim" role="presentation">
      <aside class="diagnostics-panel" aria-label={$t("workspace.timingEvidence")}>
        <div class="panel-heading">
          <div><span>{$t("workspace.localDiagnostics")}</span><h2>{$t("workspace.timingEvidence")}</h2></div>
          <button class="icon-button" type="button" aria-label={$t("workspace.closeTimingEvidence")} onclick={() => (metricsOpen = false)}>×</button>
        </div>
        <p class="diagnostic-path">{session.root}\.goodtype\phase0-metrics.json</p>
        <dl>
          <dt>{$t("workspace.recordedStrokes")}</dt><dd>{strokeMetrics.length}</dd>
          <dt>{$t("workspace.activeFeedbackMedianPWorst")}</dt><dd>{timingSummary("activeFeedbackMs")}</dd>
          <dt>{$t("workspace.maximumSampleGapMedianPWorst")}</dt><dd>{timingSummary("maxSampleGapMs")}</dd>
          <dt>{$t("workspace.strokeCommitMedianPWorst")}</dt><dd>{timingSummary("commitMs")}</dd>
          <dt>{$t("workspace.latestTypstCompile")}</dt><dd>{$t("workspace.msDebounce", { "value1": milliseconds(compileMs), "TYPST_IDLE_DEBOUNCE_MS": TYPST_IDLE_DEBOUNCE_MS })}</dd>
          <dt>{$t("workspace.latestZoomFrame")}</dt><dd>{milliseconds(zoomFrameMs)}</dd>
          <dt>{$t("workspace.saveReopenExport")}</dt><dd>{milliseconds(session.saveMs)} / {milliseconds(session.reopenMs)} / {milliseconds(session.exportMs)}</dd>
        </dl>
      </aside>
    </div>
  {/if}
  <p class="screen-reader-status" aria-live="polite">{renderMessage(session.status, $t)}</p>
  {/if}
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

  .command-actions { display: flex; align-items: center; flex: none; gap: 4px; }

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

  .command-actions svg { width: var(--icon); height: var(--icon); }
  /* Filled glyphs — the overflow dots, the pen nib — are shapes, not line drawings; everything
     else is a stroke at the one weight. */
  .command-actions svg:not(.solid-icon) {
    fill: none;
    stroke: currentColor;
    stroke-width: var(--stroke);
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .command-actions svg.solid-icon { fill: currentColor; }
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
  :global(.instrument-palette.dock-top) .palette-panel-anchor { top: calc(100% + 10px); bottom: auto; }
  /* Centred on the chip; the panel itself measures and nudges back inside the window, so no
     height is guessed here. */
  :global(.instrument-palette.dock-left) .palette-panel-anchor,
  :global(.instrument-palette.dock-right) .palette-panel-anchor {
    top: calc(var(--anchor) - 130px);
    bottom: auto;
    left: auto;
  }
  /* Side docks: the card cannot track the tile vertically. The rail is nearly as tall as the
     workspace, so a card anchored to a tile near its foot would hang off the bottom, and CSS
     cannot know how far down the page the rail actually starts. The rail is centred in the
     chrome, so centring the card on the rail centres it in the workspace — the one placement
     that always fits. The pointer sits at the card's middle, and the lit tile says which tool. */
  :global(.instrument-palette.dock-left) .palette-panel-anchor.card,
  :global(.instrument-palette.dock-right) .palette-panel-anchor.card {
    --arrow: 50%;
    top: 50%;
    bottom: auto;
    transform: translateY(-50%);
  }
  :global(.instrument-palette.dock-left) .palette-panel-anchor { left: calc(100% + 10px); }
  :global(.instrument-palette.dock-right) .palette-panel-anchor { right: calc(100% + 10px); }

  .closed-state { display: grid; align-content: center; justify-items: center; padding: 2rem; background: var(--surround); text-align: center; }
  .closed-mark { width: 34px; height: 42px; border: 1px solid var(--quiet); border-radius: 3px; box-shadow: inset 0 5px var(--panel); }
  .closed-state h1 { margin: 18px 0 5px; font-size: 20px; font-weight: 600; }
  .closed-state p { margin: 0; color: var(--muted); font-size: var(--text-md); }
  .closed-state button { margin-top: 20px; padding: 10px 14px; border-radius: var(--radius); background: var(--blueprint); color: #10141a; font-size: var(--text-md); font-weight: 700; cursor: pointer; }

  .workspace-app:has(:global(.typst-block.editing)) :global(.selection-actions) { visibility: hidden; pointer-events: none; }
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

  }
</style>
