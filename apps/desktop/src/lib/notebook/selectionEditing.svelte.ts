import type { createNavigation } from "./viewportNavigation.svelte";

import { translate, notice, type UiText } from "../i18n";

import { storePastedImage } from "../ipc/notebook";

import { finishInkPreview, inkPreviewBounds, type InkPreview } from "../ink/preview";
import type { NotebookSnapshot } from "../ipc/types";

import { pageToScreen, type Point } from "../geometry/coordinates";
import { placeFloatingToolbar, type ViewRect } from "../geometry/placement";

import { type Stroke } from "../model";

import { pointerRole, type InkTool, type PointerMapping } from "../ink/pipeline";
import { keepsSelection, moveSelected, restyleStrokes, selectionBounds, toolAfterSelection, type StylePatch } from "../ink/selection";

import { inkLayersWithStrokes, type TypstBlockState, type ImageView, type ShapeView, type TypstTransform } from "./pageView";

import { DEFAULT_LASSO_TARGETS, boxOutline, shapeLassoItem, selectLassoObjects, combinedBounds, transformSelectionPoint, type LassoItem } from "./lasso";
import { nativeContextMenuAllowed } from "./contextMenu";

import { type ManagedMixedGroup } from "./snapshot";

import { moveReadingObject, moveVisualItems, type VisualMove } from "./objectOrder";

import { clipboardMarker, isEmptyBundle, pasteOffset, pastedShapes, pastedStrokes, shiftTo, type ClipboardBundle } from "./clipboard";

import { type AppSettings } from "../settings";

export type SelectionEnvironment = {
  activeInkLayerId: string;
  activeInkLayerPath: string;
  readonly activePageId: string;
  activeSnapshot: NotebookSnapshot | null;
  readonly changeStrokes: (next: Stroke[], label: UiText, replaced?: ReadonlyMap<string, string[]>) => void;
  readonly closeToolCard: () => void;
  readonly editContext: () => { root: string; pageId: string; current: () => boolean };
  images: ImageView[];
  readonly message: (error: unknown) => string;
  readonly navigation: Pick<ReturnType<typeof createNavigation>, "fingerDraws" | "pagePointAt">;
  readonly nextReadingOrder: () => number;
  readonly nextVisualZIndex: () => number;
  readonly pageFrame: HTMLElement | undefined;
  readonly pageInputLocked: boolean;
  readonly pointerMapping: PointerMapping;
  readonly queueCommit: (label: UiText) => void;
  readonly root: string;
  readonly scheduleInkCommit: (label: UiText) => void;
  readonly selectedInk: Stroke[];
  readonly selectedShapes: ShapeView[];
  readonly settings: AppSettings;
  shapes: ShapeView[];
  status: UiText;
  strokes: Stroke[];
  readonly tauriAvailable: boolean;
  tool: InkTool;
  typstBlocks: TypstBlockState[];
  workspace: HTMLElement | undefined;
  readonly zoom: number;
};

/** Instance-owned selection state and operations. Callers supply live values at the boundary. */
export function createSelection(env: SelectionEnvironment) {

  let selectedStrokeIds = $state<string[]>([]);

  let groupedStrokeIds = $state<string[]>([]);

  let selectedImageId = $state<string | null>(null);

  let selectedShapeId = $state<string | null>(null);

  let editingShapeId = $state<string | null>(null);

  let mixedGroup = $state<ManagedMixedGroup | null>(null);

  let selectedTypstId = $state<string | null>(null);

  let lassoObjectIds = $state<string[]>([]);

  let mixedStrokePreview = $state.raw<InkPreview | null>(null);

  function lassoItems(): LassoItem[] {
    return [
      ...env.shapes.map(shapeLassoItem),
      ...env.images.map(image => ({ id: image.id, kind: "images" as const, closed: true,
        outline: boxOutline(image.x, image.y, image.widthPt * image.scale, image.heightPt * image.scale, image.rotation) })),
      ...env.typstBlocks.map(block => ({ id: block.id, kind: "text" as const, closed: true,
        outline: boxOutline(block.transform.x, block.transform.y,
          (block.result?.widthPt ?? block.transform.layoutWidthPt) * block.transform.scale,
          (block.result?.heightPt ?? 64) * block.transform.scale, block.transform.rotation) })),
    ];
  }

  const mixedSelectionBounds = $derived(lassoObjectIds.length ? combinedBounds(
    lassoItems().filter(item => lassoObjectIds.includes(item.id)), inkPreviewBounds(env.strokes, selectedStrokeIds, mixedStrokePreview)) : null);

  function lassoSelection(polygon: Point[], inkIds: string[]) {
    selectedTypstId = null; selectedImageId = null; selectedShapeId = null; editingShapeId = null;
    lassoObjectIds = selectLassoObjects(lassoItems(), polygon, env.settings.lassoTargets ?? DEFAULT_LASSO_TARGETS);
    updateInkSelection(inkIds);
    env.status = notice("workspace.selectedItems", { "value1": inkIds.length + lassoObjectIds.length });
  }

  let mixedTransformBase: { blocks: typeof env.typstBlocks; images: typeof env.images; shapes: typeof env.shapes; strokes: Stroke[] } | null = null;

  let mixedTransformChanged = false;

  function beginMixedTransform() {
    mixedTransformBase = { blocks: env.typstBlocks, images: env.images, shapes: env.shapes, strokes: env.strokes };
    mixedTransformChanged = false;
    mixedStrokePreview = null;
  }

  function previewMixedTransform(delta: Point, scale: number, anchor: Point) {
    if (!mixedTransformBase) return;
    const base = mixedTransformBase;
    const selected = new Set(lassoObjectIds);
    env.typstBlocks = base.blocks.map(block => selected.has(block.id) ? { ...block, transform: {
      ...block.transform, ...transformSelectionPoint(block.transform, delta, scale, anchor), scale: block.transform.scale * scale,
    } } : block);
    env.images = base.images.map(image => selected.has(image.id) ? { ...image,
      ...transformSelectionPoint(image, delta, scale, anchor), scale: image.scale * scale } : image);
    env.shapes = base.shapes.map(shape => selected.has(shape.id) ? { ...shape,
      ...transformSelectionPoint(shape, delta, scale, anchor), scale: shape.scale * scale } : shape);
    mixedStrokePreview = { ids: selectedStrokeIds, delta, scale, anchor };
    mixedTransformChanged = delta.x !== 0 || delta.y !== 0 || scale !== 1;
  }

  function endMixedTransform(cancelled: boolean) {
    const base = mixedTransformBase;
    mixedTransformBase = null;
    if (!base) return;
    if (cancelled) { env.typstBlocks = base.blocks; env.images = base.images; env.shapes = base.shapes; env.strokes = base.strokes; }
    else if (mixedTransformChanged) {
      if (mixedStrokePreview) env.strokes = finishInkPreview(base.strokes, mixedStrokePreview);
      env.queueCommit(notice("workspace.transformedSelection"));
    }
    mixedStrokePreview = null;
  }

  let directObjectInput = $state(false);

  /** True while the selection tool is only active because the lasso handed over to it. */
  /** The tool a selection hand-over gives back once the selection empties. */
  let selectionReturnTo = $state<InkTool | null>(null);

  let selectionToolbarElement = $state<HTMLElement>();

  let selectionToolbarFrame: number | undefined;

  let selectionToolbarPosition = $state({ left: 0, top: 0, ready: false });

  function groupSelectedInk() {
    if (selectedStrokeIds.length === 0) return;
    const layers = inkLayersWithStrokes(env.activeSnapshot?.inkLayers ?? [], env.strokes, env.activeInkLayerId)
      .filter(layer => layer.strokes.some(stroke => selectedStrokeIds.includes(stroke.id)));
    if (layers.length > 1) {
      env.status = notice("workspace.selectInkFromOneLayerBeforeCreating");
      return;
    }
    const drawingLayer = env.activeSnapshot?.page.inkLayers.find(layer => layer.id === layers[0]?.id);
    if (drawingLayer) { env.activeInkLayerId = drawingLayer.id; env.activeInkLayerPath = drawingLayer.path; }
    const typstId = selectedTypstId;
    if (!typstId) {
      env.status = notice("workspace.selectATypstBlockBeforeGroupingIt");
      return;
    }
    const typstObject = env.activeSnapshot?.page.objects.find((object) => object.id === typstId);
    if (
      (typstObject?.groupId && typstObject.groupId !== mixedGroup?.groupId) ||
      (mixedGroup?.active && mixedGroup.typstId !== typstId) ||
      env.strokes.some(
        (stroke) =>
          selectedStrokeIds.includes(stroke.id) &&
          stroke.groupId &&
          stroke.groupId !== mixedGroup?.inkGroupId,
      )
    ) {
      env.status = notice("workspace.ungroupTheExistingSelectionBeforeCreatingAnother");
      return;
    }
    const occupied = new Set(env.activeSnapshot?.page.objects.map((object) => object.id) ?? []);
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
    env.strokes = env.strokes.map((stroke) => ({
      ...stroke,
      groupId: selected.has(stroke.id) ? mixedGroup!.inkGroupId : stroke.groupId,
    }));
    env.status = notice("counts.groupedStrokes", { count: groupedStrokeIds.length });
    env.queueCommit(notice("workspace.groupedInkWithTypst"));
  }

  /**
   * Every change to the ink selection goes through here, because the lasso's hand-over to the
   * selection tool has to be undone when the selection empties — and a delete that assigned
   * `selectedStrokeIds` directly would skip that and strand the writer on a tool they never
   * picked.
   */
  function updateInkSelection(ids: string[]) {
    selectedStrokeIds = ids;
    const next = toolAfterSelection(env.tool, ids.length + lassoObjectIds.length, selectionReturnTo);
    env.tool = next.tool;
    selectionReturnTo = next.returnTo;
  }

  function restyleSelection(patch: StylePatch) {
    const count = env.selectedInk.length + env.selectedShapes.length;
    env.strokes = restyleStrokes(env.strokes, selectedStrokeIds, patch);
    const ids = new Set(env.selectedShapes.map(shape => shape.id));
    env.shapes = env.shapes.map(shape => {
      if (!ids.has(shape.id)) return shape;
      const style = { ...shape.style };
      if (patch.color !== undefined) {
        style.strokeColor = patch.color;
        if (style.fillColor) style.fillColor = `${patch.color}24`;
      }
      if (patch.widthPt !== undefined) style.strokeWidthPt = patch.widthPt;
      if (patch.pattern !== undefined) style.pattern = patch.pattern;
      if (patch.patternSpacing !== undefined) style.patternSpacing = patch.patternSpacing;
      return { ...shape, style };
    });
    env.queueCommit(notice("workspace.restyledSelection"));
    env.status = notice("counts.restyled", { count: count });
  }

  function routeObjectPointer(event: PointerEvent) {
    // Explicit handles own contact, even with a brush or the Shape tool in hand.
    if (event.target instanceof Element && event.target.closest(".shape-controls .control-hit, .mixed-selection, .typst-block .handle")) {
      directObjectInput = true;
      const role = pointerRole(event, env.tool, env.pointerMapping);
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
      event.target.closest(".selection-actions, .instrument-palette, .palette-editor, .editor-dock")
    ) {
      return;
    }
    // The shape tool builds on the ink surface, and the eraser has to reach it to rub anything
    // out, so neither hands the pointer to an object under the cursor.
    if (env.tool === "shape" || env.tool === "eraser" || env.tool === "lasso" || pointerRole(event, env.tool, env.pointerMapping) === "lasso") {
      directObjectInput = false;
      return;
    }
    if ((event.pointerType === "pen" || env.navigation.fingerDraws(event)) && !keepsSelection(env.tool)) {
      directObjectInput = false;
      return;
    }
    const object = document
      .elementsFromPoint(event.clientX, event.clientY)
      .map((element) => element.closest<HTMLElement>(".typst-block, .image-object, .shape-object"))
      .find((element) => element && env.workspace?.contains(element));
    directObjectInput = Boolean(object);
    if (event.type !== "pointerdown") return;
    lassoObjectIds = [];
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
      env.status = selectedTypstId
        ? notice("workspace.typstBlockSelected")
        : selectedImageId
          ? notice("workspace.imageSelected")
          : notice("workspace.shapeSelected");
    }
  }

  function closeObjectSelection(event: PointerEvent) {
    // The card lives inside the bar, so a press anywhere outside it dismisses the card.
    if (!(event.target instanceof Element) || !event.target.closest(".instrument-palette")) {
      env.closeToolCard();
    }
    // A stylus press mid-stroke is writing, not "deselect" — unless a selection tool is active,
    // where a press on empty page means exactly that.
    if (
      ((event.pointerType === "pen" || env.navigation.fingerDraws(event)) && !keepsSelection(env.tool)) ||
      (event.target instanceof Element &&
        event.target.closest(
          ".mixed-selection, .typst-block, .image-object, .shape-object, .selection-actions, .overflow-menu, .instrument-palette, .palette-editor, [data-preserve-selection]",
        ))
    ) {
      return;
    }
    if (lassoObjectIds.length) { lassoObjectIds = []; updateInkSelection([]); }
    selectedTypstId = null;
    selectedImageId = null;
    selectedShapeId = null;
    editingShapeId = null;
  }

  /// Delete the selected object or ink as one undoable committed action. Original asset
  /// files are never removed; deleting an image only drops the page's reference.
  function selectedItems() {
    return { objects: [...new Set([...lassoObjectIds, selectedTypstId, selectedImageId, selectedShapeId]
      .filter((id): id is string => id !== null))], strokes: [...selectedStrokeIds] };
  }

  function deleteSelection(selection = selectedItems()): boolean {
    const ids = new Set(selection.objects);
    const inkIds = new Set(selection.strokes);
    if (!ids.size && !inkIds.size) return false;
    if (env.activeSnapshot?.page.objects.some(object => ids.has(object.id) && object.groupId && object.groupId !== mixedGroup?.groupId) ||
      env.strokes.some(stroke => inkIds.has(stroke.id) && stroke.groupId && stroke.groupId !== mixedGroup?.inkGroupId)) {
      env.status = notice("workspace.ungroupThisSelectionBeforeDeletingIt");
      return false;
    }
    if (mixedGroup?.active && ids.has(mixedGroup.typstId)) {
      mixedGroup = { ...mixedGroup, active: false };
      env.strokes = env.strokes.map(stroke => groupedStrokeIds.includes(stroke.id) ? { ...stroke, groupId: null } : stroke);
      groupedStrokeIds = [];
    }
    env.typstBlocks = env.typstBlocks.filter(block => !ids.has(block.id));
    for (const image of env.images) if (ids.has(image.id) && image.url.startsWith("blob:")) URL.revokeObjectURL(image.url);
    env.images = env.images.filter(image => !ids.has(image.id));
    env.shapes = env.shapes.filter(shape => !ids.has(shape.id));
    env.strokes = env.strokes.filter(stroke => !inkIds.has(stroke.id));
    groupedStrokeIds = groupedStrokeIds.filter(id => !inkIds.has(id));
    lassoObjectIds = lassoObjectIds.filter(id => !ids.has(id));
    if (selectedTypstId && ids.has(selectedTypstId)) selectedTypstId = null;
    if (selectedImageId && ids.has(selectedImageId)) selectedImageId = null;
    if (selectedShapeId && ids.has(selectedShapeId)) { selectedShapeId = null; editingShapeId = null; }
    updateInkSelection(selectedStrokeIds.filter(id => !inkIds.has(id)));
    env.queueCommit(notice("workspace.deletedSelection"));
    env.status = notice("workspace.deletedSelectedItemsOriginalFilesAreKept");
    return true;
  }

  /// Arrow-key movement for the current selection; batched through the ink debounce so
  /// holding a key produces one committed action.
  function nudgeSelection(dx: number, dy: number): boolean {
    if (lassoObjectIds.length) {
      beginMixedTransform(); previewMixedTransform({ x: dx, y: dy }, 1, { x: 0, y: 0 });
      mixedTransformBase = null;
      env.scheduleInkCommit(notice("workspace.movedSelection"));
      return true;
    }
    if (selectedTypstId) {
      env.typstBlocks = env.typstBlocks.map((block) =>
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
      env.scheduleInkCommit(notice("workspace.movedTypstBlock"));
      return true;
    }
    if (selectedImageId) {
      env.images = env.images.map((image) =>
        image.id === selectedImageId
          ? { ...image, x: image.x + dx, y: image.y + dy }
          : image,
      );
      env.scheduleInkCommit(notice("workspace.movedImage"));
      return true;
    }
    if (selectedShapeId) {
      env.shapes = env.shapes.map((shape) =>
        shape.id === selectedShapeId
          ? { ...shape, x: shape.x + dx, y: shape.y + dy }
          : shape,
      );
      env.scheduleInkCommit(notice("workspace.movedShape"));
      return true;
    }
    if (selectedStrokeIds.length > 0) {
      env.strokes = moveSelected(env.strokes, selectedStrokeIds, { x: dx, y: dy });
      env.scheduleInkCommit(notice("workspace.movedInkSelection"));
      return true;
    }
    return false;
  }

  function ungroupInk() {
    env.strokes = env.strokes.map((stroke) =>
      groupedStrokeIds.includes(stroke.id) &&
      (!mixedGroup || stroke.groupId === mixedGroup.inkGroupId)
        ? { ...stroke, groupId: null }
        : stroke,
    );
    groupedStrokeIds = [];
    if (mixedGroup) mixedGroup = { ...mixedGroup, active: false };
    env.status = notice("workspace.removedTheTypstAndInkGroup");
    env.queueCommit(notice("workspace.ungroupedInkAndTypst"));
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
      lassoObjectIds.length > 0 ||
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

  async function bundleSelection(selection = selectedItems()): Promise<ClipboardBundle | null> {
    const context = env.editContext();
    const ids = new Set(selection.strokes);
    const objectIds = new Set(selection.objects);
    const selectedImages = env.images.filter(image => objectIds.has(image.id));
    // Capture every value before fetching bytes; selection and page state may change meanwhile.
    const bundle: ClipboardBundle = {
      token: crypto.randomUUID(),
      pageId: context.pageId,
      strokes: env.strokes.filter(stroke => ids.has(stroke.id)),
      shapes: env.shapes.filter(shape => objectIds.has(shape.id)),
      blocks: env.typstBlocks.filter(block => objectIds.has(block.id)).map(block => ({
        source: block.source, ...block.transform,
        heightPt: env.workspace?.querySelector<HTMLElement>(`.active-page [data-object-id="${CSS.escape(block.id)}"]`)?.offsetHeight ?? 0,
      })),
      images: [],
    };
    const imageData = await Promise.all(selectedImages.map(image => imageBytes(image.url)));
    if (!context.current()) return null;
    if (imageData.some(data => !data)) {
      env.status = notice("workspace.anImageCouldNotBeCopiedThe");
      return null;
    }
    bundle.images = selectedImages.map((image, index) => ({
      ...imageData[index]!, alt: image.alt, x: image.x, y: image.y,
      widthPt: image.widthPt, heightPt: image.heightPt, scale: image.scale, rotation: image.rotation,
    }));
    return isEmptyBundle(bundle) ? null : bundle;
  }

  async function copySelection(cut = false) {
    const context = env.editContext();
    const selection = selectedItems();
    const selectedIds = new Set([...selection.objects, ...selection.strokes]);
    const captured = [...env.typstBlocks, ...env.images, ...env.shapes, ...env.strokes].filter(item => selectedIds.has(item.id));
    const bundle = await bundleSelection(selection);
    if (!context.current()) return;
    if (!bundle) return;
    clipboard = bundle;
    pasteCount = 0;
    try {
      if (!navigator.clipboard) throw new Error("Clipboard unavailable");
      await navigator.clipboard.writeText(clipboardMarker(bundle));
    } catch {
      if (context.current()) env.status = notice("workspace.clipboardWriteFailed");
      return;
    }
    if (!context.current()) return;
    if (cut) {
      const current = new Set([...env.typstBlocks, ...env.images, ...env.shapes, ...env.strokes]);
      if (captured.some(item => !current.has(item))) {
        env.status = notice("workspace.copiedCutWasCanceledBecauseTheCopied");
        return;
      }
      if (deleteSelection(selection)) env.status = notice("workspace.cutToTheClipboard");
    } else env.status = notice("workspace.copied");
  }

  async function duplicateSelection(at: Point | null = null) {
    const context = env.editContext();
    const bundle = await bundleSelection();
    if (!bundle || !context.current()) return;
    clipboard = bundle;
    pasteCount = 0;
    await pasteBundle(bundle, at);
  }

  function freshObjectId(prefix: string): string {
    const taken = new Set([
      ...env.typstBlocks.map((entry) => entry.id),
      ...env.images.map((entry) => entry.id),
      ...env.shapes.map((entry) => entry.id),
      ...(env.activeSnapshot?.page.objects.map((object) => object.id) ?? []),
    ]);
    let number = 1;
    while (taken.has(`${prefix}-${String(number).padStart(3, "0")}`)) number += 1;
    return `${prefix}-${String(number).padStart(3, "0")}`;
  }

  /// `at` is a page point to centre the paste on (from the context menu); without it a paste
  /// steps away from its original on the same page and keeps its place on any other.
  async function pasteBundle(bundle: ClipboardBundle, at: Point | null = null) {
    const context = env.editContext();
    const paths: string[] = [];
    try {
      for (const image of bundle.images) {
        const filename = `pasted-${crypto.randomUUID()}.${extensionForMime(image.mime)}`;
        paths.push(env.tauriAvailable && context.root
          ? await storePastedImage(context.root, filename, image.bytes)
          : `assets/${filename}`);
        if (!context.current()) return;
      }
    } catch (error) {
      if (context.current()) env.status = notice("workspace.theSelectionCouldNotBePasted", { "value1": env.message(error) });
      return;
    }
    if (!context.current()) return;
    const oldObjects = new Set([...env.typstBlocks, ...env.images, ...env.shapes].map(object => object.id));
    pasteCount += 1;
    const shift = at ? shiftTo(bundle, at) : pasteOffset(bundle, env.activePageId, pasteCount);
    let zIndex = env.nextVisualZIndex();
    let readingOrder = env.nextReadingOrder();

    const newStrokes = pastedStrokes(bundle.strokes, shift, zIndex, () => crypto.randomUUID());
    zIndex += newStrokes.length;
    const newShapes = pastedShapes(bundle.shapes, shift, zIndex, readingOrder, () => crypto.randomUUID());
    zIndex += newShapes.length;
    readingOrder += newShapes.length;

    let lastObject: { kind: "typst" | "image" | "shape"; id: string } | null =
      newShapes.length > 0 ? { kind: "shape", id: newShapes.at(-1)!.id } : null;
    if (newShapes.length > 0) env.shapes = [...env.shapes, ...newShapes];

    for (const block of bundle.blocks) {
      const id = freshObjectId("typst");
      env.typstBlocks = [
        ...env.typstBlocks,
        {
          id,
          path: `blocks/${env.activePageId}-${id}.typ`,
          source: block.source,
          transform: {
            x: block.x + shift.dx,
            y: block.y + shift.dy,
            layoutWidthPt: block.layoutWidthPt,
            rotation: block.rotation ?? 0,
            scale: block.scale,
          },
          result: null,
          zIndex: zIndex++,
          readingOrder: readingOrder++,
        },
      ];
      lastObject = { kind: "typst", id };
    }

    for (const [index, image] of bundle.images.entries()) {
      const path = paths[index];
      const id = freshObjectId("image");
      env.images = [
        ...env.images,
        {
          id,
          path,
          url: URL.createObjectURL(new Blob([image.bytes.slice()], { type: image.mime })),
          alt: image.alt,
          x: image.x + shift.dx,
          y: image.y + shift.dy,
          widthPt: image.widthPt,
          heightPt: image.heightPt,
          rotation: image.rotation ?? 0,
          scale: image.scale,
          zIndex: zIndex++,
          readingOrder: readingOrder++,
        },
      ];
      lastObject = { kind: "image", id };
    }

    if (newStrokes.length > 0) env.changeStrokes([...env.strokes, ...newStrokes], notice("workspace.pastedInk"));
    if (lastObject) env.queueCommit(notice("workspace.pasted"));

    // The paste stays selected, ready to be dragged into place.
    lassoObjectIds = [...env.typstBlocks, ...env.images, ...env.shapes].filter(object => !oldObjects.has(object.id)).map(object => object.id);
    selectedTypstId = null;
    selectedImageId = null;
    selectedShapeId = null;
    editingShapeId = null;
    updateInkSelection(newStrokes.map((stroke) => stroke.id));
    const count =
      newStrokes.length + newShapes.length + bundle.blocks.length + bundle.images.length;
    env.status = notice("counts.pasted", { count: count });
  }

  /**
   * The canvas context menu: right-click, or a long press with a finger. Paste lands where it was
   * opened; with something selected it also offers the selection's own actions.
   */
  let contextMenu = $state<{ x: number; y: number; at: Point | null } | null>(null);

  function openContextMenu(clientX: number, clientY: number) {
    env.closeToolCard();
    contextMenu = { x: clientX, y: clientY, at: env.navigation.pagePointAt(clientX, clientY) };
  }

  function canvasContextMenu(event: MouseEvent) {
    // A pen's barrel press is a tool, handled on the ink surface, which cancels the menu there.
    if (event.defaultPrevented) return;
    const target = event.target instanceof Element ? event.target : null;
    if (!target?.closest(".page-scroll-content")) return;
    if (target.closest(".cm-editor, input, textarea, [contenteditable=true]")) return;
    event.preventDefault();
    // Windows/browser touch right-taps can arrive even after the long-press timer was cancelled.
    if (!nativeContextMenuAllowed(event)) return;
    openContextMenu(event.clientX, event.clientY);
  }

  let contextBarElement = $state<HTMLElement>();

  let contextBarPosition = $state({ left: 0, top: 0, ready: false });

  /** Closes the bar and runs a context-bar action, handing it the page point the bar opened at. */
  function fromContext(action: (at: Point | null) => unknown) {
    return () => {
      const at = contextMenu?.at ?? null;
      contextMenu = null;
      void action(at);
    };
  }

  async function handlePaste(event: ClipboardEvent) {
    if (env.pageInputLocked) return;
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
      env.status = notice("workspace.thatImageIsLargerThanTheMiB");
      return;
    }
    const context = env.editContext();
    const url = URL.createObjectURL(file);
    const dimensions = await imageDimensions(url);
    const filename = `pasted-${crypto.randomUUID()}.${extensionForMime(file.type)}`;

    // Store the original once, here. Commits then reference the asset by path instead of
    // carrying its bytes, so ordinary pen strokes never marshal an image across IPC.
    let path = `assets/${filename}`;
    if (env.tauriAvailable && env.root) {
      try {
        path = await storePastedImage(context.root, filename, new Uint8Array(await file.arrayBuffer()));
      } catch (error) {
        URL.revokeObjectURL(url);
        env.status = notice("workspace.theImageCouldNotBeStored", { "value1": env.message(error) });
        return;
      }
    }

    if (!context.current()) { URL.revokeObjectURL(url); return; }

    const fit = Math.min(1, 220 / dimensions.width, 160 / dimensions.height);
    const existingIds = new Set([
      ...env.typstBlocks.map((block) => block.id),
      ...env.images.map((image) => image.id),
      ...env.shapes.map((shape) => shape.id),
      ...(env.activeSnapshot?.page.objects.map((object) => object.id) ?? []),
    ]);
    let number = env.images.length + 1;
    while (existingIds.has(`image-${String(number).padStart(3, "0")}`)) number += 1;
    const id = `image-${String(number).padStart(3, "0")}`;
    env.images = [...env.images, {
      id,
      path,
      url,
      alt: file.name || translate("workspace.pastedImage"),
      x: 300,
      y: 380,
      widthPt: Math.max(1, dimensions.width * fit),
      heightPt: Math.max(1, dimensions.height * fit),
      scale: 1,
      zIndex: env.nextVisualZIndex(),
      readingOrder: env.nextReadingOrder(),
    }];
    selectedImageId = id;
    selectedTypstId = null;
    selectedShapeId = null;
    editingShapeId = null;
    env.status = notice("workspace.pastedOneOriginalImage");
    env.queueCommit(notice("workspace.pastedImage"));
  }

  function selectedObjectId(): string | null {
    return selectedTypstId ?? selectedImageId ?? selectedShapeId;
  }

  function canMoveVisual(direction: -1 | 1): boolean {
    const id = selectedObjectId();
    if (!id || !env.activeSnapshot) return false;
    return moveVisualItems(
      env.activeSnapshot.page,
      env.strokes,
      [id],
      [],
      direction < 0 ? "backward" : "forward",
    ) !== null;
  }

  function canMoveReading(direction: -1 | 1): boolean {
    const id = selectedObjectId();
    if (!id || !env.activeSnapshot) return false;
    const index = env.activeSnapshot.page.readingOrder.indexOf(id);
    return direction < 0
      ? index > 0
      : index >= 0 && index < env.activeSnapshot.page.readingOrder.length - 1;
  }

  function inkActionIds(): string[] {
    return selectedStrokeIds.length > 0 ? selectedStrokeIds : groupedStrokeIds;
  }

  function canMoveInkVisual(direction: -1 | 1): boolean {
    return Boolean(
      env.activeSnapshot &&
        moveVisualItems(
          env.activeSnapshot.page,
          env.strokes,
          lassoObjectIds,
          inkActionIds(),
          direction < 0 ? "backward" : "forward",
        ),
    );
  }

  function changeInkVisualOrder(direction: -1 | 1) {
    if (!env.activeSnapshot) return;
    const next = moveVisualItems(
      env.activeSnapshot.page,
      env.strokes,
      lassoObjectIds,
      inkActionIds(),
      direction < 0 ? "backward" : "forward",
    );
    if (!next) return;
    env.strokes = next.strokes;
    applyObjectOrder(next.page, notice("workspace.changedVisualOrder"));
    env.status = direction < 0 ? notice("workspace.movedSelectedInkBack") : notice("workspace.movedSelectedInkForward");
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

  /** Places the toolbar; false when the selection's element is not on the page yet. */
  function positionSelectionToolbar(): boolean {
    if (!env.workspace || !selectionToolbarElement) return false;
    let anchor: ViewRect | null = null;
    const objectId = selectedObjectId();
    if (objectId) {
      anchor = env.workspace
        .querySelector<HTMLElement>(`.active-page [data-object-id="${CSS.escape(objectId)}"]`)
        ?.getBoundingClientRect() ?? null;
    } else {
      const bounds = mixedSelectionBounds ?? selectionBounds(env.strokes, inkActionIds());
      const frame = env.pageFrame?.getBoundingClientRect();
      if (bounds && frame) {
        const view = { pageOriginX: 0, pageOriginY: 0, zoom: env.zoom };
        const topLeft = pageToScreen({ x: bounds.left, y: bounds.top }, frame, view);
        const bottomRight = pageToScreen({ x: bounds.right, y: bounds.bottom }, frame, view);
        anchor = {
          left: topLeft.x, top: topLeft.y, right: bottomRight.x, bottom: bottomRight.y,
          width: bottomRight.x - topLeft.x, height: bottomRight.y - topLeft.y,
        };
      }
    }
    if (!anchor) return false;
    const boundary = env.workspace.getBoundingClientRect();
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

  function applyObjectOrder(page: NonNullable<typeof env.activeSnapshot>["page"], label: UiText) {
    if (!env.activeSnapshot) return;
    env.activeSnapshot = { ...env.activeSnapshot, page };
    const fields = new Map(page.objects.map((object) => [object.id, object]));
    env.typstBlocks = env.typstBlocks.map((block) => {
      const object = fields.get(block.id);
      return object
        ? { ...block, zIndex: object.zIndex, readingOrder: object.readingOrder }
        : block;
    });
    env.images = env.images.map((image) => {
      const object = fields.get(image.id);
      return object
        ? { ...image, zIndex: object.zIndex, readingOrder: object.readingOrder }
        : image;
    });
    env.shapes = env.shapes.map((shape) => {
      const object = fields.get(shape.id);
      return object
        ? { ...shape, zIndex: object.zIndex, readingOrder: object.readingOrder }
        : shape;
    });
    env.queueCommit(label);
  }

  function changeVisualOrder(move: VisualMove) {
    const id = selectedObjectId();
    if (!id || !env.activeSnapshot) return;
    const next = moveVisualItems(env.activeSnapshot.page, env.strokes, [id], [], move);
    if (!next) return;
    env.strokes = next.strokes;
    applyObjectOrder(next.page, notice("workspace.changedVisualOrder"));
  }

  function changeReadingOrder(direction: -1 | 1) {
    const id = selectedObjectId();
    if (!id || !env.activeSnapshot) return;
    const next = moveReadingObject(env.activeSnapshot.page, id, direction);
    if (next === env.activeSnapshot.page) {
      env.status = notice("workspace.thatObjectCannotMoveFurtherInReading");
      return;
    }
    applyObjectOrder(next, notice("workspace.changedReadingOrder"));
  }

  function imageDimensions(url: string): Promise<{ width: number; height: number }> {
    return new Promise((resolve, reject) => {
      const candidate = new Image();
      candidate.onload = () =>
        resolve({ width: candidate.naturalWidth, height: candidate.naturalHeight });
      candidate.onerror = () => reject(new Error(translate("workspace.thePastedImageCouldNotBeDecoded")));
      candidate.src = url;
    });
  }

  function extensionForMime(mime: string) {
    if (mime === "image/jpeg") return "jpg";
    if (mime === "image/svg+xml") return "svg";
    if (mime === "image/webp") return "webp";
    return "png";
  }
  function dispose() {
    if (selectionToolbarFrame) cancelAnimationFrame(selectionToolbarFrame);
    selectionToolbarFrame = undefined;
  }
  return {
    dispose,
    lassoSelection,
    beginMixedTransform,
    previewMixedTransform,
    endMixedTransform,
    groupSelectedInk,
    updateInkSelection,
    restyleSelection,
    routeObjectPointer,
    closeObjectSelection,
    deleteSelection,
    nudgeSelection,
    ungroupInk,
    hasCopyableSelection,
    copySelection,
    duplicateSelection,
    pasteBundle,
    openContextMenu,
    canvasContextMenu,
    fromContext,
    handlePaste,
    selectedObjectId,
    canMoveVisual,
    canMoveReading,
    canMoveInkVisual,
    changeInkVisualOrder,
    scheduleSelectionToolbar,
    changeVisualOrder,
    changeReadingOrder,
    get selectedStrokeIds() { return selectedStrokeIds; },
    set selectedStrokeIds(value: typeof selectedStrokeIds) { selectedStrokeIds = value; },
    get groupedStrokeIds() { return groupedStrokeIds; },
    set groupedStrokeIds(value: typeof groupedStrokeIds) { groupedStrokeIds = value; },
    get selectedImageId() { return selectedImageId; },
    set selectedImageId(value: typeof selectedImageId) { selectedImageId = value; },
    get selectedShapeId() { return selectedShapeId; },
    set selectedShapeId(value: typeof selectedShapeId) { selectedShapeId = value; },
    get editingShapeId() { return editingShapeId; },
    set editingShapeId(value: typeof editingShapeId) { editingShapeId = value; },
    get mixedGroup() { return mixedGroup; },
    set mixedGroup(value: typeof mixedGroup) { mixedGroup = value; },
    get selectedTypstId() { return selectedTypstId; },
    set selectedTypstId(value: typeof selectedTypstId) { selectedTypstId = value; },
    get lassoObjectIds() { return lassoObjectIds; },
    set lassoObjectIds(value: typeof lassoObjectIds) { lassoObjectIds = value; },
    get mixedStrokePreview() { return mixedStrokePreview; },
    get selectionReturnTo() { return selectionReturnTo; },
    set selectionReturnTo(value: typeof selectionReturnTo) { selectionReturnTo = value; },
    get directObjectInput() { return directObjectInput; },
    set directObjectInput(value: typeof directObjectInput) { directObjectInput = value; },
    get mixedSelectionBounds() { return mixedSelectionBounds; },
    get clipboard() { return clipboard; },
    get contextMenu() { return contextMenu; },
    set contextMenu(value: typeof contextMenu) { contextMenu = value; },
    get contextBarElement() { return contextBarElement; },
    set contextBarElement(value: typeof contextBarElement) { contextBarElement = value; },
    get contextBarPosition() { return contextBarPosition; },
    set contextBarPosition(value: typeof contextBarPosition) { contextBarPosition = value; },
    get selectionToolbarElement() { return selectionToolbarElement; },
    set selectionToolbarElement(value: typeof selectionToolbarElement) { selectionToolbarElement = value; },
    get selectionToolbarPosition() { return selectionToolbarPosition; },
    set selectionToolbarPosition(value: typeof selectionToolbarPosition) { selectionToolbarPosition = value; },
  };
}
