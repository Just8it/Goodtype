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

describe("helper edit generation", () => {
  it("counts source edits before debounce and does not count a persistence flush as an edit", async () => {
    const { session } = workspace(); const initial = session.editGeneration;
    session.addTypstBlock(); expect(session.editGeneration).toBeGreaterThan(initial);
    const id = session.typstBlocks[0].id; const before = session.editGeneration;
    session.updateTypstSource(id, "hello"); expect(session.editGeneration).toBe(before + 1);
    const generation = session.editGeneration; expect(await session.persist()).toBe(true);
    expect(session.editGeneration).toBe(generation);
  });
  it("counts stroke commits before debounce and not again on flush", async () => {
    const { session } = workspace(); const before = session.editGeneration;
    session.scheduleInkCommit("ink"); expect(session.editGeneration).toBe(before + 1);
    expect(await session.persist()).toBe(true); expect(session.editGeneration).toBe(before + 1);
  });
  it("counts immediate transforms and neighbor ink", () => {
    const { session } = workspace(); const before = session.editGeneration;
    session.queueCommit("transform"); expect(session.editGeneration).toBe(before + 1);
    session.commitNeighborInk("page-002", [], "neighbor"); expect(session.editGeneration).toBe(before + 2);
  });
});
