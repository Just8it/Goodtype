<script lang="ts">
  import type { Point } from "../geometry/coordinates";
  import {
    boundedRasterScale,
    screenToPage,
    visiblePageRegion,
    zoomTier,
  } from "../geometry/coordinates";
  import {
    DEFAULT_INK_Z_INDEX,
    type ShapeStyle,
    type Stroke,
    type StrokePattern,
    type StrokePoint,
  } from "../model";
  import {
    maximumSampleGap,
    type StrokePerformance,
  } from "../ink/metrics";
  import {
    barrelInContact,
    DEFAULT_PRESSURE_CALIBRATION,
    eraserInHand,
    DEFAULT_POINTER_MAPPING,
    normalizePointerSample,
    pointerRole,
    quantizePoints,
    smoothPoints,
    type InkTool,
    type PointerMapping,
    type PressureCalibration,
  } from "../ink/pipeline";
  import { patternedOutlinePoints } from "../ink/pattern";
  import { limitedTaper, outlinePath } from "../ink/outline";
  import { paintStrata } from "../ink/paint";
  import { straightenStroke } from "../ink/straighten";
  import type { EraserReach } from "../settings";
  import {
    shapeFromDrag,
    shapeDashArray,
    shapePath,
    splineFromPoints,
    type ShapeDraft,
    type ShapeKind,
  } from "../shape/geometry";
  import { closedOutline, recognizeHeldShape } from "../shape/recognize";
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
    lassoTargets?: import("./lasso").LassoTargets;
    onLassoSelect?: (polygon: Point[], strokeIds: string[]) => void;
    showSelectionBounds?: boolean;
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
    taperLengthWidths?: number;
    /** Solid, dashed, or dotted; resolved into the stroke at pen-up. */
    pattern?: StrokePattern;
    /** Gap factor for the pattern, resolved into the stroke with it. */
    patternSpacing?: number;
    /** Ink opacity for the active tool, 0–1. */
    opacity?: number;
    /** Snap an almost-straight stroke to the line it was aiming for, on release. */
    straighten?: boolean;
    shapeKind?: ShapeKind;
    shapeStyle?: ShapeStyle;
    shapeConstrain?: boolean;
    drawAndHoldShapes?: boolean;
    heldLineMinimumPt?: number;
    eraseRadiusPt?: number;
    /** Highlighter-only leaves pen ink and shapes alone. */
    eraserReach?: EraserReach;
    calibration?: PressureCalibration;
    pointerMapping?: PointerMapping;
    /// The topmost shape the eraser reaches at this point, if any. Shapes live above this
    /// component, so it asks rather than holding them.
    shapeAt?: (point: Point, reachPt: number) => { id: string; zIndex: number } | null;
    onEraseShape?: (id: string) => void;
    /** Fires as an erase gesture lifts, whether or not it reached anything. */
    onEraseEnd?: () => void;
    /** An erase began from the pen's eraser end or side button while another tool was held. */
    onEraserEngaged?: () => void;
    onStrokeFinalized?: (stroke: Stroke) => void;
    onShapeFinalized?: (shape: ShapeDraft) => void;
    onInkShapeRecognized?: (stroke: Stroke, shape: ShapeDraft) => void;
    onStrokesChange?: (strokes: Stroke[]) => void;
    onSelectionChange?: (strokeIds: string[]) => void;
    onStrokeMetrics?: (metrics: StrokePerformance) => void;
  };

  let {
    lassoTargets = { handwriting: true, highlighter: true, shapes: true, images: true, text: true },
    onLassoSelect,
    showSelectionBounds = true,
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
    taperLengthWidths = 3,
    pattern = "solid",
    patternSpacing = 1,
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
    heldLineMinimumPt = 6,
    eraseRadiusPt = 8,
    eraserReach = "everything",
    calibration = DEFAULT_PRESSURE_CALIBRATION,
    pointerMapping = DEFAULT_POINTER_MAPPING,
    shapeAt,
    onEraseShape,
    onEraseEnd,
    onEraserEngaged,
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
  const MAX_IMMEDIATE_CANVAS_PIXELS = 12_000_000;
  /** Live ink a little past the window edge still shows while the page settles. */
  const LIVE_CANVAS_MARGIN_PT = 24;
  const SHAPE_HOLD_MS = 500;
  const SHAPE_SNAP_DEGREES = 15;
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
  // The hairline floor follows zoom in half-octave tiers, not every pinch step: with a per-step
  // floor every page rebuilt and re-joined all of its outlines dozens of times a second.
  const inkZoomTier = $derived(zoomTier(zoom));
  const committedStrata = $derived(
    paintStrata(localStrokes, objectZIndices, 0.5 / inkZoomTier),
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
    canvas.style.left = "0px";
    canvas.style.top = "0px";
    canvas.getContext("2d")?.setTransform(renderScale, 0, 0, renderScale, 0, 0);
  }

  /**
   * Re-fit the live canvas to what is on screen, at the resolution the zoom actually shows.
   *
   * The page-sized bitmap above is device resolution at 100%, and the zoom transform enlarges it,
   * so a live stroke at 4x looked four times blurrier than the same stroke once committed as SVG.
   * Covering only the visible part of the page keeps the bitmap near one screen of pixels at any
   * zoom. It runs once as a gesture starts, never during a pinch, so zooming allocates nothing.
   */
  function fitCanvasToView(canvas: HTMLCanvasElement): void {
    if (!surface) return;
    const page = { width: Math.max(pageWidthPt, 1), height: Math.max(pageHeightPt, 1) };
    const region = visiblePageRegion(
      surface.getBoundingClientRect(),
      { left: 0, top: 0, width: window.innerWidth, height: window.innerHeight },
      page,
      LIVE_CANVAS_MARGIN_PT,
    );
    if (!region) return;
    const renderScale = boundedRasterScale(
      region,
      (window.devicePixelRatio || 1) * Math.max(zoom, 0.01),
      MAX_IMMEDIATE_CANVAS_PIXELS,
    );
    canvas.style.left = `${region.x}px`;
    canvas.style.top = `${region.y}px`;
    canvas.style.width = `${region.width}px`;
    canvas.style.height = `${region.height}px`;
    const pixelWidth = Math.ceil(region.width * renderScale);
    const pixelHeight = Math.ceil(region.height * renderScale);
    if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) {
      canvas.width = pixelWidth;
      canvas.height = pixelHeight;
    }
    canvas
      .getContext("2d")
      ?.setTransform(renderScale, 0, 0, renderScale, -region.x * renderScale, -region.y * renderScale);
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
    if (immediateCanvas) fitCanvasToView(immediateCanvas);

    surface?.focus({ preventScroll: true });
    surface?.setPointerCapture(event.pointerId);
    if (role === "erase") {
      if (tool !== "eraser") onEraserEngaged?.();
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
    // Holding the barrel button and then touching down sends no second pointerdown: the press
    // began in the air. The gesture starts on the move that brings the tip onto the glass.
    if (!gesture && barrelInContact(event)) {
      pointerDown(event);
      return;
    }
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
      const polygon = gesture.points.concat(pagePoint(event));
      const ids = selectStrokesInLasso(localStrokes, polygon,
        stroke => stroke.tool === "highlighter" ? lassoTargets.highlighter : lassoTargets.handwriting);
      localSelection = ids;
      if (onLassoSelect) onLassoSelect(polygon, ids);
      else setSelection(ids);
    } else if (gesture.kind === "erase") {
      if (localStrokes !== gesture.initial) {
        onStrokesChange?.(localStrokes);
        onSelectionChange?.(localSelection);
      }
      finishPointer(event.pointerId);
      onEraseEnd?.();
      return;
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
    const erasing =
      gesture?.kind === "erase" || eraserInHand(event, tool, pointerMapping);
    eraserCursor = erasing ? pagePoint(event) : null;
  }

  function finishPointer(pointerId: number): void {
    clearHoldTimer();
    clearSplineFrame();
    gesture = null;
    eraserPressed = false;
    if (surface?.hasPointerCapture(pointerId)) surface.releasePointerCapture(pointerId);
    redrawImmediate();
  }

  /**
   * The eraser takes whole marks, and takes the one on top.
   *
   * Ink and shapes are held in different places, so the topmost of each is found separately and
   * then compared by the same z-index the renderer stacks them by. Without that, erasing ink drawn
   * over a shape would take the shape out from under it. A tie goes to the shape, which is the
   * case where a held mark was promoted and the two carry the same index by construction.
   */
  function eraseAt(point: Point): void {
    const highlighterOnly = eraserReach === "highlighter";
    const reaches = (stroke: Stroke) => !highlighterOnly || stroke.tool === "highlighter";
    const shape = highlighterOnly ? null : (shapeAt?.(point, eraserRadiusOnPage) ?? null);
    const stroke = hitStroke(localStrokes, point, eraserRadiusOnPage, reaches);
    if (!shape && !stroke) return;
    if (shape && (!stroke || shape.zIndex >= (stroke.zIndex ?? DEFAULT_INK_Z_INDEX))) {
      onEraseShape?.(shape.id);
      status = "Erased one shape";
      return;
    }
    const next = eraseStrokeAt(localStrokes, point, eraserRadiusOnPage, reaches);
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
          gesture.strokeTool === "pen" ? limitedTaper(gesture.points, taper, widthPt * taperLengthWidths) : taper,
          gesture.strokeTool === "pen" ? pattern : "solid",
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
    strokePattern: StrokePattern,
    strokeOpacity: number,
  ): void {
    if (points.length === 0) return;
    const polygons = patternedOutlinePoints(points, {
      // Below roughly half a device pixel the fill has nothing to cover, so hairline ink stays
      // visible when zoomed far out instead of dropping out entirely.
      widthPt: Math.max(baseWidthPt, 0.5 / zoom),
      pressure: usePressure,
      taper: strokeTaper,
    }, strokePattern, patternSpacing, gesture?.kind === "draw" && gesture.strokeTool === "pen");
    if (polygons.length === 0) return;
    context.save();
    context.fillStyle = strokeColor;
    context.globalAlpha = Math.min(1, Math.max(0, strokeOpacity));
    // Fill the complete stroke once: overlapping joins must not darken translucent ink.
    context.fill(new Path2D(polygons.map(outlinePath).join(" ")));
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
      pattern: strokeTool === "pen" ? pattern : "solid",
      ...(strokeTool === "pen" && pattern !== "solid" && patternSpacing !== 1
        ? { patternSpacing }
        : {}),
      pressure,
      taper: strokeTool === "pen" ? limitedTaper(points, taper, widthPt * taperLengthWidths) : taper,
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
      // Ending a freehand curve back where it began closes it, by the same rule that decides
      // whether a held mark was a ring — so a loop drawn deliberately and a loop recognised
      // from ink come out as the same shape.
      const { outline, closed } = closedOutline(active.points);
      return splineFromPoints(outline, shapeStyle, 1.5 / Math.max(zoom, 0.1), closed);
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
      // The same preference that squares a rectangle snaps a line, and by default neither
      // happens: what was drawn is what is kept.
      active.shapePreview = recognizeHeldShape(
        smoothPoints(active.points, calibration.smoothing),
        shapeStyle,
        { snapDegrees: shapeConstrain ? SHAPE_SNAP_DEGREES : 0, minimumLineLengthPt: heldLineMinimumPt },
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
    context.setLineDash(shapeDashArray(draft.style) ?? []);
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
<canvas
  bind:this={immediateCanvas}
  class="immediate"
  style:z-index={tool === "highlighter" && newStrokeZIndex < 0 ? newStrokeZIndex : 2147483645}
  aria-hidden="true"
></canvas>
<div
  bind:this={surface}
  class="ink-surface"
  class:eraser-active={tool === "eraser" || eraserCursor !== null}
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
  onlostpointercapture={pointerCancel}
  oncontextmenu={(event) => {
    // Windows turns the pen's barrel button into a right-click; on the page it is a tool.
    if (event instanceof PointerEvent && event.pointerType === "pen") event.preventDefault();
  }}
  onkeydown={keyDown}
>
  {#if showSelectionBounds && selectedBounds}
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

  .immediate { pointer-events: none; }

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
