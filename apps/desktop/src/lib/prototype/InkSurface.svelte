<script lang="ts">
  import type { Point } from "../geometry/coordinates";
  import { boundedRasterScale, screenToPage } from "../geometry/coordinates";
  import type { ShapeStyle, Stroke, StrokePoint } from "../model";
  import {
    maximumSampleGap,
    type StrokePerformance,
  } from "../ink/metrics";
  import {
    DEFAULT_PRESSURE_CALIBRATION,
    DEFAULT_POINTER_MAPPING,
    normalizePointerSample,
    pointerRole,
    quantizePoints,
    smoothPoints,
    type InkTool,
    type PointerMapping,
    type PressureCalibration,
  } from "../ink/pipeline";
  import { outlinePoints } from "../ink/outline";
  import { paintStrata } from "../ink/paint";
  import { straightenStroke } from "../ink/straighten";
  import {
    shapeFromDrag,
    shapePath,
    splineFromPoints,
    type ShapeDraft,
    type ShapeKind,
  } from "../shape/geometry";
  import { recognizeHeldShape } from "../shape/recognize";
  import {
    eraseStrokeAt,
    hitStroke,
    moveSelected,
    scaleSelected,
    selectStrokesInLasso,
    selectionBounds,
    transformedPoint,
  } from "../ink/selection";

  type Props = {
    strokes?: Stroke[];
    newStrokeZIndex?: number;
    objectZIndices?: number[];
    selectedStrokeIds?: string[];
    pageWidthPt: number;
    pageHeightPt: number;
    zoom?: number;
    tool?: InkTool;
    color?: string;
    widthPt?: number;
    /** Whether the active nib varies its width with stylus pressure. */
    pressure?: boolean;
    /** Fraction of the stroke's length over which each end tapers to a point. */
    taper?: number;
    /** Ink opacity for the active tool, 0–1. */
    opacity?: number;
    /** Snap an almost-straight stroke to the line it was aiming for, on release. */
    straighten?: boolean;
    shapeKind?: ShapeKind;
    shapeStyle?: ShapeStyle;
    shapeConstrain?: boolean;
    drawAndHoldShapes?: boolean;
    eraseRadiusPt?: number;
    calibration?: PressureCalibration;
    pointerMapping?: PointerMapping;
    onStrokeFinalized?: (stroke: Stroke) => void;
    onShapeFinalized?: (shape: ShapeDraft) => void;
    onInkShapeRecognized?: (stroke: Stroke, shape: ShapeDraft) => void;
    onStrokesChange?: (strokes: Stroke[]) => void;
    onSelectionChange?: (strokeIds: string[]) => void;
    onStrokeMetrics?: (metrics: StrokePerformance) => void;
  };

  let {
    strokes = [],
    newStrokeZIndex = 1_000_001,
    objectZIndices = [],
    selectedStrokeIds = [],
    pageWidthPt,
    pageHeightPt,
    zoom = 1,
    tool = "select",
    color = "#16212b",
    widthPt = 2,
    pressure = true,
    taper = 0,
    opacity = 1,
    straighten = false,
    shapeKind = "line",
    shapeStyle = {
      strokeColor: "#16212b",
      strokeWidthPt: 2,
      fillColor: null,
      opacity: 1,
    },
    shapeConstrain = false,
    drawAndHoldShapes = true,
    eraseRadiusPt = 8,
    calibration = DEFAULT_PRESSURE_CALIBRATION,
    pointerMapping = DEFAULT_POINTER_MAPPING,
    onStrokeFinalized,
    onShapeFinalized,
    onInkShapeRecognized,
    onStrokesChange,
    onSelectionChange,
    onStrokeMetrics,
  }: Props = $props();

  let surface = $state<HTMLDivElement>();
  let immediateCanvas = $state<HTMLCanvasElement>();
  let status = $state("No ink selected");
  let strokeCount = $state(0);
  let eraserCursor = $state<Point | null>(null);
  let eraserPressed = $state(false);
  // `$state.raw` rather than `$state`: the array is always replaced, never mutated in place, so
  // deep-proxying five thousand strokes and their samples would be pure cost.
  let localStrokes = $state.raw<Stroke[]>([]);
  let localSelection = $state.raw<string[]>([]);

  type Gesture =
    | {
        kind: "draw";
        pointerId: number;
        points: StrokePoint[];
        strokeTool: "pen" | "highlighter";
        startedAt: number;
        feedbackMs: number | null;
        holdAnchor: Point;
        shapePreview: ShapeDraft | null;
      }
    | {
        kind: "shape";
        pointerId: number;
        start: Point;
        points: StrokePoint[];
        constrain: boolean;
        preview: ShapeDraft | null;
      }
    | { kind: "lasso"; pointerId: number; points: Point[] }
    | {
        kind: "erase";
        pointerId: number;
        initial: Stroke[];
        initialSelection: string[];
      }
    | {
        kind: "move";
        pointerId: number;
        start: Point;
        initial: Stroke[];
      }
    | {
        kind: "scale";
        pointerId: number;
        anchor: Point;
        startDistance: number;
        initial: Stroke[];
      };

  let gesture: Gesture | null = null;
  let holdTimer: ReturnType<typeof setTimeout> | undefined;
  let splineFrame: number | undefined;
  const MAX_IMMEDIATE_CANVAS_PIXELS = 6_000_000;
  const SHAPE_HOLD_MS = 500;
  const SHAPE_HOLD_DRIFT_PT = 4;

  $effect(() => {
    // Reads `strokes`, never `localStrokes`: now that the local copy is reactive, reading it here
    // would make this effect depend on its own output, and every freshly drawn stroke would be
    // wiped by a re-run before the parent had committed it back.
    localStrokes = strokes.slice();
    strokeCount = strokes.length;
    localSelection = selectedStrokeIds.slice();
    pageWidthPt;
    pageHeightPt;
    const canvas = immediateCanvas;
    if (!canvas) return;
    sizeCanvas(canvas);
    redrawImmediate();
  });

  /**
   * Committed ink as SVG rather than canvas. Zoom becomes a transform the browser
   * composites, so it stays crisp without JavaScript re-rasterising every stroke — which is what
   * made zooming lag. Only filled outlines can be merged like this; stroked polylines could not.
   */
  const committedStrata = $derived(
    paintStrata(localStrokes, objectZIndices, 0.5 / zoom),
  );
  const selectedBounds = $derived(selectionBounds(localStrokes, localSelection));
  const eraserRadiusOnPage = $derived(eraseRadiusPt / zoom);

  function sizeCanvas(canvas: HTMLCanvasElement): void {
    const width = Math.max(pageWidthPt, 1);
    const height = Math.max(pageHeightPt, 1);
    const density = window.devicePixelRatio || 1;
    // The parent page already applies zoom as a GPU-composited CSS transform. This transient
    // canvas only needs device resolution; scaling it with zoom caused a large bitmap allocation
    // on every pinch update, even on empty pages.
    const renderScale = boundedRasterScale(
      { width, height },
      density,
      MAX_IMMEDIATE_CANVAS_PIXELS,
    );
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
    const pixelWidth = Math.ceil(width * renderScale);
    const pixelHeight = Math.ceil(height * renderScale);
    if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) {
      canvas.width = pixelWidth;
      canvas.height = pixelHeight;
    }
    canvas.getContext("2d")?.setTransform(renderScale, 0, 0, renderScale, 0, 0);
  }

  function pagePoint(event: PointerEvent): Point {
    if (!surface) return { x: 0, y: 0 };
    const bounds = surface.getBoundingClientRect();
    return screenToPage(
      { x: event.clientX, y: event.clientY },
      { left: bounds.left, top: bounds.top },
      { pageOriginX: 0, pageOriginY: 0, zoom },
    );
  }

  function sample(event: PointerEvent): StrokePoint {
    if (!surface) {
      return { x: 0, y: 0, pressure: 0, timeMs: 0, tiltX: 0, tiltY: 0 };
    }
    const bounds = surface.getBoundingClientRect();
    return normalizePointerSample(
      event,
      { left: bounds.left, top: bounds.top },
      { pageOriginX: 0, pageOriginY: 0, zoom },
      { width: pageWidthPt, height: pageHeightPt },
      calibration,
    );
  }

  function pointerDown(event: PointerEvent): void {
    updateEraserCursor(event);
    const role = pointerRole(event, tool, pointerMapping);
    if (role === "ignore" || gesture) return;
    event.preventDefault();

    surface?.focus({ preventScroll: true });
    surface?.setPointerCapture(event.pointerId);
    if (role === "erase") {
      eraserPressed = true;
      gesture = {
        kind: "erase",
        pointerId: event.pointerId,
        initial: localStrokes,
        initialSelection: localSelection,
      };
      eraseAt(pagePoint(event));
      return;
    }
    if (role === "draw") {
      const drawGesture: Extract<Gesture, { kind: "draw" }> = {
        kind: "draw",
        pointerId: event.pointerId,
        points: [sample(event)],
        strokeTool: tool === "highlighter" ? "highlighter" : "pen",
        startedAt: performance.now(),
        feedbackMs: null,
        holdAnchor: pagePoint(event),
        shapePreview: null,
      };
      gesture = drawGesture;
      armHoldPreview(drawGesture);
      redrawImmediate();
      requestAnimationFrame(() => {
        drawGesture.feedbackMs = performance.now() - drawGesture.startedAt;
      });
      return;
    }
    if (role === "shape") {
      const point = sample(event);
      gesture = {
        kind: "shape",
        pointerId: event.pointerId,
        start: point,
        points: [point],
        constrain: shapeConstrain || event.shiftKey,
        preview: null,
      };
      redrawImmediate();
      return;
    }
    if (role === "lasso") {
      gesture = { kind: "lasso", pointerId: event.pointerId, points: [pagePoint(event)] };
      redrawImmediate();
      return;
    }

    beginSelectionGesture(event);
  }

  function beginSelectionGesture(event: PointerEvent): void {
    const point = pagePoint(event);
    const bounds = selectionBounds(localStrokes, localSelection);
    const handleRadius = 10 / zoom;
    if (
      bounds &&
      Math.hypot(point.x - bounds.right, point.y - bounds.bottom) <= handleRadius
    ) {
      gesture = {
        kind: "scale",
        pointerId: event.pointerId,
        anchor: { x: bounds.left, y: bounds.top },
        startDistance: Math.max(
          Math.hypot(bounds.right - bounds.left, bounds.bottom - bounds.top),
          1,
        ),
        initial: localStrokes,
      };
      return;
    }

    const hit = hitStroke(localStrokes, point, 6 / zoom);
    const insideSelection =
      bounds &&
      point.x >= bounds.left &&
      point.x <= bounds.right &&
      point.y >= bounds.top &&
      point.y <= bounds.bottom;
    if (!hit && !insideSelection) {
      setSelection([]);
      return;
    }
    if (hit && !localSelection.includes(hit.id)) setSelection([hit.id]);
    gesture = {
      kind: "move",
      pointerId: event.pointerId,
      start: point,
      initial: localStrokes,
    };
  }

  function pointerMove(event: PointerEvent): void {
    updateEraserCursor(event);
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    event.preventDefault();
    if (gesture.kind === "draw") {
      const events = event.getCoalescedEvents?.() ?? [];
      for (const next of events.length > 0 ? events : [event]) {
        gesture.points.push(sample(next));
      }
      updateHoldPreview(gesture);
    } else if (gesture.kind === "shape") {
      const events = event.getCoalescedEvents?.() ?? [];
      for (const next of events.length > 0 ? events : [event]) {
        gesture.points.push(sample(next));
      }
      gesture.constrain = shapeConstrain || event.shiftKey;
      // A line, rectangle or ellipse is two points of arithmetic and can follow the pointer
      // exactly. A spline is a curve fit over every sample so far, which is far too much to
      // repeat per pointer event on a pen reporting hundreds a second — so it re-fits once a
      // frame, and is fitted exactly once more on release, which is the one that gets stored.
      if (shapeKind === "spline") requestSplinePreview(gesture);
      else gesture.preview = shapeDraft(gesture);
    } else if (gesture.kind === "lasso") {
      gesture.points.push(pagePoint(event));
    } else if (gesture.kind === "erase") {
      eraseAt(pagePoint(event));
    } else if (gesture.kind === "move") {
      const point = pagePoint(event);
      localStrokes = moveSelected(gesture.initial, localSelection, {
        x: point.x - gesture.start.x,
        y: point.y - gesture.start.y,
      });
    } else {
      const point = pagePoint(event);
      const distance = Math.hypot(point.x - gesture.anchor.x, point.y - gesture.anchor.y);
      localStrokes = scaleSelected(
        gesture.initial,
        localSelection,
        gesture.anchor,
        distance / gesture.startDistance,
      );
    }
    redrawImmediate();
  }

  function pointerUp(event: PointerEvent): void {
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    event.preventDefault();
    if (gesture.kind === "draw") {
      const commitStartedAt = performance.now();
      // Straighten before quantising, so the snapped angle is rounded once rather than twice, and
      // store the result: what is committed is the straightened stroke, not a flag to reinterpret.
      const smoothed = smoothPoints(
        gesture.points.concat(sample(event)),
        calibration.smoothing,
      );
      const heldShape = gesture.shapePreview;
      const points = quantizePoints(
        heldShape ? smoothed : straighten ? straightenStroke(smoothed) : smoothed,
      );
      const stroke = completedStroke(gesture.strokeTool, points);
      if (heldShape && onInkShapeRecognized) {
        status = `Converted held ink to ${heldShape.geometry.kind}`;
        onInkShapeRecognized(stroke, heldShape);
      } else {
        localStrokes = [...localStrokes, stroke];
        strokeCount = localStrokes.length;
        status = `Added ${stroke.tool} stroke`;
        onStrokeFinalized?.(stroke);
      }
      onStrokeMetrics?.({
        sampleCount: points.length,
        maxSampleGapMs: maximumSampleGap(points),
        activeFeedbackMs:
          gesture.feedbackMs ?? performance.now() - gesture.startedAt,
        commitMs: performance.now() - commitStartedAt,
      });
    } else if (gesture.kind === "shape") {
      const finalGesture = {
        ...gesture,
        points: [...gesture.points, sample(event)],
        constrain: shapeConstrain || event.shiftKey,
      };
      const draft = shapeDraft(finalGesture);
      if (draft) {
        status = `Added ${draft.geometry.kind}`;
        onShapeFinalized?.(draft);
      } else {
        status = "Shape was too small to add";
      }
    } else if (gesture.kind === "lasso") {
      setSelection(selectStrokesInLasso(localStrokes, gesture.points.concat(pagePoint(event))));
    } else if (gesture.kind === "erase") {
      if (localStrokes !== gesture.initial) {
        onStrokesChange?.(localStrokes);
        onSelectionChange?.(localSelection);
      }
    } else {
      onStrokesChange?.(localStrokes);
      status =
        gesture.kind === "move"
          ? `Moved ${localSelection.length} selected stroke${localSelection.length === 1 ? "" : "s"}`
          : `Scaled ${localSelection.length} selected stroke${localSelection.length === 1 ? "" : "s"}`;
    }
    finishPointer(event.pointerId);
  }

  function pointerCancel(event: PointerEvent): void {
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    if (
      gesture.kind === "move" ||
      gesture.kind === "scale" ||
      gesture.kind === "erase"
    ) {
      localStrokes = gesture.initial;
      if (gesture.kind === "erase") localSelection = gesture.initialSelection;
    }
    finishPointer(event.pointerId);
  }

  function pointerLeave(): void {
    eraserCursor = null;
  }

  function updateEraserCursor(event: PointerEvent): void {
    eraserCursor =
      tool === "eraser" && pointerRole(event, tool, pointerMapping) === "erase"
        ? pagePoint(event)
        : null;
  }

  function finishPointer(pointerId: number): void {
    clearHoldTimer();
    clearSplineFrame();
    gesture = null;
    eraserPressed = false;
    if (surface?.hasPointerCapture(pointerId)) surface.releasePointerCapture(pointerId);
    redrawImmediate();
  }

  function eraseAt(point: Point): void {
    const next = eraseStrokeAt(localStrokes, point, eraserRadiusOnPage);
    if (next === localStrokes) return;
    localStrokes = next;
    strokeCount = localStrokes.length;
    localSelection = localSelection.filter((id) =>
      localStrokes.some((stroke) => stroke.id === id),
    );
    redrawImmediate();
    status = "Erased one complete stroke";
  }

  function setSelection(ids: string[]): void {
    localSelection = ids;
    status =
      ids.length === 0
        ? "No ink selected"
        : `${ids.length} stroke${ids.length === 1 ? "" : "s"} selected`;
    onSelectionChange?.(ids);
    redrawImmediate();
  }

  function keyDown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      setSelection([]);
      return;
    }
    if (localSelection.length === 0) return;
    if (event.key === "Delete" || event.key === "Backspace") {
      event.preventDefault();
      const selected = new Set(localSelection);
      localStrokes = localStrokes.filter((stroke) => !selected.has(stroke.id));
      strokeCount = localStrokes.length;
      onStrokesChange?.(localStrokes);
      setSelection([]);
      status = "Deleted selected ink";
      return;
    }
    const amount = event.shiftKey ? 10 : 1;
    const deltas: Record<string, Point> = {
      ArrowLeft: { x: -amount, y: 0 },
      ArrowRight: { x: amount, y: 0 },
      ArrowUp: { x: 0, y: -amount },
      ArrowDown: { x: 0, y: amount },
    };
    if (deltas[event.key]) {
      event.preventDefault();
      localStrokes = moveSelected(localStrokes, localSelection, deltas[event.key]);
      onStrokesChange?.(localStrokes);
      status = "Moved selected ink";
      redrawImmediate();
      return;
    }
    if (event.key === "+" || event.key === "=" || event.key === "-") {
      event.preventDefault();
      const bounds = selectionBounds(localStrokes, localSelection);
      if (!bounds) return;
      localStrokes = scaleSelected(
        localStrokes,
        localSelection,
        { x: bounds.left, y: bounds.top },
        event.key === "-" ? 0.9 : 1.1,
      );
      onStrokesChange?.(localStrokes);
      status = "Scaled selected ink";
      redrawImmediate();
    }
  }

  function redrawImmediate(): void {
    const context = canvasContext(immediateCanvas);
    if (!context || !immediateCanvas) return;
    context.clearRect(0, 0, pageWidthPt, pageHeightPt);
    if (gesture?.kind === "draw") {
      if (gesture.shapePreview) drawShapePreview(context, gesture.shapePreview);
      else
        drawPoints(
          context,
          gesture.points,
          color,
          liveWidthPt(gesture.strokeTool),
          pressure,
          taper,
          opacity,
        );
    } else if (gesture?.kind === "shape" && gesture.preview) {
      drawShapePreview(context, gesture.preview);
    } else if (gesture?.kind === "lasso") {
      context.save();
      context.strokeStyle = "#206acb";
      context.lineWidth = 1;
      context.setLineDash([5, 4]);
      path(context, gesture.points.map(toCanvasPoint));
      context.stroke();
      context.restore();
    }
  }

  /**
   * The stroke still under the pen. It lives on canvas because immediate feedback matters more
   * than crispness for the few milliseconds before release; on release it joins the SVG layer.
   * Same silhouette either way, so nothing shifts at the handover.
   */
  function drawPoints(
    context: CanvasRenderingContext2D,
    points: StrokePoint[],
    strokeColor: string,
    baseWidthPt: number,
    usePressure: boolean,
    strokeTaper: number,
    strokeOpacity: number,
  ): void {
    if (points.length === 0) return;
    const polygon = outlinePoints(points, {
      // Below roughly half a device pixel the fill has nothing to cover, so hairline ink stays
      // visible when zoomed far out instead of dropping out entirely.
      widthPt: Math.max(baseWidthPt, 0.5 / zoom),
      pressure: usePressure,
      taper: strokeTaper,
    });
    if (polygon.length === 0) return;
    context.save();
    context.fillStyle = strokeColor;
    context.globalAlpha = Math.min(1, Math.max(0, strokeOpacity));
    path(context, polygon);
    context.closePath();
    context.fill();
    context.restore();
  }

  function path(context: CanvasRenderingContext2D, points: Point[]): void {
    if (points.length === 0) return;
    context.beginPath();
    context.moveTo(points[0].x, points[0].y);
    for (const point of points.slice(1)) context.lineTo(point.x, point.y);
  }

  /** The highlighter sweeps wider than the width chip suggests; a pen writes at face value. */
  function liveWidthPt(strokeTool: "pen" | "highlighter"): number {
    return strokeTool === "highlighter" ? widthPt * 3 : widthPt;
  }

  function completedStroke(
    strokeTool: "pen" | "highlighter",
    points: StrokePoint[],
  ): Stroke {
    return {
      id: crypto.randomUUID(),
      zIndex: newStrokeZIndex,
      tool: strokeTool,
      color,
      widthPt: liveWidthPt(strokeTool),
      pressure,
      taper,
      opacity,
      groupId: null,
      points,
      transform: {
        translateX: 0,
        translateY: 0,
        scaleX: 1,
        scaleY: 1,
        rotation: 0,
      },
    };
  }

  function shapeDraft(
    active: Extract<Gesture, { kind: "shape" }>,
  ): ShapeDraft | null {
    if (shapeKind === "spline") {
      return splineFromPoints(active.points, shapeStyle, 1.5 / Math.max(zoom, 0.1));
    }
    return shapeFromDrag(
      shapeKind,
      active.start,
      active.points.at(-1) ?? active.start,
      shapeStyle,
      active.constrain,
    );
  }

  function armHoldPreview(active: Extract<Gesture, { kind: "draw" }>): void {
    clearHoldTimer();
    if (!drawAndHoldShapes || active.strokeTool !== "pen") return;
    holdTimer = setTimeout(() => {
      if (gesture !== active) return;
      active.shapePreview = recognizeHeldShape(
        smoothPoints(active.points, calibration.smoothing),
        shapeStyle,
      )?.draft ?? null;
      if (active.shapePreview) {
        status = `Release to use ${active.shapePreview.geometry.kind}; continue drawing to keep ink`;
        redrawImmediate();
      }
    }, SHAPE_HOLD_MS);
  }

  function updateHoldPreview(active: Extract<Gesture, { kind: "draw" }>): void {
    const point = active.points.at(-1);
    if (!point) return;
    if (Math.hypot(point.x - active.holdAnchor.x, point.y - active.holdAnchor.y) <= SHAPE_HOLD_DRIFT_PT / zoom) return;
    active.holdAnchor = point;
    active.shapePreview = null;
    armHoldPreview(active);
  }

  function clearHoldTimer(): void {
    if (holdTimer) clearTimeout(holdTimer);
    holdTimer = undefined;
  }

  function requestSplinePreview(active: Extract<Gesture, { kind: "shape" }>): void {
    if (splineFrame !== undefined) return;
    splineFrame = requestAnimationFrame(() => {
      splineFrame = undefined;
      if (gesture !== active) return;
      active.preview = shapeDraft(active);
      redrawImmediate();
    });
  }

  function clearSplineFrame(): void {
    if (splineFrame !== undefined) cancelAnimationFrame(splineFrame);
    splineFrame = undefined;
  }

  function drawShapePreview(
    context: CanvasRenderingContext2D,
    draft: ShapeDraft,
  ): void {
    context.save();
    context.translate(draft.x, draft.y);
    context.rotate((draft.rotation * Math.PI) / 180);
    context.strokeStyle = draft.style.strokeColor;
    context.lineWidth = Math.max(draft.style.strokeWidthPt, 0.5 / zoom);
    context.lineCap = "round";
    context.lineJoin = "round";
    context.globalAlpha = draft.style.opacity;
    const path = new Path2D(shapePath(draft.geometry));
    const closed =
      draft.geometry.kind === "rectangle" ||
      draft.geometry.kind === "ellipse" ||
      (draft.geometry.kind === "spline" && draft.geometry.closed);
    if (closed && draft.style.fillColor) {
      context.fillStyle = draft.style.fillColor;
      context.fill(path);
    }
    context.stroke(path);
    context.restore();
  }

  function toCanvasPoint(point: Point): Point {
    return point;
  }

  function canvasContext(
    canvas: HTMLCanvasElement | undefined,
  ): CanvasRenderingContext2D | null {
    return canvas?.getContext("2d") ?? null;
  }
</script>

{#each committedStrata as stratum (stratum.key)}
  <svg
    class="committed"
    style:z-index={stratum.zIndex}
    viewBox={`0 0 ${pageWidthPt} ${pageHeightPt}`}
    width={pageWidthPt}
    height={pageHeightPt}
    aria-hidden="true"
  >
    {#each stratum.paths as painted (painted.key)}
      <path d={painted.d} fill={painted.color} fill-opacity={painted.opacity} fill-rule="nonzero" />
    {/each}
  </svg>
{/each}
<div
  bind:this={surface}
  class="ink-surface"
  class:eraser-active={tool === "eraser"}
  class:shape-active={tool === "shape"}
  style:width={`${pageWidthPt}px`}
  style:height={`${pageHeightPt}px`}
  role="button"
  aria-label={`Ink canvas. ${strokeCount} strokes. ${status}. Arrow keys move selected ink; plus and minus scale it; Delete removes it.`}
  tabindex="0"
  onpointerdown={pointerDown}
  onpointermove={pointerMove}
  onpointerleave={pointerLeave}
  onpointerup={pointerUp}
  onpointercancel={pointerCancel}
  onkeydown={keyDown}
>
  <canvas bind:this={immediateCanvas} aria-hidden="true"></canvas>
  {#if selectedBounds}
    <svg
      class="selection-overlay"
      viewBox={`0 0 ${pageWidthPt} ${pageHeightPt}`}
      width={pageWidthPt}
      height={pageHeightPt}
      aria-hidden="true"
    >
      <rect
        x={selectedBounds.left}
        y={selectedBounds.top}
        width={selectedBounds.right - selectedBounds.left}
        height={selectedBounds.bottom - selectedBounds.top}
        fill="none"
        stroke-width={1.5 / zoom}
        stroke-dasharray={`${4 / zoom} ${3 / zoom}`}
      />
      <circle
        cx={selectedBounds.right}
        cy={selectedBounds.bottom}
        r={6 / zoom}
        stroke-width={1.5 / zoom}
      />
    </svg>
  {/if}
  {#if eraserCursor}
    <svg
      class="eraser-cursor"
      class:pressed={eraserPressed}
      viewBox={`0 0 ${pageWidthPt} ${pageHeightPt}`}
      width={pageWidthPt}
      height={pageHeightPt}
      aria-hidden="true"
    >
      <circle
        class="eraser-cursor-halo"
        cx={eraserCursor.x}
        cy={eraserCursor.y}
        r={eraserRadiusOnPage}
        stroke-width={3.5 / zoom}
      />
      <circle
        class="eraser-cursor-ring"
        cx={eraserCursor.x}
        cy={eraserCursor.y}
        r={eraserRadiusOnPage}
        stroke-width={1.5 / zoom}
      />
    </svg>
  {/if}
</div>

<p class="status" aria-live="polite">{status}</p>

<style>
  .ink-surface {
    position: absolute;
    inset: 0;
    z-index: 2147483646;
    overflow: hidden;
    background: transparent;
    outline: 0;
    touch-action: none;
    user-select: none;
  }

  .ink-surface:focus-visible {
    outline: 2px solid #206acb;
    outline-offset: 2px;
  }

  .ink-surface.eraser-active {
    cursor: none;
  }

  .ink-surface.shape-active {
    cursor: crosshair;
  }

  canvas,
  .selection-overlay,
  .eraser-cursor {
    position: absolute;
    inset: 0;
    display: block;
  }

  /* Ink is painted here; input is handled by the surface itself, which sits above it. */
  .committed {
    position: absolute;
    inset: 0;
    display: block;
    pointer-events: none;
  }

  .selection-overlay {
    fill: var(--paper, #fcfcfa);
    stroke: var(--blueprint, #4c8df0);
    pointer-events: none;
  }

  .eraser-cursor {
    pointer-events: none;
  }

  .eraser-cursor-halo {
    fill: rgb(125 135 149 / 8%);
    stroke: rgb(255 255 255 / 80%);
  }

  .eraser-cursor-ring {
    fill: none;
    stroke: #7d8795;
  }

  .eraser-cursor.pressed .eraser-cursor-halo {
    fill: rgb(76 141 240 / 12%);
    stroke: rgb(255 255 255 / 90%);
  }

  .eraser-cursor.pressed .eraser-cursor-ring {
    stroke: #206acb;
  }

  .status {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
