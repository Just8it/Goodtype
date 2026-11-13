import { it, expect, vi } from "vitest";
import type { Stroke } from "../model";
import { strokeBounds, hitStroke, selectStrokesInLasso } from "./selection";
import { eraseThrough, contactIntervals } from "./erase";

it("records deterministic 5000 x 60 work and dense erase reference", async () => {
  let reads = 0;
  const strokes: Stroke[] = Array.from({length:5000}, (_, i) => ({
    id: String(i), zIndex:i, tool:"pen", color:"#000000", widthPt:1, pressure:false,
    taper:0, opacity:1, groupId:null,
    transform:{translateX:0,translateY:i*10,scaleX:1,scaleY:1,rotation:0},
    points:Array.from({length:60}, (_, j) => ({get x(){reads++; return j;}, y:0, pressure:0.5,timeMs:j,tiltX:0,tiltY:0})),
  }));
  strokes.forEach(strokeBounds);
  let boundsVisits = 0;
  expect(hitStroke(strokes, {x:100000,y:100000}, 1, () => {boundsVisits++; return true;})).toBeNull();
  expect(boundsVisits).toBe(5000); // Baseline also visits 5000 bounds; no spatial index.
  // Measured pre-change: 120 / 300000 / 1180000 / 5000 respectively below.
  reads=0;
  hitStroke(strokes,{x:10,y:0},1); hitStroke(strokes,{x:10,y:0},1);
  const hitReads=reads;
  reads=0;
  selectStrokesInLasso(strokes,[{x:5,y:-1},{x:15,y:-1},{x:15,y:1},{x:5,y:1}]);
  const lassoReads=reads;
  const hypot=vi.spyOn(Math,"hypot");
  for (const s of strokes) contactIntervals(s.points,Array(60).fill(0.5),false,
    [{from:{x:100,y:100},to:{x:110,y:100},radius:1}]);
  const distanceCalls=hypot.mock.calls.length;
  hypot.mockRestore();
  let lengthReads=0;
  const counted=strokes.map(s=>({...s,points:new Proxy(s.points,{get(target,key,receiver){if(key==="length")lengthReads++;return Reflect.get(target,key,receiver);}})}));
  counted.forEach(strokeBounds);
  const options={newId:()=>"new",limits:{maxStrokes:20000,maxPoints:750000}};
  const miss=[{from:{x:100000,y:100000},to:{x:100001,y:100000},radius:1}];
  eraseThrough(counted,miss,options); lengthReads=0;
  expect(eraseThrough(counted,miss,options).strokes).toBe(counted);
  const dense=strokes.slice(0,12).map(s=>({...s,transform:{...s.transform,translateY:0}}));
  let id=0;
  const output=eraseThrough(dense,[{from:{x:20,y:-5},to:{x:20,y:5},radius:2},{from:{x:40,y:-5},to:{x:40,y:5},radius:2}],{newId:()=>`cut-${id++}`});
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(JSON.stringify({...output,replaced:[...output.replaced]})));
  const hash = Array.from(new Uint8Array(digest), byte => byte.toString(16).padStart(2,"0")).join("");
  expect({hitReads,lassoReads,distanceCalls,lengthReads,hash}).toEqual({hitReads:0,lassoReads:0,distanceCalls:0,lengthReads:0,
    hash:"ba3a3bd20fab7796e9bba491ebb17efb1d044722a8aec133d5117d443e4765fa"});
});

