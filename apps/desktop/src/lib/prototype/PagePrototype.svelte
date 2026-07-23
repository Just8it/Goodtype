<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { onDestroy, onMount } from "svelte";
  import { clampZoom, type Point } from "../geometry/coordinates";
  import type {
    InkLayer,
    NotebookManifest,
    Page,
    PageObject,
    Stroke,
  } from "../model";
  import {
    TYPST_IDLE_DEBOUNCE_MS,
    type TypstCompileResult,
  } from "../editor/typst";
  import {
    summarizeMetric,
    type StrokePerformance,
  } from "../ink/metrics";
  import type { InkTool } from "../ink/pipeline";
  import { moveSelected, scaleSelected } from "../ink/selection";
  import ImageObject from "./ImageObject.svelte";
  import InkSurface from "./InkSurface.svelte";
  import { nearestPaletteDock, type PaletteDock } from "./palette";
  import TypstBlock from "./TypstBlock.svelte";

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
    bytes: number[];
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

  const PAGE_WIDTH_PT = 595;
  const PAGE_HEIGHT_PT = 842;
  const MAIN_TYPST_ID = "typst-001";
  const BLOCK_PATH = "blocks/equation.typ";
  const INK_LAYER_ID = "ink-layer-001";
  const INK_GROUP_ID = "ink-group-001";
  const GROUP_ID = "group-001";
  const TYPST_SAVE_DEBOUNCE_MS = 250;
  const tauriAvailable =
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

  let root = $state("");
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
  let typstDirty = false;
  let workspace = $state<HTMLElement>();
  let pageViewport = $state<HTMLElement>();
  let pageFrame = $state<HTMLElement>();
  let paletteX = $state(24);
  let paletteY = $state(96);
  let paletteDock = $state<PaletteDock>("left");
  let paletteDrag = $state<PaletteDrag | null>(null);
  let penPreset = $state<1 | 2>(1);
  let moreOpen = $state(false);
  let metricsOpen = $state(false);
  const touchPoints = new Map<number, Point>();
  let pinchStart: PinchStart | null = null;

  onMount(() => {
    void initialize();
    window.addEventListener("keydown", historyShortcut);
  });
  onDestroy(() => {
    if (typstCommitTimer) clearTimeout(typstCommitTimer);
    window.removeEventListener("keydown", historyShortcut);
    revokeImageUrl();
  });

  $effect(() => {
    const metrics = metricsPayload();
    if (tauriAvailable && root) {
      void invoke("write_phase0_metrics", { root, metrics }).catch(() => {});
    }
  });

  async function initialize() {
    busy = true;
    if (!tauriAvailable) {
      root = "Browser preview (persistence and real Typst compilation require Tauri)";
      pageOpen = true;
      busy = false;
      status = "Browser preview ready";
      return;
    }

    try {
      root = await invoke<string>("phase0_notebook_root");
      let snapshot: NotebookSnapshot;
      try {
        snapshot = await invoke<NotebookSnapshot>("open_notebook", { root });
      } catch {
        snapshot = buildSnapshot();
        await invoke("create_notebook", { root, snapshot });
      }
      applySnapshot(snapshot);
      pageOpen = true;
      status = "Notebook ready";
    } catch (error) {
      status = `Could not open the prototype notebook: ${message(error)}`;
    } finally {
      busy = false;
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
          inkLayerId: INK_LAYER_ID,
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
      id: "page-001",
      revision,
      geometry: { widthPt: PAGE_WIDTH_PT, heightPt: PAGE_HEIGHT_PT },
      background: { kind: "plain", color: "#ffffff" },
      objects,
      readingOrder: grouped
        ? [GROUP_ID, ...extraTypstIds, ...(image ? ["image-001"] : [])]
        : [...typstBlocks.map((block) => block.id), ...(image ? ["image-001"] : [])],
      inkLayers: [{ id: INK_LAYER_ID, path: "ink/page-001-layer-001.json" }],
    };
    return {
      manifest: {
        schemaVersion: 1,
        id: "phase0b-notebook",
        title: "Goodtype",
        pages: [{ id: page.id, path: "pages/page-001.json" }],
        defaultPage: {
          geometry: page.geometry,
          background: page.background,
        },
        sharedStylePath: null,
        createdAt,
        modifiedAt: now,
      },
      page,
      blocks: typstBlocks.map((block) => ({
        path: block.path,
        bytes: Array.from(new TextEncoder().encode(block.source)),
      })),
      assets: image ? [{ path: image.path, bytes: image.bytes }] : [],
      inkLayers: [
        {
          schemaVersion: 1,
          id: INK_LAYER_ID,
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
    if (typstCommitTimer) clearTimeout(typstCommitTimer);
    typstCommitTimer = undefined;
    typstDirty = false;
    revokeImageUrl();
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
        bytes: asset.bytes,
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
      try {
        result = await invoke<TypstCompileResult>("compile_typst", {
          root,
          request,
        });
      } catch (error) {
        result = {
          generation: request.generation,
          svg: null,
          widthPt: null,
          heightPt: null,
          diagnostics: [{ severity: "error", message: message(error) }],
        };
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

  function addTypstBlock() {
    let number = typstBlocks.length + 1;
    while (typstBlocks.some((block) => block.id === `typst-${String(number).padStart(3, "0")}`)) number += 1;
    const id = `typst-${String(number).padStart(3, "0")}`;
    typstBlocks = [
      ...typstBlocks,
      {
        id,
        path: `blocks/${id}.typ`,
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

  function activateTool(next: InkTool, preset?: 1 | 2) {
    if (preset) penPreset = preset;
    tool = next;
    const names: Record<InkTool, string> = {
      pen: `Pen ${penPreset}`,
      highlighter: "Highlighter",
      eraser: "Eraser",
      lasso: "Lasso",
      select: "Ink selection",
    };
    status = `${names[next]} active`;
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
    if (
      event.pointerType === "pen" ||
      (event.target instanceof Element &&
        event.target.closest(".typst-block, .image-object"))
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
    const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
    const url = URL.createObjectURL(file);
    const dimensions = await imageDimensions(url);
    revokeImageUrl();
    const fit = Math.min(1, 220 / dimensions.width, 160 / dimensions.height);
    image = {
      path: `assets/pasted-${Date.now()}.${extensionForMime(file.type)}`,
      bytes,
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

  function queueCommit(label: string) {
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
          status = `${label}; saved revision ${revision}`;
        } catch (error) {
          transactionFailed = true;
          status = `Change could not be saved: ${message(error)}. Reopen to restore the last confirmed state.`;
        }
      })
      .finally(() => {
        pendingTransactions -= 1;
      });
  }

  function changeStrokes(next: Stroke[], label: string) {
    const remaining = new Set(next.map((stroke) => stroke.id));
    groupedStrokeIds = groupedStrokeIds.filter((id) => remaining.has(id));
    strokes = next;
    queueCommit(label);
  }

  function addStroke(stroke: Stroke) {
    changeStrokes([...strokes, stroke], `Added ${stroke.tool} stroke`);
  }

  function changeImage(next: Partial<Pick<ImageState, "x" | "y" | "scale">>) {
    if (!image) return;
    image = { ...image, ...next };
    queueCommit("Updated image");
  }

  function undo() {
    queueHistory("undo_notebook", "Undid change");
  }

  function redo() {
    queueHistory("redo_notebook", "Redid change");
  }

  function queueHistory(command: "undo_notebook" | "redo_notebook", label: string) {
    if (!tauriAvailable || transactionFailed) return;
    flushTypstCommit();
    pendingTransactions += 1;
    transactionQueue = transactionQueue
      .then(async () => {
        try {
          const result = await invoke<HistoryResult>(command, { root });
          applySnapshot(result.snapshot);
          canUndo = result.canUndo;
          canRedo = result.canRedo;
          status = `${label}; saved revision ${revision}`;
        } catch (error) {
          transactionFailed = true;
          status = `${label} failed: ${message(error)}. Reopen to restore the last confirmed state.`;
        }
      })
      .finally(() => {
        pendingTransactions -= 1;
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
    }
    const editingText =
      event.target instanceof Element &&
      event.target.closest(".cm-editor, input, textarea, [contenteditable=true]");
    if (editingText) return;
    if (event.key === "Escape") {
      moreOpen = false;
      metricsOpen = false;
      selectedTypstId = null;
      selectedImage = false;
      directObjectInput = false;
      return;
    }
    if (!event.ctrlKey && !event.metaKey && !event.altKey) {
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
      const path = await invoke<string>("export_pdf", {
        root,
        outputName: "phase0b-page.pdf",
        page: {
          widthPt: PAGE_WIDTH_PT,
          heightPt: PAGE_HEIGHT_PT,
          blocks: typstBlocks.map((block) => ({
            ...block.transform,
            source: block.source,
          })),
          strokes,
          images: image
            ? [
                {
                  relativePath: image.path,
                  x: image.x,
                  y: image.y,
                  widthPt: image.widthPt,
                  heightPt: image.heightPt,
                  scale: image.scale,
                },
              ]
            : [],
        },
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
    status = "Page closed in memory; use Reopen to load the saved files";
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
    if (tool === "highlighter") return "#e0912b";
    return penPreset === 1 ? "#1e232b" : "#2f6fdb";
  }

  function inkWidthPt() {
    if (tool === "highlighter") return 3.78;
    return penPreset === 1 ? 1 : 2;
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

  function mimeForPath(path: string) {
    if (path.endsWith(".jpg") || path.endsWith(".jpeg")) return "image/jpeg";
    if (path.endsWith(".svg")) return "image/svg+xml";
    if (path.endsWith(".webp")) return "image/webp";
    return "image/png";
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
  <header class="command-strip">
    <div class="notebook-identity">
      <span class="app-mark" aria-hidden="true"></span>
      <div>
        <div class="notebook-title">Goodtype notebook</div>
        <div class="save-state">
          <span class:warning={transactionFailed} class:saving={pendingTransactions > 0} class="state-dot"></span>
          <span>{transactionFailed ? "Save blocked" : pendingTransactions > 0 ? "Saving" : "Saved"}</span>
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
      <button class="icon-button" class:active={moreOpen} type="button" aria-label="More notebook actions" aria-expanded={moreOpen} onclick={() => (moreOpen = !moreOpen)}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="5" cy="12" r="1.7"></circle><circle cx="12" cy="12" r="1.7"></circle><circle cx="19" cy="12" r="1.7"></circle></svg>
      </button>
    </div>
  </header>

  {#if moreOpen}
    <aside class="overflow-menu" aria-label="Notebook actions">
      <div class="menu-path" title={root}><span>Local notebook</span><strong>{root || "Opening..."}</strong></div>
      <button type="button" onclick={() => void persist()}>Confirm saved</button>
      <button type="button" onclick={() => { metricsOpen = true; moreOpen = false; }}>Timing evidence</button>
      <div class="menu-divider"></div>
      {#if pageOpen}
        <button class="muted-action" type="button" onclick={closePage}>Close notebook</button>
      {:else}
        <button type="button" onclick={reopen}>Reopen notebook</button>
      {/if}
    </aside>
  {/if}

  {#if pageOpen}
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
        <div class="page-frame" bind:this={pageFrame} style:width={`${PAGE_WIDTH_PT * zoom}px`} style:height={`${PAGE_HEIGHT_PT * zoom}px`}>
          <div class="page" style:width={`${PAGE_WIDTH_PT}px`} style:height={`${PAGE_HEIGHT_PT}px`} style:transform={`scale(${zoom})`}>
            <div class="objects">
              {#each typstBlocks as block (block.id)}
                <TypstBlock
                  id={block.id}
                  source={block.source}
                  initialX={block.transform.x}
                  initialY={block.transform.y}
                  initialLayoutWidthPt={block.transform.layoutWidthPt}
                  initialScale={block.transform.scale}
                  compileResult={block.result}
                  selected={selectedTypstId === block.id}
                  toPageDelta={(x, y) => ({ x: x / zoom, y: y / zoom })}
                  onSelect={() => {
                    selectedTypstId = block.id;
                    selectedImage = false;
                  }}
                  onDeselect={() => (selectedTypstId = null)}
                  onCompile={(request) => compileTypst(block.id, request)}
                  onSourceChange={(source) => updateTypstSource(block.id, source)}
                  onTransform={(transform) => updateTypstTransform(block.id, transform)}
                />
              {/each}
              {#if image}
                <ImageObject
                  src={image.url}
                  alt={image.alt}
                  x={image.x}
                  y={image.y}
                  widthPt={image.widthPt}
                  heightPt={image.heightPt}
                  scale={image.scale}
                  selected={selectedImage}
                  toPageDelta={(x, y) => ({ x: x / zoom, y: y / zoom })}
                  onSelect={() => {
                    selectedImage = true;
                    selectedTypstId = null;
                  }}
                  onMove={(position) => changeImage(position)}
                  onScale={(scale) => changeImage({ scale })}
                />
              {/if}
            </div>
            <div class:object-input={directObjectInput} class="ink-layer">
              <InkSurface
                {strokes}
                {selectedStrokeIds}
                pageWidthPt={PAGE_WIDTH_PT}
                pageHeightPt={PAGE_HEIGHT_PT}
                {zoom}
                color={inkColor()}
                widthPt={inkWidthPt()}
                {tool}
                onStrokeFinalized={addStroke}
                onStrokesChange={(next) => changeStrokes(next, "Updated ink")}
                onSelectionChange={updateInkSelection}
                onStrokeMetrics={recordStrokeMetrics}
              />
            </div>
          </div>
        </div>
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
        <button class="palette-grip" type="button" aria-label="Move tool palette" title="Drag to move the palette" onpointerdown={beginPaletteDrag} onpointermove={movePalette} onpointerup={finishPaletteDrag} onpointercancel={finishPaletteDrag}>
          <span></span><i></i><i></i><i></i>
        </button>
        <button class:active={tool === "pen" && penPreset === 1} class="preset-tool" type="button" aria-pressed={tool === "pen" && penPreset === 1} title="Pen 1 · 0.35 mm · graphite (1)" onclick={() => activateTool("pen", 1)}>
          <span class="stroke-sample pen-one"></span><kbd>1</kbd>
        </button>
        <button class:active={tool === "pen" && penPreset === 2} class="preset-tool" type="button" aria-pressed={tool === "pen" && penPreset === 2} title="Pen 2 · 0.70 mm · blueprint (2)" onclick={() => activateTool("pen", 2)}>
          <span class="stroke-sample pen-two"></span><kbd>2</kbd>
        </button>
        <button class:active={tool === "highlighter"} class="preset-tool" type="button" aria-pressed={tool === "highlighter"} title="Highlighter · 4.0 mm · amber (3)" onclick={() => activateTool("highlighter")}>
          <span class="stroke-sample highlighter"></span><kbd>3</kbd>
        </button>
        <span class="palette-divider"></span>
        <button class:active={tool === "eraser"} class="symbol-tool" type="button" aria-pressed={tool === "eraser"} title="Erase whole strokes (4)" onclick={() => activateTool("eraser")}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3.5" y="12" width="13" height="7" rx="1.6" transform="rotate(-38 10 15)"></rect><path d="M9 21h11"></path></svg><kbd>4</kbd>
        </button>
        <button class:active={tool === "lasso" || tool === "select"} class="symbol-tool" type="button" aria-pressed={tool === "lasso" || tool === "select"} title="Select ink with lasso (5)" onclick={() => activateTool("lasso")}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><ellipse cx="12" cy="10" rx="8" ry="6"></ellipse><path d="M9 16c0 2 1 4 3 4"></path><circle cx="12" cy="20" r="1.4"></circle></svg><kbd>5</kbd>
        </button>
        <button class="symbol-tool" type="button" aria-label="New Typst block" title="New Typst block (6)" onclick={addTypstBlock}>
          <span class="typst-symbol" aria-hidden="true">T+</span><kbd>6</kbd>
        </button>
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
  {:else}
    <section class="closed-state">
      <span class="closed-mark" aria-hidden="true"></span><h1>Notebook closed</h1>
      <p>The confirmed local files are still safe.</p>
      <button type="button" onclick={reopen} disabled={busy}>Reopen notebook</button>
    </section>
  {/if}

  <footer class="status-strip">
    <div class="tool-status"><span class="blue-dot"></span><strong>{currentToolLabel()}</strong><span>{currentToolDetail()}</span></div>
    <div class:failure={transactionFailed} class="operation-status" title={status}>{status}</div>
    <span class="page-count">Page 1 of 1</span><span class="footer-divider"></span>
    <button type="button" onclick={() => changeZoom(1)}>{Math.round(zoom * 100)}%</button>
    <span class="footer-divider"></span><span class:failure={transactionFailed} class="local-state">{transactionFailed ? "Needs attention" : "Local · saved"}</span>
  </footer>

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
    position: relative;
    display: grid;
    grid-template-rows: 58px minmax(0, 1fr) 34px;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: var(--surround);
    color: var(--text);
    font-family: "Segoe UI Variable", "Segoe UI", system-ui, sans-serif;
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
    font-family: Bahnschrift, "Arial Narrow", sans-serif;
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
  .revision, .page-count, .zoom-pill output, .preset-tool kbd, .symbol-tool kbd {
    color: var(--quiet);
    font-family: "Cascadia Mono", Consolas, monospace;
    font-size: 10px;
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
  .export-button:hover, .icon-button:hover, .icon-button.active { background: rgb(255 255 255 / 8%); }

  .overflow-menu {
    position: absolute;
    z-index: 50;
    top: 52px;
    right: 14px;
    width: min(300px, calc(100vw - 28px));
    padding: 7px;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 11px;
    background: var(--panel);
    box-shadow: 0 18px 44px rgb(0 0 0 / 55%);
  }

  .overflow-menu button { width: 100%; padding: 10px 11px; border-radius: 7px; background: transparent; color: var(--text); text-align: left; cursor: pointer; }
  .overflow-menu button:hover { background: rgb(255 255 255 / 6%); }
  .menu-path { padding: 8px 10px 10px; border-bottom: 1px solid rgb(255 255 255 / 8%); margin-bottom: 5px; }
  .menu-path span { display: block; color: var(--quiet); font-size: 10px; letter-spacing: .08em; text-transform: uppercase; }
  .menu-path strong { display: block; overflow: hidden; margin-top: 4px; color: var(--muted); font: 400 10px "Cascadia Mono", Consolas, monospace; text-overflow: ellipsis; white-space: nowrap; }
  .menu-divider { height: 1px; margin: 5px 8px; background: rgb(255 255 255 / 9%); }
  .overflow-menu .muted-action { color: var(--muted); }

  .workspace-surround {
    position: relative;
    min-height: 0;
    overflow: hidden;
    background: radial-gradient(circle at 50% 46%, rgb(255 255 255 / 2%), transparent 42%), var(--surround);
  }

  .page-scroll-content {
    display: grid;
    width: 100%;
    height: 100%;
    overflow: auto;
    padding: 46px 108px 58px;
    scrollbar-width: none;
    place-items: center;
  }
  .page-scroll-content::-webkit-scrollbar { display: none; }
  .page-frame { position: relative; flex: none; }
  .page {
    position: relative;
    overflow: hidden;
    background: var(--paper);
    box-shadow: 0 2px 6px rgb(0 0 0 / 30%), 0 24px 60px rgb(0 0 0 / 45%);
    transform-origin: top left;
  }

  .objects, .ink-layer { position: absolute; inset: 0; }
  .objects { z-index: 1; pointer-events: none; }
  .objects :global(.typst-block), .objects :global(.image-object) { pointer-events: auto; }
  .ink-layer { z-index: 2; }
  .ink-layer.object-input { pointer-events: none; }

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
    width: 74px;
    flex-direction: column;
    align-items: center;
    gap: 5px;
    padding: 7px 0 11px;
    border: 1px solid rgb(255 255 255 / 12%);
    border-radius: 16px;
    background: var(--panel);
    box-shadow: 0 24px 60px rgb(0 0 0 / 55%);
    touch-action: none;
  }

  .instrument-palette.dock-left { top: 50%; left: 24px; transform: translateY(-50%); }
  .instrument-palette.dock-right { top: 50%; right: 24px; transform: translateY(-50%); }
  .instrument-palette.dock-top { top: 16px; left: 50%; transform: translateX(-50%); }
  .instrument-palette.dock-bottom { bottom: 16px; left: 50%; transform: translateX(-50%); }
  .instrument-palette.horizontal {
    width: auto;
    height: 74px;
    flex-direction: row;
    padding: 0 11px 0 7px;
  }

  .instrument-palette.dragging { box-shadow: 0 30px 70px rgb(0 0 0 / 70%); }
  .palette-grip {
    display: grid;
    width: 100%;
    height: 37px;
    flex: none;
    grid-template-columns: repeat(3, 3px);
    grid-template-rows: 4px 3px;
    justify-content: center;
    gap: 4px;
    padding: 6px 0 7px;
    background: transparent;
    cursor: grab;
  }

  .dragging .palette-grip { cursor: grabbing; }
  .palette-grip span { width: 26px; height: 4px; grid-column: 1 / -1; border-radius: 2px; background: rgb(255 255 255 / 22%); }
  .palette-grip i { width: 3px; height: 3px; border-radius: 50%; background: rgb(255 255 255 / 28%); }

  .preset-tool, .symbol-tool {
    position: relative;
    display: flex;
    width: 58px;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    border-radius: 9px;
    background: transparent;
    color: #c4cad2;
    cursor: pointer;
  }

  .preset-tool { height: 47px; }
  .symbol-tool { height: 49px; }
  .preset-tool:hover, .symbol-tool:hover { background: rgb(255 255 255 / 6%); }
  .preset-tool.active, .symbol-tool.active { outline: 1.5px solid var(--blueprint); background: rgb(76 141 240 / 18%); }
  .preset-tool.active::before, .symbol-tool.active::before {
    position: absolute;
    top: 9px;
    bottom: 9px;
    left: 0;
    width: 3px;
    border-radius: 0 3px 3px 0;
    background: var(--blueprint);
    content: "";
  }

  .stroke-sample { display: grid; width: 42px; height: 20px; border-radius: 5px; background: var(--paper); place-items: center; }
  .stroke-sample::after { display: block; width: 25px; border-radius: 3px; content: ""; }
  .pen-one::after { height: 2px; background: #1e232b; }
  .pen-two::after { height: 4px; background: #2f6fdb; }
  .stroke-sample.highlighter::after { width: 28px; height: 11px; border-radius: 2px; background: rgb(224 145 43 / 55%); }
  .preset-tool kbd, .symbol-tool kbd { border: 0; background: transparent; }
  .active kbd { color: #c4cad2; }
  .symbol-tool svg { width: 21px; height: 21px; fill: none; stroke: currentColor; stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; }
  .symbol-tool svg circle { fill: currentColor; stroke: none; }
  .typst-symbol { font: 600 18px Bahnschrift, "Arial Narrow", sans-serif; }
  .palette-divider { width: 42px; height: 1px; margin: 2px 0; background: rgb(255 255 255 / 10%); }
  .horizontal .palette-divider { width: 1px; height: 42px; margin: 0 2px; }
  .horizontal .palette-grip { width: 37px; height: 100%; align-content: center; padding: 0; }

  .context-actions { top: 18px; left: 50%; gap: 4px; padding: 5px; border-radius: 9px; transform: translateX(-50%); }
  .context-actions span { padding: 0 9px; color: var(--muted); font-size: 12px; }
  .context-actions button { padding: 7px 9px; border-radius: 5px; background: transparent; color: var(--text); font-size: 12px; cursor: pointer; }
  .context-actions button:hover { background: rgb(255 255 255 / 7%); }

  .zoom-pill { right: 18px; bottom: 16px; gap: 2px; padding: 4px; border-radius: 9px; }
  .zoom-pill button { width: 34px; height: 34px; font-size: 19px; }
  .zoom-pill output { min-width: 48px; padding: 0 7px; color: #c4cad2; text-align: center; }

  .closed-state { display: grid; align-content: center; justify-items: center; padding: 2rem; background: var(--surround); text-align: center; }
  .closed-mark { width: 34px; height: 42px; border: 1px solid var(--quiet); border-radius: 3px; box-shadow: inset 0 5px var(--panel); }
  .closed-state h1 { margin: 18px 0 5px; font: 500 24px Bahnschrift, "Arial Narrow", sans-serif; }
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
  .panel-heading h2 { margin: 4px 0 0; font: 500 22px Bahnschrift, "Arial Narrow", sans-serif; }
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
    .instrument-palette { gap: 3px; transform: scale(.88); transform-origin: top left; }
  }

  @media (prefers-reduced-motion: reduce) {
    .state-dot.saving { animation: none; }
  }
</style>
