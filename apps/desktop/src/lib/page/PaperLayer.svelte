<script lang="ts">
  import type { PageBackground } from "../model";
  import PdfBackground from "../pdf/PdfBackground.svelte";
  import { templateSvg } from "./template";

  /**
   * The paper a page is drawn on, under everything else.
   *
   * Rendered as one SVG rather than a repeating CSS background so it lines up with the exported
   * PDF: both sides resolve the same definition through the same rules, and a CSS pattern would
   * have its own opinion about where the first line goes.
   */
  let {
    background,
    widthPt,
    heightPt,
    root = null,
    zoom = 1,
  }: {
    background: PageBackground;
    widthPt: number;
    heightPt: number;
    root?: string | null;
    zoom?: number;
  } = $props();

  // Recomputed only when the paper or the page size changes, not on every zoom step: the SVG is
  // in page coordinates and the frame scales it.
  const markup = $derived(
    background.kind === "template"
      ? templateSvg(background.template, { widthPt, heightPt })
      : null,
  );

  const flat = $derived(
    background.kind === "plain" ? background.color : background.kind === "pdf" ? "#ffffff" : null,
  );
</script>

<div class="paper-layer" style:background={flat ?? undefined}>
  {#if markup}
    <!-- Built from the page's own definition by `templateSvg`, never read from a file. -->
    {@html markup}
  {:else if background.kind === "pdf" && root}
    <PdfBackground
      {root}
      sourcePath={background.sourcePath}
      page={background.page}
      {widthPt}
      {zoom}
    />
  {/if}
</div>

<style>
  .paper-layer {
    position: absolute;
    z-index: 0;
    inset: 0;
    pointer-events: none;
  }

  .paper-layer :global(svg) {
    display: block;
    width: 100%;
    height: 100%;
  }
</style>
