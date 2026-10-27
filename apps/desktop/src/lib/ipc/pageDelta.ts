import type { InkLayer, Stroke } from "../model";
import type { NotebookSnapshot, StoredFile } from "./types";

type StrokeDelta = { kind: "reuse"; id: string; transform?: Stroke["transform"] }
  | { kind: "put"; stroke: Stroke };
export type PageDelta = {
  page: NotebookSnapshot["page"];
  modifiedAt: string;
  blocks: StoredFile[];
  assets: StoredFile[];
  knownAssets: Record<string, string>;
  inkLayers: (Omit<InkLayer, "strokes"> & { strokes: StrokeDelta[] })[];
};

function sameFields(left: object, right: object): boolean {
  const a = left as Record<string, unknown>, b = right as Record<string, unknown>;
  return [...new Set([...Object.keys(a), ...Object.keys(b)])].every(key => Object.is(a[key], b[key]));
}

function strokeDelta(stroke: Stroke, previous?: Stroke): StrokeDelta {
  if (stroke === previous) return { kind: "reuse", id: stroke.id };
  if (previous) {
    const { points, transform, ...fields } = stroke;
    const { points: oldPoints, transform: oldTransform, ...oldFields } = previous;
    if (sameFields(fields, oldFields) && (points === oldPoints ||
      (points.length === oldPoints.length && points.every((point, index) => sameFields(point, oldPoints[index]))))) {
      return { kind: "reuse", id: stroke.id,
        ...(sameFields(transform, oldTransform) ? {} : { transform }) };
    }
  }
  return { kind: "put", stroke };
}

export function pageDelta(snapshot: NotebookSnapshot, base: NotebookSnapshot): PageDelta {
  const blocks = new Map(base.blocks.map(file => [file.path, file.bytes]));
  const layers = new Map(base.inkLayers.map(layer => [layer.id, new Map(layer.strokes.map(stroke => [stroke.id, stroke]))]));
  return {
    page: snapshot.page, modifiedAt: snapshot.manifest.modifiedAt,
    blocks: snapshot.blocks.filter(file => {
      const old = blocks.get(file.path);
      return !old || old.length !== file.bytes.length || file.bytes.some((byte, index) => byte !== old[index]);
    }),
    assets: snapshot.assets,
    knownAssets: {},
    inkLayers: snapshot.inkLayers.map(({ strokes, ...layer }) => ({ ...layer,
      strokes: strokes.map(stroke => strokeDelta(stroke, layers.get(layer.id)?.get(stroke.id))),
    })),
  };
}

const assetHashes = new WeakMap<number[], Promise<string>>();
export async function knownAssets(files: StoredFile[]): Promise<Record<string, string>> {
  if (!globalThis.crypto?.subtle) return {};
  const entries = await Promise.all(files.map(async file => {
    let hash = assetHashes.get(file.bytes);
    if (!hash) {
      hash = crypto.subtle.digest("SHA-256", new Uint8Array(file.bytes)).then(buffer =>
        [...new Uint8Array(buffer)].map(byte => byte.toString(16).padStart(2, "0")).join("")).catch(() => "");
      assetHashes.set(file.bytes, hash);
    }
    return [file.path, await hash] as const;
  }));
  return Object.fromEntries(entries);
}
