import { describe, expect, it } from "vitest";
import type { NotebookSnapshot } from "../ipc/types";
import type { NotebookManifest, ObjectFields, PageObject, Stroke } from "../model";
import { projectSnapshot } from "./snapshot";

const now = "2026-07-28T12:00:00Z";
const fields = (id: string, order: number): ObjectFields => ({
  id,
  x: order * 10,
  y: order * 20,
  rotation: order,
  scale: 1,
  zIndex: order + 4,
  readingOrder: order,
  groupId: null,
  createdAt: "2026-01-01T00:00:00Z",
  modifiedAt: "2026-01-01T00:00:00Z",
});
const stroke = (id: string): Stroke => ({
  id,
  tool: "pen",
  color: "#111111",
  widthPt: 1,
  pressure: false,
  taper: 0,
  opacity: 1,
  groupId: null,
  points: [],
  transform: { translateX: 0, translateY: 0, scaleX: 1, scaleY: 1, rotation: 0 },
});

describe("projectSnapshot", () => {
  it("patches editable objects without dropping canonical material or extra ink layers", () => {
    const objects: PageObject[] = [
      {
        ...fields("typst-a", 0),
        type: "typst",
        sourcePath: "blocks/a.typ",
        layoutWidthPt: 200,
        measuredWidthPt: 180,
        measuredHeightPt: 40,
      },
      {
        ...fields("image-a", 1),
        type: "image",
        sourcePath: "assets/a.png",
        widthPt: 100,
        heightPt: 80,
        altText: "original",
      },
      {
        ...fields("pdf-a", 2),
        type: "pdf_material",
        sourcePath: "assets/source.pdf",
        page: 3,
        sourceWidthPt: 612,
        sourceHeightPt: 792,
      },
    ];
    const base: NotebookSnapshot = {
      manifest: {
        schemaVersion: 1,
        id: "notebook",
        title: "Notebook",
        pages: [
          {
            id: "page-a",
            path: "pages/page-a.json",
            geometry: { widthPt: 595, heightPt: 842 },
          },
        ],
        defaultPage: {
          geometry: { widthPt: 595, heightPt: 842 },
          background: { kind: "plain", color: "#ffffff" },
        },
        sharedStylePath: "shared.typ",
        createdAt: now,
        modifiedAt: now,
      },
      page: {
        schemaVersion: 1,
        id: "page-a",
        revision: 7,
        geometry: { widthPt: 595, heightPt: 842 },
        background: { kind: "plain", color: "#ffffff" },
        objects,
        readingOrder: objects.map((object) => object.id),
        inkLayers: [
          { id: "ink-a", path: "ink/a.json" },
          { id: "ink-b", path: "ink/b.json" },
        ],
      },
      blocks: [
        { path: "shared.typ", bytes: [35, 108, 101, 116] },
        { path: "blocks/a.typ", bytes: [111, 108, 100] },
      ],
      assets: [],
      inkLayers: [
        { schemaVersion: 1, id: "ink-a", pageId: "page-a", strokes: [stroke("old")] },
        { schemaVersion: 1, id: "ink-b", pageId: "page-a", strokes: [stroke("keep")] },
      ],
    };

    const projected = projectSnapshot({
      base,
      manifest: base.manifest,
      pageId: "page-a",
      revision: 7,
      geometry: base.page.geometry,
      background: base.page.background,
      inkLayerId: "ink-a",
      inkLayerPath: "ink/a.json",
      strokes: [stroke("new"), ...base.inkLayers[1].strokes],
      typst: [
        {
          id: "typst-a",
          path: "blocks/a.typ",
          source: "new",
          x: 44,
          y: 55,
          layoutWidthPt: 240,
          scale: 1.2,
          measuredWidthPt: 220,
          measuredHeightPt: 60,
          zIndex: 4,
          readingOrder: 0,
        },
      ],
      pageTypst: null,
      images: [
        {
          id: "image-a",
          path: "assets/a.png",
          alt: "updated",
          x: 66,
          y: 77,
          widthPt: 100,
          heightPt: 80,
          scale: 1.1,
          zIndex: 5,
          readingOrder: 1,
        },
      ],
      shapes: [
        {
          id: "shape-a",
          x: 90,
          y: 110,
          rotation: 0,
          scale: 1,
          zIndex: 8,
          readingOrder: 3,
          geometry: { kind: "ellipse", widthPt: 70, heightPt: 40 },
          style: {
            strokeColor: "#16212b",
            strokeWidthPt: 1.6,
            fillColor: null,
            opacity: 1,
          },
        },
      ],
      sharedStyle: { path: "shared.typ", source: "#let" },
      mixedGroup: null,
      groupedStrokeIds: [],
      now,
    });

    expect(projected.page.objects.find((object) => object.id === "pdf-a")).toEqual(objects[2]);
    expect(projected.page.objects.find((object) => object.id === "image-a")).toMatchObject({
      rotation: 1,
      zIndex: 5,
      createdAt: "2026-01-01T00:00:00Z",
      altText: "updated",
    });
    expect(projected.page.objects.find((object) => object.id === "shape-a")).toMatchObject({
      type: "shape",
      x: 90,
      geometry: { kind: "ellipse", widthPt: 70, heightPt: 40 },
    });
    expect(projected.inkLayers[0].strokes.map(({ id }) => id)).toEqual(["new"]);
    expect(projected.manifest.schemaVersion).toBe(2);
    expect(projected.page.schemaVersion).toBe(2);
    expect(projected.inkLayers[1]).toEqual({ ...base.inkLayers[1], schemaVersion: 2 });
    expect(projected.blocks.find(({ path }) => path === "shared.typ")).toEqual(base.blocks[0]);
  });

  it("restamps modifiedAt only on objects whose fields or Typst source changed", () => {
    const earlier = "2026-01-01T00:00:00Z";
    const typst: PageObject = { ...fields("typst-a", 0), type: "typst", sourcePath: "blocks/a.typ",
      layoutWidthPt: 200, measuredWidthPt: 180, measuredHeightPt: 40 };
    const image: PageObject = { ...fields("image-a", 1), type: "image", sourcePath: "assets/a.png",
      widthPt: 100, heightPt: 80, altText: "kept" };
    const base: NotebookSnapshot = {
      manifest: { schemaVersion: 1, id: "notebook", title: "Notebook",
        pages: [{ id: "page-a", path: "pages/page-a.json", geometry: { widthPt: 595, heightPt: 842 } }],
        defaultPage: { geometry: { widthPt: 595, heightPt: 842 }, background: { kind: "plain", color: "#ffffff" } },
        sharedStylePath: null, createdAt: earlier, modifiedAt: earlier },
      page: { schemaVersion: 1, id: "page-a", revision: 3, geometry: { widthPt: 595, heightPt: 842 },
        background: { kind: "plain", color: "#ffffff" }, objects: [typst, image],
        readingOrder: ["typst-a", "image-a"], inkLayers: [{ id: "ink-a", path: "ink/a.ink" }] },
      blocks: [{ path: "blocks/a.typ", bytes: Array.from(new TextEncoder().encode("old")) }],
      assets: [],
      inkLayers: [{ schemaVersion: 1, id: "ink-a", pageId: "page-a", strokes: [] }],
    };
    const project = (source: string, imageX: number) => projectSnapshot({
      base, manifest: base.manifest, pageId: "page-a", revision: 3,
      geometry: base.page.geometry, background: base.page.background,
      inkLayerId: "ink-a", inkLayerPath: "ink/a.ink", strokes: [stroke("new")],
      typst: [{ id: "typst-a", path: "blocks/a.typ", source, x: 0, y: 0, rotation: 0, layoutWidthPt: 200,
        scale: 1, measuredWidthPt: 180, measuredHeightPt: 40, zIndex: 4, readingOrder: 0 }],
      pageTypst: null,
      images: [{ id: "image-a", path: "assets/a.png", alt: "kept", x: imageX, y: 20, rotation: 1,
        widthPt: 100, heightPt: 80, scale: 1, zIndex: 5, readingOrder: 1 }],
      shapes: [], sharedStyle: null, mixedGroup: null, groupedStrokeIds: [], now,
    }).page.objects;
    const stamp = (objects: PageObject[], id: string) => objects.find((object) => object.id === id)?.modifiedAt;

    const strokeOnly = project("old", 10);
    expect(stamp(strokeOnly, "typst-a")).toBe(earlier);
    expect(stamp(strokeOnly, "image-a")).toBe(earlier);

    const edited = project("new", 30);
    expect(stamp(edited, "typst-a")).toBe(now);
    expect(stamp(edited, "image-a")).toBe(now);
  });
});

describe("schema version follows content, not the act of saving", () => {
  const emptyProjection = (manifest: NotebookManifest) => ({
    base: null,
    manifest,
    pageId: "page-001",
    revision: 1,
    geometry: { widthPt: 595, heightPt: 842 },
    background: { kind: "plain" as const, color: "#ffffff" },
    inkLayerId: "ink-001",
    inkLayerPath: "ink/page-001-layer-001.json",
    strokes: [],
    typst: [],
    pageTypst: null,
    images: [],
    shapes: [],
    sharedStyle: null,
    mixedGroup: null,
    groupedStrokeIds: [],
    now: "2026-09-03T00:00:00Z",
  });

  const manifest: NotebookManifest = {
    schemaVersion: 1,
    id: "notebook-1",
    title: "Older notebook",
    pages: [
      { id: "page-001", path: "pages/page-001.json", geometry: { widthPt: 595, heightPt: 842 } },
    ],
    defaultPage: {
      geometry: { widthPt: 595, heightPt: 842 },
      background: { kind: "plain", color: "#ffffff" },
    },
    sharedStylePath: null,
    createdAt: "2026-01-01T00:00:00Z",
    modifiedAt: "2026-01-01T00:00:00Z",
  };

  it("leaves a version-1 notebook at version 1 when nothing needs more", () => {
    const projected = projectSnapshot(emptyProjection(manifest));
    expect(projected.manifest.schemaVersion).toBe(1);
    expect(projected.page.schemaVersion).toBe(1);
    expect(projected.inkLayers[0].schemaVersion).toBe(1);
  });

  it("creates a referenced drawing layer for a valid page that previously had no ink", () => {
    const base = projectSnapshot(emptyProjection(manifest));
    base.page.inkLayers = [];
    base.inkLayers = [];
    const projected = projectSnapshot({ ...emptyProjection(manifest), base, strokes: [stroke("first-ink")] });
    expect(projected.page.inkLayers).toHaveLength(1);
    expect(projected.page.inkLayers[0].id).toBe(projected.inkLayers[0].id);
    expect(projected.inkLayers[0].strokes[0].id).toBe("first-ink");
  });

  it("raises it the moment the page holds a shape", () => {
    const projected = projectSnapshot({
      ...emptyProjection(manifest),
      shapes: [
        {
          id: "shape-a",
          x: 10,
          y: 10,
          rotation: 0,
          scale: 1,
          zIndex: 1,
          readingOrder: 0,
          geometry: { kind: "ellipse", widthPt: 40, heightPt: 20 },
          style: { strokeColor: "#16212b", strokeWidthPt: 1.6, fillColor: null, opacity: 1 },
        },
      ],
    });
    expect(projected.manifest.schemaVersion).toBe(2);
    expect(projected.page.schemaVersion).toBe(2);
    expect(projected.inkLayers[0].schemaVersion).toBe(2);
  });
});
