<script lang="ts">
  import type { RenderTask } from "pdfjs-dist";
  import { pdfDocument } from "./document";

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

  $effect(() => {
    if (!canvas) return;
    const target = canvas;
    const pixelRatio = Math.min(4, Math.max(1, window.devicePixelRatio * zoom));
    let cancelled = false;
    let renderTask: RenderTask | null = null;

    void pdfDocument(root, sourcePath)
      .then((document) => document.getPage(page))
      .then((sourcePage) => {
        if (cancelled) return;
        const natural = sourcePage.getViewport({ scale: 1 });
        const viewport = sourcePage.getViewport({
          scale: (widthPt * pixelRatio) / natural.width,
        });
        target.width = Math.ceil(viewport.width);
        target.height = Math.ceil(viewport.height);
        const context = target.getContext("2d");
        if (!context) return;
        renderTask = sourcePage.render({ canvas: target, canvasContext: context, viewport });
        return renderTask.promise;
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
