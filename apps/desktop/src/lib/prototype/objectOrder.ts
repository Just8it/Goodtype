import { DEFAULT_INK_Z_INDEX, type Page, type PageObject, type Stroke } from "../model";

export type VisualMove = "forward" | "backward" | "front" | "back";

type VisualEntry = {
  kind: "object" | "stroke";
  id: string;
  zIndex: number;
  order: number;
};

function isVisualObject(object: PageObject): boolean {
  return (
    object.type === "typst" ||
    object.type === "image" ||
    object.type === "pdf_material" ||
    object.type === "shape"
  );
}

function entries(page: Page, strokes: Stroke[]): VisualEntry[] {
  return [
    ...page.objects
      .filter(isVisualObject)
      .map((object, order) => ({ kind: "object" as const, id: object.id, zIndex: object.zIndex, order })),
    ...strokes.map((stroke, index) => ({
      kind: "stroke" as const,
      id: stroke.id,
      zIndex: stroke.zIndex ?? DEFAULT_INK_Z_INDEX,
      order: page.objects.length + index,
    })),
  ]
    .sort((left, right) => left.zIndex - right.zIndex || left.order - right.order);
}

const key = ({ kind, id }: Pick<VisualEntry, "kind" | "id">) => `${kind}:${id}`;

export function visualOrder(page: Page, strokes: Stroke[]): string[] {
  return entries(page, strokes).map(key);
}

export function moveVisualItems(
  page: Page,
  strokes: Stroke[],
  objectIds: string[],
  strokeIds: string[],
  move: VisualMove,
): { page: Page; strokes: Stroke[] } | null {
  const ordered = entries(page, strokes);
  const selectedObjects = new Set(objectIds);
  const selectedStrokes = new Set(strokeIds);
  const selected = (entry: VisualEntry) =>
    entry.kind === "object" ? selectedObjects.has(entry.id) : selectedStrokes.has(entry.id);
  if (!ordered.some(selected)) return null;
  const before = ordered.map(key);

  if (move === "back" || move === "front") {
    const picked = ordered.filter(selected);
    const rest = ordered.filter((entry) => !selected(entry));
    ordered.splice(0, ordered.length, ...(move === "back" ? [...picked, ...rest] : [...rest, ...picked]));
  } else if (move === "backward") {
    for (let index = 1; index < ordered.length; index += 1) {
      if (selected(ordered[index]) && !selected(ordered[index - 1])) {
        [ordered[index - 1], ordered[index]] = [ordered[index], ordered[index - 1]];
      }
    }
  } else {
    for (let index = ordered.length - 2; index >= 0; index -= 1) {
      if (selected(ordered[index]) && !selected(ordered[index + 1])) {
        [ordered[index], ordered[index + 1]] = [ordered[index + 1], ordered[index]];
      }
    }
  }
  if (ordered.every((entry, index) => key(entry) === before[index])) return null;

  const zByKey = new Map(ordered.map((entry, index) => [key(entry), index + 1]));
  const strokeById = new Map(strokes.map((stroke) => [stroke.id, stroke]));
  return {
    page: {
      ...page,
      objects: page.objects.map((object) =>
        isVisualObject(object)
          ? { ...object, zIndex: zByKey.get(key({ kind: "object", id: object.id }))! }
          : object,
      ),
    },
    strokes: ordered
      .filter((entry) => entry.kind === "stroke")
      .map((entry) => ({
        ...strokeById.get(entry.id)!,
        zIndex: zByKey.get(key(entry))!,
      })),
  };
}

export function moveReadingObject(page: Page, id: string, direction: -1 | 1): Page {
  const order = [...page.readingOrder];
  const index = order.indexOf(id);
  const target = index + direction;
  if (index < 0 || target < 0 || target >= order.length) return page;
  [order[index], order[target]] = [order[target], order[index]];
  const positionById = new Map(order.map((objectId, position) => [objectId, position]));
  return {
    ...page,
    readingOrder: order,
    objects: page.objects.map((object) => {
      const position = positionById.get(object.id);
      return position === undefined ? object : { ...object, readingOrder: position };
    }),
  };
}
