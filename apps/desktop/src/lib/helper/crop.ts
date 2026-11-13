import { selectionBounds } from "../ink/selection";
import type { PageObject, Stroke } from "../model";
import { boxOutline, combinedBounds, shapeLassoItem, type LassoItem } from "../notebook/lasso";
import type { ImageView, ShapeView, TypstBlockState } from "../notebook/pageView";
import type { CropPt } from "../ipc/helper";
export function selectionCrop(ids: string[], strokes: Stroke[], images: ImageView[], shapes: ShapeView[], blocks: TypstBlockState[], page?: { widthPt: number; heightPt: number }, materials: PageObject[] = []): CropPt | null {
  const selected = new Set(ids);
  const items: LassoItem[] = [
    ...materialItems(materials).filter(s => selected.has(s.id)),
    ...shapes.filter(s => selected.has(s.id)).map(shapeLassoItem),
    ...images.filter(s => selected.has(s.id)).map(s => ({ id: s.id, kind: "images" as const, closed: true, outline: boxOutline(s.x, s.y, s.widthPt * s.scale, s.heightPt * s.scale, s.rotation) })),
    ...blocks.filter(s => selected.has(s.id)).map(s => ({ id: s.id, kind: "text" as const, closed: true, outline: boxOutline(s.transform.x, s.transform.y, (s.result?.widthPt ?? s.transform.layoutWidthPt) * s.transform.scale, (s.result?.heightPt ?? 64) * s.transform.scale, s.transform.rotation) })),
  ];
  const b = combinedBounds(items, selectionBounds(strokes, ids));
  if (b && page) {
    b.left = Math.max(0, b.left); b.top = Math.max(0, b.top);
    b.right = Math.min(page.widthPt, b.right); b.bottom = Math.min(page.heightPt, b.bottom);
    if (![b.left, b.top, b.right, b.bottom].every(Number.isFinite) || b.right <= b.left || b.bottom <= b.top) return null;
    return { x: b.left, y: b.top, width: b.right - b.left, height: b.bottom - b.top };
  }
  return b ? { x: b.left, y: b.top, width: Math.max(1, b.right - b.left), height: Math.max(1, b.bottom - b.top) } : null;
}

export function materialItems(objects: PageObject[]): LassoItem[] {
  return objects.filter(o => o.type === "pdf_material").map(o => ({ id: o.id, kind: "images", closed: true,
    outline: boxOutline(o.x, o.y, o.type === "pdf_material" ? o.sourceWidthPt * o.scale : 0, o.type === "pdf_material" ? o.sourceHeightPt * o.scale : 0, o.rotation) }));
}
export function clampCrop(crop: CropPt, page: { widthPt: number; heightPt: number }): CropPt | null {
  if (![crop.x,crop.y,crop.width,crop.height].every(Number.isFinite)) return null;
  const x = Math.max(0, crop.x), y = Math.max(0, crop.y);
  const width = Math.min(page.widthPt, crop.x + crop.width) - x, height = Math.min(page.heightPt, crop.y + crop.height) - y;
  return [x,y,width,height].every(Number.isFinite) && width > 0 && height > 0 ? { x,y,width,height } : null;
}
export function pdfRegion(points: {x:number;y:number}[], page: {widthPt:number;heightPt:number}, pdf: boolean, materials: PageObject[]): CropPt | null {
  const b = combinedBounds([{id:"region",kind:"images",closed:true,outline:points}], null);
  if (!b) return null;
  const crop = clampCrop({x:b.left,y:b.top,width:b.right-b.left,height:b.bottom-b.top},page);
  if (!crop) return null;
  return pdf || materialItems(materials).some(item => {
    const m = combinedBounds([item],null)!;
    return m.right > crop.x && m.left < crop.x+crop.width && m.bottom > crop.y && m.top < crop.y+crop.height;
  }) ? crop : null;
}

export function selectedObjectTypes(objects: PageObject[], ids: string[]): string[] {
  const selected = new Set(ids);
  for (let changed = true; changed;) {
    changed = false;
    for (const object of objects) if (selected.has(object.id) && object.type === "group") {
      for (const child of object.childIds) if (!selected.has(child)) { selected.add(child); changed = true; }
    }
  }
  return objects.filter(object => selected.has(object.id)).map(object => object.type);
}

/** A crop gesture owns its draft; callers commit this result only on release. */
export function resizeCrop(base: CropPt, corner: number, delta: {x:number;y:number}, page: {widthPt:number;heightPt:number}): CropPt | null {
  const left = corner % 2 === 0, top = corner < 2;
  const x = left ? Math.min(base.x + base.width - 1, Math.max(0, base.x + delta.x)) : base.x;
  const y = top ? Math.min(base.y + base.height - 1, Math.max(0, base.y + delta.y)) : base.y;
  return clampCrop({x,y,width:left ? base.x+base.width-x : Math.max(1,base.width+delta.x),height:top ? base.y+base.height-y : Math.max(1,base.height+delta.y)},page);
}
