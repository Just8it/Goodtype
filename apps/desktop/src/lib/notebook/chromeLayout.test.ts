import { describe, expect, it } from "vitest";
import {
  areasOverlap,
  chromeLayout,
  nearestHistoryCorner,
  paletteClaim,
  type ChromeArea,
} from "./chromeLayout";
import type { PaletteDock } from "./palette";

const DOCKS: PaletteDock[] = ["top", "right", "bottom", "left"];

describe("canvas chrome placement", () => {
  it("never lets the pill sit in space the palette needs", () => {
    // The reason this module exists. Each piece used to name its own corner, so nothing was in a
    // position to notice a collision; here it is checked for every dock at once.
    //
    // Against the claim rather than the drawn area, because an end-docked rail is drawn across its
    // whole row on purpose — that is what centres it on the canvas — while only reaching the
    // middle of it. The corners of that row are still the pill's to use.
    for (const dock of DOCKS) {
      const { history } = chromeLayout(dock);
      expect({ dock, overlap: areasOverlap(paletteClaim(dock), history) }).toEqual({
        dock,
        overlap: false,
      });
    }
  });

  it("puts the history pill away from the side the palette is on", () => {
    expect(chromeLayout("left").history.justify).toBe("end");
    expect(chromeLayout("right").history.justify).toBe("start");
  });

  it("hangs the palette at the same height on either side", () => {
    // The bug this replaces: the two side docks dodged different corners, so the same rail sat
    // higher on one side than the other.
    const left = chromeLayout("left").palette;
    const right = chromeLayout("right").palette;
    expect({ rowStart: left.rowStart, rowEnd: left.rowEnd, align: left.align }).toEqual({
      rowStart: right.rowStart,
      rowEnd: right.rowEnd,
      align: right.align,
    });
  });

  it("docks the palette to the edge it was dropped on", () => {
    const spans = (area: ChromeArea) => ({
      columns: [area.columnStart, area.columnEnd],
      rows: [area.rowStart, area.rowEnd],
      justify: area.justify,
      align: area.align,
    });
    expect(spans(chromeLayout("left").palette)).toMatchObject({ justify: "start" });
    expect(spans(chromeLayout("right").palette)).toMatchObject({ justify: "end" });
    expect(spans(chromeLayout("top").palette)).toMatchObject({ align: "start" });
    expect(spans(chromeLayout("bottom").palette)).toMatchObject({ align: "end" });
  });

  it("centres an end-docked rail on the canvas, not on the space beside the pill", () => {
    // A grid column is as wide as the widest thing in any of its rows, so the pill widens its
    // column wherever it is parked. A rail centred in the middle column alone is pushed over by
    // exactly that; spanning the row keeps it centred on the canvas whatever the pill is doing.
    for (const dock of ["top", "bottom"] as const) {
      expect(chromeLayout(dock).palette).toMatchObject({
        columnStart: 1,
        columnEnd: 4,
        justify: "center",
      });
    }
  });

  it("keeps every area inside the three by three grid", () => {
    for (const dock of DOCKS) {
      const { palette, history } = chromeLayout(dock);
      for (const area of [palette, history]) {
        expect(area.columnStart).toBeGreaterThanOrEqual(1);
        expect(area.rowStart).toBeGreaterThanOrEqual(1);
        expect(area.columnEnd).toBeLessThanOrEqual(4);
        expect(area.rowEnd).toBeLessThanOrEqual(4);
        expect(area.columnEnd).toBeGreaterThan(area.columnStart);
        expect(area.rowEnd).toBeGreaterThan(area.rowStart);
      }
    }
  });
});

describe("parking the history pill", () => {
  const CORNERS = ["top-left", "top-right", "bottom-left", "bottom-right"] as const;

  it("honours a corner the writer chose", () => {
    // Docked at the bottom, the palette owns only the bottom row, so all three other corners and
    // the bottom-left one the rail does not reach are the writer's to use.
    expect(chromeLayout("bottom", "top-right").history).toMatchObject({
      columnStart: 3,
      rowStart: 1,
      justify: "end",
      align: "start",
    });
    expect(chromeLayout("left", "bottom-right").history).toMatchObject({
      columnStart: 3,
      rowStart: 3,
      align: "end",
    });
  });

  it("gets out of the way when the palette is dragged onto it", () => {
    // Parked bottom-left, then the rail docks left: the pill has to move rather than hide under
    // it. What it must not do is overlap, whichever corner was asked for.
    for (const dock of DOCKS) {
      for (const corner of CORNERS) {
        const { history } = chromeLayout(dock, corner);
        expect({ dock, corner, overlap: areasOverlap(paletteClaim(dock), history) }).toEqual({
          dock,
          corner,
          overlap: false,
        });
      }
    }
  });

  it("keeps the preference so the pill comes back", () => {
    // The rail covering a corner does not overwrite the choice, it only overrides it while it is
    // there — moving the rail away puts the pill back where it was left.
    const covered = chromeLayout("left", "top-left").history;
    const clear = chromeLayout("bottom", "top-left").history;
    expect(covered.columnStart).toBe(3);
    expect(clear.columnStart).toBe(1);
  });

  it("falls back to following the palette when told to", () => {
    for (const dock of DOCKS) {
      expect(chromeLayout(dock, "auto")).toEqual(chromeLayout(dock));
    }
  });

  it("never moves the palette to make room for the pill", () => {
    // The rail is what the writer aims at, so it goes where it was docked and everything else
    // works around it. Whatever corner the pill is asked for, the palette lands identically.
    for (const dock of DOCKS) {
      const settled = chromeLayout(dock).palette;
      for (const corner of CORNERS) {
        expect({ dock, corner, palette: chromeLayout(dock, corner).palette }).toEqual({
          dock,
          corner,
          palette: settled,
        });
      }
    }
  });

  it("slides to the nearest free corner rather than jumping across", () => {
    // Pushed out of the bottom-left by a left-hand rail, the pill should cross to the other
    // bottom corner — not leap to the top of the screen, which is where it started from.
    expect(chromeLayout("left", "bottom-left").historyCorner).toBe("bottom-right");
    expect(chromeLayout("right", "bottom-right").historyCorner).toBe("bottom-left");
  });

  it("leaves both corners of an end-docked row free when there is room", () => {
    // The side columns are sized to what is in them, so a corner the rail is not using is not
    // space it needs. Refusing it read as the pill being kept out of an empty corner.
    expect(chromeLayout("top", "top-right").historyCorner).toBe("top-right");
    expect(chromeLayout("top", "top-left").historyCorner).toBe("top-left");
    expect(chromeLayout("bottom", "bottom-right").historyCorner).toBe("bottom-right");
  });

  it("gives the whole row to the palette when the workspace is too narrow for both", () => {
    // The side editor pulled fully out. The rail keeps its row and the pill crosses to the other,
    // rather than the rail being cut into.
    expect(chromeLayout("top", "top-right", true).historyCorner).toBe("bottom-right");
    expect(chromeLayout("top", "top-left", true).historyCorner).toBe("bottom-left");
    expect(chromeLayout("bottom", "bottom-left", true).historyCorner).toBe("top-left");
  });

  it("still draws the palette in the same place when crowded", () => {
    // Only its claim on the row widens. If the drawn area moved, the rail would shift under the
    // pointer as the side editor opened.
    for (const dock of DOCKS) {
      expect(chromeLayout(dock, "auto", true).palette).toEqual(chromeLayout(dock, "auto").palette);
    }
  });

  it("keeps the pill clear of the palette however crowded it is", () => {
    for (const dock of DOCKS) {
      for (const corner of CORNERS) {
        for (const crowded of [false, true]) {
          const { history } = chromeLayout(dock, corner, crowded);
          const claim = paletteClaim(dock, crowded);
          expect({ dock, corner, crowded, overlap: areasOverlap(claim, history) }).toEqual({
            dock,
            corner,
            crowded,
            overlap: false,
          });
        }
      }
    }
  });

  it("reports the corner it settled on, which is what a drag previews", () => {
    expect(chromeLayout("bottom", "top-right").historyCorner).toBe("top-right");
    expect(chromeLayout("left", "top-left").historyCorner).not.toBe("top-left");
  });
});

describe("finding the corner under the pointer", () => {
  it("splits the workspace into quarters", () => {
    expect(nearestHistoryCorner(10, 10, 1000, 800)).toBe("top-left");
    expect(nearestHistoryCorner(990, 10, 1000, 800)).toBe("top-right");
    expect(nearestHistoryCorner(10, 790, 1000, 800)).toBe("bottom-left");
    expect(nearestHistoryCorner(990, 790, 1000, 800)).toBe("bottom-right");
  });
});
