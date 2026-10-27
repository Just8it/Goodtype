<script lang="ts">
  import { t } from "../i18n";
  import { onDestroy, onMount } from "svelte";
  import TypstEditor from "../editor/TypstEditor.svelte";
  import TypstSizeControls from "./TypstSizeControls.svelte";
  import {
    applyTypstCompileResult,
    emptyTypstPreview,
    requestTypstCompile,
    TYPST_IDLE_DEBOUNCE_MS,
    type TypstCompileResult,
  } from "../editor/typst";
  import type { CachedTypst } from "../editor/typstCache";
  import { keepOnPage } from "../geometry/placement";
  import { rotatedVector } from "../geometry/coordinates";
  import type { TypstTransform } from "./pageView";
  import { MIN_TYPST_WIDTH_PT, scaleTypstCorner } from "./typstSizing";

  type Gesture = {
    kind: "move" | "reflow" | "scale";
    pointerId: number;
    clientX: number;
    clientY: number;
    start: TypstTransform;
    heightPt: number;
  };

  let {
    id,
    source,
    initialX,
    initialY,
    initialLayoutWidthPt,
    initialScale = 1,
    rotation = 0,
    zIndex = 0,
    readingOrder = 0,
    compileResult = null,
    cached = null,
    compileContext = "",
    root = null,
    selected = false,
    toPageDelta,
    pageWidthPt,
    pageHeightPt,
    onSelect,
    onDeselect,
    onCompile,
    onSourceChange,
    onTransform,
    onEditingChange,
    inlineEditing = true,
    onRequestEdit,
    zoom = 1,
  }: {
    id: string;
    source: string;
    initialX: number;
    initialY: number;
    initialLayoutWidthPt: number;
    initialScale?: number;
    rotation?: number;
    zIndex?: number;
    readingOrder?: number;
    compileResult?: TypstCompileResult | null;
    cached?: CachedTypst | null;
    /** Shared notebook source whose changes require a fresh preview. */
    compileContext?: string;
    /** Notebook root, forwarded so the editor can ask Rust for completions. */
    root?: string | null;
    selected?: boolean;
    toPageDelta: (screenDx: number, screenDy: number) => { x: number; y: number };
    /** The sheet this block has to stay reachable on. */
    pageWidthPt: number;
    pageHeightPt: number;
    onSelect?: () => void;
    onDeselect?: () => void;
    onCompile: (request: {
      source: string;
      widthPt: number;
      generation: number;
    }) => void;
    onSourceChange: (source: string) => void;
    onTransform: (transform: TypstTransform) => void;
    /** Lets the page lift the whole object layer while this block is being edited. */
    onEditingChange?: (editing: boolean) => void;
    /** False while the side view is open: editing happens there instead of on the page. */
    inlineEditing?: boolean;
    /** Asks the page to edit this block in the side view. */
    onRequestEdit?: () => void;
    zoom?: number;
  } = $props();

  let x = $state(0);
  let y = $state(0);
  let layoutWidthPt = $state(240);
  let scale = $state(1);
  let draftSource = $state("");
  let editing = $state(false);
  let blockElement = $state<HTMLElement>();
  let editorPosition = $state({ x: 0, y: 0 });
  let editorWidthPx = $state(360);
  /// Roughly the docked editor's height (10 lines plus chrome); used only to choose a side.
  const DOCKED_EDITOR_HEIGHT_PX = 260;
  let gesture = $state<Gesture | null>(null);
  let preview = $state(emptyTypstPreview());
  let svgUrl = $state<string | null>(null);
  let compileTimer: ReturnType<typeof setTimeout> | undefined;
  let mounted = $state(false);
  let appliedCompileContext = $state("");

  // What the block actually occupies. Falls back to a line's worth while a first compile is
  // still in flight, so a brand-new block is clamped by something rather than by zero.
  const renderedHeightPt = $derived(preview.heightPt || 48);

  // The SVG carries `padPt` of slack on every side so Typst does not clip its own descenders and
  // inline math. Drawing it at its full size and pulling it back by the pad puts the content
  // itself at the block's origin, at exactly the size the PDF prints — the overflow simply
  // bleeds outside the box, which is what it does on the printed page too.
  const svgWidthPx = $derived((preview.widthPt ?? layoutWidthPt) + 2 * preview.padPt);

  onMount(() => {
    mounted = true;
    appliedCompileContext = compileContext;
    x = initialX;
    y = initialY;
    layoutWidthPt = initialLayoutWidthPt;
    scale = initialScale;
    draftSource = source;
    // Paint a cached SVG synchronously so a block that scrolls into view never flashes blank
    // for a frame before its (cache-served) compile resolves.
    if (cached?.svg) {
      preview = {
        requestedGeneration: 0,
        appliedGeneration: 0,
        svg: cached.svg,
        widthPt: cached.widthPt,
        heightPt: cached.heightPt,
        padPt: cached.padPt,
        diagnostics: cached.diagnostics,
      };
      svgUrl = URL.createObjectURL(new Blob([cached.svg], { type: "image/svg+xml" }));
    }
    requestCompile(0);
  });

  $effect(() => {
    if (!compileResult) return;
    const nextPreview = applyTypstCompileResult(preview, compileResult);
    if (nextPreview === preview) return;
    if (nextPreview.svg !== preview.svg && nextPreview.svg) {
      const nextUrl = URL.createObjectURL(
        new Blob([nextPreview.svg], { type: "image/svg+xml" }),
      );
      if (svgUrl) URL.revokeObjectURL(svgUrl);
      svgUrl = nextUrl;
    }
    preview = nextPreview;
  });

  $effect(() => {
    if (!mounted || compileContext === appliedCompileContext) return;
    appliedCompileContext = compileContext;
    requestCompile(0);
  });

  $effect(() => {
    if (gesture) return;
    x = initialX;
    y = initialY;
    const widthChanged = layoutWidthPt !== initialLayoutWidthPt;
    layoutWidthPt = initialLayoutWidthPt;
    scale = initialScale;
    if (draftSource !== source) {
      draftSource = source;
      requestCompile(0);
    } else if (mounted && widthChanged) requestCompile(0);
  });

  $effect(() => {
    if (!selected) editing = false;
  });

  // The side view takes over editing while it is open; never show both surfaces at once.
  $effect(() => {
    if (!inlineEditing) editing = false;
  });

  $effect(() => {
    onEditingChange?.(editing);
  });

  // Place once on entry, outside the block transform: scaling/reflow must not move its controls.
  $effect(() => {
    if (!editing || !blockElement) return;
    const rect = blockElement.getBoundingClientRect();
    const parent = blockElement.parentElement!.getBoundingClientRect();
    editorWidthPx = Math.min(360, window.innerWidth - 24);
    const above = rect.bottom + DOCKED_EDITOR_HEIGHT_PX > window.innerHeight && rect.top > DOCKED_EDITOR_HEIGHT_PX;
    const left = Math.max(12, Math.min(rect.left, window.innerWidth - editorWidthPx - 12));
    const top = above ? rect.top - DOCKED_EDITOR_HEIGHT_PX - 12 : rect.bottom + 12;
    editorPosition = toPageDelta(left - parent.left, top - parent.top);
  });

  onDestroy(() => {
    if (compileTimer) clearTimeout(compileTimer);
    if (svgUrl) URL.revokeObjectURL(svgUrl);
  });

  function requestCompile(delay = TYPST_IDLE_DEBOUNCE_MS) {
    if (compileTimer) clearTimeout(compileTimer);
    compileTimer = setTimeout(() => {
      preview = requestTypstCompile(preview);
      onCompile({
        source: draftSource,
        widthPt: layoutWidthPt,
        generation: preview.requestedGeneration,
      });
    }, delay);
  }

  function startGesture(event: PointerEvent, kind: Gesture["kind"]) {
    if (event.button !== 0) return;
    onSelect?.();
    const target = event.currentTarget as HTMLElement;
    target.setPointerCapture(event.pointerId);
    gesture = {
      kind,
      pointerId: event.pointerId,
      clientX: event.clientX,
      clientY: event.clientY,
      start: { x, y, layoutWidthPt, scale, rotation },
      heightPt: renderedHeightPt,
    };
    event.preventDefault();
  }

  function moveGesture(event: PointerEvent) {
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    const delta = toPageDelta(
      event.clientX - gesture.clientX,
      event.clientY - gesture.clientY,
    );

    if (gesture.kind === "move") {
      // Clamped while dragging rather than on drop, so the limit is something you feel rather
      // than something that snaps the block back after you let go.
      const held = keepOnPage(
        { x: gesture.start.x + delta.x, y: gesture.start.y + delta.y },
        { widthPt: layoutWidthPt * scale, heightPt: renderedHeightPt * scale },
        { widthPt: pageWidthPt, heightPt: pageHeightPt },
        undefined, rotation,
      );
      x = held.x;
      y = held.y;
    } else if (gesture.kind === "scale") {
      scale = scaleTypstCorner(gesture.start, gesture.heightPt, delta);
    } else {
      layoutWidthPt = Math.max(
        MIN_TYPST_WIDTH_PT,
        gesture.start.layoutWidthPt + rotatedVector(delta, -rotation).x / gesture.start.scale,
      );
      requestCompile();
    }
  }

  function finishGesture(event: PointerEvent) {
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    if (event.type === "pointerup") moveGesture(event);
    const completed = gesture;
    if (event.type !== "pointerup") {
      ({ x, y, layoutWidthPt, scale } = completed.start);
      if (completed.kind === "reflow") requestCompile(0);
      gesture = null;
      return;
    }
    gesture = null;
    if (completed.kind === "reflow") requestCompile(0);
    if (
      x !== completed.start.x ||
      y !== completed.start.y ||
      layoutWidthPt !== completed.start.layoutWidthPt ||
      scale !== completed.start.scale
    ) {
      onTransform({ x, y, layoutWidthPt, scale, rotation });
    }
  }

  function beginMove(event: PointerEvent) {
    startGesture(event, "move");
  }

  function updateSource(value: string) {
    draftSource = value;
    onSourceChange(value);
    requestCompile();
  }
</script>

<section
  bind:this={blockElement}
  class="typst-block"
  class:editing
  class:selected
  data-object-id={id}
  style:left={`${x}px`}
  style:top={`${y}px`}
  style:width={`${layoutWidthPt}px`}
  style:transform={`rotate(${rotation}deg) scale(${scale})`}
  style:--handle-scale={1 / (zoom * scale)}
  style:z-index={editing ? 1000 : zIndex}
  aria-label={$t("workspace.typstBlockReadingPosition", { "id": id, "value2": readingOrder + 1 })}
  onpointermove={moveGesture}
  onpointerup={finishGesture}
  onpointercancel={finishGesture}
  onlostpointercapture={finishGesture}
>
  <button
    type="button"
    class="preview"
    aria-pressed={selected}
    aria-label={$t("workspace.selectTypstBlockDoubleClickToEdit", { "id": id })}
    title={$t("workspace.clickToSelectDoubleClickToEdit")}
    onpointerdown={beginMove}
    ondblclick={() => (inlineEditing ? (editing = true) : onRequestEdit?.())}
    onclick={onSelect}
  >
    {#if svgUrl}
      <img
        src={svgUrl}
        alt={$t("workspace.renderedTypstContent")}
        draggable="false"
        style:width={`${svgWidthPx}px`}
        style:margin={`${-preview.padPt}px`}
      />
    {:else}
      <span class="empty">{$t("workspace.noValidPreview")}</span>
    {/if}
  </button>

  {#if preview.diagnostics.length}
    <ul class="diagnostics" aria-live="polite">
      {#each preview.diagnostics as diagnostic}
        <li>{diagnostic.severity}: {diagnostic.message}</li>
      {/each}
    </ul>
  {/if}

  {#if selected}
  <button
    type="button"
    class="handle resize"
    aria-label={$t("workspace.changeTypstLayoutWidth")}
    title={$t("workspace.dragToChangeLineWrapping")}
    onpointerdown={(event) => startGesture(event, "reflow")}></button
  >
  <button
    type="button"
    class="handle scale-corner"
    aria-label={$t("workspace.scaleTypstBlock")}
    title={$t("workspace.dragToScaleTypstBlock")}
    onpointerdown={(event) => startGesture(event, "scale")}></button
  >
  {/if}
</section>

{#if editing}
  <div class="editor-dock" data-preserve-selection
    style:left={`${editorPosition.x}px`} style:top={`${editorPosition.y}px`}
    style:width={`${editorWidthPx}px`} style:transform={`scale(${1 / zoom})`}>
    <TypstSizeControls {scale} {layoutWidthPt}
      onChange={change => onTransform({ x, y, layoutWidthPt, scale, rotation, ...change })} />
    <TypstEditor
      value={draftSource}
      {root}
      ariaLabel={$t("workspace.sourceForTypstBlock", { "id": id })}
      onChange={updateSource}
      onExit={() => { editing = false; onDeselect?.(); }}
    />
  </div>
{/if}

<style>
  /* The block's box is its Typst layout box, which runs cap height to baseline — that is what the
     export places, so it has to stay exact. It is not what the text looks like: descenders hang
     below it and accents rise above it. So the chrome is drawn on a pseudo-element pushed out by
     a gutter, and the box itself carries no border or background. Draw them on the box and the
     frame slices through the writer's own descenders. */
  .typst-block {
    --block-gutter: 6px;

    position: absolute;
    box-sizing: border-box;
    transform-origin: top left;
    /* Sits above sibling objects while being edited; the page lifts the whole object layer over
       the ink so the editing surface is never buried under strokes. */
    z-index: 0;
    color: #111;
    touch-action: none;
  }

  .typst-block::before {
    position: absolute;
    z-index: -1;
    border: 1px solid rgb(30 35 43 / 14%);
    border-radius: var(--radius);
    background: transparent;
    content: "";
    inset: calc(-1 * var(--block-gutter));
  }

  .typst-block.selected::before,
  .typst-block:focus-within::before,
  .typst-block.editing::before {
    border-color: #2f6fdb;
    box-shadow: 0 0 0 0.5px #2f6fdb;
  }

  .typst-block.editing {
    z-index: 3;
  }

  .editor-dock {
    position: absolute;
    z-index: 1001;
    padding: 5px;
    border: 1px solid var(--edge);
    border-radius: var(--radius-lg);
    background: var(--panel);
    color: var(--text);
    pointer-events: auto;
    transform-origin: top left;
    box-shadow: 0 18px 44px rgb(0 0 0 / 35%);
  }

  .preview:focus-visible,
  .handle:focus-visible {
    outline: 2px solid #4c8df0;
    outline-offset: 2px;
  }

  /* No padding: the content box is the block's true footprint, so what is on screen is the size
     the PDF prints. Padding here scaled every preview down by however wide it was. */
  .preview {
    display: block;
    box-sizing: border-box;
    width: 100%;
    padding: 0;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    text-align: left;
    cursor: move;
  }

  /* Width and a negative margin of the same size come from the pad, set inline. `height: auto`
     keeps the SVG's aspect ratio, so the content lands at 1pt to the pixel. */
  .preview img {
    display: block;
    height: auto;
    pointer-events: none;
  }

  /* Only the placeholder needs a floor; a real preview is exactly as tall as its content. */
  .empty {
    display: block;
    min-height: 3rem;
    padding: 8px;
  }

  .diagnostics {
    margin: 0;
    padding: 8px 10px 8px 27px;
    border-top: 1px solid rgb(179 58 53 / 28%);
    background: #fbeeec;
    color: #8a2b27;
    font: 11px/1.45 "Cascadia Mono", Consolas, monospace;
  }

  .handle {
    position: absolute;
    padding: 0;
    border: 1.5px solid #2f6fdb;
    background: white;
    opacity: 0;
  }

  .selected .handle,
  .editing .handle {
    opacity: 1;
  }

  .resize {
    /* Straddles the frame, which the gutter has moved outward. */
    right: calc(-14px - var(--block-gutter));
    top: 50%;
    width: 28px;
    min-width: 28px;
    height: 44px;
    min-height: 44px;
    transform: translateY(-50%) scale(var(--handle-scale));
    transform-origin: center;
    border: 0;
    background: transparent;
    cursor: ew-resize;
  }

  .resize::after {
    position: absolute;
    top: 8px;
    bottom: 8px;
    left: 11px;
    width: 4px;
    border: 1.5px solid white;
    border-radius: 3px;
    background: #2f6fdb;
    content: "";
  }
  .scale-corner {
    right: calc(-22px - var(--block-gutter));
    bottom: calc(-22px - var(--block-gutter));
    width: 44px;
    height: 44px;
    border: 0;
    background: transparent;
    transform: scale(var(--handle-scale));
    cursor: nwse-resize;
  }
  .scale-corner::after {
    position: absolute;
    inset: 16px;
    border: 2px solid #2f6fdb;
    border-radius: 2px;
    background: white;
    content: "";
  }
</style>
