// Selection and ordering for the page overview. Pure, so the panel only wires events to them.

/** The pages from `anchor` to `target` inclusive, in notebook order, whichever comes first. */
export function selectRange(order: readonly string[], anchor: string, target: string): string[] {
  const from = order.indexOf(anchor);
  const to = order.indexOf(target);
  if (to < 0) return [];
  if (from < 0) return [target];
  const [low, high] = from <= to ? [from, to] : [to, from];
  return order.slice(low, high + 1);
}

/** `id` added to the selection, or taken out if it was already in it. */
export function toggleSelection(selected: readonly string[], id: string): string[] {
  return selected.includes(id) ? selected.filter((candidate) => candidate !== id) : [...selected, id];
}

/**
 * The order after moving `moving` into the gap `gap` of the current order, where gap 0 is before
 * the first page and gap `order.length` after the last. Moved pages keep their notebook order
 * among themselves, so dragging a selection of 2 and 5 lands them as 2, 5.
 */
export function moveIntoGap(order: readonly string[], moving: readonly string[], gap: number): string[] {
  const moved = new Set(moving);
  const block = order.filter((id) => moved.has(id));
  if (block.length === 0) return [...order];
  const before = order.slice(0, Math.max(0, Math.min(gap, order.length))).filter((id) => !moved.has(id));
  const after = order.slice(Math.max(0, Math.min(gap, order.length))).filter((id) => !moved.has(id));
  return [...before, ...block, ...after];
}

/**
 * Alt+Arrow: move the pages one place earlier (-1) or later (1). A scattered selection gathers
 * next to its first page (moving earlier) or its last (moving later). At either end it stays.
 */
export function moveByOne(order: readonly string[], moving: readonly string[], direction: -1 | 1): string[] {
  const indices = order.flatMap((id, index) => (moving.includes(id) ? [index] : []));
  if (indices.length === 0) return [...order];
  if (direction < 0) {
    const first = indices[0]!;
    return first === 0 ? [...order] : moveIntoGap(order, moving, first - 1);
  }
  const last = indices[indices.length - 1]!;
  return last === order.length - 1 ? [...order] : moveIntoGap(order, moving, last + 2);
}

/** Where focus goes from `index` for a navigation key in a grid of `columns`, or null if none. */
export function focusTarget(key: string, index: number, count: number, columns: number): number | null {
  const next =
    key === "ArrowLeft" ? index - 1
    : key === "ArrowRight" ? index + 1
    : key === "ArrowUp" ? index - columns
    : key === "ArrowDown" ? index + columns
    : key === "Home" ? 0
    : key === "End" ? count - 1
    : null;
  if (next === null) return null;
  return Math.max(0, Math.min(count - 1, next));
}

/** Whether two orders list the same pages in the same places. */
export function sameOrder(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((id, index) => b[index] === id);
}
