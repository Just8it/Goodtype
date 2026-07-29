import type {
  InkLayer,
  NotebookManifest,
  Page,
  PageBackground,
  PageGeometry,
  PageObject,
  Stroke,
} from "../model";
import type { NotebookSnapshot } from "../ipc/types";

export type EditableTypst = {
  id: string;
  path: string;
  source: string;
  x: number;
  y: number;
  layoutWidthPt: number;
  scale: number;
  measuredWidthPt: number;
  measuredHeightPt: number;
  zIndex: number;
  readingOrder: number;
};

export type EditableImage = {
  id: string;
  path: string;
  alt: string;
  x: number;
  y: number;
  widthPt: number;
  heightPt: number;
  scale: number;
  zIndex: number;
  readingOrder: number;
};

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
  images: EditableImage[];
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
    if (object.type === "image") {
      const image = imageById.get(object.id);
      if (!image) continue;
      emitted.add(object.id);
      objects.push(patchImage(object, image, input.now));
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
  for (const image of input.images) {
    if (emitted.has(image.id)) continue;
    emitted.add(image.id);
    objects.push(newImage(image, input.now));
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

  const page: Page = {
    schemaVersion: basePage?.schemaVersion ?? 1,
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
    input.base?.inkLayers ?? [],
    input.inkLayerId,
    input.pageId,
    input.strokes,
  );
  const editableBlocks = new Map(
    [
      ...input.typst.map((block) => ({ path: block.path, source: block.source })),
      ...(input.sharedStyle ? [input.sharedStyle] : []),
    ].map((block) => [
      block.path,
      { path: block.path, bytes: Array.from(new TextEncoder().encode(block.source)) },
    ]),
  );
  const blockPaths = new Set([
    ...(input.manifest.sharedStylePath ? [input.manifest.sharedStylePath] : []),
    ...orderedObjects.flatMap((object) =>
      object.type === "typst" ? [object.sourcePath] : [],
    ),
  ]);
  const baseBlocks = new Map(input.base?.blocks.map((file) => [file.path, file]) ?? []);
  const blocks = [...blockPaths].flatMap((path) => {
    const file = editableBlocks.get(path) ?? baseBlocks.get(path);
    return file ? [file] : [];
  });

  return {
    manifest: { ...input.manifest, modifiedAt: input.now },
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
  existing: InkLayer[],
  activeId: string,
  pageId: string,
  strokes: Stroke[],
): InkLayer[] {
  if (existing.length === 0) {
    return [{ schemaVersion: 1, id: activeId, pageId, strokes }];
  }
  return existing.map((layer) => (layer.id === activeId ? { ...layer, strokes } : layer));
}
