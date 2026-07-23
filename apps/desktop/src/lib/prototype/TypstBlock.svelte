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
    selected = false,
    toPageDelta,
    onSelect,
    onDeselect,
    onCompile,
    onSourceChange,
    onTransform,
  }: {
    id: string;
    source: string;
    initialX: number;
    initialY: number;
    initialLayoutWidthPt: number;
    initialScale?: number;
    compileResult?: TypstCompileResult | null;
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
  } = $props();

  let x = $state(0);
  let y = $state(0);
  let layoutWidthPt = $state(240);
  let scale = $state(1);
  let draftSource = $state("");
  let editing = $state(false);
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
    <TypstEditor
      value={draftSource}
      ariaLabel={`Source for Typst block ${id}`}
      onChange={updateSource}
      onExit={() => {
        editing = false;
        onDeselect?.();
      }}
    />
  {/if}

  <button
    type="button"
    class="preview"
    aria-label={`Select Typst block ${id}; double-click to edit`}
    title="Click to select; double-click to edit"
    onpointerdown={beginMove}
    ondblclick={() => (editing = true)}
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
    aria-label="Resize Typst text box width"
    title="Drag to change text box width"
    onpointerdown={(event) => startGesture(event, "reflow")}></button
  >
</section>

<style>
  .typst-block {
    position: absolute;
    box-sizing: border-box;
    transform-origin: top left;
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
    bottom: -14px;
    width: 28px;
    min-width: 28px;
    height: 28px;
    min-height: 28px;
    border: 0;
    background: transparent;
    cursor: ew-resize;
  }

  .resize::after {
    position: absolute;
    inset: 8px;
    border: 1.5px solid white;
    background: #2f6fdb;
    content: "";
  }
</style>
