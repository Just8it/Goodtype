import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createSession, type SessionEnvironment } from "./notebookSession.svelte";
import { blankNotebookSnapshot } from "./newNotebook";
import { DEFAULT_SETTINGS } from "../settings";
import type { NotebookSnapshot } from "../ipc/types";
import type { Stroke } from "../model";

const ipc = vi.hoisted(() => ({
  open: vi.fn(), page: vi.fn(), focus: vi.fn(), commit: vi.fn(), history: vi.fn(), structure: vi.fn(),
  move: vi.fn(), add: vi.fn(),
}));
vi.mock("../ipc/notebook", async original => ({ ...await original<typeof import("../ipc/notebook")>(),
  openNotebook: ipc.open, openPage: ipc.page, focusPage: ipc.focus, commitNotebook: ipc.commit,
  runHistory: ipc.history, runStructureHistory: ipc.structure, createPage: ipc.add,
}));
vi.mock("../ipc/library", () => ({ moveLibraryEntries: ipc.move }));
vi.mock("../ipc/workspace", () => ({
  listRecentNotebooks: async () => [], recordNotebookOpened: async () => {},
  recordNotebookPage: async () => {}, recordNotebookSession: async () => {},
}));
vi.mock("../ipc/presets", () => ({ listTypstPresets: async () => [] }));
vi.mock("../ipc/recovery", () => ({ listRecoveryCandidates: async () => [] }));
vi.mock("../library/cover", () => ({ notebookCover: async () => null }));
vi.mock("../pdf/document", () => ({ releasePdfDocuments: async () => {} }));

const sessions: ReturnType<typeof createSession>[] = [];
beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("document", { querySelector: () => null });
  vi.stubGlobal("requestAnimationFrame", vi.fn(() => 1));
  vi.stubGlobal("cancelAnimationFrame", vi.fn());
  ipc.move.mockResolvedValue({ rootMoves: [{ from: "old", to: "new" }], warning: null });
  ipc.open.mockResolvedValue(snapshot());
  ipc.page.mockImplementation(async (_root, id) => snapshot(id));
  ipc.focus.mockImplementation(async (_root, id) => result(snapshot(id)));
  ipc.history.mockImplementation(async (_root, _command, id) => result(snapshot(id)));
  ipc.structure.mockResolvedValue(result(snapshot()));
  ipc.add.mockResolvedValue(result(snapshot()));
  ipc.commit.mockImplementation(async (_root, value) => result(value));
});
afterEach(() => {
  for (const session of sessions.splice(0)) session.dispose();
  vi.useRealTimers(); vi.resetAllMocks(); vi.unstubAllGlobals();
});

function snapshot(pageId = "page-001"): NotebookSnapshot {
  const value = blankNotebookSnapshot({ name: "Notes", geometry: { widthPt: 595, heightPt: 842 },
    background: { kind: "plain", color: "#ffffff" }, preset: { kind: "none" } });
  value.manifest.pages = ["page-001", "page-002", "page-003"].map(id => ({
    id, path: `pages/${id}.json`, geometry: value.page.geometry,
  }));
  value.page.id = pageId;
  value.inkLayers.forEach(layer => { layer.pageId = pageId; });
  return value;
}
function result(value: NotebookSnapshot) { return { snapshot: value, canUndo: false, canRedo: false }; }
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
}
function workspace() {
  const env = {
    addPageOpen: false, collectMetrics: false, followPageText: vi.fn(), message: String, moreOpen: false,
    navigation: { stopTouchInertia: vi.fn(), pagePointers: new Map(), announceTouchHistory: vi.fn() },
    pageViewport: undefined, paletteDragging: false, previews: { reset: vi.fn() }, resetWorkspaceEditors: vi.fn(),
    saveTemplate: vi.fn(async () => true), searchOpen: false,
    selection: { groupedStrokeIds: [], updateInkSelection: vi.fn() },
    settings: { ...DEFAULT_SETTINGS, undoScope: "notebook" }, shapeView: vi.fn(), sideEditor: undefined,
    sideEditorOpen: false, tauriAvailable: true, tool: "pen", windowCloseInProgress: false,
  } as unknown as SessionEnvironment;
  const session = createSession(env);
  sessions.push(session);
  session.root = "old";
  session.applySnapshot(snapshot());
  session.busy = false; session.pageOpen = true; session.notebookChosen = true;
  session.openTabs = [{ root: "old", title: "Notes" }];
  return { session, env };
}

describe("notebook session coordination", () => {
  it.each([true, false])("preserves structural Undo across relocation, reopen=%s", async reopened => {
    const { session } = workspace();
    await session.addPage({ kind: "last" });
    if (!reopened) ipc.open.mockRejectedValueOnce(new Error("temporarily unreadable"));
    await session.moveLibrarySelection(["Notes"], "Archive");
    await session.routeHistory("undo_notebook", "Undo");
    await session.persist();
    expect(ipc.structure).toHaveBeenCalledWith("new", "undo_page_structure", expect.any(String));
    expect(ipc.history).not.toHaveBeenCalled();
    expect(session.transactionFailed).toBe(false);
    expect(session.root).toBe("new");
  });

  it("keeps notebook-wide Undo and Redo on the page that changed after a move", async () => {
    const { session } = workspace();
    await session.activatePage("page-002");
    session.queueCommit("Ink"); await session.persist();
    await session.activatePage("page-001");
    await session.moveLibrarySelection(["Notes"], "Archive");
    await session.routeHistory("undo_notebook", "Undo"); await session.persist();
    expect(ipc.history).toHaveBeenLastCalledWith("new", "undo_notebook", "page-002");
    await session.activatePage("page-001");
    await session.routeHistory("redo_notebook", "Redo"); await session.persist();
    expect(ipc.history).toHaveBeenLastCalledWith("new", "redo_notebook", "page-002");
    expect(session.transactionFailed).toBe(false);
  });

  it("keeps newer neighbor ink visible while an older save is in flight", async () => {
    const { session } = workspace();
    vi.stubGlobal("IntersectionObserver", class {
      constructor(private callback: (entries: { isIntersecting: boolean }[]) => void) {}
      observe() { this.callback([{ isIntersecting: true }]); }
      disconnect() {}
    });
    const observer = session.observePage({} as HTMLElement, "page-002");
    await vi.waitFor(() => expect(session.pageEntries.find(page => page.id === "page-002")?.snapshot).not.toBeNull());
    observer.destroy();
    const first = [{ id: "a", points: [] } as unknown as Stroke];
    const second = [...first, { id: "b", points: [] } as unknown as Stroke];
    const saving = deferred<ReturnType<typeof result>>();
    const started = deferred<void>();
    ipc.commit.mockImplementationOnce(() => { started.resolve(); return saving.promise; });
    session.commitNeighborInk("page-002", first, "Ink");
    const flush = session.persist(); await started.promise;
    session.commitNeighborInk("page-002", second, "Ink");
    saving.resolve(result(ipc.commit.mock.calls[0][1])); await flush;
    const entry = session.pageEntries.find(page => page.id === "page-002")!;
    expect(session.neighborStrokesFor(entry)).toBe(second);
    expect(session.pendingNeighborPages.has("page-002")).toBe(true);
    await session.persist();
    expect(ipc.commit.mock.calls[1][1].inkLayers[0].strokes).toHaveLength(2);
  });

  it("does not apply a focus reply over typing that happened during the request", async () => {
    const { session } = workspace();
    const focusing = deferred<ReturnType<typeof result>>(), started = deferred<void>();
    ipc.focus.mockImplementationOnce(() => { started.resolve(); return focusing.promise; });
    const activation = session.activatePage("page-002", true); await started.promise;
    session.updateSharedStyle("new local source");
    focusing.resolve(result(snapshot("page-002"))); await activation;
    expect(session.activePageId).toBe("page-001");
    expect(session.sharedStyleSource).toBe("new local source");
    await session.persist();
    expect(ipc.commit).toHaveBeenCalled();
  });

  it("refuses close or switching when the template buffer cannot save", async () => {
    const { session, env } = workspace();
    vi.mocked(env.saveTemplate).mockResolvedValue(false);
    await session.closeNotebookTab("old");
    expect(session.openTabs).toHaveLength(1);
    expect(await session.openNotebookAt("other")).toBe(false);
    expect(ipc.open).not.toHaveBeenCalled();
    expect(session.root).toBe("old");
  });
});
