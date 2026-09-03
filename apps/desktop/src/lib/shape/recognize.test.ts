import { describe, expect, it } from "vitest";
import type { ShapeStyle, StrokePoint } from "../model";
import { isClosedStroke, recognizeHeldShape, turningConcentration } from "./recognize";

const style: ShapeStyle = {
  strokeColor: "#16212b",
  strokeWidthPt: 1.6,
  fillColor: null,
  opacity: 1,
};

const samples = (points: [number, number][]): StrokePoint[] =>
  points.map(([x, y], index) => ({
    x,
    y,
    pressure: 0.5,
    timeMs: index * 8,
    tiltX: 0,
    tiltY: 0,
  }));

describe("draw-and-hold shape recognition", () => {
  it("uses the existing conservative straight-line judgement", () => {
    const found = recognizeHeldShape(samples([[10, 10], [50, 11], [100, 10]]), style);
    expect(found?.draft.geometry.kind).toBe("line");
  });

  it("recognizes a rough closed rectangle", () => {
    const found = recognizeHeldShape(
      samples([[10, 10], [60, 9], [110, 11], [111, 50], [109, 80], [60, 81], [9, 79], [11, 45], [10, 10]]),
      style,
    );
    expect(found?.draft.geometry.kind).toBe("rectangle");
  });

  it("fits a deliberate open curve but leaves a small handwriting loop alone", () => {
    const curve = recognizeHeldShape(samples([[0, 40], [20, 10], [50, 0], [80, 10], [100, 40]]), style);
    const loop = recognizeHeldShape(samples([[0, 0], [4, -4], [8, 0], [4, 4], [0, 0]]), style);
    expect(curve?.draft.geometry.kind).toBe("spline");
    expect(loop).toBeNull();
  });
});

/**
 * Hand-drawn marks, not geometric ones.
 *
 * These generators carry the two kinds of error a real pen produces — slow tremor across the
 * stroke and per-sample digitiser jitter — because that combination is what used to decide the
 * outcome: a small shaky ring scored worse against an ellipse than against its own bounding box,
 * so it came out square. One tidy circle would never have caught it, so these run over a spread
 * of sizes, gaps and noise and assert on the tally rather than on a lucky seed.
 */
function random(seed: number) {
  let state = seed;
  return () => {
    state = (state * 1664525 + 1013904223) % 4294967296;
    return state / 4294967296;
  };
}

function handRing(radius: number, gapDegrees: number, noise: number, seed: number): StrokePoint[] {
  const next = random(seed);
  const phase = [next() * 6.28, next() * 6.28, next() * 6.28];
  const squash = 0.88 + next() * 0.24;
  const tilt = next() * 0.6;
  const sweep = ((360 - gapDegrees) * Math.PI) / 180;
  return Array.from({ length: 71 }, (_, index) => {
    const eased = index / 70 + Math.sin((index / 70) * 6.28) * 0.02;
    const angle = eased * sweep;
    const tremor =
      Math.sin(angle * 3 + phase[0]) * 0.5 +
      Math.sin(angle * 5 + phase[1]) * 0.3 +
      Math.sin(angle * 9 + phase[2]) * 0.2;
    const r = radius + tremor * radius * 0.06 + (next() - 0.5) * 2 * noise;
    const x = Math.cos(angle) * r;
    const y = Math.sin(angle) * r * squash;
    return {
      x: 200 + x * Math.cos(tilt) - y * Math.sin(tilt),
      y: 200 + x * Math.sin(tilt) + y * Math.cos(tilt),
      pressure: 0.5,
      timeMs: index * 8,
      tiltX: 0,
      tiltY: 0,
    };
  });
}

function handBox(size: number, gapPt: number, seed: number): StrokePoint[] {
  const next = random(seed);
  const tilt = (next() - 0.5) * 0.4;
  const height = size * (0.7 + next() * 0.6);
  const corners = [
    { x: 0, y: 0 },
    { x: size, y: 0 },
    { x: size, y: height },
    { x: 0, y: height },
    { x: 0, y: 0 },
  ];
  const travelled = 2 * (size + height) - gapPt;
  return Array.from({ length: 81 }, (_, index) => {
    let remaining = (index / 80) * travelled;
    let at = 0;
    while (at < 3) {
      const span = Math.hypot(corners[at + 1].x - corners[at].x, corners[at + 1].y - corners[at].y);
      if (remaining <= span) break;
      remaining -= span;
      at += 1;
    }
    const from = corners[at];
    const to = corners[at + 1];
    const span = Math.hypot(to.x - from.x, to.y - from.y) || 1;
    const along = remaining / span;
    const drift = Math.sin(along * 3.1 + at * 2.3) * size * 0.03;
    const x = from.x + (to.x - from.x) * along + (-(to.y - from.y) / span) * drift;
    const y = from.y + (to.y - from.y) * along + ((to.x - from.x) / span) * drift;
    return {
      x: 200 + x * Math.cos(tilt) - y * Math.sin(tilt),
      y: 200 + x * Math.sin(tilt) + y * Math.cos(tilt),
      pressure: 0.5,
      timeMs: index * 8,
      tiltX: 0,
      tiltY: 0,
    };
  });
}

const tally = (make: (seed: number) => StrokePoint[]) => {
  const counts: Record<string, number> = {};
  for (let seed = 1; seed <= 40; seed += 1) {
    const kind = recognizeHeldShape(make(seed), style)?.draft.geometry.kind ?? "none";
    counts[kind] = (counts[kind] ?? 0) + 1;
  }
  return counts;
};

describe("a shaky hand still gets the shape it drew", () => {
  it("reads wobbly rings as ellipses at every size and noise level", () => {
    for (const radius of [20, 40, 80]) {
      for (const noise of [0, 0.75, 1.5]) {
        for (const gap of [0, 5, 10, 20]) {
          const counts = tally((seed) => handRing(radius, gap, noise, seed));
          expect({ radius, noise, gap, ...counts }).toMatchObject({ ellipse: 40 });
        }
      }
    }
  });

  it("never mistakes a ring for its own bounding box", () => {
    for (const radius of [20, 40, 80]) {
      for (const noise of [0, 0.75, 1.5]) {
        expect(tally((seed) => handRing(radius, 10, noise, seed)).rectangle ?? 0).toBe(0);
      }
    }
  });

  it("reads wobbly boxes as rectangles", () => {
    const summary: Record<string, number> = {};
    for (const size of [30, 60, 120]) {
      for (const gap of [3, 6]) {
        summary[`box${size}-gap${gap}`] = tally((seed) => handBox(size, gap, seed)).rectangle ?? 0;
      }
    }
    // Every size and gap reads as a rectangle at least 36 times in 40. The one cell that is not
    // perfect is a large box drawn with bowed sides, where the mark genuinely is somewhat round.
    const weak = Object.entries(summary).filter(([, correct]) => correct < 36);
    expect({ weak, summary }).toMatchObject({ weak: [] });
  });
});

describe("closing a mark that came back to where it started", () => {
  it("treats a small absolute gap as closed however big the mark is", () => {
    // A few points of slack closes a small ring and a large one alike. A purely proportional
    // rule would hold the small one to a far tighter tolerance for no reason the hand can feel.
    for (const radius of [20, 80]) {
      const points = handRing(radius, 0, 0.75, 7)
        .slice(0, -3)
        .map(({ x, y }) => ({ x, y }));
      expect(isClosedStroke(points)).toBe(true);
    }
  });

  it("leaves a genuinely open arc open", () => {
    const arc = handRing(60, 120, 0.75, 3).map(({ x, y }) => ({ x, y }));
    expect(isClosedStroke(arc)).toBe(false);
  });

  it("fits a closed freehand loop as a closed curve, not one with a gap in it", () => {
    // A three-lobed loop is neither box nor ring, so what is left is the curve itself — and it
    // still has to come back closed, or the drawing gains an opening the hand never made.
    const blob = Array.from({ length: 60 }, (_, index) => {
      const angle = (index / 59) * Math.PI * 2;
      const r = 60 + Math.sin(angle * 3) * 24;
      return {
        x: 200 + Math.cos(angle) * r,
        y: 200 + Math.sin(angle) * r * 0.9,
        pressure: 0.5,
        timeMs: index * 8,
        tiltX: 0,
        tiltY: 0,
      };
    });
    const found = recognizeHeldShape(blob, style);
    expect(found?.draft.geometry.kind).toBe("spline");
    expect(found?.draft.geometry.kind === "spline" && found.draft.geometry.closed).toBe(true);
  });
});

describe("a loop closed by carrying past the start", () => {
  // Overshoot is how a loop usually gets closed: the hand comes round and keeps going a little
  // rather than stopping on the exact sample it began at, which leaves a tail lying across the
  // outline. Measuring the two endpoints calls that wide open, however neatly it was drawn.
  const overshoot = (radius: number, degrees: number, seed: number) =>
    handRing(radius, -degrees, 0.75, seed);

  it("recognizes rings that carry past where they began", () => {
    const summary: Record<string, number> = {};
    for (const radius of [30, 60]) {
      for (const past of [10, 25, 45, 70]) {
        summary[`r${radius}-past${past}`] =
          tally((seed) => overshoot(radius, past, seed)).ellipse ?? 0;
      }
    }
    const weak = Object.entries(summary).filter(([, correct]) => correct < 40);
    expect({ weak, summary }).toMatchObject({ weak: [] });
  });

  it("trims the tail instead of fitting around it", () => {
    // The tail is part of the gesture, not the shape. Left in, it drags the bounding box out and
    // the fitted ring comes back visibly larger than the one on the page.
    const clean = recognizeHeldShape(handRing(60, 0, 0, 5), style);
    const past = recognizeHeldShape(handRing(60, -60, 0, 5), style);
    expect(clean?.draft.geometry.kind).toBe("ellipse");
    expect(past?.draft.geometry.kind).toBe("ellipse");
    if (clean?.draft.geometry.kind !== "ellipse" || past?.draft.geometry.kind !== "ellipse") return;
    expect(past.draft.geometry.widthPt).toBeCloseTo(clean.draft.geometry.widthPt, 0);
    expect(past.draft.geometry.heightPt).toBeCloseTo(clean.draft.geometry.heightPt, 0);
  });

  it("still refuses a spiral that never came back", () => {
    // Going round twice is not a closed outline, and the run that would have to close it is more
    // than a tail — it is another lap.
    const spiral = Array.from({ length: 140 }, (_, index) => {
      const angle = (index / 139) * Math.PI * 4;
      const r = 30 + index * 0.5;
      return {
        x: 200 + Math.cos(angle) * r,
        y: 200 + Math.sin(angle) * r,
        pressure: 0.5,
        timeMs: index * 8,
        tiltX: 0,
        tiltY: 0,
      };
    });
    expect(recognizeHeldShape(spiral, style)).toBeNull();
  });
});

describe("a rectangle as a hand actually draws one", () => {
  /**
   * Rotated, corners rounded off because the hand does not stop to turn, sides that bow, and a
   * short overshoot past the start.
   *
   * Rounding is the part that matters. It takes area out of the bounding box until a box fills no
   * more of it than a ring would, so anything reasoning from area alone reads this as an ellipse —
   * which is exactly what it used to do. The turning is what stays put.
   */
  function sloppyBox(size: number, bow: number, round: number, seed: number): StrokePoint[] {
    const next = random(seed);
    const tilt = (next() - 0.5) * 0.5;
    const height = size * (0.8 + next() * 0.5);
    const corners = [
      { x: 0, y: 0 },
      { x: size, y: 0 },
      { x: size, y: height },
      { x: 0, y: height },
    ];
    const perimeter = 2 * (size + height);
    return Array.from({ length: 91 }, (_, index) => {
      let remaining = ((index / 90) * (perimeter + 20)) % perimeter;
      let at = 0;
      while (at < 3) {
        const span = Math.hypot(
          corners[(at + 1) % 4].x - corners[at].x,
          corners[(at + 1) % 4].y - corners[at].y,
        );
        if (remaining <= span) break;
        remaining -= span;
        at += 1;
      }
      const from = corners[at];
      const to = corners[(at + 1) % 4];
      const span = Math.hypot(to.x - from.x, to.y - from.y) || 1;
      const along = remaining / span;
      const bowOut = Math.sin(along * Math.PI) * bow;
      const cut = Math.min(along, 1 - along) < round / span ? round * 0.5 : 0;
      const x = from.x + (to.x - from.x) * along + (-(to.y - from.y) / span) * (bowOut - cut);
      const y = from.y + (to.y - from.y) * along + ((to.x - from.x) / span) * (bowOut - cut);
      const jitter = (next() - 0.5) * 1.5;
      return {
        x: 200 + (x + jitter) * Math.cos(tilt) - y * Math.sin(tilt),
        y: 200 + (x + jitter) * Math.sin(tilt) + y * Math.cos(tilt),
        pressure: 0.5,
        timeMs: index * 8,
        tiltX: 0,
        tiltY: 0,
      };
    });
  }

  it("keeps reading as a rectangle however bowed and rounded it gets", () => {
    const summary: Record<string, number> = {};
    for (const bow of [0, 4, 8, 14]) {
      for (const round of [0, 8, 16]) {
        const counts: Record<string, number> = {};
        for (let seed = 1; seed <= 30; seed += 1) {
          const kind =
            recognizeHeldShape(sloppyBox(140, bow, round, seed), style)?.draft.geometry.kind ??
            "none";
          counts[kind] = (counts[kind] ?? 0) + 1;
        }
        summary[`bow${bow}-round${round}`] = counts.rectangle ?? 0;
      }
    }
    // Only the most rounded corners leave any doubt, and there the mark genuinely is near the line
    // between the two — which is the one place the fit, rather than the turning, should decide.
    const weak = Object.entries(summary).filter(([, correct]) => correct < 25);
    expect({ weak, summary }).toMatchObject({ weak: [] });
  });

  it("separates boxes from rings by where they turn, not how full they are", () => {
    // The measure the decision rests on, asserted directly: rings turn steadily wherever you look,
    // boxes do it in four places, and rounding the corners never closes the gap between them.
    const ringTurn = [20, 40, 80].flatMap((radius) =>
      [0, 1.5].flatMap((noise) =>
        Array.from({ length: 10 }, (_, seed) => turningConcentration(handRing(radius, 0, noise, seed + 1).map(({ x, y }) => ({ x, y })))),
      ),
    );
    const boxTurn = [0, 8, 16].flatMap((round) =>
      Array.from({ length: 10 }, (_, seed) => turningConcentration(sloppyBox(140, 8, round, seed + 1).map(({ x, y }) => ({ x, y })))),
    );
    expect(Math.max(...ringTurn)).toBeLessThan(Math.min(...boxTurn));
  });
});
