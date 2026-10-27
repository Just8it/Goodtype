
import { notice, type UiText } from "../i18n";

import { flushSync } from "svelte";

import { clampZoom, dampedVelocity, pannedScroll, screenToPage, pageToScreen, type Point } from "../geometry/coordinates";

import { type NotebookManifest, type PageBackground, type PageGeometry, type PagePosition } from "../model";

import { type InkTool } from "../ink/pipeline";

import { createTouchTaps, type TouchHistoryAction } from "./touchTaps";

import { atScrollEnd, PULL_WHEEL_IDLE_MS, pullProgress, rubberBand, wheelPixels, type PullSource } from "./pullToAdd";

import { type AppSettings } from "../settings";
type PinchStart = { distance: number; zoom: number; pagePoint: Point };
type TouchPanStart = {
    pointerId: number;
    pointer: Point;
    scroll: Point;
    lastPointer: Point;
    lastTime: number;
    velocity: Point;
  };

export type NavigationEnvironment = {
  readonly addPage: (position: PagePosition, background?: PageBackground | null, geometry?: PageGeometry | null, count?: number) => Promise<void>;
  busy: boolean;
  readonly canRedo: boolean;
  readonly canUndo: boolean;
  readonly collectMetrics: boolean;
  directObjectInput: boolean;
  readonly notebookManifest: NotebookManifest | null;
  readonly openContextMenu: (clientX: number, clientY: number) => void;
  pageFrame: HTMLElement | undefined;
  readonly pageOpen: boolean;
  pageViewport: HTMLElement | undefined;
  readonly routeHistory: (command: "undo_notebook" | "redo_notebook", label: UiText, announce?: boolean) => Promise<void>;
  readonly routeObjectPointer: (event: PointerEvent) => void;
  readonly scheduleFocus: (delay?: number) => void;
  readonly scheduleResidency: () => void;
  readonly settings: AppSettings;
  readonly tauriAvailable: boolean;
  readonly tool: InkTool;
  readonly transactionFailed: boolean;
  workspace: HTMLElement | undefined;
  zoom: number;
  zoomFrameMs: number | null;
};

/** Instance-owned navigation state and operations. Callers supply live values at the boundary. */
export function createNavigation(env: NavigationEnvironment) {

  const touchPoints = new Map<number, Point>();

  const touchTaps = createTouchTaps();

  let tapScrollOrigin: { left: number; top: number } | null = null;

  let touchHistoryToast = $state<{ label: UiText } | null>(null);

  let touchHistoryToastTimer: ReturnType<typeof setTimeout> | undefined;

  let pinchStart: PinchStart | null = null;

  let touchPanStart: TouchPanStart | null = null;

  let automaticPageFocusLocked = false;

  let touchInertiaFrame: number | undefined;

  const pagePointers = new Map<number, string>();

  /**
   * Panning with the middle button, from anywhere on the page.
   *
   * Two fingers already pan and pinch, and a trackpad scrolls, but a mouse had only the
   * scrollbars — and the one gesture every drawing tool shares is that the middle button shoves
   * the canvas around without disturbing the tool in hand. It reads the scroll container directly
   * rather than going through `zoomAt`, because a pan does not change scale and so needs none of
   * the anchoring that a zoom does.
   */
  let panFrom = $state<{ pointerId: number; x: number; y: number; left: number; top: number } | null>(
    null,
  );

  function beginPan(event: PointerEvent): boolean {
    if (event.button !== 1 || !env.pageViewport) return false;
    stopTouchInertia();
    automaticPageFocusLocked = false;
    panFrom = {
      pointerId: event.pointerId,
      x: event.clientX,
      y: event.clientY,
      left: env.pageViewport.scrollLeft,
      top: env.pageViewport.scrollTop,
    };
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    // Middle-press is "autoscroll" on Windows; without this the browser drops its own scroll
    // puck on the page and takes the gesture over.
    event.preventDefault();
    event.stopPropagation();
    return true;
  }

  function continuePan(event: PointerEvent): boolean {
    if (!panFrom || event.pointerId !== panFrom.pointerId || !env.pageViewport) return false;
    env.pageViewport.scrollLeft = panFrom.left - (event.clientX - panFrom.x);
    env.pageViewport.scrollTop = panFrom.top - (event.clientY - panFrom.y);
    event.preventDefault();
    event.stopPropagation();
    return true;
  }

  function endPan(event: PointerEvent): boolean {
    if (!panFrom || event.pointerId !== panFrom.pointerId) return false;
    panFrom = null;
    event.preventDefault();
    event.stopPropagation();
    return true;
  }

  /**
   * Palm rejection for finger drawing. A hand resting on the glass while the pen hovers or writes
   * is a palm, not a finger, so for a moment after any pen activity touch goes back to only
   * navigating. Hover counts: Windows reports a pen in range as pointer moves with no buttons.
   */
  const PALM_REJECTION_MS = 500;

  let lastPenSeenAt = -Infinity;

  function notePen(event: PointerEvent) {
    if (event.pointerType === "pen") {
      lastPenSeenAt = event.timeStamp;
      touchTaps.cancel();
    }
  }

  function touchHistoryAllowed(event: PointerEvent): boolean {
    return env.settings.touchHistory && event.timeStamp - lastPenSeenAt > PALM_REJECTION_MS;
  }

  function applyTouchHistory(action: TouchHistoryAction) {
    if (env.busy || env.transactionFailed || !(action === "undo" ? env.canUndo : env.canRedo)) return;
    void env.routeHistory(action === "undo" ? "undo_notebook" : "redo_notebook",
      action === "undo" ? notice("workspace.undidChange") : notice("workspace.redidChange"), true);
  }

  function announceTouchHistory(label: UiText) {
    if (touchHistoryToastTimer) clearTimeout(touchHistoryToastTimer);
    touchHistoryToast = { label };
    touchHistoryToastTimer = setTimeout(() => { touchHistoryToast = null; }, 1000);
  }

  /** Whether this press is a finger the writer has asked to draw with. */
  function fingerDraws(event: PointerEvent): boolean {
    return (
      env.settings.drawWithFinger &&
      event.pointerType === "touch" &&
      event.timeStamp - lastPenSeenAt > PALM_REJECTION_MS
    );
  }

  function workspacePointerDown(event: PointerEvent) {
    notePen(event);
    const page = event.target instanceof Element ? event.target.closest<HTMLElement>("[data-page-id]") : null;
    if (page?.dataset.pageId && (event.button === 0 || event.pointerType === "pen")) pagePointers.set(event.pointerId, page.dataset.pageId);
    stopTouchInertia();
    if (beginPan(event)) return;
    // Fixed controls inside the scroll field own their touch. Without this exception the
    // one-finger canvas pan captures the pointer before a reorder button can receive its click.
    if (
      event.target instanceof Element &&
      event.target.closest(".selection-actions, .page-number button, .editor-dock")
    ) {
      if (event.pointerType === "touch") touchTaps.cancel();
      return;
    }
    env.routeObjectPointer(event);
    if (event.pointerType !== "touch") return;
    const onInk = event.target instanceof Element && Boolean(event.target.closest(".ink-surface"));
    touchTaps.start(event, touchHistoryAllowed(event) && onInk && !env.directObjectInput);
    if (touchPoints.size === 0 && env.pageViewport) {
      tapScrollOrigin = { left: env.pageViewport.scrollLeft, top: env.pageViewport.scrollTop };
    }
    touchPoints.set(event.pointerId, { x: event.clientX, y: event.clientY });
    if (env.tool === "shape" && onInk && touchPoints.size === 1) return;
    // A still finger held on the page opens the context menu. Not while a finger draws: there a
    // held finger is a dot being written.
    cancelLongPress();
    if (touchPoints.size === 1 && !fingerDraws(event) && !env.directObjectInput) {
      const { pointerId, clientX, clientY } = event;
      longPress = {
        pointerId,
        x: clientX,
        y: clientY,
        timer: setTimeout(() => {
          longPress = null;
          touchPanStart = null;
          env.openContextMenu(clientX, clientY);
        }, LONG_PRESS_MS),
      };
    }
    // A drawing finger is left to the ink surface. Only a second finger is claimed here, and
    // capturing it for the pinch takes the first away from the surface, which drops that stroke.
    if (
      touchPoints.size === 1 &&
      fingerDraws(event) &&
      !env.directObjectInput &&
      event.target instanceof Element &&
      event.target.closest(".ink-surface")
    ) return;
    if (touchPoints.size === 1 && !env.directObjectInput && env.pageViewport && env.workspace) {
      automaticPageFocusLocked = false;
      touchPanStart = {
        pointerId: event.pointerId,
        pointer: { x: event.clientX, y: event.clientY },
        scroll: { x: env.pageViewport.scrollLeft, y: env.pageViewport.scrollTop },
        lastPointer: { x: event.clientX, y: event.clientY },
        lastTime: performance.now(),
        velocity: { x: 0, y: 0 },
      };
      env.workspace.setPointerCapture(event.pointerId);
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    if (touchPoints.size > 2) {
      env.workspace?.setPointerCapture(event.pointerId);
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    if (touchPoints.size !== 2 || !env.workspace) return;
    // Undo a tap's tiny first-finger pan; larger motion has already rejected the tap candidate.
    if (touchTaps.isMultiTapCandidate(event.timeStamp) && tapScrollOrigin && env.pageViewport) {
      env.pageViewport.scrollLeft = tapScrollOrigin.left;
      env.pageViewport.scrollTop = tapScrollOrigin.top;
    }
    automaticPageFocusLocked = true;
    touchPanStart = null;
    if (pull?.source === "touch") releasePull(false);
    const points = [...touchPoints.values()];
    const center = {
      x: (points[0].x + points[1].x) / 2,
      y: (points[0].y + points[1].y) / 2,
    };
    const frame = env.pageFrame?.getBoundingClientRect();
    if (!frame) return;
    pinchStart = {
      distance: distance(points[0], points[1]),
      zoom: env.zoom,
      pagePoint: screenToPage(center, frame, { pageOriginX: 0, pageOriginY: 0, zoom: env.zoom }),
    };
    for (const pointerId of touchPoints.keys()) env.workspace.setPointerCapture(pointerId);
    event.preventDefault();
    event.stopPropagation();
  }

  function workspacePointerMove(event: PointerEvent) {
    notePen(event);
    if (event.pointerType === "touch") touchTaps.move(event, touchHistoryAllowed(event));
    if (
      longPress?.pointerId === event.pointerId &&
      Math.hypot(event.clientX - longPress.x, event.clientY - longPress.y) > LONG_PRESS_SLOP_PX
    ) {
      cancelLongPress();
    }
    if (continuePan(event)) return;
    env.routeObjectPointer(event);
    if (!touchPoints.has(event.pointerId)) return;
    touchPoints.set(event.pointerId, { x: event.clientX, y: event.clientY });
    // Jitter during a possible history tap must not zoom. Once the tap limits are exceeded,
    // the existing pan/pinch uses the complete movement from its original anchor.
    if (touchTaps.isMultiTapCandidate(event.timeStamp)) {
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    if (touchPanStart && event.pointerId === touchPanStart.pointerId && env.pageViewport) {
      const now = performance.now();
      const next = pannedScroll(touchPanStart.scroll, touchPanStart.pointer, {
        x: event.clientX,
        y: event.clientY,
      });
      const elapsed = Math.max(now - touchPanStart.lastTime, 1);
      const rawVelocity = {
        x: -(event.clientX - touchPanStart.lastPointer.x) / elapsed,
        y: -(event.clientY - touchPanStart.lastPointer.y) / elapsed,
      };
      touchPanStart.velocity = {
        x: touchPanStart.velocity.x * 0.35 + rawVelocity.x * 0.65,
        y: touchPanStart.velocity.y * 0.35 + rawVelocity.y * 0.65,
      };
      touchPanStart.lastPointer = { x: event.clientX, y: event.clientY };
      touchPanStart.lastTime = now;
      // Past the end the finger pulls against resistance instead of scrolling into nothing.
      const end = env.pageViewport.scrollHeight - env.pageViewport.clientHeight;
      if (canPull() && (next.y > end || pull?.source === "touch")) {
        const raw = next.y - end;
        pull = raw > 0 ? { raw, source: "touch" } : null;
        next.y = Math.min(next.y, end);
      }
      env.pageViewport.scrollLeft = next.x;
      env.pageViewport.scrollTop = next.y;
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    if (!pinchStart) return;
    const points = [...touchPoints.values()];
    if (points.length !== 2) return;
    // The page point first pinched stays under the fingers' *current* midpoint, so moving both
    // fingers pans while spreading them zooms — one gesture, as on paper apps. Anchoring to where
    // the pinch began made the fingers' travel do nothing.
    zoomAt(
      clampZoom(pinchStart.zoom * (distance(points[0], points[1]) / pinchStart.distance)),
      (points[0].x + points[1].x) / 2,
      (points[0].y + points[1].y) / 2,
      pinchStart.pagePoint,
    );
    event.preventDefault();
    event.stopPropagation();
  }

  function workspacePointerEnd(event: PointerEvent) {
    notePen(event);
    releasePagePointer(event);
    if (longPress?.pointerId === event.pointerId) cancelLongPress();
    if (endPan(event)) return;
    if (event.pointerType !== "touch") return;
    const tapCandidate = touchTaps.isMultiTapCandidate(event.timeStamp);
    const tap = touchTaps.end(event, event.type !== "pointerup", touchHistoryAllowed(event));
    touchPoints.delete(event.pointerId);
    if (touchPoints.size === 0) tapScrollOrigin = null;
    if (tapCandidate || tap) {
      touchPanStart = null;
      pinchStart = null;
      if (pull?.source === "touch") releasePull(false);
      event.preventDefault();
      event.stopPropagation();
      if (tap?.action) applyTouchHistory(tap.action);
      return;
    }
    if (touchPanStart?.pointerId === event.pointerId) {
      const finishedPan = touchPanStart;
      touchPanStart = null;
      if (pull?.source === "touch") {
        // A pull ends in the page or in nothing; it never hands off to a glide.
        releasePull(event.type === "pointerup");
        event.preventDefault();
        event.stopPropagation();
        return;
      }
      if (event.type === "pointerup" && performance.now() - finishedPan.lastTime < 80) {
        startTouchInertia(finishedPan.velocity);
      }
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    if (pinchStart) {
      touchPoints.clear();
      pinchStart = null;
      touchPanStart = null;
      event.preventDefault();
      event.stopPropagation();
    }
  }

  function releasePagePointer(event: PointerEvent) {
    if (!pagePointers.delete(event.pointerId)) return;
    env.scheduleResidency();
    env.scheduleFocus();
  }

  function distance(a: Point, b: Point) {
    return Math.max(Math.hypot(a.x - b.x, a.y - b.y), 1);
  }

  const TOUCH_INERTIA_RETENTION = 0.86;

  function stopTouchInertia() {
    if (touchInertiaFrame) cancelAnimationFrame(touchInertiaFrame);
    touchInertiaFrame = undefined;
  }

  function startTouchInertia(initialVelocity: Point) {
    stopTouchInertia();
    if (env.settings.reducedMotion || !env.pageViewport) return;
    let velocity = {
      x: Math.max(-1.5, Math.min(1.5, initialVelocity.x)) * env.settings.touchGlide,
      y: Math.max(-1.5, Math.min(1.5, initialVelocity.y)) * env.settings.touchGlide,
    };
    if (Math.hypot(velocity.x, velocity.y) < 0.04) return;
    let previous = performance.now();
    const glide = (now: number) => {
      if (!env.pageViewport) return;
      // Real time, so a busy frame (a page loading nearby) keeps the glide's distance instead of
      // quietly stealing it; capped only so a frame after a long stall cannot leap a whole page.
      const elapsed = Math.min(now - previous, 100);
      previous = now;
      env.pageViewport.scrollLeft += velocity.x * elapsed;
      env.pageViewport.scrollTop += velocity.y * elapsed;
      // Stop at an edge, not whenever one frame happened not to move.
      const maxLeft = env.pageViewport.scrollWidth - env.pageViewport.clientWidth;
      const maxTop = env.pageViewport.scrollHeight - env.pageViewport.clientHeight;
      if ((velocity.x < 0 && env.pageViewport.scrollLeft <= 0) || (velocity.x > 0 && env.pageViewport.scrollLeft >= maxLeft - 1)) velocity.x = 0;
      if ((velocity.y < 0 && env.pageViewport.scrollTop <= 0) || (velocity.y > 0 && env.pageViewport.scrollTop >= maxTop - 1)) velocity.y = 0;
      velocity = dampedVelocity(velocity, elapsed, TOUCH_INERTIA_RETENTION);
      if (Math.hypot(velocity.x, velocity.y) < 0.025) {
        touchInertiaFrame = undefined;
        return;
      }
      touchInertiaFrame = requestAnimationFrame(glide);
    };
    touchInertiaFrame = requestAnimationFrame(glide);
  }

  let longPress: { pointerId: number; x: number; y: number; timer: ReturnType<typeof setTimeout> } | null =
    null;

  const LONG_PRESS_MS = 500;

  const LONG_PRESS_SLOP_PX = 10;

  /**
   * The point on the active page nearest a viewport point, in page points: under it on the page,
   * pulled to the edge when it is in the margin, so a paste there still lands by the pointer.
   */
  function pagePointAt(clientX: number, clientY: number): Point | null {
    const frame = env.pageFrame?.getBoundingClientRect();
    if (!frame) return null;
    const x = Math.min(Math.max(clientX, frame.left), frame.right);
    const y = Math.min(Math.max(clientY, frame.top), frame.bottom);
    return screenToPage({ x, y }, frame, { pageOriginX: 0, pageOriginY: 0, zoom: env.zoom });
  }

  function cancelLongPress() {
    if (longPress) clearTimeout(longPress.timer);
    longPress = null;
  }

  function changeZoom(next: number) {
    const bounds = env.pageViewport?.getBoundingClientRect();
    if (bounds) {
      zoomAt(next, bounds.left + bounds.width / 2, bounds.top + bounds.height / 2);
      return;
    }
    env.zoom = clampZoom(next);
  }

  /**
   * The zoom the next frame will show, with the point that must stay under the cursor or fingers.
   *
   * A trackpad delivers several wheel events per frame, and each used to set the zoom, lay out the
   * whole page stack, and correct the scroll only on the *following* frame. The page was drawn
   * once at the new size in the old scroll position, which is the jump. Now events only record a
   * target; one frame applies it, lays out once, and corrects the scroll before anything paints.
   */
  let pendingZoom: { zoom: number; clientX: number; clientY: number; fixedPagePoint?: Point } | null =
    null;

  let pendingZoomFrame = 0;

  /** Where zoom is heading, so events arriving within one frame build on each other. */
  function targetZoom() {
    return pendingZoom?.zoom ?? env.zoom;
  }

  function zoomAt(next: number, clientX: number, clientY: number, fixedPagePoint?: Point) {
    if (!env.pageViewport || !env.pageFrame) return;
    automaticPageFocusLocked = true;
    pendingZoom = { zoom: clampZoom(next), clientX, clientY, fixedPagePoint };
    if (!pendingZoomFrame) pendingZoomFrame = requestAnimationFrame(applyPendingZoom);
  }

  function applyPendingZoom() {
    pendingZoomFrame = 0;
    const pending = pendingZoom;
    pendingZoom = null;
    if (!pending || !env.pageViewport || !env.pageFrame) return;
    const viewport = env.pageViewport;
    const frame = env.pageFrame;
    const startedAt = performance.now();
    const before = frame.getBoundingClientRect();
    const pagePoint = pending.fixedPagePoint ?? screenToPage(
      { x: pending.clientX, y: pending.clientY }, before,
      { pageOriginX: 0, pageOriginY: 0, zoom: env.zoom },
    );
    // Apply synchronously so the layout below already has the new size; the scroll is then fixed
    // in the same frame, before the browser paints.
    flushSync(() => {
      env.zoom = pending.zoom;
    });
    const after = frame.getBoundingClientRect();
    const anchored = pageToScreen(pagePoint, after, { pageOriginX: 0, pageOriginY: 0, zoom: env.zoom });
    viewport.scrollLeft += anchored.x - pending.clientX;
    viewport.scrollTop += anchored.y - pending.clientY;
    if (env.collectMetrics) env.zoomFrameMs = performance.now() - startedAt;
  }

  /**
   * Pull past the last page to add one (specs/qol-wave.md section 1).
   *
   * A finger pull is measured from the pan and decided on release; a wheel or trackpad pull fills
   * as it scrolls and adds the moment the ring is full, because a wheel has no release. After a
   * wheel add the pull stays disarmed until the wheel goes quiet, so a trackpad's momentum tail
   * cannot add a second page behind the first.
   */
  let pull = $state<{ raw: number; source: PullSource } | null>(null);

  let pullIdleTimer: ReturnType<typeof setTimeout> | undefined;

  let pullDisarmed = false;

  const pullFill = $derived(pull ? pullProgress(pull.raw, pull.source) : 0);

  const pullShift = $derived(pull && !env.settings.reducedMotion ? rubberBand(pull.raw) : 0);

  const pullPaper = $derived.by(() => {
    const paper = env.notebookManifest?.defaultPage;
    return paper && paper.background.kind !== "pdf" ? paper : null;
  });

  function canPull(): boolean {
    return env.settings.pullToAddPage && env.tauriAvailable && !env.busy && env.notebookManifest !== null && env.pageOpen;
  }

  function pullFromWheel(event: WheelEvent) {
    const viewport = env.pageViewport;
    if (!viewport) return;
    const delta = wheelPixels(event.deltaY, event.deltaMode, viewport.clientHeight);
    clearTimeout(pullIdleTimer);
    pullIdleTimer = setTimeout(() => {
      pullDisarmed = false;
      if (pull?.source === "wheel") pull = null;
    }, PULL_WHEEL_IDLE_MS);
    if (pullDisarmed || !canPull()) return;
    if (pull?.source === "wheel") {
      const raw = pull.raw + delta;
      pull = raw > 0 ? { raw, source: "wheel" } : null;
    } else if (
      delta > 0 &&
      atScrollEnd(viewport.scrollTop, viewport.clientHeight, viewport.scrollHeight)
    ) {
      pull = { raw: delta, source: "wheel" };
    } else {
      return;
    }
    event.preventDefault();
    if (pull && pullProgress(pull.raw, "wheel") >= 1) {
      pull = null;
      pullDisarmed = true;
      void addPageFromPull();
    }
  }

  function releasePull(add: boolean) {
    const ready = pull !== null && pullProgress(pull.raw, pull.source) >= 1;
    pull = null;
    if (add && ready) void addPageFromPull();
  }

  async function addPageFromPull() {
    await env.addPage({ kind: "last" }, pullPaper?.background ?? null, pullPaper?.geometry ?? null);
  }

  function wheelZoom(event: WheelEvent) {
    stopTouchInertia();
    if (!event.ctrlKey) {
      automaticPageFocusLocked = false;
      pullFromWheel(event);
      return;
    }
    event.preventDefault();
    zoomAt(targetZoom() * Math.exp(-event.deltaY * 0.01), event.clientX, event.clientY);
  }
  function dispose() {
    stopTouchInertia();
    cancelLongPress();
    clearTimeout(touchHistoryToastTimer);
    clearTimeout(pullIdleTimer);
    if (pendingZoomFrame) cancelAnimationFrame(pendingZoomFrame);
    pendingZoom = null;
    pendingZoomFrame = 0;
    pagePointers.clear();
    touchPoints.clear();
    touchTaps.reset();
  }
  return {
    dispose,
    announceTouchHistory,
    fingerDraws,
    workspacePointerDown,
    workspacePointerMove,
    workspacePointerEnd,
    releasePagePointer,
    stopTouchInertia,
    changeZoom,
    zoomAt,
    wheelZoom,
    pagePointAt,
    get panFrom() { return panFrom; },
    get touchTaps() { return touchTaps; },
    get touchHistoryToast() { return touchHistoryToast; },
    get pagePointers() { return pagePointers; },
    get tapScrollOrigin() { return tapScrollOrigin; },
    set tapScrollOrigin(value: typeof tapScrollOrigin) { tapScrollOrigin = value; },
    get pinchStart() { return pinchStart; },
    get touchPanStart() { return touchPanStart; },
    get automaticPageFocusLocked() { return automaticPageFocusLocked; },
    get touchInertiaFrame() { return touchInertiaFrame; },
    get pull() { return pull; },
    get pullPaper() { return pullPaper; },
    get pullFill() { return pullFill; },
    get pullShift() { return pullShift; },
  };
}
