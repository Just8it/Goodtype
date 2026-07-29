import type { PageObject, Stroke } from "../model";

// One view model for a page, shared by every renderer. Before this existed, the active page and
// its neighbours each derived objects, sources, and asset URLs their own way, which is what let
// the two paths drift apart. Deriving both from here keeps one page looking the same whether it
// is being edited or merely displayed.

export type BlockView = {
  id: string;
  path: string;
  source: string;
  x: number;
  y: number;
  layoutWidthPt: number;
  scale: number;
  zIndex: number;
  readingOrder: number;
};

export type ImageView = {
  id: string;
  path: string;
  url: string;
  alt: string;
  x: number;
  y: number;
  widthPt: number;
  heightPt: number;
  scale: number;
  zIndex: number;
  readingOrder: number;
};

type StoredFile = { path: string; bytes: number[] };

type PageLike = { objects: PageObject[] };

type SnapshotLike = {
  page: PageLike;
  blocks: StoredFile[];
  assets: StoredFile[];
  inkLayers: { strokes: Stroke[] }[];
};

function decode(file: StoredFile | undefined): string {
  return file ? new TextDecoder().decode(new Uint8Array(file.bytes)) : "";
}

export function blockViewsFromSnapshot(snapshot: SnapshotLike): BlockView[] {
  return snapshot.page.objects
    .filter(
      (object): object is Extract<PageObject, { type: "typst" }> =>
        object.type === "typst",
    )
    .map((object) => ({
      id: object.id,
      path: object.sourcePath,
      source: decode(
        snapshot.blocks.find((file) => file.path === object.sourcePath),
      ),
      x: object.x,
      y: object.y,
      layoutWidthPt: object.layoutWidthPt,
      scale: object.scale,
      zIndex: object.zIndex,
      readingOrder: object.readingOrder,
    }));
}

export function imageViewsFromSnapshot(
  snapshot: SnapshotLike,
  urls: AssetUrlCache,
): ImageView[] {
  return snapshot.page.objects
    .filter(
      (object): object is Extract<PageObject, { type: "image" }> =>
        object.type === "image",
    )
    .flatMap((object) => {
      const asset = snapshot.assets.find(
        (file) => file.path === object.sourcePath,
      );
      const url = asset ? urls.get(asset.path, asset.bytes) : "";
      if (!url) return [];
      return [
        {
          id: object.id,
          path: object.sourcePath,
          url,
          alt: object.altText,
          x: object.x,
          y: object.y,
          widthPt: object.widthPt,
          heightPt: object.heightPt,
          scale: object.scale,
          zIndex: object.zIndex,
          readingOrder: object.readingOrder,
        },
      ];
    });
}

export function strokesFromSnapshot(snapshot: SnapshotLike): Stroke[] {
  return snapshot.inkLayers.flatMap((layer) => layer.strokes);
}

export function mimeForPath(path: string): string {
  if (path.endsWith(".jpg") || path.endsWith(".jpeg")) return "image/jpeg";
  if (path.endsWith(".svg")) return "image/svg+xml";
  if (path.endsWith(".webp")) return "image/webp";
  return "image/png";
}

/**
 * Object URLs for a page's assets, revoked together when the page is released. Object URLs leak
 * for the life of the document otherwise, and a notebook scrolled end to end creates one per
 * image per visit.
 */
export class AssetUrlCache {
  #urls = new Map<string, string>();

  get(path: string, bytes: number[]): string {
    const existing = this.#urls.get(path);
    if (existing) return existing;
    const url = URL.createObjectURL(
      new Blob([new Uint8Array(bytes)], { type: mimeForPath(path) }),
    );
    this.#urls.set(path, url);
    return url;
  }

  dispose(): void {
    for (const url of this.#urls.values()) URL.revokeObjectURL(url);
    this.#urls.clear();
  }
}
