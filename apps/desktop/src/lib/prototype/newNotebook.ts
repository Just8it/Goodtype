import type { NotebookSnapshot } from "../ipc/types";
import { SCHEMA_VERSION, type NotebookManifest } from "../model";
import type { NotebookSetup } from "../page/presets";
import { projectSnapshot } from "./snapshot";

export function blankNotebookSnapshot(
  setup: NotebookSetup,
  now = new Date().toISOString(),
): NotebookSnapshot {
  const pageId = "page-001";
  const manifest: NotebookManifest = {
    // New notebooks start on the current format so their first ink layer is binary immediately.
    schemaVersion: SCHEMA_VERSION,
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
    inkLayerPath: "ink/page-001-layer-001.ink",
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
