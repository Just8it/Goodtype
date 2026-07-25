<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import TypstEditor from "../editor/TypstEditor.svelte";
  import {
    applyTypstCompileResult,
    emptyTypstPreview,
    requestTypstCompile,
    TYPST_IDLE_DEBOUNCE_MS,
    type TypstCompileResult,
  } from "../editor/typst";
  import type { CachedTypst } from "../editor/typstCache";

  type Transform = {
    x: number;
    y: number;
    layoutWidthPt: number;
    scale: number;
  };

  type Gesture = {
    kind: "move" | "reflow";
    pointerId: number;
    clientX: number;
    clientY: number;
    start: Transform;
  };

  let {
    id,
    source,
    initialX,
    initialY,
    initialLayoutWidthPt,
    initialScale = 1,
    compileResult = null,
    cached = null,
    root = null,
    selected = false,
    toPageDelta,
    onSelect,
    onDeselect,
    onCompile,
    onSourceChange,
    onTransform,
    onEditingChange,
    inlineEditing = true,
    onRequestEdit,
  }: {
    id: string;
    source: string;
    initialX: number;
    initialY: number;
    initialLayoutWidthPt: number;
    initialScale?: number;
    compileResult?: TypstCompileResult | null;
    cached?: CachedTypst | null;
    /** Notebook root, forwarded so the editor can ask Rust for completions. */
    root?: string | null;
    selected?: boolean;
    toPageDelta: (screenDx: number, screenDy: number) => { x: number; y: number };
    onSelect?: () => void;
    onDeselect?: () => void;
    onCompile: (request: {
      source: string;
      widthPt: number;
      generation: number;
    }) => void;
    onSourceChange: (source: string) => void;
    onTransform: (transform: Transform) => void;
    /** Lets the page lift the whole object layer while this block is being edited. */
    onEditingChange?: (editing: boolean) => void;
    /** False while the side view is open: editing happens there instead of on the page. */
    inlineEditing?: boolean;
    /** Asks the page to edit this block in the side view. */
    onRequestEdit?: () => void;
  } = $props();

  let x = $state(0);
  let y = $state(0);
  let layoutWidthPt = $state(240);
  let scale = $state(1);
  let draftSource = $state("");
  let editing = $state(false);
  let blockElement = $state<HTMLElement>();
  let editorAbove = $state(false);
  /// Roughly the docked editor's height (10 lines plus chrome); used only to choose a side.
  const DOCKED_EDITOR_HEIGHT_PX = 260;
  let gesture = $state<Gesture | null>(null);
  let preview = $state(emptyTypstPreview());
  let svgUrl = $state<string | null>(null);
  let compileTimer: ReturnType<typeof setTimeout> | undefined;

  onMount(() => {
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
    if (gesture) return;
    x = initialX;
    y = initialY;
    layoutWidthPt = initialLayoutWidthPt;
    scale = initialScale;
    if (draftSource !== source) {
      draftSource = source;
      requestCompile(0);
    }
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

  // The docked editor opens below the block, or above it when the block sits too close to the
  // bottom of the window for the panel to fit.
  $effect(() => {
    if (!editing || !blockElement) return;
    const rect = blockElement.getBoundingClientRect();
    editorAbove =
      rect.bottom + DOCKED_EDITOR_HEIGHT_PX > window.innerHeight &&
      rect.top > DOCKED_EDITOR_HEIGHT_PX;
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
    const target = event.currentTarget as HTMLElement;
    target.setPointerCapture(event.pointerId);
    gesture = {
      kind,
      pointerId: event.pointerId,
      clientX: event.clientX,
      clientY: event.clientY,
      start: { x, y, layoutWidthPt, scale },
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
      x = gesture.start.x + delta.x;
      y = gesture.start.y + delta.y;
    } else {
      layoutWidthPt = Math.max(
        72,
        gesture.start.layoutWidthPt + delta.x / gesture.start.scale,
      );
      requestCompile();
    }
  }

  function finishGesture(event: PointerEvent) {
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    const completed = gesture;
    gesture = null;
    if (completed.kind === "reflow") requestCompile(0);
    if (
      x !== completed.start.x ||
      y !== completed.start.y ||
      layoutWidthPt !== completed.start.layoutWidthPt ||
      scale !== completed.start.scale
    ) {
      onTransform({ x, y, layoutWidthPt, scale });
    }
  }

  function beginMove(event: PointerEvent) {
    onSelect?.();
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
  style:transform={`scale(${scale})`}
  aria-label={`Typst block ${id}`}
  onpointermove={moveGesture}
  onpointerup={finishGesture}
  onpointercancel={finishGesture}
>
  {#if editing}
    <!-- Floated out of flow so opening the editor never shifts the rendered block: the preview
         stays exactly where it was and the source docks beside it. -->
    <div class="editor-dock" class:above={editorAbove}>
      <TypstEditor
        value={draftSource}
        {root}
        ariaLabel={`Source for Typst block ${id}`}
        onChange={updateSource}
        onExit={() => {
          editing = false;
          onDeselect?.();
        }}
      />
    </div>
  {/if}

  <button
    type="button"
    class="preview"
    aria-label={`Select Typst block ${id}; double-click to edit`}
    title="Click to select; double-click to edit"
    onpointerdown={beginMove}
    ondblclick={() => (inlineEditing ? (editing = true) : onRequestEdit?.())}
    onclick={onSelect}
  >
    {#if svgUrl}
      <img src={svgUrl} alt="Rendered Typst content" draggable="false" />
    {:else}
      <span>No valid preview</span>
    {/if}
  </button>

  {#if preview.diagnostics.length}
    <ul class="diagnostics" aria-live="polite">
      {#each preview.diagnostics as diagnostic}
        <li>{diagnostic.severity}: {diagnostic.message}</li>
      {/each}
    </ul>
  {/if}

  <button
    type="button"
    class="handle resize"
    aria-label="Change Typst layout width"
    title="Drag to change line wrapping"
    onpointerdown={(event) => startGesture(event, "reflow")}></button
  >
</section>

<style>
  .typst-block {
    position: absolute;
    box-sizing: border-box;
    transform-origin: top left;
    /* Sits above sibling objects while being edited; the page lifts the whole object layer over
       the ink so the editing surface is never buried under strokes. */
    z-index: 0;
    border: 1px solid rgb(30 35 43 / 14%);
    border-radius: 4px;
    background: #fff;
    color: #111;
    touch-action: none;
  }

  .typst-block.selected,
  .typst-block:focus-within,
  .typst-block.editing {
    outline: 1.5px solid #2f6fdb;
    outline-offset: 0;
  }

  .typst-block.editing {
    z-index: 3;
  }

  /* Out of flow, so the rendered block never moves when the source opens. */
  .editor-dock {
    position: absolute;
    top: calc(100% + 6px);
    left: 0;
    z-index: 4;
    min-width: 100%;
  }

  .editor-dock.above {
    top: auto;
    bottom: calc(100% + 6px);
  }

  .preview:focus-visible,
  .handle:focus-visible {
    outline: 2px solid #4c8df0;
    outline-offset: 2px;
  }

  .preview {
    display: block;
    width: 100%;
    min-height: 3rem;
    padding: 8px;
    border: 0;
    border-radius: 4px;
    background: transparent;
    text-align: left;
    cursor: move;
  }

  .preview img {
    display: block;
    width: 100%;
    height: auto;
    pointer-events: none;
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
    right: -14px;
    top: 50%;
    width: 28px;
    min-width: 28px;
    height: 44px;
    min-height: 44px;
    transform: translateY(-50%);
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
</style>
