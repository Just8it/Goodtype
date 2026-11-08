import { describe, expect, it } from "vitest";
import { selectionCrop, pdfRegion, clampCrop, selectedObjectTypes, resizeCrop } from "./crop";
import type { ImageView, TypstBlockState } from "../notebook/pageView";
import type { Stroke } from "../model";
describe("helper crop in page points", () => {
  it("returns null for empty selection", () => expect(selectionCrop([], [], [], [], [])).toBeNull());
  it("uses transformed ink bounds", () => { const stroke = { id: "ink", points: [{ x: 1, y: 2 }, { x: 4, y: 8 }], transform: { translateX: 10, translateY: 20, scaleX: 2, scaleY: 3, rotation: 0 } } as Stroke; expect(selectionCrop(["ink"], [stroke], [], [], [])).toEqual({ x: 12, y: 26, width: 6, height: 18 }); });
  it("combines rotated images and scaled Typst and ignores unselected objects", () => { const image = { id: "image", x: 100, y: 50, widthPt: 20, heightPt: 10, scale: 2, rotation: 90 } as ImageView; const block = { id: "text", transform: { x: 10, y: 20, layoutWidthPt: 10, scale: 2 }, result: { widthPt: 15, heightPt: 10 } } as TypstBlockState;
    expect(selectionCrop(["image", "text"], [], [image, { ...image, id: "other", x: 1000 }], [], [block])).toEqual({ x: 10, y: 20, width: 90, height: 70 }); });
});

it("clips edge-crossing selections to the actual page", () => {
 const image = { id: "edge", x: -5, y: -3, widthPt: 120, heightPt: 240, scale: 1, rotation: 0 } as ImageView;
 expect(selectionCrop(["edge"], [], [image], [], [], { widthPt: 100, heightPt: 200 })).toEqual({ x: 0, y: 0, width: 100, height: 200 });
 expect(selectionCrop(["edge"], [], [{ ...image, x: 101 }], [], [], { widthPt: 100, heightPt: 200 })).toBeNull();
});

it("creates only PDF regions and clamps them to the page", () => {
 const page={widthPt:100,heightPt:200}, points=[{x:-10,y:10},{x:50,y:80}];
 expect(pdfRegion(points,page,false,[])).toBeNull();
 expect(pdfRegion(points,page,true,[])).toEqual({x:0,y:10,width:50,height:70});
 expect(clampCrop({x:110,y:0,width:10,height:10},page)).toBeNull();
 expect(clampCrop({x:0,y:0,width:Infinity,height:10},page)).toBeNull();
});
it("includes transformed PDF materials in crops and detects material regions", () => {
 const material={id:"pdf",type:"pdf_material",x:20,y:30,scale:2,rotation:0,sourceWidthPt:10,sourceHeightPt:15} as import("../model").PageObject;
 const page={widthPt:100,heightPt:200};
 expect(selectionCrop(["pdf"],[],[],[],[],page,[material])).toEqual({x:20,y:30,width:20,height:30});
 expect(pdfRegion([{x:21,y:31},{x:25,y:35}],page,false,[material])).not.toBeNull();
 expect(pdfRegion([{x:70,y:70},{x:80,y:80}],page,false,[material])).toBeNull();
});

it("classifies pixel needs through selected groups", async () => {
 const {helperSelectionNeedsPixels}=await import("../ipc/helper");
 const objects=[{id:"group",type:"group",childIds:["nested"]},{id:"nested",type:"group",childIds:["pdf"]},{id:"pdf",type:"pdf_material"}] as import("../model").PageObject[];
 expect(helperSelectionNeedsPixels(selectedObjectTypes(objects,["group"]),false,false)).toBe(true);
 expect(helperSelectionNeedsPixels(["typst"],false,false)).toBe(false);
 expect(helperSelectionNeedsPixels([],false,true)).toBe(true);
 expect(helperSelectionNeedsPixels([],true,false)).toBe(true);
});

it("resizes crop corners within paper without mutating the approved crop", () => {
 const base={x:20,y:30,width:50,height:60},page={widthPt:100,heightPt:200};
 expect(resizeCrop(base,0,{x:-40,y:-50},page)).toEqual({x:0,y:0,width:70,height:90});
 expect(resizeCrop(base,3,{x:100,y:200},page)).toEqual({x:20,y:30,width:80,height:170});
 expect(resizeCrop(base,0,{x:100,y:100},page)).toEqual({x:69,y:89,width:1,height:1});
 expect(base).toEqual({x:20,y:30,width:50,height:60});
});
