<script lang="ts">
  import { keepOnPage } from "../geometry/placement";

  type Position = { x: number; y: number };
  type Gesture =
    | {
        kind: "move";
        pointerId: number;
        clientX: number;
        clientY: number;
        x: number;
        y: number;
      }
    | {
        kind: "scale";
        pointerId: number;
        clientX: number;
        clientY: number;
        scale: number;
      };

  type Props = {
    src: string;
    alt: string;
    x: number;
    y: number;
    widthPt: number;
    heightPt: number;
    scale: number;
    selected?: boolean;
    toPageDelta?: (screenDx: number, screenDy: number) => Position;
    /** The sheet this image has to stay reachable on. */
    pageWidthPt: number;
    pageHeightPt: number;
    onSelect?: () => void;
    onMove?: (position: Position) => void;
    onScale?: (scale: number) => void;
  };

  let {
    src,
    alt,
    x,
    y,
    widthPt,
    heightPt,
    scale,
    selected = false,
    toPageDelta = (screenDx, screenDy) => ({ x: screenDx, y: screenDy }),
    pageWidthPt,
    pageHeightPt,
    onSelect,
    onMove,
    onScale,
  }: Props = $props();

  let previewX = $state(0);
  let previewY = $state(0);
  let previewScale = $state(1);
  let gesture = $state<Gesture | null>(null);

  $effect(() => {
    if (!gesture) {
      previewX = x;
      previewY = y;
      previewScale = scale;
    }
  });

  function beginMove(event: PointerEvent) {
    if (event.button !== 0) return;
    onSelect?.();
    event.currentTarget instanceof HTMLElement &&
      event.currentTarget.setPointerCapture(event.pointerId);
    gesture = {
      kind: "move",
      pointerId: event.pointerId,
      clientX: event.clientX,
      clientY: event.clientY,
      x: previewX,
      y: previewY,
    };
  }

  function beginScale(event: PointerEvent) {
    if (event.button !== 0) return;
    event.stopPropagation();
    onSelect?.();
    event.currentTarget instanceof HTMLElement &&
      event.currentTarget.setPointerCapture(event.pointerId);
    gesture = {
      kind: "scale",
      pointerId: event.pointerId,
      clientX: event.clientX,
      clientY: event.clientY,
      scale: previewScale,
    };
  }

  function update(event: PointerEvent) {
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    const delta = toPageDelta(
      event.clientX - gesture.clientX,
      event.clientY - gesture.clientY,
    );
    if (gesture.kind === "move") {
      // Clamped while dragging rather than on drop, so the limit is something you feel rather
      // than something that snaps the image back after you let go.
      const held = keepOnPage(
        { x: gesture.x + delta.x, y: gesture.y + delta.y },
        { widthPt: widthPt * previewScale, heightPt: heightPt * previewScale },
        { widthPt: pageWidthPt, heightPt: pageHeightPt },
      );
      previewX = held.x;
      previewY = held.y;
    } else {
      previewScale = Math.max(0.1, gesture.scale + delta.x / widthPt);
    }
  }

  function finish(event: PointerEvent) {
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    if (gesture.kind === "move") onMove?.({ x: previewX, y: previewY });
    else onScale?.(previewScale);
    gesture = null;
  }

  function cancel(event: PointerEvent) {
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    gesture = null;
    previewX = x;
    previewY = y;
    previewScale = scale;
  }

  function keydown(event: KeyboardEvent) {
    const step = event.shiftKey ? 10 : 1;
    const moves: Record<string, Position> = {
      ArrowLeft: { x: x - step, y },
      ArrowRight: { x: x + step, y },
      ArrowUp: { x, y: y - step },
      ArrowDown: { x, y: y + step },
    };
    if (moves[event.key]) {
      event.preventDefault();
      // Nudging is bounded like dragging is; an arrow key held down would otherwise walk the
      // image off the sheet just as effectively.
      onMove?.(
        keepOnPage(
          moves[event.key],
          { widthPt: widthPt * scale, heightPt: heightPt * scale },
          { widthPt: pageWidthPt, heightPt: pageHeightPt },
        ),
      );
    } else if (event.key === "+" || event.key === "=") {
      event.preventDefault();
      onScale?.(scale + 0.1);
    } else if (event.key === "-") {
      event.preventDefault();
      onScale?.(Math.max(0.1, scale - 0.1));
    }
  }
</script>

<div
  class:selected
  class="image-object"
  style:left={`${previewX}px`}
  style:top={`${previewY}px`}
  style:width={`${widthPt}px`}
  style:height={`${heightPt}px`}
  style:transform={`scale(${previewScale})`}
>
  <button
    class="move-surface"
    type="button"
    aria-label={`Move ${alt}`}
    onpointerdown={beginMove}
    onpointermove={update}
    onpointerup={finish}
    onpointercancel={cancel}
    onkeydown={keydown}
  >
    <img {src} {alt} draggable="false" />
  </button>
  <button
    class="scale-handle"
    type="button"
    aria-label={`Scale ${alt}`}
    onpointerdown={beginScale}
    onpointermove={update}
    onpointerup={finish}
    onpointercancel={cancel}
  ></button>
</div>

<style>
  .image-object {
    position: absolute;
    box-sizing: border-box;
    transform-origin: top left;
    touch-action: none;
    user-select: none;
  }

  .image-object.selected {
    outline: 1.5px solid #2f6fdb;
    outline-offset: 0;
  }

  .move-surface {
    display: block;
    width: 100%;
    height: 100%;
    padding: 0;
    border: 0;
    background: transparent;
    cursor: move;
  }

  .move-surface:focus-visible,
  .scale-handle:focus-visible {
    outline: 2px solid #4c8df0;
    outline-offset: 2px;
  }

  img {
    display: block;
    width: 100%;
    height: 100%;
    pointer-events: none;
  }

  .scale-handle {
    position: absolute;
    right: -6px;
    bottom: -6px;
    width: 12px;
    height: 12px;
    padding: 0;
    border: 1.5px solid white;
    background: #2f6fdb;
    cursor: nwse-resize;
  }
</style>
