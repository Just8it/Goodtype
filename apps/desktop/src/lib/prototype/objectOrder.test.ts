import { describe, expect, it } from "vitest";
import type { PageObject } from "../model";
import { moveReadingObject, moveVisualObject } from "./objectOrder";

const object = (id: string, order: number): PageObject => ({
  type: "typst",
  id,
  x: 0,
  y: 0,
  rotation: 0,
  scale: 1,
  zIndex: order + 1,
  readingOrder: order,
  groupId: null,
  createdAt: "now",
  modifiedAt: "now",
  sourcePath: `blocks/${id}.typ`,
  layoutWidthPt: 100,
  measuredWidthPt: 100,
  measuredHeightPt: 20,
});

describe("object order", () => {
  it("changes visual and reading order independently", () => {
    const objects = [object("a", 0), object("b", 1), object("c", 2)];
    const visual = moveVisualObject(objects, "a", "front");
    expect(visual.find((item) => item.id === "a")?.zIndex).toBe(3);

    const page = {
      schemaVersion: 1,
      id: "page-001",
      revision: 1,
      geometry: { widthPt: 100, heightPt: 100 },
      background: { kind: "plain" as const, color: "#ffffff" },
      objects: visual,
      readingOrder: ["a", "b", "c"],
      inkLayers: [],
    };
    const reading = moveReadingObject(page, "a", 1);
    expect(reading.readingOrder).toEqual(["b", "a", "c"]);
    expect(reading.objects.find((item) => item.id === "a")?.zIndex).toBe(3);
  });
});
