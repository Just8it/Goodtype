import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CalculationCell, CalculationDocument, CalculationResult } from "../ipc/calculation";
import type { CalculationReference } from "../model";
import type { CellEditor } from "./cellEditor";

const ipc = vi.hoisted(() => ({
  discover: vi.fn(), useLocal: vi.fn(),
  plan: vi.fn(), edit: vi.fn(), run: vi.fn(), history: vi.fn(), end: vi.fn(), status: vi.fn(), read: vi.fn(), layout: vi.fn(),
}));
vi.mock("../ipc/calculation", () => ({
  discoverPythonRuntimes: ipc.discover, usePythonRuntime: ipc.useLocal,
  calculationExecutionPlan: ipc.plan, editCalculation: ipc.edit, runCalculationCell: ipc.run, calculationHistory: ipc.history, endCalculationSession: ipc.end,
  pythonRuntimeStatus: ipc.status, readCalculation: ipc.read, layoutCalculation: ipc.layout,
  installPythonRuntime: vi.fn(), cancelPythonInstall: vi.fn(),
}));
import { createCalculationWorkspace } from "./calculationWorkspace.svelte";

const reference: CalculationReference = { id: "calc", sourcePath: "calculations/calc.ipynb", partId: "part" };
const root = "C:/notebook";
const target = { root, pageId: "page", reference };

function cell(id: string, source = "", type: "code" | "markdown" = "code", partId = "part", outputs: CalculationCell["outputs"] = []): CalculationCell {
  return { id, cell_type: type, source, outputs, execution_count: null, metadata: { goodtype: { partId } } };
}
function document(revision: number, cells: CalculationCell[]): CalculationDocument {
  return { nbformat: 4, nbformat_minor: 5, metadata: { goodtype: { id: "calc", title: "Test", revision, parts: [{ id: "part", title: "Part" }] } }, cells };
}
const result = (doc: CalculationDocument, canUndo = true): CalculationResult => ({ reference, document: doc, canUndo, canRedo: false });
function snapshot(doc: CalculationDocument) {
  return { page: { calculation: reference }, calculations: [{ path: reference.sourcePath, bytes: Array.from(new TextEncoder().encode(JSON.stringify(doc))) }] } as never;
}

/** A DOM-free stand-in that keeps the same state contract as the real editor. */
function fakeEditor() {
  const states = new Map<string, { text: string; type: string }>();
  let active = "";
  let change: (key: string, source: string) => void = () => {};
  const make = (onChange: (key: string, source: string) => void): CellEditor => {
    change = onChange;
    return {
      select: (key, source, type) => { active = key; const cached = states.get(key); if (!cached || cached.type !== type) states.set(key, { text: source, type }); },
      reset: vi.fn((key: string, source: string, type: string) => { states.set(key, { text: source, type }); }),
      forget: vi.fn((key: string) => { states.delete(key); if (key === active) active = ""; }),
      release: () => { active = ""; },
      mount: () => ({ destroy() {} }),
      settleComposition: async () => {},
      has: key => states.has(key),
      text: key => states.get(key)?.text,
      type: key => states.get(key)?.type,
      keys: () => [...states.keys()],
      focus: () => {},
      dispose: () => {},
    } as CellEditor;
  };
  return { make, type: (text: string) => { states.get(active)!.text = text; change(active, text); }, states, key: () => active };
}

function setup(initial: CalculationDocument) {
  const fake = fakeEditor();
  const saved = vi.fn();
  const workspace = createCalculationWorkspace(saved, vi.fn(), fake.make);
  workspace.ingest(root, snapshot(initial));
  return { workspace, fake, saved };
}
const cellTarget = (cellId: string) => ({ ...target, cellId });

beforeEach(() => {
  vi.useFakeTimers();
  Object.values(ipc).forEach(mock => mock.mockReset());
  ipc.status.mockResolvedValue({ installed: true, installing: false, downloadsAllowed: true, progress: "", downloadBytes: 0 });
  ipc.end.mockResolvedValue(undefined);
  ipc.discover.mockResolvedValue([]);
});
afterEach(() => vi.useRealTimers());

describe("local Python", () => {
  it("stops the queued page run when its notebook closes", async () => {
    const { workspace } = setup(document(1, [cell("a"), cell("b")]));
    let finish!: () => void;
    ipc.run.mockImplementationOnce(() => new Promise(resolve => { finish = () => resolve(result(document(2, [cell("a"), cell("b")]))); }));
    const running = workspace.runPage(target);
    await vi.advanceTimersByTimeAsync(0);
    workspace.forgetRoot(root);
    finish();
    expect(await running).toBe(false);
    expect(ipc.run).toHaveBeenCalledOnce();
    expect(workspace.runningIn(root)).toBe(false);
    workspace.dispose();
  });
  it("uses a local installation with downloads disabled and resumes the pending run", async () => {
    const { workspace } = setup(document(1, [cell("a", "x = 2")]));
    ipc.status.mockResolvedValue({ installed: false, installing: false, downloadsAllowed: false });
    ipc.discover.mockResolvedValue([{ id: "candidate", executable: "C:/Python/python.exe" }]);
    await workspace.runCell(cellTarget("a"));
    await workspace.discoverLocal();
    expect(workspace.candidates).toHaveLength(1);
    ipc.useLocal.mockResolvedValue({ code: "compatible", missing: [], provenance: {}, selected: true });
    ipc.status.mockResolvedValue({ installed: true, installing: false, downloadsAllowed: false });
    ipc.run.mockResolvedValue(result(document(2, [cell("a", "x = 2")])));
    await workspace.useLocal("candidate");
    await vi.advanceTimersByTimeAsync(0);
    expect(ipc.useLocal).toHaveBeenCalledWith("candidate");
    expect(ipc.run).toHaveBeenCalledOnce();
    expect(workspace.installOpen).toBe(false);
    workspace.dispose();
  });
  it("keeps the install dialog open when a local library is missing", async () => {
    const { workspace } = setup(document(1, [cell("a")]));
    workspace.installOpen = true;
    ipc.useLocal.mockResolvedValue({ code: "missing_packages", missing: ["numpy"], provenance: {}, selected: false });
    await workspace.useLocal("candidate");
    expect(workspace.installOpen).toBe(true);
    expect(workspace.installMessage).toBeTruthy();
    expect(ipc.run).not.toHaveBeenCalled();
    workspace.dispose();
  });
});

describe("document adoption", () => {
  it("never rolls the page back to an older revision", () => {
    const { workspace } = setup(document(5, [cell("a", "new")]));
    workspace.ingest(root, snapshot(document(3, [cell("a", "old")])));
    expect(workspace.documentFor(root, reference).cells[0].source).toBe("new");
  });

  it("discards the cached editor state after canonical history changes the source", async () => {
    const { workspace, fake } = setup(document(1, [cell("a", "x = 1")]));
    await workspace.choose(cellTarget("a"), true, async () => {});
    ipc.history.mockResolvedValue(result(document(2, [cell("a", "")])));
    expect(await workspace.history(target, true)).toBe(true);
    expect(fake.states.get(fake.key())?.text).toBe("");
  });

  it("rebuilds the editor when the cell type changes and forgets deleted cells", async () => {
    const { workspace, fake } = setup(document(1, [cell("a", "text"), cell("b", "keep")]));
    await workspace.choose(cellTarget("a"), true, async () => {});
    workspace.ingest(root, snapshot(document(2, [cell("a", "text", "markdown"), cell("b", "keep")])));
    expect(fake.states.get(fake.key())?.type).toBe("markdown");
    workspace.ingest(root, snapshot(document(3, [cell("b", "keep")])));
    expect(workspace.selected).toBeNull();
    expect([...fake.states.keys()].some(key => key.endsWith("\0a"))).toBe(false);
  });

  it("keeps an unsaved draft when a canonical document arrives", async () => {
    const { workspace, fake } = setup(document(1, [cell("a", "x")]));
    await workspace.choose(cellTarget("a"), true, async () => {});
    fake.type("x + 1");
    workspace.ingest(root, snapshot(document(2, [cell("a", "x"), cell("b", "")])));
    expect(fake.states.get(fake.key())?.text).toBe("x + 1");
  });
});

describe("saving", () => {
  it("saves a draft against the known source and keeps a newer keystroke pending", async () => {
    const { workspace, fake } = setup(document(1, [cell("a", "x")]));
    await workspace.choose(cellTarget("a"), true, async () => {});
    fake.type("xy");
    let release!: () => void;
    ipc.edit.mockImplementation(() => new Promise(resolve => { release = () => resolve(result(document(2, [cell("a", "xy")]))); }));
    const flushing = workspace.flush();
    await vi.advanceTimersByTimeAsync(0);
    fake.type("xyz");
    release();
    ipc.edit.mockResolvedValue(result(document(3, [cell("a", "xyz")])));
    expect(await flushing).toBe(true);
    expect(ipc.edit).toHaveBeenCalledTimes(2);
    expect(ipc.edit.mock.calls[0][2]).toMatchObject({ kind: "source", expectedSource: "x", source: "xy" });
    expect(ipc.edit.mock.calls[1][2]).toMatchObject({ expectedSource: "xy", source: "xyz" });
  });

  it("reports an unsaved draft after a failure and recovers on retry", async () => {
    const { workspace, fake } = setup(document(1, [cell("a", "x")]));
    await workspace.choose(cellTarget("a"), true, async () => {});
    fake.type("y");
    ipc.edit.mockRejectedValueOnce(new Error("disk full"));
    expect(await workspace.flush()).toBe(false);
    expect(workspace.failed).toBe(true);
    ipc.edit.mockResolvedValue(result(document(2, [cell("a", "y")])));
    expect(await workspace.flush()).toBe(true);
    expect(workspace.failed).toBe(false);
  });

  it("does not drop a structural change when a draft is waiting", async () => {
    const { workspace, fake } = setup(document(1, [cell("a", "x")]));
    await workspace.choose(cellTarget("a"), true, async () => {});
    fake.type("y");
    ipc.edit.mockResolvedValueOnce(result(document(2, [cell("a", "y")]))).mockResolvedValueOnce(result(document(3, [])));
    expect(await workspace.deleteCell(cellTarget("a"))).toBe(true);
    expect(ipc.edit.mock.calls.map(call => call[2].kind)).toEqual(["source", "delete"]);
  });
});

describe("selection", () => {
  it("lets the newest choice win over a slower page activation", async () => {
    const { workspace } = setup(document(1, [cell("a"), cell("b")]));
    let finish!: () => void;
    const slow = workspace.choose(cellTarget("a"), false, () => new Promise<void>(resolve => { finish = resolve; }));
    const fast = workspace.choose(cellTarget("b"), true, async () => {});
    expect(await fast).toBe(true);
    finish();
    expect(await slow).toBe(false);
    expect(workspace.selected?.cellId).toBe("b");
  });

  it("refuses a cell that vanished during activation", async () => {
    const { workspace } = setup(document(1, [cell("a")]));
    expect(await workspace.choose(cellTarget("gone"), false, async () => {})).toBe(false);
    expect(workspace.selected).toBeNull();
  });
});

describe("running", () => {
  const output = (name?: string) => (name ? [{ output_type: "error" as const, ename: name, evalue: "x" }] : [{ output_type: "stream" as const, text: "ok" }]);

  it("runs only the code cells of this part in order and stops at the first error", async () => {
    const { workspace } = setup(document(1, [cell("a"), cell("note", "", "markdown"), cell("other", "", "code", "elsewhere"), cell("b"), cell("c")]));
    let revision = 1;
    ipc.run.mockImplementation(async (_root, _page, id: string) => result(document(++revision, [cell(id, "", "code", "part", output(id === "b" ? "NameError" : undefined))])));
    await workspace.runPage(target);
    expect(ipc.run.mock.calls.map(call => call[2])).toEqual(["a", "b"]);
    expect(workspace.runningFor(root, reference)).toBe(false);
    expect(workspace.sessionFor(root, reference)).toBe("live");
  });

  it("rejects a second run while one is active and refuses markdown cells", async () => {
    const { workspace } = setup(document(1, [cell("a"), cell("note", "", "markdown")]));
    let finish!: () => void;
    ipc.run.mockImplementation(() => new Promise(resolve => { finish = () => resolve(result(document(2, [cell("a", "", "code", "part", output())]))); }));
    const first = workspace.runCell(cellTarget("a"));
    expect(await workspace.runCell(cellTarget("a"))).toBe(false);
    await vi.advanceTimersByTimeAsync(0);
    finish();
    expect(await first).toBe(true);
    expect(await workspace.runCell(cellTarget("note"))).toBe(false);
    expect(ipc.run).toHaveBeenCalledTimes(1);
  });

  it("opens the install dialog and resumes the run after installing", async () => {
    const { workspace } = setup(document(1, [cell("a")]));
    ipc.status.mockResolvedValue({ installed: false, installing: false, downloadsAllowed: true, progress: "", downloadBytes: 1 });
    expect(await workspace.runCell(cellTarget("a"))).toBe(false);
    expect(workspace.installOpen).toBe(true);
    expect(ipc.run).not.toHaveBeenCalled();
    ipc.status.mockResolvedValue({ installed: true, installing: false, downloadsAllowed: true, progress: "", downloadBytes: 1 });
    ipc.run.mockResolvedValue(result(document(2, [cell("a", "", "code", "part", output())])));
    await workspace.install();
    await vi.advanceTimersByTimeAsync(0);
    expect(workspace.installOpen).toBe(false);
    expect(ipc.run).toHaveBeenCalledTimes(1);
  });

  it("lets a stopped run end without cancelling the run that replaced it", async () => {
    const { workspace } = setup(document(1, [cell("a"), cell("b")]));
    let rejectFirst!: (error: Error) => void;
    ipc.run.mockImplementationOnce(() => new Promise((_, reject) => { rejectFirst = reject; }));
    const first = workspace.runCell(cellTarget("a"));
    await vi.advanceTimersByTimeAsync(0);
    await workspace.endSession(target);
    expect(ipc.end).toHaveBeenCalledWith(root, "calc");
    let finishSecond!: () => void;
    ipc.run.mockImplementationOnce(() => new Promise(resolve => { finishSecond = () => resolve(result(document(2, [cell("b", "", "code", "part", output())]))); }));
    const second = workspace.runCell(cellTarget("b"));
    await vi.advanceTimersByTimeAsync(0);
    rejectFirst(new Error("Python session ended"));
    await first;
    expect(workspace.runningFor(root, reference)).toBe(true);
    finishSecond();
    expect(await second).toBe(true);
  });

  it("restarts by ending the session before running the page", async () => {
    const { workspace } = setup(document(1, [cell("a")]));
    ipc.plan.mockResolvedValue({ revision: 1, steps: [{ cellId: "a", pageId: "page", partId: "part" }] });
    ipc.run.mockResolvedValue(result(document(2, [cell("a", "", "code", "part", output())])));
    await workspace.restartAndRun(target);
    expect(ipc.end.mock.invocationCallOrder[0]).toBeLessThan(ipc.run.mock.invocationCallOrder[0]);
  });
  it("runs shared parts through their owning pages in calculation order", async () => {
    const cells = [cell("first", "x = 2"), cell("second", "print(x)", "code", "part-2")];
    const { workspace } = setup(document(1, cells));
    ipc.plan.mockResolvedValue({ revision: 1, steps: [
      { cellId: "first", pageId: "earlier", partId: "part" },
      { cellId: "second", pageId: "later", partId: "part-2" },
    ] });
    let revision = 1;
    ipc.run.mockImplementation(async () => result(document(++revision, cells)));
    expect(await workspace.runCalculation(target)).toBe(true);
    expect(ipc.run.mock.calls.map(call => [call[1], call[2]])).toEqual([["earlier", "first"], ["later", "second"]]);
  });

});
