<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { PageDefaults } from "../model";
  import { templateSvg } from "../page/template";

  let {
    paper = null,
    path = null,
    widthPx,
  }: {
    /** The notebook's own paper. Null when its manifest could not be read. */
    paper?: PageDefaults | null;
    /**
     * Library-relative path, used to fetch the stored cover. Null suppresses the fetch, which is
     * what a notebook outside any library wants.
     */
    path?: string | null;
    widthPx: number;
  } = $props();

  let cover = $state<string | null>(null);

  /**
   * Fetched per tile rather than as part of the listing, so a folder of a hundred notebooks does
   * not put a hundred rasters into one reply for the sake of the dozen on screen. A notebook that
   * has never been saved since covers existed simply has none, and the paper below shows through.
   */
  $effect(() => {
    const wanted = path;
    if (!wanted) return;
    let current = true;
    void (async () => {
      try {
        const found = await invoke<string | null>("library_cover", { path: wanted });
        if (current) cover = found;
      } catch {
        // A missing or unreadable cover is not worth reporting: the tile still draws its paper.
      }
    })();
    return () => (current = false);
  });

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
  {#if cover}
    <!-- The cover already contains its own paper and ruling, drawn at save time from the same
         geometry, so it covers rather than sits beside what is underneath. The ruling above is
         what a notebook saved before it had a cover falls back to. -->
    <img src={cover} alt="" draggable="false" />
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

  img {
    position: absolute;
    display: block;
    width: 100%;
    height: 100%;
    inset: 0;
    pointer-events: none;
  }
</style>
