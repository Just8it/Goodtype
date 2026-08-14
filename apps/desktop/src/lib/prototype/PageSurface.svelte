<script lang="ts">
  import type { PageBackground, Stroke } from "../model";
  import PaperLayer from "../page/PaperLayer.svelte";
  import type { StrokePerformance } from "../ink/metrics";
  import type { TypstCompileResult } from "../editor/typst";
  import { getCachedTypst } from "../editor/typstCache";
  import { DEFAULT_PRESET_PATH, pagePresetPath } from "../page/presets";
  import {
    DEFAULT_PRESSURE_CALIBRATION,
    type InkTool,
    type PressureCalibration,
  } from "../ink/pipeline";
  import ImageObject from "./ImageObject.svelte";
  import InkSurface from "./InkSurface.svelte";
  import PageText from "./PageText.svelte";
  import TypstBlock from "./TypstBlock.svelte";
  import type { BlockView, ImageView, PageTypstView, TypstTransform } from "./pageView";

  // The one page renderer. `interactive` is the only difference between the page being edited
  // and the pages above and below it, so a page keeps its rendered blocks — and its painted
  // SVGs — when focus moves onto it, instead of being torn down and rebuilt by a second
  // component.
  let {
    blocks = [],
    pageTypst = null,
    images = [],
    results = {},
    strokes = [],
    newStrokeZIndex = 1_000_001,
    selectedStrokeIds = [],
    background = { kind: "plain", color: "#ffffff" },
    pageWidthPt,
    pageHeightPt,
    zoom = 1,
    interactive = false,
    root = null,
    inlineEditing = true,
    sharedStyle = "",
    pageTextBaselineGrid = true,
    presetRevision = 0,
    onRequestEdit,
    tool = "select",
    color = "#16212b",
    widthPt = 2,
    pressure = true,
    taper = 0,
    opacity = 1,
    straighten = false,
    eraseRadiusPt = 8,
    calibration = DEFAULT_PRESSURE_CALIBRATION,
    directObjectInput = false,
    selectedBlockId = null,
    selectedImageId = null,
    onCompile,
    onSourceChange,
    onTransform,
    onSelectBlock,
    onDeselectBlock,
    onSelectImage,
    onMoveImage,
    onScaleImage,
    onStrokeFinalized,
    onStrokesChange,
    onSelectionChange,
    onStrokeMetrics,
  }: {
    blocks?: BlockView[];
    pageTypst?: PageTypstView | null;
    images?: ImageView[];
    results?: Record<string, TypstCompileResult | null>;
    strokes?: Stroke[];
    newStrokeZIndex?: number;
    selectedStrokeIds?: string[];
    /** The paper: a flat colour, or a template resolved against this page's geometry. */
    background?: PageBackground;
    pageWidthPt: number;
    pageHeightPt: number;
    zoom?: number;
    interactive?: boolean;
    /** Notebook root, forwarded to the editor for completions. */
    root?: string | null;
    /** False while the side view is open: blocks route editing there instead. */
    inlineEditing?: boolean;
    sharedStyle?: string;
    pageTextBaselineGrid?: boolean;
    presetRevision?: number;
    onRequestEdit?: (id: string) => void;
    tool?: InkTool;
    color?: string;
    widthPt?: number;
    /** Nib parameters for the active tool, resolved by the palette and stored on each stroke. */
    pressure?: boolean;
    taper?: number;
    opacity?: number;
    straighten?: boolean;
    eraseRadiusPt?: number;
    calibration?: PressureCalibration;
    directObjectInput?: boolean;
    selectedBlockId?: string | null;
    selectedImageId?: string | null;
    onCompile: (
      id: string,
      request: {
        source: string;
        sharedStyle?: string | null;
        widthPt: number;
        generation: number;
      },
    ) => void;
    onSourceChange?: (id: string, source: string) => void;
    onTransform?: (id: string, transform: TypstTransform) => void;
    onSelectBlock?: (id: string) => void;
    onDeselectBlock?: () => void;
    onSelectImage?: (id: string) => void;
    onMoveImage?: (id: string, position: { x: number; y: number }) => void;
    onScaleImage?: (id: string, scale: number) => void;
    onStrokeFinalized?: (stroke: Stroke) => void;
    onStrokesChange?: (strokes: Stroke[]) => void;
    onSelectionChange?: (ids: string[]) => void;
    onStrokeMetrics?: (metrics: StrokePerformance) => void;
  } = $props();

  const toPageDelta = (screenDx: number, screenDy: number) => ({
    x: screenDx / zoom,
    y: screenDy / zoom,
  });

  // Editing temporarily lifts the object input surface; committed content otherwise follows the
  // page's shared visual order.
  let editingBlockId = $state<string | null>(null);
</script>

<PaperLayer {background} widthPt={pageWidthPt} heightPt={pageHeightPt} {root} {zoom} />
<div class="page-text-layer">
  {#if pageTypst}
    <PageText
      source={pageTypst.source}
      compileResult={results[pageTypst.id] ?? null}
      compileContext={pagePresetPath(pageTypst.source) ? "" : sharedStyle}
      {background}
      geometry={{ widthPt: pageWidthPt, heightPt: pageHeightPt }}
      snapBlocksToGrid={pageTextBaselineGrid}
      compileDependency={pagePresetPath(pageTypst.source) === DEFAULT_PRESET_PATH ? presetRevision : 0}
      readingOrder={pageTypst.readingOrder}
      onCompile={(request) => onCompile(pageTypst.id, request)}
    />
  {/if}
</div>
<div class:interactive class:editing={editingBlockId !== null} class="objects">
  {#each blocks as block (block.id)}
    <TypstBlock
      id={block.id}
      source={block.source}
      initialX={block.x}
      initialY={block.y}
      initialLayoutWidthPt={block.layoutWidthPt}
      initialScale={block.scale}
      zIndex={block.zIndex}
      readingOrder={block.readingOrder}
      compileResult={results[block.id] ?? null}
      cached={getCachedTypst(`${sharedStyle}\n${block.source}`, block.layoutWidthPt) ?? null}
      compileContext={sharedStyle}
      {root}
      selected={interactive && selectedBlockId === block.id}
      {toPageDelta}
      {pageWidthPt}
      {pageHeightPt}
      onSelect={interactive ? () => onSelectBlock?.(block.id) : undefined}
      onDeselect={interactive ? () => onDeselectBlock?.() : undefined}
      onCompile={(request) => onCompile(block.id, { ...request, sharedStyle })}
      onSourceChange={(source) => onSourceChange?.(block.id, source)}
      onTransform={(transform) => onTransform?.(block.id, transform)}
      onEditingChange={(editing) =>
        (editingBlockId = editing ? block.id : editingBlockId === block.id ? null : editingBlockId)}
      inlineEditing={interactive && inlineEditing}
      onRequestEdit={() => onRequestEdit?.(block.id)}
    />
  {/each}
  {#each images as image (image.id)}
    <ImageObject
      id={image.id}
      src={image.url}
      alt={image.alt}
      x={image.x}
      y={image.y}
      widthPt={image.widthPt}
      heightPt={image.heightPt}
      scale={image.scale}
      zIndex={image.zIndex}
      readingOrder={image.readingOrder}
      selected={interactive && selectedImageId === image.id}
      {toPageDelta}
      {pageWidthPt}
      {pageHeightPt}
      onSelect={interactive ? () => onSelectImage?.(image.id) : undefined}
      onMove={(position) => onMoveImage?.(image.id, position)}
      onScale={(scale) => onScaleImage?.(image.id, scale)}
    />
  {/each}
</div>
<div class:object-input={directObjectInput} class="ink-layer">
  <InkSurface
    {strokes}
    {newStrokeZIndex}
    objectZIndices={[...blocks.map((block) => block.zIndex), ...images.map((image) => image.zIndex)]}
    {selectedStrokeIds}
    {pageWidthPt}
    {pageHeightPt}
    {zoom}
    {tool}
    {color}
    {widthPt}
    {pressure}
    {taper}
    {opacity}
    {straighten}
    {eraseRadiusPt}
    {calibration}
    onStrokeFinalized={(stroke) => onStrokeFinalized?.(stroke)}
    onStrokesChange={(next) => onStrokesChange?.(next)}
    onSelectionChange={(next) => onSelectionChange?.(next)}
    onStrokeMetrics={(metrics) => onStrokeMetrics?.(metrics)}
  />
</div>

<style>
  .page-text-layer,
  .objects,
  .ink-layer {
    position: absolute;
    inset: 0;
  }

  .page-text-layer {
    z-index: 1;
    overflow: hidden;
    pointer-events: none;
  }

  .objects {
    pointer-events: none;
  }

  /* Editing must receive input even when this block is normally below committed ink. */
  .objects.editing {
    z-index: 2147483647;
  }

  /* Only the page being edited hands pointer input to its objects; neighbours stay readable
     but inert, so a stray tap near the edge of the viewport cannot drag a block on a page the
     writer is not looking at. Ink stays live on every page. */
  .objects.interactive :global(.typst-block),
  .objects.interactive :global(.image-object) {
    pointer-events: auto;
  }

  .ink-layer.object-input {
    pointer-events: none;
  }
</style>
