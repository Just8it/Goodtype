import { afterEach, expect, it, vi } from "vitest";
import { refreshPageRaster } from "./refreshPageRaster";

afterEach(() => vi.unstubAllGlobals());

function setup(zooming: boolean) {
  let nextId = 0;
  const frames = new Map<number, FrameRequestCallback>();
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    frames.set(++nextId, callback);
    return nextId;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
  const properties = new Map<string, string>();
  const node = { style: {
    setProperty: (name: string, value: string) => properties.set(name, value),
    removeProperty: (name: string) => properties.delete(name),
  } } as unknown as HTMLElement;
  const action = refreshPageRaster(node, zooming);
  return { action, properties, frames, tick() {
    const callbacks = [...frames.values()];
    frames.clear();
    callbacks.forEach(callback => callback(0));
  } };
}

it("refreshes mounted pages and settled zooms for one painted frame, never during a pinch", () => {
  const { action, properties, frames, tick } = setup(false);
  expect(properties.get("--page-layer-hint")).toBe("auto");
  tick();
  expect(properties.get("--page-layer-hint")).toBe("auto");
  tick();
  expect(properties.size).toBe(0);
  action.update(true);
  action.update(true);
  expect(frames.size).toBe(0);
  action.update(false);
  expect(properties.get("--page-layer-hint")).toBe("auto");
  tick();
  tick();
  expect(properties.size).toBe(0);
});

it("cancels pending refreshes when zoom resumes or the page unmounts", () => {
  const { action, properties, frames, tick } = setup(false);
  tick();
  action.update(true);
  expect(frames.size).toBe(0);
  expect(properties.size).toBe(0);
  action.update(false);
  action.destroy();
  expect(frames.size).toBe(0);
  expect(properties.size).toBe(0);
});
