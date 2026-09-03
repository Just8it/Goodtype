import type { NotebookSnapshot } from "../ipc/types";
import { MIN_SUPPORTED_SCHEMA_VERSION, type NotebookManifest } from "../model";
import type { NotebookSetup } from "../page/presets";
import { projectSnapshot } from "./snapshot";

export function blankNotebookSnapshot(
  setup: NotebookSetup,
  now = new Date().toISOString(),
): NotebookSnapshot {
  const pageId = "page-001";
  const manifest: NotebookManifest = {
    // A blank notebook holds nothing that needs version 2, so it starts at the version the
    // widest range of builds can open. It rises the first time it stores a shape.
    schemaVersion: MIN_SUPPORTED_SCHEMA_VERSION,
    id: `notebook-${Date.now().toString(36)}`,
    title: setup.name,
    pages: [{ id: pageId, path: "pages/page-001.json", geometry: setup.geometry }],
    defaultPage: { geometry: setup.geometry, background: setup.background },
    sharedStylePath: null,
    createdAt: now,
    modifiedAt: now,
  };
  return projectSnapshot({
    base: null,
    manifest,
    pageId,
    revision: 1,
    geometry: setup.geometry,
    background: setup.background,
    inkLayerId: "ink-layer-001",
    inkLayerPath: "ink/page-001-layer-001.json",
    strokes: [],
    typst: [],
    pageTypst: null,
    images: [],
    shapes: [],
    sharedStyle: null,
    mixedGroup: null,
    groupedStrokeIds: [],
    now,
  });
}
