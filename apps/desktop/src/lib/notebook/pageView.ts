import type { TypstCompileResult } from "../editor/typst";
import type { InkLayer, PageObject, ShapeGeometry, ShapeStyle, Stroke } from "../model";

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
  rotation?: number;
  scale: number;
  zIndex: number;
  readingOrder: number;
};

export type TypstTransform = Pick<BlockView, "x" | "y" | "layoutWidthPt" | "scale" | "rotation">;

/** Interactive block view; compilation output remains disposable session state. */
export type TypstBlockState = Omit<BlockView, keyof TypstTransform> & {
  transform: TypstTransform;
  result: TypstCompileResult | null;
};

export type PageTypstView = {
  id: string;
  path: string;
  source: string;
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
  rotation?: number;
  scale: number;
  zIndex: number;
  readingOrder: number;
};

export type ShapeView = {
  id: string;
  x: number;
  y: number;
  rotation: number;
  scale: number;
  zIndex: number;
  readingOrder: number;
  geometry: ShapeGeometry;
  style: ShapeStyle;
};

/**
 * How a shape edit should reach the store.
 *
 * A finished pointer gesture is one decision and earns a commit of its own. A held arrow key is
 * a single continuous adjustment, and writing a full snapshot per repeat would flood both the
 * transaction queue and the undo stack with steps nobody wants to step back through.
 */
export type ShapeEditCommit = "now" | "settled";

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
      rotation: object.rotation,
      scale: object.scale,
      zIndex: object.zIndex,
      readingOrder: object.readingOrder,
    }));
}

export function pageTypstViewFromSnapshot(snapshot: SnapshotLike): PageTypstView | null {
  const object = snapshot.page.objects.find(
    (candidate): candidate is Extract<PageObject, { type: "page_typst" }> =>
      candidate.type === "page_typst",
  );
  if (!object) return null;
  return {
    id: object.id,
    path: object.sourcePath,
    source: decode(snapshot.blocks.find((file) => file.path === object.sourcePath)),
    zIndex: object.zIndex,
    readingOrder: object.readingOrder,
  };
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
          rotation: object.rotation,
          scale: object.scale,
          zIndex: object.zIndex,
          readingOrder: object.readingOrder,
        },
      ];
    });
}

export function shapeViewsFromSnapshot(snapshot: SnapshotLike): ShapeView[] {
  return snapshot.page.objects
    .filter(
      (object): object is Extract<PageObject, { type: "shape" }> =>
        object.type === "shape",
    )
    .map((object) => ({
      id: object.id,
      x: object.x,
      y: object.y,
      rotation: object.rotation,
      scale: object.scale,
      zIndex: object.zIndex,
      readingOrder: object.readingOrder,
      geometry: object.geometry,
      style: object.style,
    }));
}

export function strokesFromSnapshot(snapshot: Pick<SnapshotLike, "inkLayers">): Stroke[] {
  if (snapshot.inkLayers.length === 1) return snapshot.inkLayers[0].strokes;
  return snapshot.inkLayers.flatMap((layer) => layer.strokes);
}

/** Rendered ink is flat; saving restores existing ownership and puts new IDs in the drawing layer. */
export function inkLayersWithStrokes(layers: InkLayer[], strokes: Stroke[], drawingLayerId: string): InkLayer[] {
  const owners = new Map(layers.flatMap(layer => layer.strokes.map(stroke => [stroke.id, layer.id] as const)));
  const grouped = new Map(layers.map(layer => [layer.id, [] as Stroke[]]));
  for (const stroke of strokes) grouped.get(owners.get(stroke.id) ?? drawingLayerId)?.push(stroke);
  return layers.map(layer => ({ ...layer, strokes: grouped.get(layer.id)! }));
}

/** Keep display caches when a fresh Rust confirmation contains exactly the same ink. */
export function reuseStrokeObjects(incoming: Stroke[], previous: Stroke[]): Stroke[] {
  if (!previous.length || incoming === previous) return incoming;
  const known = new Map(previous.map(stroke => [stroke.id, stroke]));
  const sameFields = (a: object, b: object, skip: string[] = []) => {
    const keys = Object.keys(a).filter(key => !skip.includes(key));
    return keys.length === Object.keys(b).filter(key => !skip.includes(key)).length &&
      keys.every(key => Object.prototype.hasOwnProperty.call(b, key) && Object.is(Reflect.get(a, key), Reflect.get(b, key)));
  };
  return incoming.map(stroke => {
    const old = known.get(stroke.id);
    if (!old || old.points.length !== stroke.points.length ||
      !sameFields(old, stroke, ["points", "transform"]) || !sameFields(old.transform, stroke.transform)) return stroke;
    for (let i = 0; i < stroke.points.length; i += 1) {
      const a = old.points[i], b = stroke.points[i];
      if (!Object.is(a.x, b.x) || !Object.is(a.y, b.y) || !Object.is(a.pressure, b.pressure) ||
        !Object.is(a.timeMs, b.timeMs) || !Object.is(a.tiltX, b.tiltX) || !Object.is(a.tiltY, b.tiltY)) return stroke;
    }
    return old;
  });
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
