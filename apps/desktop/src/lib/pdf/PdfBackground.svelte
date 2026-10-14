<script lang="ts">
  import type { RenderTask } from "pdfjs-dist";
  import { untrack } from "svelte";
  import { zoomTier } from "../geometry/coordinates";
  import { pdfDocument } from "./document";

  const SETTLE_MS = 180;

  let {
    root,
    sourcePath,
    page,
    widthPt,
    zoom,
  }: {
    root: string;
    sourcePath: string;
    page: number;
    widthPt: number;
    zoom: number;
  } = $props();

  let canvas = $state<HTMLCanvasElement>();

  /**
   * The zoom the raster is drawn for. It trails the live zoom: during a pinch the existing bitmap
   * is simply scaled by the page transform, and the page is only re-rendered once zooming has
   * paused. Re-rendering on every step cancelled and restarted a full-page pdf.js render dozens
   * of times a second on the main thread, which was most of the stutter on PDF pages.
   */
  // Starts at the zoom the page opens with; afterwards it moves only once zooming settles.
  let renderZoom = $state(untrack(() => zoomTier(zoom)));

  $effect(() => {
    const tier = zoomTier(zoom);
    if (tier === renderZoom) return;
    const timer = setTimeout(() => (renderZoom = tier), SETTLE_MS);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    if (!canvas) return;
    const target = canvas;
    const pixelRatio = Math.min(4, Math.max(1, window.devicePixelRatio * renderZoom));
    let cancelled = false;
    let renderTask: RenderTask | null = null;

    void pdfDocument(root, sourcePath)
      .then((pdf) => pdf.getPage(page))
      .then(async (sourcePage) => {
        if (cancelled) return;
        const natural = sourcePage.getViewport({ scale: 1 });
        const viewport = sourcePage.getViewport({
          scale: (widthPt * pixelRatio) / natural.width,
        });
        // Rendered off screen and copied over when complete: resizing the visible canvas clears
        // it, and the page used to flash blank for the length of every render.
        const offscreen = document.createElement("canvas");
        offscreen.width = Math.ceil(viewport.width);
        offscreen.height = Math.ceil(viewport.height);
        const offscreenContext = offscreen.getContext("2d");
        if (!offscreenContext) return;
        renderTask = sourcePage.render({ canvas: offscreen, canvasContext: offscreenContext, viewport });
        await renderTask.promise;
        if (cancelled) return;
        target.width = offscreen.width;
        target.height = offscreen.height;
        target.getContext("2d")?.drawImage(offscreen, 0, 0);
      })
      .catch((error) => {
        if (!cancelled && error?.name !== "RenderingCancelledException") {
          console.error("Could not render PDF background", error);
        }
      });

    return () => {
      cancelled = true;
      renderTask?.cancel();
    };
  });
</script>

<canvas bind:this={canvas} aria-hidden="true"></canvas>

<style>
  canvas {
    display: block;
    width: 100%;
    height: 100%;
  }
</style>
