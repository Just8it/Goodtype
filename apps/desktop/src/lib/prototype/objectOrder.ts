import type { Page, PageObject } from "../model";

export type VisualMove = "forward" | "backward" | "front" | "back";

export function moveVisualObject(
  objects: PageObject[],
  id: string,
  move: VisualMove,
): PageObject[] {
  const ordered = [...objects].sort((left, right) => left.zIndex - right.zIndex);
  const index = ordered.findIndex((object) => object.id === id);
  if (index < 0) return objects;
  const [selected] = ordered.splice(index, 1);
  const target =
    move === "front"
      ? ordered.length
      : move === "back"
        ? 0
        : move === "forward"
          ? Math.min(ordered.length, index + 1)
          : Math.max(0, index - 1);
  ordered.splice(target, 0, selected);
  const zById = new Map(ordered.map((object, position) => [object.id, position + 1]));
  return objects.map((object) => ({ ...object, zIndex: zById.get(object.id)! }));
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
