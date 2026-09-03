<script lang="ts">
  import { untrack } from "svelte";
  import type { Point } from "../geometry/coordinates";
  import {
    anchorPoints,
    canCloseSpline,
    localDelta,
    moveShape,
    moveShapeAnchor,
    removeSplineNode,
    resizeShape,
    rotateShape,
    rotationHandlePoint,
    toggleSplineClosed,
    type ShapeAnchor,
  } from "../shape/edit";
  import { describeShape, shapeBounds, shapePath } from "../shape/geometry";
  import type { ShapeEditCommit, ShapeView } from "./pageView";

  type Gesture = {
    kind: "move" | "resize" | "anchor" | "rotate";
    pointerId: number;
    clientX: number;
    clientY: number;
    start: ShapeView;
    anchor?: ShapeAnchor;
    /// Where the rotation handle was grabbed, in the shape's own coordinates.
    grabbed?: { x: number; y: number };
  };

  let {
    shape,
    selected = false,
    editing = false,
    pageWidthPt,
    pageHeightPt,
    zoom = 1,
    toPageDelta,
    onSelect,
    onChange,
    onEditingChange,
  }: {
    shape: ShapeView;
    selected?: boolean;
    editing?: boolean;
    pageWidthPt: number;
    pageHeightPt: number;
    zoom?: number;
    toPageDelta: (screenDx: number, screenDy: number) => Point;
    onSelect?: () => void;
    onChange?: (shape: ShapeView, commit?: ShapeEditCommit) => void;
    onEditingChange?: (editing: boolean) => void;
  } = $props();

  let preview = $state.raw(untrack(() => shape));
  let gesture = $state<Gesture | null>(null);
  const path = $derived(shapePath(preview.geometry));
  const bounds = $derived(shapeBounds(preview.geometry));
  const anchors = $derived(anchorPoints(preview.geometry));
  const closed = $derived(
    preview.geometry.kind === "rectangle" ||
      preview.geometry.kind === "ellipse" ||
      (preview.geometry.kind === "spline" && preview.geometry.closed),
  );
  const label = $derived(`${describeShape(preview.geometry)}, reading position ${preview.readingOrder + 1}`);
  const controlScale = $derived(1 / Math.max(zoom * preview.scale, 0.05));
  const spline = $derived(preview.geometry.kind === "spline" ? preview.geometry : null);
  const rotationHandle = $derived(rotationHandlePoint(preview.geometry, 28 * controlScale));

  $effect(() => {
    if (!gesture) preview = shape;
  });

  function begin(event: PointerEvent, kind: Gesture["kind"], anchor?: ShapeAnchor) {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    onSelect?.();
    (event.currentTarget as SVGElement).setPointerCapture(event.pointerId);
    gesture = {
      kind,
      pointerId: event.pointerId,
      clientX: event.clientX,
      clientY: event.clientY,
      start: preview,
      anchor,
      grabbed: kind === "rotate" ? rotationHandle : undefined,
    };
  }

  function update(event: PointerEvent) {
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    event.preventDefault();
    const page = toPageDelta(event.clientX - gesture.clientX, event.clientY - gesture.clientY);
    if (gesture.kind === "move") {
      preview = moveShape(gesture.start, page);
      return;
    }
    // Rotation is decided in page space, because the angle is between the pointer and the shape
    // on the page; every other gesture is a measurement inside the shape and needs the delta
    // brought back through its own rotation and scale first.
    if (gesture.kind === "rotate") {
      preview = rotateShape(gesture.start, gesture.grabbed!, page, event.shiftKey);
      return;
    }
    const local = localDelta(page, gesture.start.rotation, gesture.start.scale);
    preview = gesture.kind === "resize"
      ? resizeShape(gesture.start, local, event.shiftKey)
      : moveShapeAnchor(gesture.start, gesture.anchor!, local);
  }

  function finish(event: PointerEvent) {
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    const changed = preview !== gesture.start;
    gesture = null;
    if (changed) onChange?.(preview);
  }

  function cancel(event: PointerEvent) {
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    preview = gesture.start;
    gesture = null;
  }

  function objectKeydown(event: KeyboardEvent) {
    if (editing && spline && (event.key === "c" || event.key === "C")) {
      event.preventDefault();
      if (spline.closed || canCloseSpline(spline)) onChange?.(toggleSplineClosed(shape));
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      onEditingChange?.(!editing);
      return;
    }
    if (event.key === "Escape" && editing) {
      event.preventDefault();
      event.stopPropagation();
      onEditingChange?.(false);
      return;
    }
    const step = event.shiftKey ? 10 : 1;
    const movements: Record<string, Point> = {
      ArrowLeft: { x: -step, y: 0 },
      ArrowRight: { x: step, y: 0 },
      ArrowUp: { x: 0, y: -step },
      ArrowDown: { x: 0, y: step },
    };
    if (movements[event.key]) {
      event.preventDefault();
      onChange?.(moveShape(shape, movements[event.key]), "settled");
    } else if (event.key === "+" || event.key === "=") {
      event.preventDefault();
      onChange?.(resizeShape(shape, { x: 8, y: 8 }, true), "settled");
    } else if (event.key === "-") {
      event.preventDefault();
      onChange?.(resizeShape(shape, { x: -8, y: -8 }, true), "settled");
    } else if (event.key === "[" || event.key === "]") {
      event.preventDefault();
      onChange?.(
        { ...shape, rotation: shape.rotation + (event.key === "[" ? -15 : 15) },
        "settled",
      );
    }
  }

  function anchorKeydown(event: KeyboardEvent, anchor: ShapeAnchor) {
    if (
      (event.key === "Delete" || event.key === "Backspace") &&
      typeof anchor !== "string" &&
      !("handle" in anchor)
    ) {
      event.preventDefault();
      event.stopPropagation();
      onChange?.(removeSplineNode(shape, anchor.node));
      return;
    }
    const step = event.shiftKey ? 10 : 1;
    const movements: Record<string, Point> = {
      ArrowLeft: { x: -step, y: 0 },
      ArrowRight: { x: step, y: 0 },
      ArrowUp: { x: 0, y: -step },
      ArrowDown: { x: 0, y: step },
    };
    if (!movements[event.key]) return;
    event.preventDefault();
    event.stopPropagation();
    onChange?.(moveShapeAnchor(shape, anchor, movements[event.key]), "settled");
  }

  function anchorName(anchor: ShapeAnchor): string {
    if (typeof anchor === "string") return anchor.replaceAll("-", " ");
    return "handle" in anchor
      ? `${anchor.handle} curve handle for anchor ${anchor.node + 1}`
      : `anchor ${anchor.node + 1}`;
  }

  function nodePoint(anchor: Extract<ShapeAnchor, { node: number }>): Point | null {
    if (preview.geometry.kind !== "spline") return null;
    return preview.geometry.nodes[anchor.node]?.point ?? null;
  }
</script>

<svg
  class="shape-object"
  class:selected
  class:editing
  style:z-index={preview.zIndex}
  viewBox={`0 0 ${pageWidthPt} ${pageHeightPt}`}
  width={pageWidthPt}
  height={pageHeightPt}
  aria-label={label}
>
  <g
    data-object-id={preview.id}
    transform={`translate(${preview.x} ${preview.y}) rotate(${preview.rotation}) scale(${preview.scale})`}
  >
    <path
      class="shape-mark"
      d={path}
      fill={closed ? (preview.style.fillColor ?? "none") : "none"}
      stroke={preview.style.strokeColor}
      stroke-width={preview.style.strokeWidthPt}
      stroke-linecap="round"
      stroke-linejoin="round"
      opacity={preview.style.opacity}
    ></path>
    <path
      class:closed
      class="shape-hit"
      d={path}
      fill={closed ? "transparent" : "none"}
      stroke="transparent"
      stroke-width={Math.max(preview.style.strokeWidthPt, 44 * controlScale)}
      role="button"
      tabindex={onSelect ? 0 : -1}
      aria-pressed={selected}
      aria-label={`${label}. Press Enter to edit anchor points${spline ? ", then C to open or close the curve and Delete to remove an anchor" : ""}.`}
      onpointerdown={(event) => begin(event, "move")}
      onpointermove={update}
      onpointerup={finish}
      onpointercancel={cancel}
      ondblclick={(event) => {
        event.stopPropagation();
        onEditingChange?.(true);
      }}
      onkeydown={objectKeydown}
    ></path>

    {#if selected}
      <rect
        class="selection-box"
        x={bounds.left}
        y={bounds.top}
        width={Math.max(bounds.right - bounds.left, controlScale)}
        height={Math.max(bounds.bottom - bounds.top, controlScale)}
        stroke-width={1.5 * controlScale}
        stroke-dasharray={`${4 * controlScale} ${3 * controlScale}`}
      ></rect>
      {#if !editing}
        <line
          class="handle-line"
          x1={(bounds.left + bounds.right) / 2}
          y1={bounds.top}
          x2={rotationHandle.x}
          y2={rotationHandle.y}
          stroke-width={1.2 * controlScale}
        ></line>
        <circle
          class="control-hit rotate"
          cx={rotationHandle.x}
          cy={rotationHandle.y}
          r={22 * controlScale}
          role="button"
          tabindex="0"
          aria-label={`Rotate ${describeShape(preview.geometry)}, currently ${Math.round(preview.rotation)} degrees. Hold Shift to snap to 15 degree steps, or use the bracket keys.`}
          onpointerdown={(event) => begin(event, "rotate")}
          onpointermove={update}
          onpointerup={finish}
          onpointercancel={cancel}
          onkeydown={objectKeydown}
        ></circle>
        <circle class="control" cx={rotationHandle.x} cy={rotationHandle.y} r={4.5 * controlScale}></circle>
        <circle
          class="control-hit"
          cx={bounds.right}
          cy={bounds.bottom}
          r={22 * controlScale}
          role="button"
          tabindex="0"
          aria-label={`Resize ${describeShape(preview.geometry)}. Hold Shift to keep proportions.`}
          onpointerdown={(event) => begin(event, "resize")}
          onpointermove={update}
          onpointerup={finish}
          onpointercancel={cancel}
          onkeydown={objectKeydown}
        ></circle>
        <circle class="control" cx={bounds.right} cy={bounds.bottom} r={5 * controlScale}></circle>
      {:else}
        {#each anchors as item, index (`${anchorName(item.anchor)}-${index}`)}
          {#if typeof item.anchor !== "string" && "handle" in item.anchor}
            {@const knot = nodePoint(item.anchor)}
            {#if knot}
              <line
                class="handle-line"
                x1={knot.x}
                y1={knot.y}
                x2={item.point.x}
                y2={item.point.y}
                stroke-width={1.2 * controlScale}
              ></line>
            {/if}
          {/if}
        {/each}
        {#each anchors as item, index (`${anchorName(item.anchor)}-${index}`)}
          {@const handle = typeof item.anchor !== "string" && "handle" in item.anchor}
          <circle
            class="control-hit"
            cx={item.point.x}
            cy={item.point.y}
            r={22 * controlScale}
            role="button"
            tabindex="0"
            aria-label={`Move ${anchorName(item.anchor)}`}
            onpointerdown={(event) => begin(event, "anchor", item.anchor)}
            onpointermove={update}
            onpointerup={finish}
            onpointercancel={cancel}
            onkeydown={(event) => anchorKeydown(event, item.anchor)}
          ></circle>
          <circle
            class:handle
            class="control"
            cx={item.point.x}
            cy={item.point.y}
            r={(handle ? 3.5 : 5) * controlScale}
          ></circle>
        {/each}
      {/if}
    {/if}
  </g>
</svg>

<style>
  .shape-object { position: absolute; inset: 0; display: block; overflow: visible; pointer-events: none; touch-action: none; user-select: none; }
  .shape-mark { pointer-events: none; }
  .shape-hit { pointer-events: stroke; cursor: move; outline: none; }
  .shape-hit.closed { pointer-events: all; }
  .shape-hit:focus-visible { stroke: var(--blueprint-light); opacity: 0.32; }
  .selection-box { fill: none; stroke: var(--blueprint); pointer-events: none; }
  .control-hit { fill: transparent; pointer-events: all; cursor: nwse-resize; outline: none; }
  .control-hit.rotate { cursor: grab; }
  .control-hit.rotate:active { cursor: grabbing; }
  .editing .control-hit { cursor: move; }
  .control-hit:focus-visible { fill: rgb(76 141 240 / 24%); }
  .control { fill: var(--paper); stroke: var(--blueprint); stroke-width: 1.5px; pointer-events: none; }
  .control.handle { fill: var(--blueprint); }
  .handle-line { stroke: var(--blueprint); opacity: 0.72; pointer-events: none; }
</style>
