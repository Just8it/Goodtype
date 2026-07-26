<script lang="ts">
  import type { PageDefaults } from "../model";
  import { templateSvg } from "../page/template";

  let {
    paper = null,
    widthPx,
  }: {
    /** The notebook's own paper. Null when its manifest could not be read. */
    paper?: PageDefaults | null;
    widthPx: number;
  } = $props();

  // A4 in points, used only when a notebook cannot say what shape it is. The tile still has to
  // have proportions, and this is the one every other page in the app defaults to.
  const FALLBACK = { widthPt: 595.2756, heightPt: 841.8898 };

  const geometry = $derived(paper?.geometry ?? FALLBACK);
  const heightPx = $derived((widthPx * geometry.heightPt) / geometry.widthPt);

  // The paper's own colour, not a fixed white: a cream or black page should read as one on the
  // shelf exactly as it does on the desk.
  const sheet = $derived(
    paper?.background.kind === "plain"
      ? paper.background.color
      : paper?.background.kind === "template"
        ? paper.background.template.backgroundColor
        : "#FCFCFA",
  );

  // Ruling is geometry, so it costs nothing to draw at true scale — the same `resolve` the page
  // and the PDF go through, pinned by `fixtures/templates/resolved.json`. A tile therefore shows
  // the paper the notebook is actually written on rather than a drawing of some paper.
  //
  // At true scale a 5mm grid lands about four pixels apart here. That is dense, and it is
  // correct: it is what squared paper looks like held at arm's length, and it tells the two
  // apart at a glance, which a coarser invented ruling would not.
  const ruling = $derived(
    paper?.background.kind === "template"
      ? templateSvg(paper.background.template, geometry)
      : null,
  );
</script>

<div
  class="cover"
  style:width={`${widthPx}px`}
  style:height={`${heightPx}px`}
  style:background={sheet}
>
  {#if ruling}
    <!-- The SVG is generated here from the notebook's own template, never read from a file. -->
    <div class="ruling">{@html ruling}</div>
  {/if}
</div>

<style>
  .cover {
    position: relative;
    flex: none;
    overflow: hidden;
    border: 1px solid rgb(0 0 0 / 40%);
    border-radius: 3px;
  }

  /* The generated SVG carries the page's size in points; the wrapper scales it to the tile. */
  .ruling {
    position: absolute;
    inset: 0;
  }

  .ruling :global(svg) {
    display: block;
    width: 100%;
    height: 100%;
  }
</style>
