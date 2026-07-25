// ponytail: manually mirror the Phase 0 Rust model; generate it only if measured drift becomes a problem.
export const SCHEMA_VERSION = 1;

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

export type PageReference = { id: string; path: string };
export type PageGeometry = { widthPt: number; heightPt: number };
export type PageDefaults = { geometry: PageGeometry; background: PageBackground };
export type PageBackground =
  | { kind: "plain"; color: string }
  | { kind: "pdf"; sourcePath: string; page: number };

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

export type PageObject =
  | (ObjectFields & {
      type: "typst";
      sourcePath: string;
      layoutWidthPt: number;
      measuredWidthPt: number;
      measuredHeightPt: number;
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
  | (ObjectFields & { type: "group"; childIds: string[] });

export type InkLayer = {
  schemaVersion: number;
  id: string;
  pageId: string;
  strokes: Stroke[];
};

export type Stroke = {
  id: string;
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
