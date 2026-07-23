<script lang="ts">
  import type { Point } from "../geometry/coordinates";
  import { screenToPage } from "../geometry/coordinates";
  import type { Stroke, StrokePoint } from "../model";
  import {
    maximumSampleGap,
    type StrokePerformance,
  } from "../ink/metrics";
  import {
    DEFAULT_PRESSURE_CALIBRATION,
    DEFAULT_POINTER_MAPPING,
    normalizePointerSample,
    pointerRole,
    smoothPoints,
    type InkTool,
    type PointerMapping,
    type PressureCalibration,
  } from "../ink/pipeline";
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
    selectedStrokeIds?: string[];
    pageWidthPt: number;
    pageHeightPt: number;
    zoom?: number;
    tool?: InkTool;
    color?: string;
    widthPt?: number;
    calibration?: PressureCalibration;
    pointerMapping?: PointerMapping;
    onStrokeFinalized?: (stroke: Stroke) => void;
    onStrokesChange?: (strokes: Stroke[]) => void;
    onSelectionChange?: (strokeIds: string[]) => void;
    onStrokeMetrics?: (metrics: StrokePerformance) => void;
  };

  let {
    strokes = [],
    selectedStrokeIds = [],
    pageWidthPt,
    pageHeightPt,
    zoom = 1,
    tool = "select",
    color = "#16212b",
    widthPt = 2,
    calibration = DEFAULT_PRESSURE_CALIBRATION,
    pointerMapping = DEFAULT_POINTER_MAPPING,
    onStrokeFinalized,
    onStrokesChange,
    onSelectionChange,
    onStrokeMetrics,
  }: Props = $props();

  let surface = $state<HTMLDivElement>();
  let completedCanvas = $state<HTMLCanvasElement>();
  let immediateCanvas = $state<HTMLCanvasElement>();
  let status = $state("No ink selected");
  let strokeCount = $state(0);
  let localStrokes: Stroke[] = [];
  let localSelection: string[] = [];

  type Gesture =
    | {
        kind: "draw";
        pointerId: number;
        points: StrokePoint[];
        strokeTool: "pen" | "highlighter";
        startedAt: number;
        feedbackMs: number | null;
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

  $effect(() => {
    localStrokes = strokes.slice();
    strokeCount = localStrokes.length;
    localSelection = selectedStrokeIds.slice();
    pageWidthPt;
    pageHeightPt;
    zoom;
    if (completedCanvas && immediateCanvas) {
      sizeCanvas(completedCanvas);
      sizeCanvas(immediateCanvas);
      redrawCompleted();
      redrawImmediate();
    }
  });

  function sizeCanvas(canvas: HTMLCanvasElement): void {
    const width = Math.max(pageWidthPt, 1);
    const height = Math.max(pageHeightPt, 1);
    const density = window.devicePixelRatio || 1;
    const renderScale = density * zoom;
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
    const role = pointerRole(event, tool, pointerMapping);
    if (role === "ignore" || gesture) return;
    event.preventDefault();

    surface?.focus({ preventScroll: true });
    surface?.setPointerCapture(event.pointerId);
    if (role === "erase") {
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
      };
      gesture = drawGesture;
      redrawImmediate();
      requestAnimationFrame(() => {
        drawGesture.feedbackMs = performance.now() - drawGesture.startedAt;
      });
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
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    event.preventDefault();
    if (gesture.kind === "draw") {
      const events = event.getCoalescedEvents?.() ?? [];
      for (const next of events.length > 0 ? events : [event]) {
        gesture.points.push(sample(next));
      }
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
      redrawCompleted();
    } else {
      const point = pagePoint(event);
      const distance = Math.hypot(point.x - gesture.anchor.x, point.y - gesture.anchor.y);
      localStrokes = scaleSelected(
        gesture.initial,
        localSelection,
        gesture.anchor,
        distance / gesture.startDistance,
      );
      redrawCompleted();
    }
    redrawImmediate();
  }

  function pointerUp(event: PointerEvent): void {
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    event.preventDefault();
    if (gesture.kind === "draw") {
      const commitStartedAt = performance.now();
      const points = smoothPoints(gesture.points.concat(sample(event)), calibration.smoothing);
      const stroke: Stroke = {
        id: crypto.randomUUID(),
        tool: gesture.strokeTool,
        color,
        widthPt: gesture.strokeTool === "highlighter" ? widthPt * 3 : widthPt,
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
      localStrokes = [...localStrokes, stroke];
      strokeCount = localStrokes.length;
      status = `Added ${stroke.tool} stroke`;
      onStrokeFinalized?.(stroke);
      onStrokeMetrics?.({
        sampleCount: points.length,
        maxSampleGapMs: maximumSampleGap(points),
        activeFeedbackMs:
          gesture.feedbackMs ?? performance.now() - gesture.startedAt,
        commitMs: performance.now() - commitStartedAt,
      });
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
      redrawCompleted();
    }
    finishPointer(event.pointerId);
  }

  function finishPointer(pointerId: number): void {
    gesture = null;
    if (surface?.hasPointerCapture(pointerId)) surface.releasePointerCapture(pointerId);
    redrawCompleted();
    redrawImmediate();
  }

  function eraseAt(point: Point): void {
    const next = eraseStrokeAt(localStrokes, point, 8 / zoom);
    if (next === localStrokes) return;
    localStrokes = next;
    strokeCount = localStrokes.length;
    localSelection = localSelection.filter((id) =>
      localStrokes.some((stroke) => stroke.id === id),
    );
    redrawCompleted();
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
      redrawCompleted();
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
      redrawCompleted();
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
      redrawCompleted();
      redrawImmediate();
    }
  }

  function redrawCompleted(): void {
    const context = canvasContext(completedCanvas);
    if (!context || !completedCanvas) return;
    context.clearRect(0, 0, completedCanvas.width, completedCanvas.height);
    for (const stroke of localStrokes) drawStroke(context, stroke);
  }

  function redrawImmediate(): void {
    const context = canvasContext(immediateCanvas);
    if (!context || !immediateCanvas) return;
    context.clearRect(0, 0, immediateCanvas.width, immediateCanvas.height);
    drawSelection(context);
    if (gesture?.kind === "draw") {
      drawPoints(
        context,
        gesture.points,
        gesture.strokeTool,
        color,
        gesture.strokeTool === "highlighter" ? widthPt * 3 : widthPt,
      );
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

  function drawStroke(context: CanvasRenderingContext2D, stroke: Stroke): void {
    drawPoints(
      context,
      stroke.points.map((point) => ({
        ...point,
        ...transformedPoint(stroke, point),
      })),
      stroke.tool,
      stroke.color,
      stroke.widthPt * Math.max(stroke.transform.scaleX, stroke.transform.scaleY),
    );
  }

  function drawPoints(
    context: CanvasRenderingContext2D,
    points: StrokePoint[],
    strokeTool: "pen" | "highlighter",
    strokeColor: string,
    baseWidthPt: number,
  ): void {
    if (points.length === 0) return;
    context.save();
    context.strokeStyle = strokeColor;
    context.fillStyle = strokeColor;
    context.lineCap = "round";
    context.lineJoin = "round";
    context.globalAlpha = strokeTool === "highlighter" ? 0.35 : 1;
    if (points.length === 1) {
      const point = toCanvasPoint(points[0]);
      context.beginPath();
      context.arc(
        point.x,
        point.y,
        Math.max(
          baseWidthPt * (0.25 + points[0].pressure * 0.75),
          0.5 / zoom,
        ) / 2,
        0,
        Math.PI * 2,
      );
      context.fill();
    } else {
      for (let index = 1; index < points.length; index += 1) {
        const start = toCanvasPoint(points[index - 1]);
        const end = toCanvasPoint(points[index]);
        context.beginPath();
        context.moveTo(start.x, start.y);
        context.lineTo(end.x, end.y);
        context.lineWidth =
          Math.max(baseWidthPt * (0.25 + points[index].pressure * 0.75), 0.5 / zoom);
        context.stroke();
      }
    }
    context.restore();
  }

  function drawSelection(context: CanvasRenderingContext2D): void {
    const bounds = selectionBounds(localStrokes, localSelection);
    if (!bounds) return;
    context.save();
    context.strokeStyle = "#206acb";
    context.fillStyle = "#ffffff";
    context.lineWidth = 1.5;
    context.setLineDash([4, 3]);
    context.strokeRect(
      bounds.left,
      bounds.top,
      bounds.right - bounds.left,
      bounds.bottom - bounds.top,
    );
    context.setLineDash([]);
    context.beginPath();
    context.arc(bounds.right, bounds.bottom, 6 / zoom, 0, Math.PI * 2);
    context.fill();
    context.stroke();
    context.restore();
  }

  function path(context: CanvasRenderingContext2D, points: Point[]): void {
    if (points.length === 0) return;
    context.beginPath();
    context.moveTo(points[0].x, points[0].y);
    for (const point of points.slice(1)) context.lineTo(point.x, point.y);
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

<div
  bind:this={surface}
  class="ink-surface"
  style:width={`${pageWidthPt}px`}
  style:height={`${pageHeightPt}px`}
  role="button"
  aria-label={`Ink canvas. ${strokeCount} strokes. ${status}. Arrow keys move selected ink; plus and minus scale it; Delete removes it.`}
  tabindex="0"
  onpointerdown={pointerDown}
  onpointermove={pointerMove}
  onpointerup={pointerUp}
  onpointercancel={pointerCancel}
  onkeydown={keyDown}
>
  <canvas bind:this={completedCanvas} aria-hidden="true"></canvas>
  <canvas bind:this={immediateCanvas} aria-hidden="true"></canvas>
</div>

<p class="status" aria-live="polite">{status}</p>

<style>
  .ink-surface {
    position: relative;
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

  canvas {
    position: absolute;
    inset: 0;
    display: block;
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
