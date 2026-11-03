import { get } from "svelte/store";
import type { NotebookSnapshot } from "../ipc/types";
import type { CalculationReference } from "../model";
import {
  calculationExecutionPlan,
  discoverPythonRuntimes, usePythonRuntime, type PythonCandidate,
  pythonRuntimeStatus, installPythonRuntime, cancelPythonInstall, forgetLocalPython, endCalculationSession, runCalculationCell,
  layoutCalculation, editCalculation, calculationHistory, readCalculation,
  type PythonRuntimeStatus, type CalculationLayout, type CalculationCell, type CalculationDocument, type CalculationResult, type CellOperation,
} from "../ipc/calculation";
import { notice, locale, type UiText } from "../i18n";
import { errorDetails } from "../ipc/errors";
import { createCellEditor, type CellEditor } from "./cellEditor";

export type Target = { root: string; pageId: string; reference: CalculationReference; cellId: string };
export type PageTarget = Omit<Target, "cellId">;
type Draft = { target: Target; source: string };
export type SessionState = "idle" | "live" | "running";

const docKey = (root: string, id: string) => `${root}\0${id}`;
const cellKey = (target: Target) => `${docKey(target.root, target.reference.id)}\0${target.cellId}`;

/** Controller for computational pages. Rust owns the document; this holds drafts, selection and run state only. */
export function createCalculationWorkspace(
  saved: (pageId: string, document: CalculationDocument, action: "change" | "undo" | "redo" | "observe", root: string) => void,
  height: (pageId: string, value: number, root: string) => void,
  makeEditor: (onChange: (key: string, source: string) => void, onRun: (advance: boolean) => void) => CellEditor = createCellEditor,
) {
  let runtime = $state<PythonRuntimeStatus | null>(null);
  let installing = $state(false);
  let installOpen = $state(false);
  let installMessage = $state<UiText | null>(null);
  let candidates = $state.raw<PythonCandidate[]>([]);
  let discovering = $state(false);
  let removing = $state(false);
  let afterInstall: (() => void) | null = null;
  let runs = $state.raw<Record<string, { token: number; cellId: string | null }>>({});
  let live = $state.raw<Record<string, boolean>>({});
  let statuses = $state.raw<Record<string, UiText>>({});
  let flags = $state.raw<Record<string, { canUndo: boolean; canRedo: boolean }>>({});
  let documents = $state.raw<Record<string, CalculationDocument>>({});
  let layouts = $state.raw<Record<string, CalculationLayout>>({});
  let drafts = $state.raw<Record<string, Draft>>({});
  let selected = $state<Target | null>(null);
  let failed = $state(false);
  let pending = $state(0);
  const layoutRequests = new Map<string, string>();
  let disposed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let queue: Promise<unknown> = Promise.resolve();
  let runCounter = 0;
  let selectToken = 0;

  const editor = makeEditor((id, source) => {
    const target = selected;
    if (!target || cellKey(target) !== id) return;
    drafts = { ...drafts, [id]: { target, source } };
    say(target.root, target.reference.id, notice("calculation.unsaved"));
    clearTimeout(timer);
    timer = setTimeout(() => { void flush(); }, 500);
  }, advance => { void runSelected(advance); });

  function say(root: string, id: string, message: UiText) { statuses = { ...statuses, [docKey(root, id)]: message }; }

  function ingest(root: string, snapshot: NotebookSnapshot) {
    const reference = snapshot.page.calculation;
    const file = snapshot.calculations?.find(file => file.path === reference?.sourcePath);
    if (!reference || !file) return;
    try {
      const document = JSON.parse(new TextDecoder().decode(new Uint8Array(file.bytes))) as CalculationDocument;
      if (adopt(root, document) === "newer") {
        const next = { ...flags }; delete next[docKey(root, reference.id)]; flags = next;
      }
    } catch { /* The page snapshot is validated by Rust; an unreadable copy keeps the last good document. */ }
  }
  /** Accept a canonical document unless a newer one is already known. A late reply must not roll the page back. */
  function adopt(root: string, document: CalculationDocument): "newer" | "same" | "older" {
    const id = docKey(root, document.metadata.goodtype.id);
    const known = documents[id];
    if (known && document.metadata.goodtype.revision < known.metadata.goodtype.revision) return "older";
    if (known && document.metadata.goodtype.revision === known.metadata.goodtype.revision) return "same";
    documents = { ...documents, [id]: document };
    reconcile(root, document);
    return "newer";
  }
  /** Canonical history, type changes and deletions invalidate cached editor states. Pending drafts always win. */
  function reconcile(root: string, document: CalculationDocument) {
    const prefix = `${docKey(root, document.metadata.goodtype.id)}\0`;
    for (const key of editor.keys()) {
      if (!key.startsWith(prefix)) continue;
      const cell = document.cells.find(cell => cell.id === key.slice(prefix.length));
      if (!cell) {
        editor.forget(key);
        if (drafts[key]) { const next = { ...drafts }; delete next[key]; drafts = next; }
        if (selected && cellKey(selected) === key) selected = null;
        continue;
      }
      if (drafts[key]) continue;
      if (editor.type(key) !== cell.cell_type || editor.text(key) !== cell.source) editor.reset(key, cell.source, cell.cell_type);
    }
  }
  function accept(target: PageTarget, result: CalculationResult, action: "change" | "undo" | "redo" | "observe" = "change"): boolean {
    const outcome = adopt(target.root, result.document);
    if (outcome === "older") return false;
    flags = { ...flags, [docKey(target.root, target.reference.id)]: { canUndo: result.canUndo, canRedo: result.canRedo } };
    if (outcome === "newer") saved(target.pageId, result.document, action, target.root);
    return true;
  }

  async function preview(root: string, pageId: string, reference: CalculationReference, revision: number, language: string) {
    const id = `${docKey(root, reference.id)}\0${reference.partId}`;
    const request = `${revision}:${language}`;
    if (layoutRequests.get(id) === request) return;
    layoutRequests.set(id, request);
    try {
      const layout = await layoutCalculation(root, pageId, language);
      if (disposed || layoutRequests.get(id) !== request || layout.revision !== revision) return;
      layouts = { ...layouts, [id]: layout };
      height(pageId, layout.heightPt, root);
    } catch (error) {
      if (!disposed && layoutRequests.get(id) === request) say(root, reference.id, notice("calculation.layoutFailed", { detail: errorDetails(error) }));
    }
  }
  const layoutFor = (root: string, reference: CalculationReference) => layouts[`${docKey(root, reference.id)}\0${reference.partId}`];
  const documentFor = (root: string, reference: CalculationReference) => documents[docKey(root, reference.id)];
  const cellsFor = (root: string, reference: CalculationReference) =>
    documentFor(root, reference)?.cells.filter(cell => cell.metadata.goodtype.partId === reference.partId) ?? [];
  const source = (target: Target, cell: CalculationCell) => drafts[cellKey(target)]?.source ?? cell.source;

  /** Learn whether undo and redo are available without changing anything. */
  function activated(target: PageTarget) {
    if (flags[docKey(target.root, target.reference.id)]) return;
    void enqueue(async () => { accept(target, await readCalculation(target.root, target.pageId), "observe"); }, target);
  }

  function select(target: Target, cell: CalculationCell) {
    selected = target;
    editor.select(cellKey(target), source(target, cell), cell.cell_type);
  }
  /**
   * Select one cell for editing. The page may have to become active first, so a newer choice wins
   * over a slower earlier one, and the cell must still exist when the activation finishes.
   */
  async function choose(target: Target, active: boolean, activate: () => Promise<void>): Promise<boolean> {
    const token = ++selectToken;
    if (!active) await activate();
    if (disposed || token !== selectToken) return false;
    const cell = cellsFor(target.root, target.reference).find(cell => cell.id === target.cellId);
    if (!cell) return false;
    select(target, cell);
    return true;
  }
  function deselect() {
    selectToken += 1;
    selected = null;
    editor.release();
  }

  function enqueue(operation: () => Promise<void>, target?: { root: string; reference: CalculationReference }): Promise<boolean> {
    pending += 1;
    const attempt = queue.then(operation).then(() => true, error => {
      if (target) say(target.root, target.reference.id, notice("calculation.failed", { detail: errorDetails(error) }));
      return false;
    }).finally(() => { pending -= 1; });
    queue = attempt;
    return attempt;
  }
  async function saveDraft(id: string) {
    const draft = drafts[id];
    if (!draft) return;
    const { target } = draft;
    const cell = documentFor(target.root, target.reference)?.cells.find(cell => cell.id === target.cellId);
    if (cell && cell.source !== draft.source) {
      accept(target, await editCalculation(target.root, target.pageId, {
        kind: "source", cellId: cell.id, expectedSource: cell.source, source: draft.source,
      }));
    }
    // A newer keystroke has replaced the draft object and must stay pending.
    if (drafts[id] === draft) { const next = { ...drafts }; delete next[id]; drafts = next; }
  }
  /** Persist every draft. True means no draft is left unsaved. */
  async function flush(): Promise<boolean> {
    await editor.settleComposition?.();
    clearTimeout(timer);
    for (let round = 0; round < 5 && Object.keys(drafts).length; round += 1) {
      for (const id of Object.keys(drafts)) {
        const target = drafts[id]?.target;
        if (!(await enqueue(() => saveDraft(id), target))) { failed = true; return false; }
      }
    }
    await queue;
    const clean = !Object.keys(drafts).length;
    failed = !clean;
    if (clean && selected) say(selected.root, selected.reference.id, notice("calculation.saved"));
    return clean;
  }

  async function operation(target: PageTarget, change: CellOperation, done: UiText = notice("calculation.saved")): Promise<boolean> {
    if (!(await flush())) return false;
    return enqueue(async () => {
      accept(target, await editCalculation(target.root, target.pageId, change));
      say(target.root, target.reference.id, done);
    }, target);
  }
  async function history(target: PageTarget, undo: boolean, pageScope = true): Promise<boolean> {
    if (runningFor(target.root, target.reference) || !(await flush())) return false;
    return enqueue(async () => {
      accept(target, await calculationHistory(target.root, target.pageId, undo, pageScope), undo ? "undo" : "redo");
      say(target.root, target.reference.id, notice("calculation.memoryUnchanged"));
    }, target);
  }
  const deleteCell = (target: Target) => operation(target, { kind: "delete", cellId: target.cellId }, notice("calculation.cellDeleted"));

  async function refreshRuntime() {
    try { runtime = await pythonRuntimeStatus(); }
    catch (error) { installMessage = notice("calculation.setupFailed", { detail: errorDetails(error) }); }
  }
  async function install() {
    if (installing) return;
    installing = true;
    installMessage = null;
    const poll = setInterval(() => { void refreshRuntime(); }, 1000);
    try {
      await installPythonRuntime();
      await refreshRuntime();
      installOpen = false;
      const resume = afterInstall; afterInstall = null;
      resume?.();
    } catch (error) { installMessage = notice("calculation.setupFailed", { detail: errorDetails(error) }); }
    finally { clearInterval(poll); installing = false; }
  }
  function closeInstall() { installOpen = false; afterInstall = null; installMessage = null; }
  function forgetRoot(root: string) {
    const keep = <T>(values: Record<string, T>) => Object.fromEntries(Object.entries(values).filter(([key]) => !key.startsWith(`${root}\0`)));
    runs = keep(runs); live = keep(live); documents = keep(documents); drafts = keep(drafts);
    flags = keep(flags); layouts = keep(layouts); statuses = keep(statuses);
    for (const key of editor.keys()) if (key.startsWith(`${root}\0`)) editor.forget(key);
    if (selected?.root === root) deselect();
    closeInstall();
  }
  async function discoverLocal() {
    if (discovering || installing) return;
    discovering = true;
    try { candidates = await discoverPythonRuntimes(); }
    catch (error) { installMessage = notice("calculation.setupFailed", { detail: errorDetails(error) }); }
    finally { discovering = false; }
  }
  async function useLocal(candidateId: string) {
    if (installing || discovering) return;
    installing = true;
    installMessage = notice("calculation.checkingLocal");
    const poll = setInterval(() => { void refreshRuntime(); }, 1000);
    try {
      const result = await usePythonRuntime(candidateId);
      if (result.selected) {
        await refreshRuntime();
        installOpen = false;
        const resume = afterInstall; afterInstall = null;
        resume?.();
      } else installMessage = result.code === "missing_packages"
        ? notice("calculation.localMissing", { packages: result.missing.join(", ") })
        : notice("calculation.localIncompatible");
    } catch (error) { installMessage = notice("calculation.setupFailed", { detail: errorDetails(error) }); }
    finally { clearInterval(poll); installing = false; }
  }

  const runningFor = (root: string, reference: CalculationReference) => Boolean(runs[docKey(root, reference.id)]);
  const runningCell = (root: string, reference: CalculationReference) => runs[docKey(root, reference.id)]?.cellId ?? null;
  const sessionFor = (root: string, reference: CalculationReference): SessionState =>
    runningFor(root, reference) ? "running" : live[docKey(root, reference.id)] ? "live" : "idle";
  const statusFor = (root: string, reference: CalculationReference): UiText | null => statuses[docKey(root, reference.id)] ?? null;
  const historyFor = (root: string, reference: CalculationReference) => flags[docKey(root, reference.id)] ?? { canUndo: false, canRedo: false };

  /** Run the listed code cells of this page's part in order. The first error stops the sequence. */
  async function run(target: PageTarget, cellIds: string[], wholeCalculation = false): Promise<boolean> {
    const id = docKey(target.root, target.reference.id);
    if (runs[id] || (!cellIds.length && !wholeCalculation)) return false;
    const token = ++runCounter;
    const current = () => runs[id]?.token === token;
    runs = { ...runs, [id]: { token, cellId: null } };
    const release = () => { if (current()) { const next = { ...runs }; delete next[id]; runs = next; } };
    let ok = false;
    try {
      if (!(await flush())) return false;
      let steps = cellIds.map(cellId => ({ cellId, pageId: target.pageId, partId: target.reference.partId }));
      if (wholeCalculation) {
        const plan = await calculationExecutionPlan(target.root, target.pageId);
        if (plan.revision !== documentFor(target.root, target.reference)?.metadata.goodtype.revision) {
          say(target.root, target.reference.id, notice("calculation.planChanged"));
          return false;
        }
        steps = plan.steps;
      }
      const cells = wholeCalculation ? documentFor(target.root, target.reference)?.cells ?? [] : cellsFor(target.root, target.reference);
      if (steps.some(step => !cells.some(cell => cell.id === step.cellId && cell.cell_type === "code" && cell.metadata.goodtype.partId === step.partId))) {
        say(target.root, target.reference.id, notice("calculation.notRunnable"));
        return false;
      }
      if (cellIds.some(cellId => cells.find(cell => cell.id === cellId)?.cell_type !== "code")) {
        say(target.root, target.reference.id, notice("calculation.notRunnable"));
        return false;
      }
      await refreshRuntime();
      if (disposed || !current()) return false;
      if (!runtime?.installed) {
        afterInstall = () => { void run(target, cellIds, wholeCalculation); };
        installOpen = true;
        void discoverLocal();
        return false;
      }
      say(target.root, target.reference.id, notice("calculation.running"));
      for (const step of steps) {
        const cellId = step.cellId;
        if (!current()) return false;
        runs = { ...runs, [id]: { token, cellId } };
        const result = await runCalculationCell(target.root, step.pageId, cellId, get(locale));
        // The reply is canonical even when this run was stopped meanwhile, so it is always adopted.
        accept({ ...target, pageId: step.pageId, reference: { ...target.reference, partId: step.partId } }, result);
        if (disposed || !current()) return false;
        live = { ...live, [id]: true };
        if (result.document.cells.find(cell => cell.id === cellId)?.outputs?.some(output => output.output_type === "error")) {
          say(target.root, target.reference.id, notice("calculation.stoppedOnError"));
          return false;
        }
      }
      say(target.root, target.reference.id, notice("calculation.ready"));
      ok = true;
    } catch (error) {
      if (current() && !disposed) {
        live = { ...live, [id]: false };
        say(target.root, target.reference.id, notice("calculation.executionFailed", { detail: errorDetails(error) }));
      }
    } finally { release(); }
    return ok;
  }
  const runCell = (target: Target) => run(target, [target.cellId]);
  const runPage = (target: PageTarget) =>
    run(target, cellsFor(target.root, target.reference).filter(cell => cell.cell_type === "code").map(cell => cell.id));
  async function runSelected(advance = true) {
    const target = selected;
    if (!target) return;
    const cells = cellsFor(target.root, target.reference);
    const index = cells.findIndex(cell => cell.id === target.cellId);
    if (index < 0) return;
    if (cells[index].cell_type === "code") await runCell(target);
    const next = cells[index + 1];
    // Shift+Enter moves on, unless the user already picked another cell while the run was in flight.
    if (advance && next && selected && cellKey(selected) === cellKey(target)) select({ ...target, cellId: next.id }, next);
  }
  /** End the session for this calculation. Variables are lost; saved outputs remain. */
  async function endSession(target: PageTarget): Promise<boolean> {
    const id = docKey(target.root, target.reference.id);
    if (runs[id]) { const next = { ...runs }; delete next[id]; runs = next; }
    live = { ...live, [id]: false };
    try {
      await endCalculationSession(target.root, target.reference.id);
      say(target.root, target.reference.id, notice("calculation.sessionEnded"));
      return true;
    } catch (error) {
      say(target.root, target.reference.id, notice("calculation.executionFailed", { detail: errorDetails(error) }));
      return false;
    }
  }
  const runCalculation = (target: PageTarget) => run(target, [], true);
  /**
   * Settings › Python: forget the selected copy of an installed Python and offer the choice again.
   * Every live session used that copy, so all of them end; saved outputs stay.
   */
  async function changePython() {
    if (installing) return;
    runs = {}; live = {};
    // Deleting a full copy takes a few seconds; show the chooser at once and say what is happening.
    installOpen = true; installing = true; removing = true;
    installMessage = notice("calculation.removingCopy");
    try { await forgetLocalPython(); installMessage = null; }
    catch (error) { installMessage = notice("calculation.changeFailed", { detail: errorDetails(error) }); }
    finally { installing = false; removing = false; }
    await refreshRuntime();
    void discoverLocal();
  }

  /** Restart clears variables, then runs every live part in canonical calculation order. */
  async function restartAndRun(target: PageTarget) {
    if (await endSession(target)) await runCalculation(target);
  }

  return {
    ingest, preview, layoutFor, documentFor, cellsFor, source, activated,
    choose, select, deselect, operation, deleteCell, history, flush, historyFor,
    refreshRuntime, install, closeInstall, cancelInstall: cancelPythonInstall,
      discoverLocal, useLocal, changePython,
      forgetRoot,
      get candidates() { return candidates; },
      get discovering() { return discovering; },
    runCell, runPage, runCalculation, runSelected, endSession, restartAndRun, runningFor, runningCell, sessionFor, statusFor,
    preparePlacement: () => editor.settleComposition?.() ?? Promise.resolve(),
    runningIn: (root: string) => Object.keys(runs).some(key => key.startsWith(`${root}\0`)),
      viewHeight: (root: string, pageId: string, heightPt: number) => height(pageId, Math.min(100_000, Math.max(60, heightPt)), root),
    mountEditor: editor.mount,
    get runtime() { return runtime; },
    get installing() { return installing; },
    get removing() { return removing; },
    get installMessage() { return installMessage; },
    get installOpen() { return installOpen; },
    set installOpen(value: boolean) { if (value) installOpen = true; else closeInstall(); },
    get selected() { return selected; },
    get failed() { return failed; },
    get pending() { return pending; },
    dispose() { disposed = true; clearTimeout(timer); editor.dispose(); },
  };
}
export type CalculationWorkspace = ReturnType<typeof createCalculationWorkspace>;
