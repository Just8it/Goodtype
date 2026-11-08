import type { SharePayload } from "../ipc/helper";

/** Validate the complete range before changing attachments; never partially add an invalid range. */
export function attachmentRange(text: string, pages: { id: string; number: number; calculation: boolean; unavailable?: boolean; loading?: boolean }[], selected: string[]): string[] | null {
  const match = text.trim().match(/^(\d+)\s*(?:[-–]\s*(\d+))?$/);
  if (!match) return null;
  const start = Number(match[1]), end = Number(match[2] ?? match[1]);
  if (!Number.isSafeInteger(start) || !Number.isSafeInteger(end) || start < 1 || end < start || end - start >= 8) return null;
  const range = pages.filter(page => page.number >= start && page.number <= end);
  if (range.length !== end - start + 1 || range.some(p => p.unavailable || p.loading)) return null;
  const ids = [...new Set([...selected, ...range.map(p => p.id)])];
  return ids.length <= 8 ? ids : null;
}

export function splitSelection(ids: string[], strokes: { id: string }[]) {
  const ink = new Set(strokes.map(s => s.id));
  return { objectIds: ids.filter(id => !ink.has(id)), strokeIds: ids.filter(id => ink.has(id)) };
}
export function sameHelperSelection(a: SharePayload, b: SharePayload) {
  return a.root === b.root && a.pageId === b.pageId
    && a.objectIds.length === b.objectIds.length && a.objectIds.every((id, i) => id === b.objectIds[i])
    && a.strokeIds.length === b.strokeIds.length && a.strokeIds.every((id, i) => id === b.strokeIds[i])
    && a.cropPt.x === b.cropPt.x && a.cropPt.y === b.cropPt.y
    && a.cropPt.width === b.cropPt.width && a.cropPt.height === b.cropPt.height;
}

/** Copy before flushing: callers can mutate the draft while the queue is awaited. */
export function captureHelperIntent(intent: import("../ipc/helper").RequestContextIntent): import("../ipc/helper").RequestContextIntent {
  return {
    ...intent,
    destination: { ...intent.destination },
    pageIds: [...intent.pageIds],
    selection: intent.selection ? {
      ...intent.selection,
      objectIds: [...intent.selection.objectIds],
      strokeIds: [...intent.selection.strokeIds],
      cropPt: { ...intent.selection.cropPt },
      sessionIds: intent.selection.sessionIds ? [...intent.selection.sessionIds] : null,
    } : null,
  };
}

/** Version comparison prevents an intervening attachment edit being hidden by A ? B ? A. */
export function sameHelperIntent(a: import("../ipc/helper").RequestContextIntent, b: import("../ipc/helper").RequestContextIntent) {
  return JSON.stringify(a.cells ?? null) === JSON.stringify(b.cells ?? null) && a.sharePageNames === b.sharePageNames && a.intentVersion === b.intentVersion
    && a.destination.root === b.destination.root && a.destination.pageId === b.destination.pageId
    && a.presets === b.presets
    && a.pageIds.length === b.pageIds.length && a.pageIds.every((id, i) => id === b.pageIds[i])
    && (a.selection === null ? b.selection === null : b.selection !== null && sameHelperSelection(a.selection, b.selection));
}
