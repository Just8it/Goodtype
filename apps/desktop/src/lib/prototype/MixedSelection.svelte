<script lang="ts">
  import type { Bounds } from "../ink/selection";
  import type { Point } from "../geometry/coordinates";
  import { viewportToPage } from "../geometry/coordinates";
  let { bounds, zoom, onStart, onPreview, onEnd }: {
    bounds: Bounds; zoom: number; onStart: () => void;
    onPreview: (delta: Point, scale: number, anchor: Point) => void;
    onEnd: (cancelled: boolean) => void;
  } = $props();
  let drag: { id: number; x: number; y: number; bounds: Bounds; scale: boolean } | null = null;
  function begin(event: PointerEvent, scale = false) {
    if (event.button !== 0 || event.buttons !== 1) return;
    event.preventDefault(); event.stopPropagation();
    const target = event.currentTarget as SVGElement;
    target.focus({ preventScroll: true });
    target.setPointerCapture(event.pointerId);
    drag = { id: event.pointerId, x: event.clientX, y: event.clientY, bounds: { ...bounds }, scale };
    onStart();
  }
  function move(event: PointerEvent) {
    if (!drag || event.pointerId !== drag.id) return;
    event.preventDefault(); event.stopPropagation();
    const { x: dx, y: dy } = viewportToPage({ x: event.clientX - drag.x, y: event.clientY - drag.y },
      { pageOriginX: 0, pageOriginY: 0, zoom });
    const b = drag.bounds;
    const scale = drag.scale ? Math.max(0.05, Math.hypot(b.right - b.left + dx, b.bottom - b.top + dy) /
      Math.max(1, Math.hypot(b.right - b.left, b.bottom - b.top))) : 1;
    onPreview(drag.scale ? { x: 0, y: 0 } : { x: dx, y: dy }, scale, { x: b.left, y: b.top });
  }
  function end(event: PointerEvent) {
    if (!drag || event.pointerId !== drag.id) return;
    event.preventDefault(); event.stopPropagation();
    const cancelled = event.type !== "pointerup";
    if (!cancelled) move(event);
    drag = null;
    onEnd(cancelled);
  }
  function keydown(event: KeyboardEvent) {
    if (event.key !== "+" && event.key !== "=" && event.key !== "-") return;
    event.preventDefault(); event.stopPropagation();
    onStart();
    onPreview({ x: 0, y: 0 }, event.key === "-" ? 0.9 : 1.1, { x: bounds.left, y: bounds.top });
    onEnd(false);
  }
</script>

<svg class="mixed-selection" role="group" aria-label="Mixed canvas selection" style:--zoom={zoom}>
  <rect role="button" tabindex="0" aria-label="Move selected items" use:focusOnMount
    x={bounds.left} y={bounds.top} width={Math.max(1, bounds.right - bounds.left)} height={Math.max(1, bounds.bottom - bounds.top)}
    onkeydown={keydown} onpointerdown={event => begin(event)} onpointermove={move} onpointerup={end} onpointercancel={end} onlostpointercapture={end} />
  <circle role="button" tabindex="0" aria-label="Scale selected items" cx={bounds.right} cy={bounds.bottom} r={22 / zoom}
    onkeydown={keydown} onpointerdown={event => begin(event, true)} onpointermove={move} onpointerup={end} onpointercancel={end} onlostpointercapture={end} />
  <circle class="visible-handle" cx={bounds.right} cy={bounds.bottom} r={6 / zoom} />
</svg>

<script lang="ts" module>
  function focusOnMount(node: SVGElement) { node.focus({ preventScroll: true }); }
</script>

<style>
  .mixed-selection { position: absolute; inset: 0; width: 100%; height: 100%; overflow: visible; z-index: 2147483647; pointer-events: none; }
  rect { fill: transparent; stroke: var(--blueprint); stroke-width: calc(1.5px / var(--zoom)); stroke-dasharray: calc(4px / var(--zoom)) calc(3px / var(--zoom)); pointer-events: all; cursor: move; touch-action: none; }
  circle { fill: transparent; pointer-events: all; cursor: nwse-resize; touch-action: none; }
  .visible-handle { fill: var(--panel); stroke: var(--blueprint); stroke-width: calc(1.5px / var(--zoom)); pointer-events: none; }
  [role=button]:focus-visible { outline: 2px solid var(--blueprint); }
</style>
