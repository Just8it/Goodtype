import { renderNotebookCover } from "../ipc/workspace";
import { openPdf } from "../pdf/document";

/** Twice the shelf's 152px width, bounded for unusually tall pages. */
export const COVER_RASTER_WIDTH_PX = 304;
const COVER_RASTER_HEIGHT_PX = 512;
/** Twice a page overview tile, bounded the same way for tall pages. */
export const THUMBNAIL_RASTER_WIDTH_PX = 240;
const THUMBNAIL_RASTER_HEIGHT_PX = 480;

/** The canonical first page, through the same mixed-content renderer as PDF export. */
export async function notebookCover(
  root: string,
  pageTextBaselineGrid = true,
): Promise<Uint8Array | null> {
  const blob = await renderedPage(
    await renderNotebookCover(root, pageTextBaselineGrid),
    COVER_RASTER_WIDTH_PX,
    COVER_RASTER_HEIGHT_PX,
  );
  return blob ? new Uint8Array(await blob.arrayBuffer()) : null;
}

/** Any page of the notebook, small, through the same renderer: a tile in the page overview. */
export async function pageThumbnail(
  root: string,
  pageId: string,
  pageTextBaselineGrid = true,
): Promise<Blob | null> {
  return renderedPage(
    await renderNotebookCover(root, pageTextBaselineGrid, pageId),
    THUMBNAIL_RASTER_WIDTH_PX,
    THUMBNAIL_RASTER_HEIGHT_PX,
  );
}

async function renderedPage(bytes: ArrayBuffer, widthPx: number, heightPx: number): Promise<Blob | null> {
  const pdf = await openPdf(bytes);
  try {
    const page = await pdf.getPage(1);
    const natural = page.getViewport({ scale: 1 });
    const viewport = page.getViewport({ scale: Math.min(widthPx / natural.width, heightPx / natural.height) });
    const canvas = document.createElement("canvas");
    canvas.width = Math.ceil(viewport.width);
    canvas.height = Math.ceil(viewport.height);
    const context = canvas.getContext("2d");
    if (!context) return null;
    await page.render({ canvas, canvasContext: context, viewport }).promise;
    return await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
  } finally {
    await pdf.destroy();
  }
}
