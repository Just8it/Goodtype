/** Release Chromium's fixed-resolution layer after zoom, then prime it for the next gesture. */
export function refreshPageRaster(node: HTMLElement, zooming: boolean) {
  let wasZooming = zooming;
  let frame: number | undefined;
  function cancel() {
    if (frame !== undefined) cancelAnimationFrame(frame);
    frame = undefined;
    node.style.removeProperty("--page-layer-hint");
  }
  function refresh() {
    node.style.setProperty("--page-layer-hint", "auto");
    // Two callbacks allow an actual paint without will-change before restoring promotion.
    frame = requestAnimationFrame(() => {
      frame = requestAnimationFrame(cancel);
    });
  }
  if (!zooming) refresh();
  return {
    update(next: boolean) {
      if (next === wasZooming) return;
      cancel();
      if (!next) refresh();
      wasZooming = next;
    },
    destroy: cancel,
  };
}
