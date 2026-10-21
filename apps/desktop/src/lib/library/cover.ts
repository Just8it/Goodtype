import { renderNotebookCover } from "../ipc/workspace";
import { openPdf } from "../pdf/document";

/** Twice the shelf's 152px width, bounded for unusually tall pages. */
export const COVER_RASTER_WIDTH_PX = 304;
const COVER_RASTER_HEIGHT_PX = 512;

/** The canonical first page, through the same mixed-content renderer as PDF export. */
export async function notebookCover(
  root: string,
  pageTextBaselineGrid = true,
): Promise<Uint8Array | null> {
  const pdf = await openPdf(await renderNotebookCover(root, pageTextBaselineGrid));
  try {
    const page = await pdf.getPage(1);
    const natural = page.getViewport({ scale: 1 });
    const viewport = page.getViewport({ scale: Math.min(
      COVER_RASTER_WIDTH_PX / natural.width, COVER_RASTER_HEIGHT_PX / natural.height,
    ) });
    const canvas = document.createElement("canvas");
    canvas.width = Math.ceil(viewport.width);
    canvas.height = Math.ceil(viewport.height);
    const context = canvas.getContext("2d");
    if (!context) return null;
    await page.render({ canvas, canvasContext: context, viewport }).promise;
    const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
    return blob ? new Uint8Array(await blob.arrayBuffer()) : null;
  } finally {
    await pdf.destroy();
  }
}