// The page sizes Goodtype offers, in points.
//
// Exact rather than rounded. A4 spent a while as 595x842, which is a tenth of a millimetre short
// in each direction — invisible until a template promises 5mm squares measured against it, and
// then it is a hundredth of a square's error accumulated across the page.
//
// Sizes are portrait; landscape is a swap rather than fourteen more entries.

import type { PageGeometry } from "../model";

const MM = 72 / 25.4;
const IN = 72;

export type PageSize = {
  id: string;
  name: string;
  /** What it is, for anyone who does not think in A-numbers. */
  detail: string;
  widthPt: number;
  heightPt: number;
};

export type Orientation = "portrait" | "landscape";

/** ISO A sizes halve along the long edge, so each is the previous one folded once. */
export const PAGE_SIZES: PageSize[] = [
  { id: "a6", name: "A6", detail: "105 × 148 mm", widthPt: 105 * MM, heightPt: 148 * MM },
  { id: "a5", name: "A5", detail: "148 × 210 mm", widthPt: 148 * MM, heightPt: 210 * MM },
  { id: "a4", name: "A4", detail: "210 × 297 mm", widthPt: 210 * MM, heightPt: 297 * MM },
  { id: "a3", name: "A3", detail: "297 × 420 mm", widthPt: 297 * MM, heightPt: 420 * MM },
  { id: "letter", name: "Letter", detail: "8.5 × 11 in", widthPt: 8.5 * IN, heightPt: 11 * IN },
  { id: "legal", name: "Legal", detail: "8.5 × 14 in", widthPt: 8.5 * IN, heightPt: 14 * IN },
  { id: "tabloid", name: "Tabloid", detail: "11 × 17 in", widthPt: 11 * IN, heightPt: 17 * IN },
];

export const DEFAULT_PAGE_SIZE = PAGE_SIZES.find((size) => size.id === "a4") ?? PAGE_SIZES[2];

export function geometryOf(size: PageSize, orientation: Orientation): PageGeometry {
  return orientation === "landscape"
    ? { widthPt: size.heightPt, heightPt: size.widthPt }
    : { widthPt: size.widthPt, heightPt: size.heightPt };
}

/**
 * Which listed size a geometry is, if any. Used to show what the current page already is —
 * a page may carry a size nothing in this list matches, and that is not an error.
 */
export function sizeOf(geometry: PageGeometry): { size: PageSize; orientation: Orientation } | null {
  const near = (a: number, b: number) => Math.abs(a - b) < 0.5;
  for (const size of PAGE_SIZES) {
    if (near(geometry.widthPt, size.widthPt) && near(geometry.heightPt, size.heightPt)) {
      return { size, orientation: "portrait" };
    }
    if (near(geometry.widthPt, size.heightPt) && near(geometry.heightPt, size.widthPt)) {
      return { size, orientation: "landscape" };
    }
  }
  return null;
}

/** How a geometry reads in the menu, for pages whose size is not one of the listed ones. */
export function describeGeometry(geometry: PageGeometry): string {
  const known = sizeOf(geometry);
  if (known) {
    return known.orientation === "landscape" ? `${known.size.name} landscape` : known.size.name;
  }
  const mm = (value: number) => Math.round(value / MM);
  return `${mm(geometry.widthPt)} × ${mm(geometry.heightPt)} mm`;
}
