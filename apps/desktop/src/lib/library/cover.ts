/**
 * A notebook's cover: one page, drawn small enough to sit on a shelf.
 *
 * Why a cached raster and not a live render. Drawing page one of forty notebooks means loading
 * forty notebooks' worth of strokes, which is exactly the cost the one-level listing exists to
 * avoid — so a cover has to be made once, when the page is saved, and read back as an image.
 *
 * Why PNG and not SVG. A dense page is hundreds of kilobytes of path data whatever size it is
 * displayed at, and forty of those in a grid is megabytes of DOM. A raster is one image element
 * and one decode, sized for the tile it will occupy.
 *
 * The geometry comes from the same `resolveTemplate` and `outlinePoints` the page and the PDF
 * use, both pinned by shared fixtures, so a cover cannot show ruling or ink that the page itself
 * would not.
 */

import { paintOrder } from "../ink/paint";
import type { PageBackground, PageGeometry, Stroke } from "../model";
import { templateBodySvg } from "../page/template";

/**
 * How wide a cover is rasterised, in pixels.
 *
 * Twice the 152px the shelf draws, so the tile stays sharp on a 2× display without storing a
 * full-page scan. Height follows the page's own proportions.
 */
export const COVER_RASTER_WIDTH_PX = 304;

/** How long to wait for the browser to decode the page before giving up on a cover. */
const RASTER_TIMEOUT_MS = 4000;

function paperColor(background: PageBackground): string {
  if (background.kind === "plain") return background.color;
  if (background.kind === "template") return background.template.backgroundColor;
  return "#FCFCFA";
}

/** One page as a standalone SVG document: paper, then ruling, then ink. */
export function coverSvg(
  background: PageBackground,
  geometry: PageGeometry,
  strokes: Stroke[],
): string {
  const ruling =
    background.kind === "template" ? templateBodySvg(background.template, geometry) : "";
  const ink = paintOrder(strokes)
    .map(
      (painted) =>
        `<path d="${painted.d}" fill="${painted.color}" fill-opacity="${painted.opacity}" fill-rule="nonzero"/>`,
    )
    .join("");
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" width="${geometry.widthPt}" height="${geometry.heightPt}"` +
    ` viewBox="0 0 ${geometry.widthPt} ${geometry.heightPt}">` +
    `<rect width="100%" height="100%" fill="${paperColor(background)}"/>${ruling}${ink}</svg>`
  );
}

/**
 * Rasterise a cover to PNG bytes.
 *
 * Goes through a blob URL rather than a `data:` URL because a page's worth of path data can run
 * past what some engines accept in one URL, and a blob has no such ceiling. The URL is revoked on
 * every path out, including the failing ones.
 *
 * Returns null rather than throwing: a cover is a nicety, and a notebook that saved correctly
 * must not report a failure because its thumbnail could not be drawn.
 */
export async function rasteriseCover(
  svg: string,
  geometry: PageGeometry,
  widthPx = COVER_RASTER_WIDTH_PX,
): Promise<Uint8Array | null> {
  const url = URL.createObjectURL(new Blob([svg], { type: "image/svg+xml" }));
  try {
    const image = new Image();
    const decoded = new Promise<boolean>((resolve) => {
      const timer = setTimeout(() => resolve(false), RASTER_TIMEOUT_MS);
      image.onload = () => (clearTimeout(timer), resolve(true));
      image.onerror = () => (clearTimeout(timer), resolve(false));
    });
    image.src = url;
    if (!(await decoded)) return null;

    const heightPx = Math.max(1, Math.round((widthPx * geometry.heightPt) / geometry.widthPt));
    const canvas = document.createElement("canvas");
    canvas.width = widthPx;
    canvas.height = heightPx;
    const context = canvas.getContext("2d");
    if (!context) return null;
    context.drawImage(image, 0, 0, widthPx, heightPx);

    const blob = await new Promise<Blob | null>((resolve) =>
      canvas.toBlob(resolve, "image/png"),
    );
    if (!blob) return null;
    return new Uint8Array(await blob.arrayBuffer());
  } catch {
    return null;
  } finally {
    URL.revokeObjectURL(url);
  }
}
