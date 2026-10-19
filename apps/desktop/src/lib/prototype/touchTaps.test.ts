import { describe, expect, it } from "vitest";
import { createTouchTaps, type TouchSample } from "./touchTaps";

const point = (pointerId: number, timeStamp: number, clientX = pointerId * 30, clientY = 100): TouchSample =>
  ({ pointerId, timeStamp, clientX, clientY });

function tap(taps: ReturnType<typeof createTouchTaps>, count: number, start: number) {
  for (let id = 1; id <= count; id += 1) taps.start(point(id, start + id * 10));
  let result = null;
  for (let id = 1; id <= count; id += 1) result = taps.end(point(id, start + 100 + id * 10));
  return result;
}

describe("touch history double taps", () => {
  it("requires two complete matching chords, then emits only once", () => {
    const taps = createTouchTaps();
    expect(tap(taps, 2, 0)).toEqual({ fingers: 2, action: null });
    expect(tap(taps, 2, 200)).toEqual({ fingers: 2, action: "undo" });
    expect(tap(taps, 2, 400)).toEqual({ fingers: 2, action: null });
    taps.reset();
    expect(tap(taps, 3, 0)?.action).toBeNull();
    expect(tap(taps, 3, 200)?.action).toBe("redo");
  });

  it("does not combine different finger counts, single fingers, or four fingers", () => {
    const taps = createTouchTaps();
    expect(tap(taps, 1, 0)).toBeNull();
    expect(tap(taps, 4, 200)).toBeNull();
    expect(tap(taps, 2, 400)?.action).toBeNull();
    expect(tap(taps, 3, 600)?.action).toBeNull();
    expect(tap(taps, 3, 800)?.action).toBe("redo");
  });

  it("expires the first tap after the double-tap window", () => {
    const taps = createTouchTaps();
    tap(taps, 2, 0);
    expect(tap(taps, 2, 500)?.action).toBeNull();
    expect(tap(taps, 2, 700)?.action).toBe("undo");
  });

  it("refuses slow arrivals, long holds and staggered replacement fingers", () => {
    for (const kind of ["late", "held", "replacement"]) {
      const taps = createTouchTaps();
      taps.start(point(1, 0));
      taps.start(point(2, kind === "late" ? 151 : 20));
      taps.end(point(1, kind === "held" ? 251 : 170));
      if (kind === "replacement") taps.start(point(3, 180));
      const result = taps.end(point(2, kind === "held" ? 260 : 190));
      expect(result).toBeNull();
      if (kind === "replacement") expect(taps.end(point(3, 200))).toBeNull();
      expect(tap(taps, 2, 400)?.action).toBeNull();
    }
  });

  it("rejects pan/pinch movement even if the finger returns, including movement only on lift", () => {
    for (const onLift of [false, true]) {
      const taps = createTouchTaps();
      tap(taps, 2, 0);
      taps.start(point(1, 200));
      taps.start(point(2, 210));
      expect(taps.isMultiTapCandidate(220)).toBe(true);
      if (!onLift) {
        taps.move(point(1, 225, 41));
        expect(taps.isMultiTapCandidate(230)).toBe(false);
      }
      taps.end(point(1, 240, onLift ? 41 : 30));
      expect(taps.end(point(2, 245))).toBeNull();
      expect(tap(taps, 2, 400)?.action).toBeNull();
    }
  });

  it("allows small jitter but rejects a ten-pixel movement boundary", () => {
    const taps = createTouchTaps();
    tap(taps, 2, 0);
    taps.start(point(1, 200)); taps.start(point(2, 210));
    taps.move(point(1, 220, 39));
    taps.end(point(1, 240, 39));
    expect(taps.end(point(2, 245))?.action).toBe("undo");
    taps.start(point(1, 400)); taps.start(point(2, 410));
    taps.move(point(1, 420, 40));
    expect(taps.isMultiTapCandidate(430)).toBe(false);
  });

  it("clears pending history on cancellation, pen activity, settings changes or disabled input", () => {
    for (const stop of ["cancel", "reset", "disabled", "pointercancel"]) {
      const taps = createTouchTaps();
      tap(taps, 2, 0);
      if (stop === "cancel") taps.cancel();
      if (stop === "reset") taps.reset();
      taps.start(point(1, 200), stop !== "disabled");
      taps.start(point(2, 210));
      taps.end(point(1, 240), stop === "pointercancel");
      expect(taps.end(point(2, 245))?.action ?? null).toBeNull();
    }
  });

  it("stops treating a held chord as a tap so navigation can continue", () => {
    const taps = createTouchTaps();
    taps.start(point(1, 0)); taps.start(point(2, 20));
    expect(taps.isMultiTapCandidate(250)).toBe(true);
    expect(taps.isMultiTapCandidate(251)).toBe(false);
    taps.move(point(1, 260));
    taps.end(point(1, 270));
    expect(taps.end(point(2, 280))).toBeNull();
  });
});
