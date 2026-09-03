// ponytail: manually mirror the Phase 0 Rust model; generate it only if measured drift becomes a problem.
export type NotebookManifest = {
  schemaVersion: number;
  id: string;
  title: string;
  pages: PageReference[];
  defaultPage: PageDefaults;
  sharedStylePath: string | null;
  createdAt: string;
  modifiedAt: string;
};

/**
 * `geometry` is a layout hint, not the truth — the page file is authoritative. Pages load
 * lazily, and the scroller has to reserve the right amount of room for one it has not read yet.
 */
export type PageReference = { id: string; path: string; geometry: PageGeometry };
export type PageGeometry = { widthPt: number; heightPt: number };
export type PageDefaults = { geometry: PageGeometry; background: PageBackground };
export type PageBackground =
  | { kind: "plain"; color: string }
  | { kind: "pdf"; sourcePath: string; page: number }
  | { kind: "template"; template: PageTemplate };

/**
 * Ruled, dotted, or squared paper, described as repeat rules rather than drawn. Mirrors
 * `goodtype_core::template`; the resolver that turns it into geometry is in `page/template.ts`.
 *
 * The definition is stored on the page rather than referenced by id, so a notebook opened on a
 * machine that never had this template still looks like itself.
 */
export type PageTemplate = {
  id: string;
  name: string;
  backgroundColor: string;
  elements: TemplateElement[];
};

/**
 * The rectangle an element lives in, as insets from each page edge. Carried per element because
 * that is what the layouts need — a legal pad wants a wider left inset than the rest, and
 * Cornell's ruling has to start at the cue column and stop at the summary band.
 */
export type Area = { topPt: number; rightPt: number; bottomPt: number; leftPt: number };

export type TemplateElement =
  | { kind: "horizontal_lines"; area: Area; spacingPt: number; color: string; weightPt: number }
  | { kind: "vertical_lines"; area: Area; spacingPt: number; color: string; weightPt: number }
  /**
   * Squared paper: both axes at once, so each rule can run from the first line of the other axis
   * to the last and close the grid's corners. Two independent line sets cannot — they overshoot
   * each other and leave a stub cell all the way round the edge.
   */
  | {
      kind: "grid";
      area: Area;
      spacingPt: number;
      color: string;
      weightPt: number;
      /** Every n-th line from the centre drawn heavier. Null for plain squares. */
      major: GridMajor | null;
    }
  | { kind: "dots"; area: Area; spacingPt: number; color: string; radiusPt: number }
  | { kind: "rule"; area: Area; edge: TemplateEdge; offsetPt: number; color: string; weightPt: number };

export type GridMajor = { every: number; color: string; weightPt: number };

/**
 * What a rule's offset is measured from. The centre variants exist because a column divider has
 * to stay in the middle whatever the page size, and a fixed distance from an edge cannot say
 * that.
 */
export type TemplateEdge = "left" | "right" | "top" | "bottom" | "center_x" | "center_y";

/**
 * Where `create_page` puts the new page. Not part of the stored format — it is an argument to
 * the command, mirroring `storage::PagePosition`.
 */
export type PagePosition =
  | { kind: "before"; pageId: string }
  | { kind: "after"; pageId: string }
  | { kind: "last" };

export type Page = {
  schemaVersion: number;
  id: string;
  revision: number;
  geometry: PageGeometry;
  background: PageBackground;
  objects: PageObject[];
  readingOrder: string[];
  inkLayers: InkLayerReference[];
};

/// The oldest notebook this build still reads and writes.
export const MIN_SUPPORTED_SCHEMA_VERSION = 1;
/// Shapes are what version 2 added. Mirrors `SHAPE_SCHEMA_VERSION` in goodtype-core.
export const SHAPE_SCHEMA_VERSION = 2;
export const SCHEMA_VERSION = 2;

/**
 * The version a page's content obliges its notebook to be written at.
 *
 * A notebook is not upgraded for being opened, nor for being saved after an unrelated edit: it
 * rises only when it stores something the new version introduced. The store applies the same
 * rule on commit and is the authority; this keeps the snapshot the app builds agreeing with it.
 */
export function requiredSchemaVersion(objects: readonly PageObject[]): number {
  return objects.some((object) => object.type === "shape")
    ? SHAPE_SCHEMA_VERSION
    : MIN_SUPPORTED_SCHEMA_VERSION;
}
export const DEFAULT_INK_Z_INDEX = 1_000_000;

export type InkLayerReference = { id: string; path: string };

export type ObjectFields = {
  id: string;
  x: number;
  y: number;
  rotation: number;
  scale: number;
  zIndex: number;
  readingOrder: number;
  groupId: string | null;
  createdAt: string;
  modifiedAt: string;
};

export type ShapePoint = { x: number; y: number };

/** Bézier handles are vectors relative to the knot, matching the canonical Rust model. */
export type BezierNode = {
  point: ShapePoint;
  handleIn: ShapePoint | null;
  handleOut: ShapePoint | null;
};

export type ShapeGeometry =
  | { kind: "line"; start: ShapePoint; end: ShapePoint }
  | { kind: "rectangle"; widthPt: number; heightPt: number; cornerRadiusPt: number }
  | { kind: "ellipse"; widthPt: number; heightPt: number }
  | { kind: "spline"; nodes: BezierNode[]; closed: boolean };

export type ShapeStyle = {
  strokeColor: string;
  strokeWidthPt: number;
  fillColor: string | null;
  opacity: number;
};

export type PageObject =
  | (ObjectFields & {
      type: "typst";
      sourcePath: string;
      layoutWidthPt: number;
      measuredWidthPt: number;
      measuredHeightPt: number;
    })
  | (ObjectFields & {
      /** Fixed page writing surface; deliberately has no canvas transform controls. */
      type: "page_typst";
      sourcePath: string;
    })
  | (ObjectFields & {
      type: "image";
      sourcePath: string;
      widthPt: number;
      heightPt: number;
      altText: string;
    })
  | (ObjectFields & {
      type: "pdf_material";
      sourcePath: string;
      page: number;
      sourceWidthPt: number;
      sourceHeightPt: number;
    })
  | (ObjectFields & {
      type: "ink_group";
      inkLayerId: string;
      strokeIds: string[];
    })
  | (ObjectFields & {
      type: "shape";
      geometry: ShapeGeometry;
      style: ShapeStyle;
    })
  | (ObjectFields & { type: "group"; childIds: string[] });

export type InkLayer = {
  schemaVersion: number;
  id: string;
  pageId: string;
  strokes: Stroke[];
};

export type Stroke = {
  id: string;
  /** Shared visual order with movable objects; absent legacy values paint above them. */
  zIndex?: number;
  tool: "pen" | "highlighter";
  color: string;
  widthPt: number;
  /**
   * Whether this stroke's width followed stylus pressure. Resolved from the nib at draw time and
   * then stored, never inferred from `tool` — nibs differ, and export must not re-decide.
   */
  pressure: boolean;
  /** Fraction of the stroke's length over which each end tapers to a point; 0 disables it. */
  taper: number;
  /** Ink opacity, 0–1. A highlighter sweep is translucent where a pen is not. */
  opacity: number;
  groupId: string | null;
  points: StrokePoint[];
  transform: Transform;
};

export type StrokePoint = {
  x: number;
  y: number;
  pressure: number;
  timeMs: number;
  tiltX: number;
  tiltY: number;
};

export type Transform = {
  translateX: number;
  translateY: number;
  scaleX: number;
  scaleY: number;
  rotation: number;
};
