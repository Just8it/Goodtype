import { afterEach, describe, expect, it, vi } from "vitest";
import { createSelection, type SelectionEnvironment } from "./selectionEditing.svelte";
import { DEFAULT_SETTINGS } from "../settings";
import type { Stroke } from "../model";
import type { ClipboardBundle } from "./clipboard";

const store = vi.hoisted(() => vi.fn());
vi.mock("../ipc/notebook", () => ({ storePastedImage: store }));
afterEach(() => { vi.resetAllMocks(); vi.unstubAllGlobals(); });

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}
function workspace() {
  const env = {
    activeInkLayerId: "ink", activeInkLayerPath: "ink/page.ink", activePageId: "page-001",
    activeSnapshot: null, changeStrokes: vi.fn(), closeToolCard: vi.fn(), images: [], message: String,
    navigation: {}, nextReadingOrder: () => 0, nextVisualZIndex: () => 0, pageInputLocked: false,
    queueCommit: vi.fn(), root: "notes", scheduleInkCommit: vi.fn(), selectedInk: [], selectedShapes: [],
    settings: DEFAULT_SETTINGS, shapes: [], status: "", strokes: [{ id: "original", points: [] } as unknown as Stroke],
    tauriAvailable: true, tool: "select", typstBlocks: [], zoom: 1,
    editContext() {
      const root = env.root, pageId = env.activePageId;
      return { root, pageId, current: () => root === env.root && pageId === env.activePageId };
    },
  } as unknown as { -readonly [K in keyof SelectionEnvironment]: SelectionEnvironment[K] };
  const selection = createSelection(env);
  selection.selectedStrokeIds = ["original"];
  return { env, selection };
}

describe("selection asynchronous edits", () => {
  it("waits for clipboard confirmation before cutting and keeps originals on failure", async () => {
    const { env, selection } = workspace();
    const confirmed = deferred<void>(), started = deferred<void>();
    const writeText = vi.fn(() => { started.resolve(); return confirmed.promise; });
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    const cut = selection.copySelection(true); await started.promise;
    expect(env.strokes).toHaveLength(1); expect(env.queueCommit).not.toHaveBeenCalled();
    confirmed.resolve(); await cut;
    expect(env.strokes).toHaveLength(0); expect(env.queueCommit).toHaveBeenCalledOnce();

    const next = workspace();
    writeText.mockRejectedValueOnce(new Error("permission denied"));
    await next.selection.copySelection(true);
    expect(next.env.strokes).toHaveLength(1); expect(next.env.queueCommit).not.toHaveBeenCalled();
  });

  it.each(["page", "content"])("does not cut when %s changes during clipboard confirmation", async change => {
    const { env, selection } = workspace();
    const confirmed = deferred<void>(), started = deferred<void>();
    vi.stubGlobal("navigator", { clipboard: { writeText: () => { started.resolve(); return confirmed.promise; } } });
    const cut = selection.copySelection(true); await started.promise;
    if (change === "page") env.activePageId = "page-002";
    else env.strokes = env.strokes.map(stroke => ({ ...stroke, color: "#ff0000" }));
    confirmed.resolve(); await cut;
    expect(env.strokes).toHaveLength(1); expect(env.queueCommit).not.toHaveBeenCalled();
  });

  it.each(["root", "activePageId"] as const)("ignores a delayed image paste after %s changes", async field => {
    const { env, selection } = workspace();
    const saved = deferred<string>(), started = deferred<void>();
    store.mockImplementation(() => { started.resolve(); return saved.promise; });
    const bundle: ClipboardBundle = { token: "copy", pageId: "page-001", strokes: [], blocks: [], shapes: [],
      images: [{ bytes: new Uint8Array([1]), mime: "image/png", alt: "Image", x: 0, y: 0,
        widthPt: 100, heightPt: 80, scale: 1 }] };
    const paste = selection.pasteBundle(bundle); await started.promise;
    env[field] = "other";
    saved.resolve("assets/pasted.png"); await paste;
    expect(env.images).toEqual([]); expect(env.queueCommit).not.toHaveBeenCalled();
  });
});
