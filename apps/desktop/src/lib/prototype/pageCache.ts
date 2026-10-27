import type { NotebookSnapshot } from "../ipc/types";

/** Concurrent preloads share work; a failed read is removed so the next visit can retry. */
export function createPageLoader(read: (root: string, pageId: string) => Promise<NotebookSnapshot>) {
  const pending = new Map<string, Promise<NotebookSnapshot>>();
  return (root: string, generation: number, pageId: string): Promise<NotebookSnapshot> => {
    const key = JSON.stringify([root, generation, pageId]);
    const existing = pending.get(key);
    if (existing) return existing;
    const request = Promise.resolve().then(() => read(root, pageId)).finally(() => pending.delete(key));
    pending.set(key, request);
    return request;
  };
}

/** Retain the visible/preload range and its neighbors, independently of edit focus. */
export function retainedPageIds(pageIds: string[], near: ReadonlySet<string>, activeId: string): Set<string> {
  const keep = new Set([activeId]);
  for (let index = 0; index < pageIds.length; index += 1) {
    if (!near.has(pageIds[index])) continue;
    for (let offset = -1; offset <= 1; offset += 1) {
      const id = pageIds[index + offset];
      if (id) keep.add(id);
    }
  }
  return keep;
}
