export type TouchHistoryAction = "undo" | "redo";
export type TouchSample = {
  pointerId: number;
  clientX: number;
  clientY: number;
  timeStamp: number;
};

const JOIN_MS = 150;
const TAP_MS = 250;
const DOUBLE_TAP_MS = 300;
const MOVEMENT_PX = 10;

/** Local pointer bookkeeping only: navigation owns capture and Rust owns history. */
export function createTouchTaps() {
  let group: {
    start: number;
    origins: Map<number, TouchSample>;
    active: Set<number>;
    releasing: boolean;
    invalid: boolean;
  } | null = null;
  let previous: { fingers: number; end: number } | null = null;

  function cancel() {
    if (group) group.invalid = true;
    previous = null;
  }

  function move(sample: TouchSample, allowed = true) {
    if (!group?.active.has(sample.pointerId)) return;
    const origin = group.origins.get(sample.pointerId)!;
    if (!allowed || sample.timeStamp - group.start > TAP_MS ||
      Math.hypot(sample.clientX - origin.clientX, sample.clientY - origin.clientY) >= MOVEMENT_PX) cancel();
  }

  return {
    start(sample: TouchSample, allowed = true) {
      if (!group) {
        group = { start: sample.timeStamp, origins: new Map(), active: new Set(), releasing: false, invalid: false };
      }
      if (group.origins.has(sample.pointerId) || group.releasing || !allowed ||
        sample.timeStamp - group.start > JOIN_MS) cancel();
      group.origins.set(sample.pointerId, sample);
      group.active.add(sample.pointerId);
      if (group.origins.size > 3) cancel();
    },
    move,
    end(sample: TouchSample, cancelled = false, allowed = true): { fingers: number; action: TouchHistoryAction | null } | null {
      if (!group?.active.has(sample.pointerId)) return null;
      move(sample, allowed);
      if (cancelled) cancel();
      group.releasing = true;
      group.active.delete(sample.pointerId);
      if (group.active.size > 0) return null;
      const finished = group;
      group = null;
      const fingers = finished.origins.size;
      if (finished.invalid || fingers < 2 || fingers > 3) {
        previous = null;
        return null;
      }
      const gap = previous ? finished.start - previous.end : Infinity;
      const matched = previous?.fingers === fingers && gap >= 0 && gap <= DOUBLE_TAP_MS;
      previous = matched ? null : { fingers, end: sample.timeStamp };
      return { fingers, action: matched ? (fingers === 2 ? "undo" : "redo") : null };
    },
    isMultiTapCandidate(now: number) {
      return Boolean(group && !group.invalid && group.origins.size >= 2 && now - group.start <= TAP_MS);
    },
    cancel,
    reset() { group = null; previous = null; },
  };
}
