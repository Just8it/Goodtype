<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onDestroy, onMount, tick } from "svelte";
  import { clampZoom, type Point } from "../geometry/coordinates";
  import type {
    InkLayer,
    NotebookManifest,
    Page,
    PageBackground,
    PageObject,
    PagePosition,
    PageTemplate,
    Stroke,
  } from "../model";
  import {
    TYPST_IDLE_DEBOUNCE_MS,
    type TypstCompileResult,
  } from "../editor/typst";
  import { getCachedTypst, setCachedTypst } from "../editor/typstCache";
  import {
    summarizeMetric,
    type StrokePerformance,
  } from "../ink/metrics";
  import type { InkTool } from "../ink/pipeline";
  import { moveSelected, scaleSelected } from "../ink/selection";
  import ColorPanel from "./ColorPanel.svelte";
  import ToolPanel from "./ToolPanel.svelte";
  import PageSurface from "./PageSurface.svelte";
  import OverflowMenu from "../workspace/OverflowMenu.svelte";
  import { populated, type MenuSection } from "../workspace/menu";
  import AddPageMenu from "../workspace/AddPageMenu.svelte";
  import type { AddPageGroup, AddPageSource, AddPageWhere } from "../workspace/addPage";
  import { templatePreviewSvg } from "../page/template";
  import { TEMPLATE_GROUPS } from "../page/templates";
  import SideEditor from "./SideEditor.svelte";
  import {
    AssetUrlCache,
    blockViewsFromSnapshot,
    imageViewsFromSnapshot,
    mimeForPath,
    strokesFromSnapshot,
    type BlockView,
    type ImageView,
  } from "./pageView";
  import { createInkCommitter, type InkCommitter } from "./inkCommitter";
  import { nearestPaletteDock, type PaletteDock } from "./palette";
  import ConflictDialog from "../workspace/ConflictDialog.svelte";
  import RecoveryDialog from "../workspace/RecoveryDialog.svelte";
  import SearchOverlay from "../workspace/SearchOverlay.svelte";
  import SettingsPanel from "../workspace/SettingsPanel.svelte";
  import StartSurface from "../workspace/StartSurface.svelte";
  import {
    DEFAULT_SETTINGS,
    loadSettings,
    saveSettings,
    ERASER_RADIUS_PT,
    MAX_SWATCHES,
    colorName,
    penType,
    withRecentColor,
    type AppSettings,
    type PenPreset,
    type RecoveryCandidate,
    type SearchHit,
  } from "../settings";

  type StoredFile = { path: string; bytes: number[] };
  type NotebookSnapshot = {
    manifest: NotebookManifest;
    page: Page;
    blocks: StoredFile[];
    assets: StoredFile[];
    inkLayers: InkLayer[];
  };
  type HistoryResult = {
    snapshot: NotebookSnapshot;
    canUndo: boolean;
    canRedo: boolean;
  };
  type PageEntry = {
    id: string;
    path: string;
    snapshot: NotebookSnapshot | null;
  };
  type TypstTransform = {
    x: number;
    y: number;
    layoutWidthPt: number;
    scale: number;
  };
  type TypstState = {
    id: string;
    path: string;
    source: string;
    transform: TypstTransform;
    result: TypstCompileResult | null;
  };
  type ImageState = {
    path: string;
    url: string;
    alt: string;
    x: number;
    y: number;
    widthPt: number;
    heightPt: number;
    scale: number;
  };
  type PaletteDrag = {
    pointerId: number;
    clientX: number;
    clientY: number;
    startX: number;
    startY: number;
    width: number;
    height: number;
  };
  type PinchStart = { distance: number; zoom: number };
  type TypstScaleEdit = { id: string; transform: TypstTransform };

  const PAGE_WIDTH_PT = 595;
  const PAGE_HEIGHT_PT = 842;
  const MAIN_TYPST_ID = "typst-001";
  const BLOCK_PATH = "blocks/equation.typ";
  const INK_LAYER_ID = "ink-layer-001";
  const INK_GROUP_ID = "ink-group-001";
  const GROUP_ID = "group-001";
  const TYPST_SAVE_DEBOUNCE_MS = 250;
  // A commit rewrites, revalidates, and refingerprints the whole page, so one commit per
  // pen-up makes saving cost grow with the ink already on the page. Batch a burst of
  // writing into one commit, but never hold unsaved ink longer than the maximum.
  const INK_SAVE_DEBOUNCE_MS = 500;
  const INK_SAVE_MAXIMUM_MS = 2000;
  const tauriAvailable =
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

  let root = $state("");
  let notebookTitle = $state("Goodtype");
  let notebookManifest = $state<NotebookManifest | null>(null);
  let activePageId = $state("page-001");
  let activeInkLayerId = $state(INK_LAYER_ID);
  let activeInkLayerPath = $state("ink/page-001-layer-001.json");
  /** The paper under the active page, kept so committing does not overwrite it. */
  let activeBackground = $state<PageBackground>({ kind: "plain", color: "#ffffff" });
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
    },
  ]);
  let image = $state<ImageState | null>(null);
  let selectedImage = $state(false);
  let selectedTypstId = $state<string | null>(null);
  let directObjectInput = $state(false);
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
  let inkCommitTimer: ReturnType<typeof setTimeout> | undefined;
  let inkCommitDeadline = 0;
  let inkCommitLabel = "Updated ink";
  // Debounced ink is real unsaved work; the save indicator must not read "Saved" while it waits.
  let inkPending = $state(false);
  let typstDirty = false;
  let workspace = $state<HTMLElement>();
  let pageViewport = $state<HTMLElement>();
  let pageFrame = $state<HTMLElement>();

  // The page being edited renders through the same surface as its neighbours, so its state is
  // projected into the same view model they use.
  const ACTIVE_IMAGE_ID = "image-001";
  const activeBlockViews: BlockView[] = $derived(
    typstBlocks.map((block) => ({
      id: block.id,
      path: block.path,
      source: block.source,
      x: block.transform.x,
      y: block.transform.y,
      layoutWidthPt: block.transform.layoutWidthPt,
      scale: block.transform.scale,
    })),
  );
  const activeResults: Record<string, TypstCompileResult | null> = $derived(
    Object.fromEntries(typstBlocks.map((block) => [block.id, block.result])),
  );
  const activeImageViews: ImageView[] = $derived(
    image
      ? [
          {
            id: ACTIVE_IMAGE_ID,
            path: image.path,
            url: image.url,
            alt: image.alt,
            x: image.x,
            y: image.y,
            widthPt: image.widthPt,
            heightPt: image.heightPt,
            scale: image.scale,
          },
        ]
      : [],
  );

  // Full-height source view beside the canvas. It is a sibling of the canvas region rather than
  // an overlay, so the canvas genuinely narrows — and the palette, which is positioned inside
  // that region, follows the paper instead of colliding with the panel.
  let sideEditorOpen = $state(false);
  let sideEditorBlockId = $state<string | null>(null);
  /// The page the target block belongs to, so scrolling elsewhere does not lose it.
  let sideEditorPageId = $state<string | null>(null);
  let sideEditor = $state<{ focus: () => void }>();

  const sideEditorBlock = $derived(
    typstBlocks.find((block) => block.id === sideEditorBlockId) ?? null,
  );
  /// `edit` when the target is on the page in view, `away` when it is held on another page, and
  /// `none` when nothing has been picked yet. The target only changes when the writer picks one.
  const sideEditorMode = $derived<"edit" | "away" | "none">(
    sideEditorBlock ? "edit" : sideEditorBlockId ? "away" : "none",
  );
  const sideEditorPageNumber = $derived(
    sideEditorPageId
      ? (notebookManifest?.pages.findIndex((page) => page.id === sideEditorPageId) ?? 0) + 1
      : null,
  );

  function openSideEditor(blockId?: string) {
    // Keeps whatever was last opened; only an explicit pick retargets the panel.
    const target = blockId ?? sideEditorBlockId ?? selectedTypstId ?? typstBlocks[0]?.id ?? null;
    sideEditorOpen = true;
    if (target) {
      if (target !== sideEditorBlockId) sideEditorPageId = activePageId;
      sideEditorBlockId = target;
      if (typstBlocks.some((block) => block.id === target)) {
        selectedTypstId = target;
        selectedImage = false;
      }
    }
    // The panel mounts this tick; take the caret once it exists.
    void tick().then(() => sideEditor?.focus());
  }

  function closeSideEditor() {
    sideEditorOpen = false;
  }

  function toggleSideEditor() {
    if (sideEditorOpen) closeSideEditor();
    else openSideEditor();
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

  /// Quick settings for a tool slot, opened by double-pressing its tile.
  let toolPanel = $state<{ kind: "pen" | "highlighter"; slot: number; anchor: number } | null>(
    null,
  );

  /// First press selects the tool; pressing the one already selected opens its settings — the
  /// same select-then-edit gesture the colour swatches use.
  function selectOrOpenTool(kind: "pen" | "highlighter", slot: number, tile: HTMLElement) {
    const alreadyActive =
      kind === "highlighter" ? tool === "highlighter" : tool === "pen" && penPreset === slot;
    colorPanel = null;
    if (!alreadyActive) {
      toolPanel = null;
      if (kind === "pen") activateTool("pen", slot as 1 | 2);
      else activateTool("highlighter");
      return;
    }
    toolPanel =
      toolPanel?.kind === kind && toolPanel?.slot === slot
        ? null
        : { kind, slot, anchor: swatchAnchor(tile) };
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
  let metricsOpen = $state(false);
  const touchPoints = new Map<number, Point>();
  let pinchStart: PinchStart | null = null;
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
  let searchOpen = $state(false);
  let conflictDetail = $state<string | null>(null);
  let recoveryCandidates = $state<RecoveryCandidate[]>([]);
  let recoveryOpen = $state(false);
  let recoveryBusy = $state(false);
  let notebookChosen = $state(false);
  // Session-local order of committed changes across pages, so notebook-scoped undo can route
  // Ctrl+Z to the page that changed most recently.
  let notebookUndoOrder: string[] = [];
  let notebookRedoOrder: string[] = [];
  let metricsTimer: ReturnType<typeof setTimeout> | undefined;

  onMount(() => {
    void initialize();
    window.addEventListener("keydown", historyShortcut);
  });
  onDestroy(() => {
    if (inkCommitTimer) clearTimeout(inkCommitTimer);
    if (typstCommitTimer) clearTimeout(typstCommitTimer);
    if (focusTimer) clearTimeout(focusTimer);
    window.removeEventListener("keydown", historyShortcut);
    revokeImageUrl();
  });

  $effect(() => {
    const metrics = metricsPayload();
    if (!tauriAvailable || !root || !notebookChosen) return;
    // Metrics are dev telemetry; batching writes keeps them off the per-stroke path.
    if (metricsTimer) clearTimeout(metricsTimer);
    metricsTimer = setTimeout(() => {
      void invoke("write_phase0_metrics", { root, metrics }).catch(() => {});
    }, 1000);
  });

  async function initialize() {
    settings = await loadSettings(tauriAvailable);
    paletteDock = settings.paletteDock;
    if (!tauriAvailable) {
      root = "Browser preview (persistence and real Typst compilation require Tauri)";
      applySnapshot(buildSnapshot());
      notebookChosen = true;
      pageOpen = true;
      busy = false;
      status = "Browser preview ready";
      return;
    }

    // First launch continuity: with no notebook history, open the local default directly so
    // pen-first startup stays instant. Otherwise the start surface offers recents/open/create.
    try {
      const recents = await invoke<unknown[]>("list_recent_notebooks");
      if (recents.length === 0) {
        const defaultRoot = await invoke<string>("phase0_notebook_root");
        await openNotebookAt(defaultRoot, { createIfMissing: true });
        return;
      }
    } catch {
      // The recents list is a convenience; failing to read it falls through to the start surface.
    }
    busy = false;
  }

  /// A freshly created notebook must start from a clean model — never from whatever
  /// manifest, ink, blocks, or image the previously open notebook left in memory.
  function resetToBlankNotebook(title: string) {
    revokeImageUrl();
    notebookTitle = title;
    notebookManifest = null;
    pageEntries = [];
    strokes = [];
    selectedStrokeIds = [];
    groupedStrokeIds = [];
    image = null;
    selectedImage = false;
    selectedTypstId = null;
    activePageId = "page-001";
    activeInkLayerId = INK_LAYER_ID;
    activeInkLayerPath = "ink/page-001-layer-001.json";
    revision = 1;
    createdAt = new Date().toISOString();
    typstBlocks = [
      {
        id: MAIN_TYPST_ID,
        path: BLOCK_PATH,
        source: "= Notes\n\nType Typst here, or write with the pen.",
        transform: { x: 96, y: 120, layoutWidthPt: 230, scale: 1 },
        result: null,
      },
    ];
  }

  function titleFromRoot(path: string) {
    return path.split(/[\\/]/).filter(Boolean).at(-1) ?? "Goodtype notebook";
  }

  async function openNotebookAt(
    nextRoot: string,
    options: { createIfMissing?: boolean } = {},
  ) {
    busy = true;
    try {
      root = nextRoot;
      let snapshot: NotebookSnapshot;
      try {
        snapshot = await invoke<NotebookSnapshot>("open_notebook", { root });
      } catch (error) {
        if (!options.createIfMissing) throw error;
        resetToBlankNotebook(titleFromRoot(nextRoot));
        snapshot = buildSnapshot();
        await invoke("create_notebook", { root, snapshot });
      }
      transactionFailed = false;
      conflictDetail = null;
      notebookUndoOrder = [];
      notebookRedoOrder = [];
      pageEntries = [];
      applySnapshot(snapshot);
      notebookChosen = true;
      pageOpen = true;
      status = "Notebook ready";
      void invoke("record_notebook_opened", {
        root,
        title: snapshot.manifest.title || "Goodtype notebook",
        openedAt: new Date().toISOString(),
      }).catch(() => {});
      await refreshRecoveryCandidates();
    } catch (error) {
      status = `Could not open the notebook: ${message(error)}`;
      notebookChosen = false;
    } finally {
      busy = false;
    }
  }

  async function refreshRecoveryCandidates() {
    if (!tauriAvailable || !root) return;
    try {
      recoveryCandidates = await invoke<RecoveryCandidate[]>("list_recovery_candidates", {
        root,
      });
      recoveryOpen = recoveryCandidates.length > 0;
    } catch {
      // A recovery listing failure must not block opening; candidates stay on disk.
    }
  }

  function buildSnapshot(): NotebookSnapshot {
    const now = new Date().toISOString();
    const grouped = groupedStrokeIds.length > 0;
    const extraTypstIds = typstBlocks.slice(1).map((block) => block.id);
    const objects: PageObject[] = typstBlocks.map((block, index) => ({
      ...fields(
        block.id,
        index,
        index === 0 && grouped ? GROUP_ID : null,
        now,
        block.transform,
      ),
      type: "typst",
      sourcePath: block.path,
      layoutWidthPt: block.transform.layoutWidthPt,
      measuredWidthPt: block.result?.widthPt ?? block.transform.layoutWidthPt,
      measuredHeightPt: block.result?.heightPt ?? 48,
    }));
    if (grouped) {
      objects.push(
        {
          ...fields(INK_GROUP_ID, 1, GROUP_ID, now),
          type: "ink_group",
          inkLayerId: activeInkLayerId,
          strokeIds: groupedStrokeIds,
        },
        {
          ...fields(GROUP_ID, 0, null, now),
          type: "group",
          childIds: [MAIN_TYPST_ID, INK_GROUP_ID],
        },
      );
    }
    if (image) {
      objects.push({
        ...fields("image-001", typstBlocks.length, null, now),
        type: "image",
        sourcePath: image.path,
        widthPt: image.widthPt,
        heightPt: image.heightPt,
        altText: image.alt,
        x: image.x,
        y: image.y,
        scale: image.scale,
      });
    }

    const page: Page = {
      schemaVersion: 1,
      id: activePageId,
      revision,
      geometry: { widthPt: PAGE_WIDTH_PT, heightPt: PAGE_HEIGHT_PT },
      background: activeBackground,
      objects,
      readingOrder: grouped
        ? [GROUP_ID, ...extraTypstIds, ...(image ? ["image-001"] : [])]
        : [...typstBlocks.map((block) => block.id), ...(image ? ["image-001"] : [])],
      inkLayers: [{ id: activeInkLayerId, path: activeInkLayerPath }],
    };
    const manifest = notebookManifest ?? {
      schemaVersion: 1,
      id: `notebook-${Date.now().toString(36)}`,
      title: notebookTitle,
      pages: [{ id: page.id, path: "pages/page-001.json" }],
      defaultPage: {
        geometry: page.geometry,
        background: page.background,
      },
      sharedStylePath: null,
      createdAt,
      modifiedAt: now,
    };
    return {
      manifest,
      page,
      blocks: typstBlocks.map((block) => ({
        path: block.path,
        bytes: Array.from(new TextEncoder().encode(block.source)),
      })),
      // Assets are written once by `store_pasted_image` and are immutable afterwards.
      // Rust resolves referenced originals from disk, so a commit never carries their bytes.
      assets: [],
      inkLayers: [
        {
          schemaVersion: 1,
          id: activeInkLayerId,
          pageId: page.id,
          strokes,
        },
      ],
    };
  }

  function fields(
    id: string,
    readingOrder: number,
    groupId: string | null,
    timestamp: string,
    position = { x: 0, y: 0, scale: 1 },
  ) {
    return {
      id,
      ...position,
      rotation: 0,
      zIndex: readingOrder + 1,
      readingOrder,
      groupId,
      createdAt: timestamp,
      modifiedAt: timestamp,
    };
  }

  function applySnapshot(snapshot: NotebookSnapshot) {
    if (inkCommitTimer) clearTimeout(inkCommitTimer);
    inkCommitTimer = undefined;
    inkPending = false;
    if (typstCommitTimer) clearTimeout(typstCommitTimer);
    typstCommitTimer = undefined;
    typstDirty = false;
    revokeImageUrl();
    notebookManifest = snapshot.manifest;
    activePageId = snapshot.page.id;
    // The active page's state below is now the single source of truth for this page; drop the
    // copy it carried while it was a neighbour so the two can never disagree.
    delete neighborStrokes[snapshot.page.id];
    const activeInk = snapshot.page.inkLayers[0];
    activeInkLayerId = activeInk?.id ?? `${activePageId}-ink-001`;
    activeInkLayerPath = activeInk?.path ?? `ink/${activePageId}-layer-001.json`;
    // Carried through so `buildSnapshot` can put it back. It used to write a hardcoded white
    // page, which meant the first stroke on a template erased the paper it was drawn on.
    activeBackground = snapshot.page.background;
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
        };
      });

    const inkGroup = snapshot.page.objects.find(
      (object): object is Extract<PageObject, { type: "ink_group" }> =>
        object.type === "ink_group",
    );
    groupedStrokeIds = inkGroup?.strokeIds ?? [];

    const imageObject = snapshot.page.objects.find(
      (object): object is Extract<PageObject, { type: "image" }> =>
        object.type === "image",
    );
    const asset = imageObject
      ? snapshot.assets.find((file) => file.path === imageObject.sourcePath)
      : undefined;
    if (imageObject && asset) {
      const blob = new Blob([new Uint8Array(asset.bytes)], {
        type: mimeForPath(asset.path),
      });
      image = {
        path: asset.path,
        url: URL.createObjectURL(blob),
        alt: imageObject.altText,
        x: imageObject.x,
        y: imageObject.y,
        widthPt: imageObject.widthPt,
        heightPt: imageObject.heightPt,
        scale: imageObject.scale,
      };
    } else {
      image = null;
    }
    selectedImage = false;
    selectedTypstId = null;
  }

  async function ensurePageLoaded(pageId: string) {
    const entry = pageEntries.find((page) => page.id === pageId);
    if (!entry || entry.snapshot) return entry?.snapshot ?? null;
    try {
      const snapshot = await invoke<NotebookSnapshot>("open_page", { root, pageId });
      entry.snapshot = snapshot;
      return snapshot;
    } catch (error) {
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
          outgoingEntry.snapshot = await invoke<NotebookSnapshot>("open_page", {
            root,
            pageId: outgoing,
          });
          // It renders as a neighbour from here on, so let it derive from the fresh bundle.
          delete neighborStrokes[outgoing];
        } catch {
          // A neighbor that fails to reload simply shows a loading state; it is not fatal.
        }
      }

      const result = await invoke<HistoryResult>("focus_page", { root, pageId });
      applySnapshot(result.snapshot);
      canUndo = result.canUndo;
      canRedo = result.canRedo;
      evictDistantPages();
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
        if (!entry?.snapshot) return;
        const inkLayers = entry.snapshot.inkLayers.map((layer, index) =>
          index === 0 ? { ...layer, strokes } : layer,
        );
        const number =
          (notebookManifest?.pages.findIndex((page) => page.id === pageId) ?? 0) + 1;
        try {
          const result = await invoke<HistoryResult>("commit_notebook", {
            root,
            // Originals already exist on disk; a commit references them by path.
            snapshot: { ...entry.snapshot, assets: [], inkLayers },
          });
          updateLoadedPage(pageId, result.snapshot);
          neighborStrokes[pageId] = strokesFromSnapshot(result.snapshot);
          status = `${label} on page ${number}`;
        } catch (error) {
          // Fall back to the last saved ink so the page never shows work it did not store.
          neighborStrokes[pageId] = entry.snapshot
            ? strokesFromSnapshot(entry.snapshot)
            : [];
          status = `Could not save page ${number}: ${message(error)}`;
        }
      },
    });
    neighborCommitters.set(pageId, committer);
    return committer;
  }

  function commitNeighborInk(pageId: string, strokes: Stroke[], label: string) {
    neighborStrokes[pageId] = strokes;
    neighborCommitter(pageId).commit(strokes, label);
  }

  async function compileNeighborTypst(
    pageId: string,
    blockId: string,
    request: { source: string; widthPt: number; generation: number },
  ) {
    const cached = getCachedTypst(request.source, request.widthPt);
    if (cached) {
      neighborResults[pageId] = {
        ...neighborResults[pageId],
        [blockId]: { ...cached, generation: request.generation },
      };
      return;
    }
    if (!tauriAvailable) return;
    try {
      const result = await invoke<TypstCompileResult>("compile_typst", {
        root,
        request,
      });
      setCachedTypst(request.source, request.widthPt, result);
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

  function releaseNeighbor(pageId: string) {
    neighborUrls.get(pageId)?.dispose();
    neighborUrls.delete(pageId);
    neighborCommitters.get(pageId)?.dispose();
    neighborCommitters.delete(pageId);
    delete neighborStrokes[pageId];
    delete neighborResults[pageId];
  }

  /// Keep only the active page's neighbors as full bundles (Phase 2 §7 residency budget).
  /// Evicted pages fall back to placeholders and reload on demand near the viewport.
  function evictDistantPages() {
    const active = pageEntries.findIndex((page) => page.id === activePageId);
    if (active < 0) return;
    for (const [index, entry] of pageEntries.entries()) {
      if (Math.abs(index - active) > 2 && entry.snapshot) {
        entry.snapshot = null;
        releaseNeighbor(entry.id);
      }
    }
  }

  function changeSettings(next: AppSettings) {
    settings = next;
    paletteDock = next.paletteDock;
    if (settingsSaveTimer) clearTimeout(settingsSaveTimer);
    settingsSaveTimer = setTimeout(() => {
      void saveSettings(tauriAvailable, settings)
        .then((sanitized) => (settings = sanitized))
        .catch((error) => (status = `Settings were not saved: ${message(error)}`));
    }, 400);
  }

  async function duplicateActivePage() {
    moreOpen = false;
    if (!tauriAvailable || !(await persist())) return;
    busy = true;
    try {
      const snapshot = await invoke<NotebookSnapshot>("duplicate_page", {
        root,
        pageId: activePageId,
        modifiedAt: new Date().toISOString(),
      });
      pageEntries = [];
      applySnapshot(snapshot);
      canUndo = false;
      canRedo = false;
      await tick();
      scrollToPage(snapshot.page.id);
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
      const snapshot = await invoke<NotebookSnapshot>("delete_page", {
        root,
        pageId: activePageId,
        modifiedAt: new Date().toISOString(),
      });
      notebookUndoOrder = notebookUndoOrder.filter((id) => id !== activePageId);
      notebookRedoOrder = notebookRedoOrder.filter((id) => id !== activePageId);
      pageEntries = [];
      applySnapshot(snapshot);
      canUndo = false;
      canRedo = false;
      await tick();
      scrollToPage(snapshot.page.id);
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
      },
    ];
    strokes = [];
    selectedStrokeIds = [];
    groupedStrokeIds = [];
    image = null;
    selectedTypstId = null;
    selectedImage = false;
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
          { kind: "action", id: "settings", label: "Settings", hint: "Ctrl ,", onSelect: () => (settingsOpen = true) },
          { kind: "action", id: "save", label: "Confirm saved", onSelect: () => void persist() },
          { kind: "action", id: "metrics", label: "Timing evidence", onSelect: () => (metricsOpen = true) },
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
      const snapshot = await invoke<NotebookSnapshot>("reorder_pages", {
        root,
        orderedIds: order,
        modifiedAt: new Date().toISOString(),
        activePageId,
      });
      pageEntries = [];
      applySnapshot(snapshot);
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
      selectedImage = false;
      status = `Found on page ${hit.pageNumber}`;
    }
  }

  async function restoreRecovery(fileName: string) {
    recoveryBusy = true;
    try {
      const result = await invoke<HistoryResult>("restore_recovery_candidate", {
        root,
        fileName,
      });
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
      await invoke("discard_recovery_candidate", { root, fileName });
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

  async function addPage(position: PagePosition, background: PageBackground | null = null) {
    moreOpen = false;
    addPageOpen = false;
    if (!(await persist())) return;
    busy = true;
    try {
      const snapshot = await invoke<NotebookSnapshot>("create_page", {
        root,
        modifiedAt: new Date().toISOString(),
        position,
        background,
      });
      pageEntries = [];
      applySnapshot(snapshot);
      canUndo = false;
      canRedo = false;
      // Load whatever now sits above the new page so the scroll lands with context above it
      // rather than against the top of an otherwise empty run.
      const index = snapshot.manifest.pages.findIndex((page) => page.id === snapshot.page.id);
      const above = index > 0 ? snapshot.manifest.pages[index - 1] : undefined;
      if (above) await ensurePageLoaded(above.id);
      await tick();
      scrollToPage(snapshot.page.id);
      status = `Added page ${activePageNumber()}`;
    } catch (error) {
      status = `Could not add page: ${message(error)}`;
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
    const geometry = { widthPt: PAGE_WIDTH_PT, heightPt: PAGE_HEIGHT_PT };
    const template = (source: PageTemplate): AddPageSource => ({
      id: source.id,
      label: source.name,
      preview: templatePreviewSvg(source, geometry),
      onSelect: (position) => void addPage(position, { kind: "template", template: source }),
    });
    return [
      {
        id: "current",
        title: "This page",
        sources: [
          {
            id: "same",
            label: "Same paper",
            detail:
              activeBackground.kind === "template" ? activeBackground.template.name : "Plain",
            preview:
              activeBackground.kind === "template"
                ? templatePreviewSvg(activeBackground.template, geometry)
                : undefined,
            onSelect: (position) => void addPage(position, activeBackground),
          },
        ],
      },
      ...TEMPLATE_GROUPS.map((group) => ({
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
    widthPt: number;
    generation: number;
  }) {
    const startedAt = performance.now();
    let result: TypstCompileResult;
    if (!tauriAvailable) {
      result = {
        generation: request.generation,
        svg: previewSvg(request.source, request.widthPt),
        widthPt: request.widthPt,
        heightPt: 64,
        diagnostics: [],
      };
      compileMs = performance.now() - startedAt;
    } else {
      const cached = getCachedTypst(request.source, request.widthPt);
      if (cached) {
        // Unchanged source: reuse the compiled SVG, stamped with this request's generation so
        // the block's preview state machine accepts it. No recompile, no IPC round trip.
        result = { ...cached, generation: request.generation };
      } else {
        try {
          result = await invoke<TypstCompileResult>("compile_typst", {
            root,
            request,
          });
          setCachedTypst(request.source, request.widthPt, result);
        } catch (error) {
          result = {
            generation: request.generation,
            svg: null,
            widthPt: null,
            heightPt: null,
            diagnostics: [{ severity: "error", message: message(error) }],
          };
        }
      }
      compileMs = performance.now() - startedAt;
    }
    typstBlocks = typstBlocks.map((block) =>
      block.id === id ? { ...block, result } : block,
    );
  }

  function updateTypstTransform(id: string, next: TypstTransform) {
    const previous = typstBlocks.find((block) => block.id === id)?.transform;
    if (!previous) return;
    if (id === MAIN_TYPST_ID && groupedStrokeIds.length > 0) {
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
    typstBlocks = typstBlocks.map((block) =>
      block.id === id ? { ...block, source } : block,
    );
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
      },
    ];
    selectedTypstId = id;
    selectedImage = false;
    status = "Created a new Typst block";
    queueCommit("Created Typst block");
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
    const selected = new Set(selectedStrokeIds);
    groupedStrokeIds = [...selectedStrokeIds];
    strokes = strokes.map((stroke) => ({
      ...stroke,
      groupId: selected.has(stroke.id) ? INK_GROUP_ID : null,
    }));
    status = `Grouped ${groupedStrokeIds.length} ink stroke${groupedStrokeIds.length === 1 ? "" : "s"} with the Typst block`;
    queueCommit("Grouped ink with Typst");
  }

  function updateInkSelection(ids: string[]) {
    selectedStrokeIds = ids;
    if (ids.length > 0 && tool === "lasso") tool = "select";
  }

  const TOOL_NAMES: Record<InkTool, string> = {
    pen: "Pen",
    highlighter: "Highlighter",
    eraser: "Eraser",
    lasso: "Lasso",
    select: "Ink selection",
  };

  function activateTool(next: InkTool, preset?: 1 | 2) {
    if (preset) penPreset = preset;
    tool = next;
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
    if (event.pointerType === "pen") {
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
    selectedImage = object?.classList.contains("image-object") ?? false;
    if (object && event.target instanceof Element && event.target.closest(".ink-surface")) {
      event.preventDefault();
      event.stopPropagation();
      status = selectedTypstId ? "Typst block selected" : "Image selected";
    }
  }

  function closeObjectSelection(event: PointerEvent) {
    // Both palette popovers live inside the bar, so a press anywhere outside it dismisses them.
    if (!(event.target instanceof Element) || !event.target.closest(".instrument-palette")) {
      colorPanel = null;
      toolPanel = null;
    }
    if (
      event.pointerType === "pen" ||
      (event.target instanceof Element &&
        event.target.closest(".typst-block, .image-object, .typst-size-control"))
    ) {
      return;
    }
    selectedTypstId = null;
    selectedImage = false;
  }

  function workspacePointerDown(event: PointerEvent) {
    routeObjectPointer(event);
    if (event.pointerType !== "touch") return;
    touchPoints.set(event.pointerId, { x: event.clientX, y: event.clientY });
    if (touchPoints.size !== 2 || !workspace) return;
    const points = [...touchPoints.values()];
    pinchStart = { distance: distance(points[0], points[1]), zoom };
    for (const pointerId of touchPoints.keys()) workspace.setPointerCapture(pointerId);
    event.preventDefault();
    event.stopPropagation();
  }

  function workspacePointerMove(event: PointerEvent) {
    routeObjectPointer(event);
    if (!touchPoints.has(event.pointerId) || !pinchStart) return;
    touchPoints.set(event.pointerId, { x: event.clientX, y: event.clientY });
    const points = [...touchPoints.values()];
    if (points.length !== 2) return;
    const center = {
      x: (points[0].x + points[1].x) / 2,
      y: (points[0].y + points[1].y) / 2,
    };
    zoomAt(
      clampZoom(pinchStart.zoom * (distance(points[0], points[1]) / pinchStart.distance)),
      center.x,
      center.y,
    );
    event.preventDefault();
    event.stopPropagation();
  }

  function workspacePointerEnd(event: PointerEvent) {
    if (event.pointerType !== "touch") return;
    touchPoints.delete(event.pointerId);
    if (pinchStart) {
      touchPoints.clear();
      pinchStart = null;
      event.preventDefault();
      event.stopPropagation();
    }
  }

  function distance(a: Point, b: Point) {
    return Math.max(Math.hypot(a.x - b.x, a.y - b.y), 1);
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
  }

  function finishPaletteDrag(event: PointerEvent) {
    if (!paletteDrag || event.pointerId !== paletteDrag.pointerId || !workspace) return;
    const bounds = workspace.getBoundingClientRect();
    paletteDock = nearestPaletteDock(
      event.clientX - bounds.left,
      event.clientY - bounds.top,
      bounds.width,
      bounds.height,
    );
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
      if (id === MAIN_TYPST_ID && groupedStrokeIds.length > 0) ungroupInk();
      typstBlocks = typstBlocks.filter((block) => block.id !== id);
      selectedTypstId = null;
      queueCommit("Deleted Typst block");
      status = "Deleted the Typst block";
      return true;
    }
    if (selectedImage && image) {
      revokeImageUrl();
      image = null;
      selectedImage = false;
      queueCommit("Deleted image");
      status = "Removed the image from this page; the original file is kept";
      return true;
    }
    if (selectedStrokeIds.length > 0) {
      const removed = new Set(selectedStrokeIds);
      selectedStrokeIds = [];
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
    if (selectedImage && image) {
      image = { ...image, x: image.x + dx, y: image.y + dy };
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
      groupedStrokeIds.includes(stroke.id) ? { ...stroke, groupId: null } : stroke,
    );
    groupedStrokeIds = [];
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
        path = await invoke<string>("store_pasted_image", {
          root,
          filename,
          bytes: Array.from(new Uint8Array(await file.arrayBuffer())),
        });
      } catch (error) {
        URL.revokeObjectURL(url);
        status = `The image could not be stored: ${message(error)}`;
        return;
      }
    }

    revokeImageUrl();
    const fit = Math.min(1, 220 / dimensions.width, 160 / dimensions.height);
    image = {
      path,
      url,
      alt: file.name || "Pasted image",
      x: 300,
      y: 380,
      widthPt: Math.max(1, dimensions.width * fit),
      heightPt: Math.max(1, dimensions.height * fit),
      scale: 1,
    };
    selectedImage = true;
    selectedTypstId = null;
    status = "Pasted one original image";
    queueCommit("Pasted image");
  }

  function scheduleInkCommit(label: string) {
    inkCommitLabel = label;
    inkPending = true;
    const now = performance.now();
    if (!inkCommitTimer) inkCommitDeadline = now + INK_SAVE_MAXIMUM_MS;
    else clearTimeout(inkCommitTimer);
    const delay = Math.max(0, Math.min(INK_SAVE_DEBOUNCE_MS, inkCommitDeadline - now));
    inkCommitTimer = setTimeout(() => queueCommit(inkCommitLabel), delay);
  }

  function flushInkCommit() {
    if (!inkCommitTimer) return;
    queueCommit(inkCommitLabel);
  }

  function queueCommit(label: string) {
    // Any commit builds a snapshot from current state, so it already carries pending ink.
    if (inkCommitTimer) clearTimeout(inkCommitTimer);
    inkCommitTimer = undefined;
    inkPending = false;
    if (!tauriAvailable || !root || transactionFailed) return;
    const snapshot = buildSnapshot();
    pendingTransactions += 1;
    transactionQueue = transactionQueue
      .then(async () => {
        snapshot.page.revision = revision;
        try {
          const result = await invoke<HistoryResult>("commit_notebook", {
            root,
            snapshot,
          });
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
    notebookUndoOrder.push(pageId);
    if (notebookUndoOrder.length > 200) notebookUndoOrder.shift();
    notebookRedoOrder = [];
  }

  function changeStrokes(next: Stroke[], label: string) {
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

  function changeImage(next: Partial<Pick<ImageState, "x" | "y" | "scale">>) {
    if (!image) return;
    image = { ...image, ...next };
    queueCommit("Updated image");
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
    if (settings.undoScope === "notebook") {
      const order = command === "undo_notebook" ? notebookUndoOrder : notebookRedoOrder;
      const target = order.at(-1);
      if (target && target !== activePageId) {
        await activatePage(target);
        if (activePageId !== target) return;
        scrollToPage(target);
      }
    }
    queueHistory(command, label);
  }

  function queueHistory(command: "undo_notebook" | "redo_notebook", label: string) {
    if (!tauriAvailable || transactionFailed) return;
    flushInkCommit();
    flushTypstCommit();
    pendingTransactions += 1;
    transactionQueue = transactionQueue
      .then(async () => {
        try {
          const result = await invoke<HistoryResult>(command, {
            root,
            pageId: activePageId,
          });
          const pageId = result.snapshot.page.id;
          if (command === "undo_notebook") {
            const index = notebookUndoOrder.lastIndexOf(pageId);
            if (index >= 0) notebookUndoOrder.splice(index, 1);
            notebookRedoOrder.push(pageId);
          } else {
            const index = notebookRedoOrder.lastIndexOf(pageId);
            if (index >= 0) notebookRedoOrder.splice(index, 1);
            notebookUndoOrder.push(pageId);
          }
          applySnapshot(result.snapshot);
          canUndo = result.canUndo;
          canRedo = result.canRedo;
          status = `${label}; saved revision ${revision}`;
        } catch (error) {
          reportCommitFailure(label, error);
        }
      })
      .finally(() => {
        pendingTransactions -= 1;
      });
  }

  function scrollToPage(pageId: string) {
    document
      .querySelector<HTMLElement>(`[data-page-id="${pageId}"]`)
      ?.scrollIntoView({
        behavior: settings.reducedMotion ? "auto" : "smooth",
        block: "center",
      });
  }

  function historyShortcut(event: KeyboardEvent) {
    if (event.defaultPrevented) return;
    if (event.ctrlKey || event.metaKey) {
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
      selectedImage = false;
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
    saveMs = performance.now() - startedAt;
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
      const path = await invoke<string>("export_notebook_pdf", {
        root,
        outputName: "notebook.pdf",
      });
      exportMs = performance.now() - startedAt;
      status = `Exported PDF to ${path}`;
    } catch (error) {
      status = `PDF export failed: ${message(error)}`;
    } finally {
      busy = false;
    }
  }

  async function closePage() {
    if (!(await persist())) return;
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
      const snapshot = await invoke<NotebookSnapshot>("open_notebook", { root });
      applySnapshot(snapshot);
      if (transactionFailed) {
        canUndo = false;
        canRedo = false;
      }
      transactionFailed = false;
      reopenMs = performance.now() - startedAt;
      pageOpen = true;
      status = `Reopened saved revision ${revision}`;
    } catch (error) {
      status = `Reopen failed: ${message(error)}`;
    } finally {
      busy = false;
    }
  }

  function revokeImageUrl() {
    if (image?.url.startsWith("blob:")) URL.revokeObjectURL(image.url);
  }

  function changeZoom(next: number) {
    const bounds = pageViewport?.getBoundingClientRect();
    if (bounds) {
      zoomAt(next, bounds.left + bounds.width / 2, bounds.top + bounds.height / 2);
      return;
    }
    zoom = clampZoom(next);
  }

  function zoomAt(next: number, clientX: number, clientY: number) {
    if (!pageViewport || !pageFrame) return;
    const viewport = pageViewport;
    const frame = pageFrame;
    const startedAt = performance.now();
    const before = frame.getBoundingClientRect();
    const pagePoint = {
      x: (clientX - before.left) / zoom,
      y: (clientY - before.top) / zoom,
    };
    zoom = clampZoom(next);
    requestAnimationFrame(() => {
      const after = frame.getBoundingClientRect();
      viewport.scrollLeft += after.left + pagePoint.x * zoom - clientX;
      viewport.scrollTop += after.top + pagePoint.y * zoom - clientY;
      zoomFrameMs = performance.now() - startedAt;
    });
  }

  function wheelZoom(event: WheelEvent) {
    if (!event.ctrlKey) return;
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
      <StartSurface
        {tauriAvailable}
        onOpen={(nextRoot) => void openNotebookAt(nextRoot)}
        onCreate={(nextRoot) => void openNotebookAt(nextRoot, { createIfMissing: true })}
        onStatus={(next) => (status = next)}
      />
    </div>
  {:else}
  <header class="command-strip">
    <div class="notebook-identity">
      <span class="app-mark" aria-hidden="true"></span>
      <div>
        <div class="notebook-title">Goodtype notebook</div>
        <div class="save-state">
          <span class:warning={transactionFailed} class:saving={pendingTransactions > 0 || inkPending} class="state-dot"></span>
          <span>{transactionFailed ? "Save blocked" : pendingTransactions > 0 || inkPending ? "Saving" : "Saved"}</span>
          <span class="revision">r{revision}</span>
        </div>
      </div>
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
              currentPageId={activePageId}
              {pageNumber}
              {pageCount}
              canPlaceRelative={pageCount > 0 && Boolean(activePageId)}
              onWhereChange={(next) => (addPageWhere = next)}
              onClose={() => (addPageOpen = false)}
            />
          {/if}
        </div>
      {/if}
      <button class="icon-button" class:active={searchOpen} type="button" aria-label="Search typed content" title="Search (Ctrl+F)" onclick={() => (searchOpen = !searchOpen)}>
        <svg class="stroke-icon" viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="7"></circle><path d="m20 20-3.6-3.6"></path></svg>
      </button>
      <button class="icon-button" class:active={sideEditorOpen} type="button" aria-label="Typst source view" aria-pressed={sideEditorOpen} title="Source view (Ctrl+Shift+E)" onclick={toggleSideEditor}>
        <svg class="stroke-icon" viewBox="0 0 24 24" aria-hidden="true"><rect x="3.5" y="4.5" width="17" height="15" rx="2"></rect><path d="M10 4.5v15"></path></svg>
      </button>
      <button class="icon-button" class:active={moreOpen} type="button" aria-label="More notebook actions" aria-expanded={moreOpen} onclick={() => (moreOpen = !moreOpen)}>
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
        source={sideEditorBlock?.source ?? ""}
        blockLabel={sideEditorBlockId ? `Typst block ${sideEditorBlockId}` : ""}
        awayPageNumber={sideEditorPageNumber}
        hasAnyBlock={typstBlocks.length > 0}
        {root}
        dock={settings.sideEditorDock}
        width={settings.sideEditorWidth}
        diagnostics={sideEditorBlock?.result?.diagnostics ?? []}
        onChange={(next) => sideEditorBlock && updateTypstSource(sideEditorBlock.id, next)}
        onClose={closeSideEditor}
        onDockChange={(dock) => changeSettings({ ...settings, sideEditorDock: dock })}
        onWidthChange={(next) => changeSettings({ ...settings, sideEditorWidth: next })}
        onGoToBlock={() => {
          if (sideEditorMode === "away" && sideEditorPageId) scrollToPage(sideEditorPageId);
          else if (typstBlocks[0]) openSideEditor(typstBlocks[0].id);
        }}
        onCreateBlock={() => {
          addTypstBlock();
          void tick().then(() => {
            const added = typstBlocks.at(-1);
            if (added) openSideEditor(added.id);
          });
        }}
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
      <div class="page-scroll-content" bind:this={pageViewport}>
        {#each pageEntries as entry, index (entry.id)}
          {@const active = entry.id === activePageId}
          <article
            class:active-page={active}
            class="page-stack-item"
            data-page-id={entry.id}
            aria-label={`Page ${index + 1}`}
            aria-current={active ? "page" : undefined}
            use:observePage={entry.id}
          >
            <span class="page-number">Page {index + 1}</span>
            <div class="page-frame" use:trackActiveFrame={active} style:width={`${PAGE_WIDTH_PT * zoom}px`} style:height={`${PAGE_HEIGHT_PT * zoom}px`}>
              <div class="page" style:width={`${PAGE_WIDTH_PT}px`} style:height={`${PAGE_HEIGHT_PT}px`} style:transform={`scale(${zoom})`}>
                {#if active || entry.snapshot}
                  <PageSurface
                    blocks={active ? activeBlockViews : blockViewsFromSnapshot(entry.snapshot!)}
                    images={active ? activeImageViews : imageViewsFromSnapshot(entry.snapshot!, assetUrls(entry.id))}
                    results={active ? activeResults : (neighborResults[entry.id] ?? {})}
                    strokes={active ? strokes : neighborStrokesFor(entry)}
                    selectedStrokeIds={active ? selectedStrokeIds : []}
                    background={active ? activeBackground : (entry.snapshot?.page.background ?? { kind: "plain", color: "#ffffff" })}
                    pageWidthPt={PAGE_WIDTH_PT}
                    pageHeightPt={PAGE_HEIGHT_PT}
                    {zoom}
                    interactive={active}
                    {root}
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
                    selectedImageId={active && selectedImage ? ACTIVE_IMAGE_ID : null}
                    onCompile={(id, request) =>
                      active
                        ? compileTypst(id, request)
                        : compileNeighborTypst(entry.id, id, request)}
                    onSourceChange={(id, source) => updateTypstSource(id, source)}
                    onTransform={(id, transform) => updateTypstTransform(id, transform)}
                    onSelectBlock={(id) => {
                      selectedTypstId = id;
                      selectedImage = false;
                    }}
                    onDeselectBlock={() => (selectedTypstId = null)}
                    onSelectImage={() => {
                      selectedImage = true;
                      selectedTypstId = null;
                    }}
                    onMoveImage={(_id, position) => changeImage(position)}
                    onScaleImage={(_id, scale) => changeImage({ scale })}
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
                    onStrokeMetrics={(metrics) => {
                      if (active) recordStrokeMetrics(metrics);
                    }}
                  />
                {:else}
                  <span class="page-loading">Loading page…</span>
                {/if}
              </div>
            </div>
          </article>
        {/each}
      </div>

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
        class:horizontal={paletteDock === "top" || paletteDock === "bottom"}
        class:dock-top={paletteDock === "top" && paletteDrag === null}
        class:dock-right={paletteDock === "right" && paletteDrag === null}
        class:dock-bottom={paletteDock === "bottom" && paletteDrag === null}
        class:dock-left={paletteDock === "left" && paletteDrag === null}
        class="instrument-palette"
        style:left={paletteDrag ? `${paletteX}px` : null}
        style:top={paletteDrag ? `${paletteY}px` : null}
        aria-label="Canvas tools"
      >
        <button class="palette-grip" type="button" aria-label="Move tool bar" title="Drag to move the bar" onpointerdown={beginPaletteDrag} onpointermove={movePalette} onpointerup={finishPaletteDrag} onpointercancel={finishPaletteDrag}>
          <i></i><i></i><i></i><i></i><i></i><i></i>
        </button>
        {#if toolPanel && toolPanelPreset}
          <div class="color-panel-anchor" style:--anchor={`${toolPanel.anchor}px`}>
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
        <button class:active={tool === "pen" && penPreset === 1} class="tool-tile" type="button" aria-label="Pen 1" aria-pressed={tool === "pen" && penPreset === 1} title="Pen 1 (1) — press again for settings" onclick={(event) => selectOrOpenTool("pen", 1, event.currentTarget)}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M15.5 3.5l5 5-9.5 9.5-5.5 1.5 1.5-5.5 9.5-9.5z"></path><path d="M6.5 19.5l1.3-3.6"></path></svg>
        </button>
        <button class:active={tool === "pen" && penPreset === 2} class="tool-tile" type="button" aria-label="Pen 2" aria-pressed={tool === "pen" && penPreset === 2} title="Pen 2 (2) — press again for settings" onclick={(event) => selectOrOpenTool("pen", 2, event.currentTarget)}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14 4l6 6-9 9-5 1 1-5 7-11z"></path><path d="M12.5 6.5l5 5"></path></svg>
        </button>
        <button class:active={tool === "highlighter"} class="tool-tile" type="button" aria-label="Highlighter" aria-pressed={tool === "highlighter"} title="Highlighter (3) — press again for settings" onclick={(event) => selectOrOpenTool("highlighter", 1, event.currentTarget)}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 15l7-9 5 4-6 9-4 1-2-5z"></path><path d="M8 20h8" stroke-width="2.4"></path></svg>
        </button>
        <button class:active={tool === "eraser"} class="tool-tile" type="button" aria-label="Eraser" aria-pressed={tool === "eraser"} title="Erase whole strokes (4)" onclick={() => activateTool("eraser")}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3.5" y="12" width="13" height="7" rx="1.6" transform="rotate(-38 10 15)"></rect><path d="M9 21h11"></path></svg>
        </button>
        <span class="palette-divider"></span>
        <button class:active={tool === "lasso" || tool === "select"} class="tool-tile" type="button" aria-label="Lasso select" aria-pressed={tool === "lasso" || tool === "select"} title="Select ink with lasso (5)" onclick={() => activateTool("lasso")}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><ellipse cx="12" cy="10" rx="8" ry="6" stroke-dasharray="3 2.6"></ellipse><path d="M9 16c0 2 1 4 3 4"></path><circle cx="12" cy="20" r="1.4"></circle></svg>
        </button>
        <button class="tool-tile dashed" type="button" aria-label="New Typst block" title="New Typst block (T)" onclick={addTypstBlock}>
          <span class="typst-symbol" aria-hidden="true">∑</span>
        </button>

        {#if tool === "pen" || tool === "highlighter"}
          <span class="palette-divider"></span>
          <div class="inline-group" role="group" aria-label="Stroke size">
            {#each activeWidthChips as chip (chip)}
              <button
                type="button"
                class="size-tile"
                class:active={nearestChip(activeWidthChips, activeWidth) === chip}
                aria-pressed={nearestChip(activeWidthChips, activeWidth) === chip}
                title={`${(chip / 2.835).toFixed(2)} mm`}
                onclick={() => setActiveWidth(chip)}
              >
                <span
                  class="size-line"
                  style:height={`${Math.max(2, Math.min(chip * (tool === "highlighter" ? 2.2 : 1.4), 9))}px`}
                  style:background={tool === "highlighter" ? `${activeInkColor}99` : "#aeb5be"}
                ></span>
              </button>
            {/each}
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
              <div class="color-panel-anchor" style:--anchor={`${colorPanel.anchor}px`}>
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
                  onRemove={() => {
                    if (colorPanel && colorPanel.index !== -1) removeSwatch(colorPanel.index);
                    colorPanel = null;
                  }}
                  onClose={() => (colorPanel = null)}
                />
              </div>
            {/if}
          </div>
        {:else if tool === "eraser"}
          <span class="palette-divider"></span>
          <div class="inline-group" role="group" aria-label="Eraser hit-area size">
            {#each [{ id: "small", d: 12 }, { id: "medium", d: 18 }, { id: "large", d: 26 }] as size (size.id)}
              <button
                type="button"
                class="size-tile"
                class:active={settings.eraserSize === size.id}
                aria-pressed={settings.eraserSize === size.id}
                title={`${size.id} hit area`}
                onclick={() => setEraserSize(size.id as "small" | "medium" | "large")}
              >
                <span class="size-ring" style:width={`${size.d}px`} style:height={`${size.d}px`}></span>
              </button>
            {/each}
          </div>
        {/if}
      </nav>

      {#if selectedStrokeIds.length > 0 || groupedStrokeIds.length > 0}
        <div class="context-actions" aria-label="Ink selection actions">
          <span>{selectedStrokeIds.length || groupedStrokeIds.length} ink selected</span>
          {#if selectedStrokeIds.length > 0}<button type="button" onclick={groupSelectedInk}>Group with Typst</button>{/if}
          {#if groupedStrokeIds.length > 0}<button type="button" onclick={ungroupInk}>Ungroup</button>{/if}
        </div>
      {/if}

      <div class="zoom-pill">
        <button type="button" aria-label="Zoom out" onclick={() => changeZoom(zoom - 0.1)}>−</button>
        <output aria-label="Page zoom">{Math.round(zoom * 100)}%</output>
        <button type="button" aria-label="Zoom in" onclick={() => changeZoom(zoom + 0.1)}>+</button>
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
    <span class="footer-divider"></span><span class:failure={transactionFailed} class="local-state">{transactionFailed ? "Needs attention" : "Local · saved"}</span>
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

  {#if metricsOpen}
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
          <dt>Latest Typst subprocess</dt><dd>{milliseconds(compileMs)} + {TYPST_IDLE_DEBOUNCE_MS} ms debounce</dd>
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

  .notebook-identity, .command-actions, .save-state, .tool-status {
    display: flex;
    align-items: center;
  }

  .notebook-identity { min-width: 0; gap: 11px; }
  .app-mark { width: 9px; height: 9px; flex: none; border-radius: 2px; background: #4a515c; }
  .notebook-title {
    overflow: hidden;
    color: var(--text);
    font-size: 16px;
    font-weight: 500;
    line-height: 1.1;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .save-state { gap: 7px; margin-top: 2px; color: var(--muted); font-size: 11px; }
  .state-dot, .blue-dot { width: 6px; height: 6px; flex: none; border-radius: 50%; background: var(--blueprint); }
  .state-dot.saving { animation: breathe 1s ease-in-out infinite alternate; }
  .state-dot.warning { background: var(--oxide); }
  .revision, .page-count, .zoom-pill output {
    color: var(--quiet);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
    letter-spacing: .02em;
  }

  .command-actions { gap: 8px; }
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
    display: flex;
    width: 100%;
    height: 100%;
    overflow: auto;
    padding: 46px 108px 58px;
    scrollbar-width: none;
    align-items: center;
    flex-direction: column;
    gap: 46px;
  }
  .page-scroll-content::-webkit-scrollbar { display: none; }
  .page-stack-item { position: relative; flex: none; }
  .page-stack-item.active-page .page-number { color: var(--blueprint-light); }
  .page-number {
    position: absolute;
    top: 0;
    right: calc(100% + 12px);
    color: var(--quiet);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
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

  .history-pill, .zoom-pill, .context-actions {
    position: absolute;
    z-index: 15;
    display: flex;
    align-items: center;
    border: 1px solid rgb(255 255 255 / 10%);
    background: var(--panel);
    box-shadow: 0 12px 30px rgb(0 0 0 / 45%);
  }

  .history-pill { top: 16px; left: 18px; gap: 2px; padding: 5px; border-radius: 10px; }
  .history-pill button, .zoom-pill button { display: grid; border-radius: 7px; background: transparent; color: var(--text); cursor: pointer; place-items: center; }
  .history-pill button { width: 40px; height: 40px; }
  .history-pill button:hover:not(:disabled), .zoom-pill button:hover { background: rgb(255 255 255 / 8%); }
  .history-pill svg { width: 19px; fill: none; stroke: currentColor; stroke-width: 1.7; stroke-linecap: round; stroke-linejoin: round; }

  .instrument-palette {
    position: absolute;
    z-index: 20;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 3px;
    padding: 6px;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 13px;
    background: var(--panel);
    box-shadow: 0 20px 50px rgb(0 0 0 / 52%);
    touch-action: none;
  }

  /* Vertical docks anchor below the top-left history pill so a long column can never cover it. */
  .instrument-palette.dock-left { top: 84px; left: 20px; }
  .instrument-palette.dock-right { top: 84px; right: 20px; }
  .instrument-palette.dock-top { top: 16px; left: 50%; transform: translateX(-50%); }
  .instrument-palette.dock-bottom { bottom: 16px; left: 50%; transform: translateX(-50%); }
  .instrument-palette.horizontal { flex-direction: row; }

  .instrument-palette.dragging { box-shadow: 0 26px 60px rgb(0 0 0 / 68%); }

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

  /* Tool tiles: uniform icon buttons; active fills blueprint. */
  .tool-tile {
    display: grid;
    width: 40px;
    height: 40px;
    flex: none;
    place-items: center;
    border-radius: 10px;
    background: transparent;
    color: #c4cad2;
    cursor: pointer;
  }
  .tool-tile:hover { background: rgb(255 255 255 / 6%); }
  .tool-tile.active { background: var(--blueprint); color: #fff; }
  .tool-tile svg { width: 21px; height: 21px; fill: none; stroke: currentColor; stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; }
  .tool-tile svg circle { fill: currentColor; stroke: none; }
  .tool-tile.dashed { border: 1px dashed rgb(255 255 255 / 22%); }
  .typst-symbol { font: 600 18px "STIX Two Text", "Times New Roman", serif; }

  .palette-divider { width: 26px; height: 1px; margin: 1px 0; background: rgb(255 255 255 / 12%); }
  .horizontal .palette-divider { width: 1px; height: 26px; margin: 0 3px; }

  /* Inline stroke sizes and colors carried on the palette bar (contextual to the active tool). */
  .inline-group { display: flex; flex-direction: column; align-items: center; gap: 3px; }
  .horizontal .inline-group { flex-direction: row; }
  .inline-group.colors { display: grid; grid-template-columns: repeat(2, auto); gap: 5px; }
  .horizontal .inline-group.colors { grid-template-columns: repeat(4, auto); }

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
    cursor: pointer;
  }

  .size-tile:hover { background: rgb(255 255 255 / 6%); }
  .size-tile.active { outline: 1.5px solid var(--blueprint); background: rgb(76 141 240 / 16%); }
  .size-line { width: 20px; border-radius: 3px; }
  .size-tile.active .size-line { background: var(--text) !important; }
  /* Border-box so the drawn circle is exactly the size asked for: otherwise each ring grows by
     its border and the three sizes step unevenly. */
  .size-ring { box-sizing: border-box; flex: none; border: 1.5px solid var(--muted); border-radius: 50%; }
  .size-tile.active .size-ring { border-color: var(--blueprint-light); }

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
  .color-panel-anchor {
    position: absolute;
    bottom: calc(100% + 10px);
    left: clamp(0px, calc(var(--anchor) - 108px), calc(100vw - 240px));
    z-index: 60;
  }
  .instrument-palette.dock-top .color-panel-anchor { top: calc(100% + 10px); bottom: auto; }
  /* Centred on the chip; the panel itself measures and nudges back inside the window, so no
     height is guessed here. */
  .instrument-palette.dock-left .color-panel-anchor,
  .instrument-palette.dock-right .color-panel-anchor {
    top: calc(var(--anchor) - 130px);
    bottom: auto;
    left: auto;
  }
  .instrument-palette.dock-left .color-panel-anchor { left: calc(100% + 10px); }
  .instrument-palette.dock-right .color-panel-anchor { right: calc(100% + 10px); }

  .context-actions { top: 18px; left: 50%; gap: 4px; padding: 5px; border-radius: 9px; transform: translateX(-50%); }
  .context-actions span { padding: 0 9px; color: var(--muted); font-size: 12px; }
  .context-actions button { padding: 7px 9px; border-radius: 5px; background: transparent; color: var(--text); font-size: 12px; cursor: pointer; }
  .context-actions button:hover { background: rgb(255 255 255 / 7%); }

  .zoom-pill { right: 18px; bottom: 16px; gap: 2px; padding: 4px; border-radius: 9px; }
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
  @keyframes breathe { from { opacity: .35; } to { opacity: 1; } }

  @media (max-width: 800px) {
    .page-scroll-content { padding-right: 88px; padding-left: 88px; }
    .operation-status, .page-count { display: none; }
    .notebook-title { max-width: 42vw; }
  }

  @media (max-height: 720px) {
    .instrument-palette.dock-left, .instrument-palette.dock-right { top: 64px; }
  }

  @media (prefers-reduced-motion: reduce) {
    .state-dot.saving { animation: none; }
  }
</style>
