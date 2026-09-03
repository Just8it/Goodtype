import {
  MIN_SUPPORTED_SCHEMA_VERSION,
  requiredSchemaVersion,
  type InkLayer,
  type NotebookManifest,
  type Page,
  type PageBackground,
  type PageGeometry,
  type PageObject,
  type Stroke,
} from "../model";
import type { NotebookSnapshot } from "../ipc/types";
import type { BlockView, ImageView, PageTypstView, ShapeView } from "./pageView";

export type EditableTypst = BlockView & {
  measuredWidthPt: number;
  measuredHeightPt: number;
};

export type EditablePageTypst = PageTypstView;

export type EditableImage = Omit<ImageView, "url">;

export type ManagedMixedGroup = {
  inkGroupId: string;
  groupId: string;
  typstId: string;
  active: boolean;
};

type Projection = {
  base: NotebookSnapshot | null;
  manifest: NotebookManifest;
  pageId: string;
  revision: number;
  geometry: PageGeometry;
  background: PageBackground;
  inkLayerId: string;
  inkLayerPath: string;
  strokes: Stroke[];
  typst: EditableTypst[];
  pageTypst: EditablePageTypst | null;
  images: EditableImage[];
  shapes: ShapeView[];
  sharedStyle: { path: string; source: string } | null;
  mixedGroup: ManagedMixedGroup | null;
  groupedStrokeIds: string[];
  now: string;
};

/**
 * Project the live controls onto a canonical page without making the frontend a second model.
 *
 * Objects Goodtype cannot currently edit are copied byte-for-byte at the model level. Editable
 * variants are patched by stable ID, and only explicitly removed editable objects disappear.
 * This is deliberately a pure function so a duplicated or future-valid page can be regression
 * tested without mounting the 3,000-line workspace.
 */
export function projectSnapshot(input: Projection): NotebookSnapshot {
  const basePage = input.base?.page;
  const typstById = new Map(input.typst.map((block) => [block.id, block]));
  const imageById = new Map(input.images.map((image) => [image.id, image]));
  const shapeById = new Map(input.shapes.map((shape) => [shape.id, shape]));
  const oldObjects = basePage?.objects ?? [];
  const oldById = new Map(oldObjects.map((object) => [object.id, object]));
  const emitted = new Set<string>();
  const objects: PageObject[] = [];

  for (const object of oldObjects) {
    if (input.mixedGroup && object.id === input.mixedGroup.inkGroupId) continue;
    if (input.mixedGroup && object.id === input.mixedGroup.groupId) continue;

    if (object.type === "typst") {
      const block = typstById.get(object.id);
      if (!block) continue;
      emitted.add(object.id);
      objects.push(patchTypst(object, block, input.mixedGroup, input.now));
      continue;
    }
    if (object.type === "page_typst") {
      if (!input.pageTypst || input.pageTypst.id !== object.id) continue;
      emitted.add(object.id);
      objects.push(patchPageTypst(object, input.pageTypst, input.now));
      continue;
    }
    if (object.type === "image") {
      const image = imageById.get(object.id);
      if (!image) continue;
      emitted.add(object.id);
      objects.push(patchImage(object, image, input.now));
      continue;
    }
    if (object.type === "shape") {
      const shape = shapeById.get(object.id);
      if (!shape) continue;
      emitted.add(object.id);
      objects.push(patchShape(object, shape, input.now));
      continue;
    }

    // PDF material, independent ink groups, and general groups are not represented by the
    // current controls. Preserving them is the difference between "not editable yet" and data
    // loss on the next pen stroke.
    emitted.add(object.id);
    objects.push(object);
  }

  for (const block of input.typst) {
    if (emitted.has(block.id)) continue;
    emitted.add(block.id);
    objects.push(newTypst(block, input.mixedGroup, input.now));
  }
  if (input.pageTypst && !emitted.has(input.pageTypst.id)) {
    emitted.add(input.pageTypst.id);
    objects.push(newPageTypst(input.pageTypst, input.now));
  }
  for (const image of input.images) {
    if (emitted.has(image.id)) continue;
    emitted.add(image.id);
    objects.push(newImage(image, input.now));
  }
  for (const shape of input.shapes) {
    if (emitted.has(shape.id)) continue;
    emitted.add(shape.id);
    objects.push(newShape(shape, input.now));
  }

  if (input.mixedGroup?.active) {
    const previousInk = oldById.get(input.mixedGroup.inkGroupId);
    const previousGroup = oldById.get(input.mixedGroup.groupId);
    objects.push({
      ...(previousInk?.type === "ink_group"
        ? previousInk
        : {
            ...newFields(
              input.mixedGroup.inkGroupId,
              objects.length,
              objects.length + 1,
              input.mixedGroup.groupId,
              input.now,
            ),
            type: "ink_group" as const,
            inkLayerId: input.inkLayerId,
            strokeIds: [],
          }),
      groupId: input.mixedGroup.groupId,
      modifiedAt: input.now,
      inkLayerId: input.inkLayerId,
      strokeIds: input.groupedStrokeIds,
    });
    objects.push({
      ...(previousGroup?.type === "group"
        ? previousGroup
        : {
            ...newFields(
              input.mixedGroup.groupId,
              objects.length,
              objects.length + 1,
              null,
              input.now,
            ),
            type: "group" as const,
            childIds: [],
          }),
      groupId: null,
      modifiedAt: input.now,
      childIds: [input.mixedGroup.typstId, input.mixedGroup.inkGroupId],
    });
  }

  const topLevelIds = new Set(
    objects.filter((object) => object.groupId === null).map((object) => object.id),
  );
  const previousReadingOrder = basePage?.readingOrder ?? [];
  let readingOrder = previousReadingOrder.filter((id) => topLevelIds.has(id));
  if (input.mixedGroup?.active) {
    const previousIndex = Math.max(0, previousReadingOrder.indexOf(input.mixedGroup.typstId));
    readingOrder = readingOrder.filter(
      (id) =>
        id !== input.mixedGroup!.typstId &&
        id !== input.mixedGroup!.inkGroupId &&
        id !== input.mixedGroup!.groupId,
    );
    readingOrder.splice(previousIndex, 0, input.mixedGroup.groupId);
  }
  for (const object of objects) {
    if (object.groupId === null && !readingOrder.includes(object.id)) {
      readingOrder.push(object.id);
    }
  }
  if (input.pageTypst) {
    readingOrder = [input.pageTypst.id, ...readingOrder.filter((id) => id !== input.pageTypst!.id)];
  }
  const objectById = new Map(objects.map((object) => [object.id, object]));
  readingOrder.sort(
    (left, right) =>
      (objectById.get(left)?.readingOrder ?? 0) -
      (objectById.get(right)?.readingOrder ?? 0),
  );
  const orderById = new Map(readingOrder.map((id, index) => [id, index]));
  const orderedObjects = objects.map((object) => {
    const order = orderById.get(object.id);
    return order === undefined ? object : { ...object, readingOrder: order };
  });

  // Never below what this page needs, never above what the notebook already is: a shape raises
  // the notebook to version 2, and everything else leaves an older notebook exactly as it found
  // it. `commit_notebook` applies the same rule and has the last word.
  const schemaVersion = Math.max(
    input.manifest.schemaVersion ?? MIN_SUPPORTED_SCHEMA_VERSION,
    requiredSchemaVersion(objects),
  );

  const page: Page = {
    schemaVersion,
    id: input.pageId,
    revision: input.revision,
    geometry: input.geometry,
    background: input.background,
    objects: orderedObjects,
    readingOrder,
    inkLayers:
      basePage?.inkLayers.length
        ? basePage.inkLayers
        : [{ id: input.inkLayerId, path: input.inkLayerPath }],
  };

  const inkLayers = mergeInkLayers(
    schemaVersion,
    input.base?.inkLayers ?? [],
    input.inkLayerId,
    input.pageId,
    input.strokes,
  );
  const editableBlocks = new Map(
    [
      ...input.typst.map((block) => ({ path: block.path, source: block.source })),
      ...(input.pageTypst
        ? [{ path: input.pageTypst.path, source: input.pageTypst.source }]
        : []),
      ...(input.sharedStyle ? [input.sharedStyle] : []),
    ].map((block) => [
      block.path,
      { path: block.path, bytes: Array.from(new TextEncoder().encode(block.source)) },
    ]),
  );
  const blockPaths = new Set([
    ...(input.manifest.sharedStylePath ? [input.manifest.sharedStylePath] : []),
    ...orderedObjects.flatMap((object) =>
      object.type === "typst" || object.type === "page_typst" ? [object.sourcePath] : [],
    ),
  ]);
  const baseBlocks = new Map(input.base?.blocks.map((file) => [file.path, file]) ?? []);
  const blocks = [...blockPaths].flatMap((path) => {
    const file = editableBlocks.get(path) ?? baseBlocks.get(path);
    return file ? [file] : [];
  });

  return {
    manifest: { ...input.manifest, schemaVersion, modifiedAt: input.now },
    page,
    blocks,
    // Originals already exist on disk. The store resolves them and rejects a missing reference.
    assets: [],
    inkLayers,
  };
}

function patchTypst(
  object: Extract<PageObject, { type: "typst" }>,
  block: EditableTypst,
  mixedGroup: ManagedMixedGroup | null,
  now: string,
): Extract<PageObject, { type: "typst" }> {
  const groupId =
    mixedGroup?.typstId === object.id
      ? mixedGroup.active
        ? mixedGroup.groupId
        : null
      : object.groupId;
  return {
    ...object,
    x: block.x,
    y: block.y,
    scale: block.scale,
    zIndex: block.zIndex,
    readingOrder: block.readingOrder,
    groupId,
    modifiedAt: now,
    sourcePath: block.path,
    layoutWidthPt: block.layoutWidthPt,
    measuredWidthPt: block.measuredWidthPt,
    measuredHeightPt: block.measuredHeightPt,
  };
}

function patchPageTypst(
  object: Extract<PageObject, { type: "page_typst" }>,
  pageTypst: EditablePageTypst,
  now: string,
): Extract<PageObject, { type: "page_typst" }> {
  return {
    ...object,
    x: 0,
    y: 0,
    rotation: 0,
    scale: 1,
    zIndex: pageTypst.zIndex,
    readingOrder: pageTypst.readingOrder,
    groupId: null,
    modifiedAt: now,
    sourcePath: pageTypst.path,
  };
}

function patchImage(
  object: Extract<PageObject, { type: "image" }>,
  image: EditableImage,
  now: string,
): Extract<PageObject, { type: "image" }> {
  return {
    ...object,
    x: image.x,
    y: image.y,
    scale: image.scale,
    zIndex: image.zIndex,
    readingOrder: image.readingOrder,
    modifiedAt: now,
    sourcePath: image.path,
    widthPt: image.widthPt,
    heightPt: image.heightPt,
    altText: image.alt,
  };
}

function patchShape(
  object: Extract<PageObject, { type: "shape" }>,
  shape: ShapeView,
  now: string,
): Extract<PageObject, { type: "shape" }> {
  return {
    ...object,
    x: shape.x,
    y: shape.y,
    rotation: shape.rotation,
    scale: shape.scale,
    zIndex: shape.zIndex,
    readingOrder: shape.readingOrder,
    modifiedAt: now,
    geometry: shape.geometry,
    style: shape.style,
  };
}

function newTypst(
  block: EditableTypst,
  mixedGroup: ManagedMixedGroup | null,
  now: string,
): Extract<PageObject, { type: "typst" }> {
  return {
    ...newFields(
      block.id,
      block.readingOrder,
      block.zIndex,
      mixedGroup?.active && mixedGroup.typstId === block.id ? mixedGroup.groupId : null,
      now,
    ),
    type: "typst",
    x: block.x,
    y: block.y,
    scale: block.scale,
    sourcePath: block.path,
    layoutWidthPt: block.layoutWidthPt,
    measuredWidthPt: block.measuredWidthPt,
    measuredHeightPt: block.measuredHeightPt,
  };
}

function newPageTypst(
  pageTypst: EditablePageTypst,
  now: string,
): Extract<PageObject, { type: "page_typst" }> {
  return {
    ...newFields(pageTypst.id, pageTypst.readingOrder, pageTypst.zIndex, null, now),
    type: "page_typst",
    x: 0,
    y: 0,
    rotation: 0,
    scale: 1,
    sourcePath: pageTypst.path,
  };
}

function newImage(
  image: EditableImage,
  now: string,
): Extract<PageObject, { type: "image" }> {
  return {
    ...newFields(image.id, image.readingOrder, image.zIndex, null, now),
    type: "image",
    x: image.x,
    y: image.y,
    scale: image.scale,
    sourcePath: image.path,
    widthPt: image.widthPt,
    heightPt: image.heightPt,
    altText: image.alt,
  };
}

function newShape(shape: ShapeView, now: string): Extract<PageObject, { type: "shape" }> {
  return {
    ...newFields(shape.id, shape.readingOrder, shape.zIndex, null, now),
    type: "shape",
    x: shape.x,
    y: shape.y,
    rotation: shape.rotation,
    scale: shape.scale,
    geometry: shape.geometry,
    style: shape.style,
  };
}

function newFields(
  id: string,
  readingOrder: number,
  zIndex: number,
  groupId: string | null,
  now: string,
) {
  return {
    id,
    x: 0,
    y: 0,
    rotation: 0,
    scale: 1,
    zIndex,
    readingOrder,
    groupId,
    createdAt: now,
    modifiedAt: now,
  };
}

function mergeInkLayers(
  schemaVersion: number,
  existing: InkLayer[],
  activeId: string,
  pageId: string,
  strokes: Stroke[],
): InkLayer[] {
  if (existing.length === 0) {
    return [{ schemaVersion, id: activeId, pageId, strokes }];
  }
  return existing.map((layer) => ({
    ...layer,
    schemaVersion,
    ...(layer.id === activeId ? { strokes } : {}),
  }));
}
