import { afterEach, describe, expect, it, vi } from "vitest";
import { createNavigation, type NavigationEnvironment } from "./viewportNavigation.svelte";
import { DEFAULT_SETTINGS } from "../settings";

// The Node/Svelte server export intentionally skips flushSync callbacks. Here the DOM layout
// boundary is a controlled box; run its update as the browser export does.
vi.mock("svelte", async original => ({ ...await original<typeof import("svelte")>(),
  flushSync: (update: () => void) => update(),
}));

afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

function workspace() {
  const env = {
    settings: DEFAULT_SETTINGS, zoom: 2, collectMetrics: false,
    pageFrame: { getBoundingClientRect: () => ({ left: 100, top: 200, right: 500, bottom: 800 }) },
    pageViewport: { scrollLeft: 10, scrollTop: 20 },
  } as NavigationEnvironment;
  return { env, navigation: createNavigation(env) };
}

describe("viewport navigation", () => {
  it("maps pointer locations through the coordinate authority and clamps margin pastes", () => {
    const { navigation, env } = workspace();
    expect(navigation.pagePointAt(180, 300)).toEqual({ x: 40, y: 50 });
    expect(navigation.pagePointAt(-100, 900)).toEqual({ x: 0, y: 300 });
    env.zoom = 0;
    expect(() => navigation.pagePointAt(180, 300)).toThrow();
  });

  it("coalesces zoom requests and preserves the page point under the pointer", () => {
    const { navigation, env } = workspace();
    let frame!: FrameRequestCallback;
    vi.stubGlobal("requestAnimationFrame", vi.fn(callback => { frame = callback; return 7; }));
    navigation.zoomAt(3, 180, 300);
    navigation.zoomAt(4, 180, 300);
    expect(requestAnimationFrame).toHaveBeenCalledOnce();
    frame(0);
    expect(env.zoom).toBe(4);
    // (40, 50) page points moved from (180, 300) to (260, 400); scroll offsets restore it.
    expect(env.pageViewport?.scrollLeft).toBe(90);
    expect(env.pageViewport?.scrollTop).toBe(120);
  });

  it("cancels queued zoom, long press, and history feedback on disposal", async () => {
    vi.useFakeTimers();
    const { navigation, env } = workspace();
    vi.stubGlobal("requestAnimationFrame", vi.fn(() => 7));
    vi.stubGlobal("cancelAnimationFrame", vi.fn());
    navigation.zoomAt(3, 180, 300);
    navigation.announceTouchHistory("Undo");
    navigation.dispose();
    expect(cancelAnimationFrame).toHaveBeenCalledWith(7);
    expect(vi.getTimerCount()).toBe(0);
    expect(env.zoom).toBe(2);
  });
});
