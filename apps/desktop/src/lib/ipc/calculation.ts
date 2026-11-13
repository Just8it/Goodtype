import { get } from "svelte/store";
import { locale } from "../i18n";
import { invoke } from "@tauri-apps/api/core";
import type { CalculationReference, PagePosition } from "../model";
import type { StructureHistoryResult } from "./types";

export type CellOutput = {
  output_type: "stream" | "display_data" | "execute_result" | "error";
  text?: string | string[];
  data?: Record<string, unknown>;
  ename?: string;
  evalue?: string;
  traceback?: string[];
};
export type CalculationCell = {
  id: string;
  cell_type: "code" | "markdown" | "raw";
  source: string;
  execution_count?: number | null;
  outputs?: CellOutput[];
  metadata: Record<string, unknown> & { goodtype: { partId: string; copiedFrom?: unknown; execution?: { sourceHash: string } } };
};
export type CalculationDocument = {
  nbformat: number;
  nbformat_minor: number;
  metadata: Record<string, unknown> & { goodtype: { id: string; title: string; revision: number; parts: { id: string; title: string }[] } };
  cells: CalculationCell[];
};
export type CalculationResult = {
  reference: CalculationReference;
  document: CalculationDocument;
  canUndo: boolean;
  canRedo: boolean;
};
export type PythonCandidate = { id: string; executable: string };
export type PythonProbeResult = { code: "compatible" | "missing_packages" | "incompatible"; missing: string[]; provenance: unknown; selected: boolean };
export const discoverPythonRuntimes = () => invoke<PythonCandidate[]>("python_runtime_discover");
export const usePythonRuntime = (candidateId: string) => invoke<PythonProbeResult>("python_runtime_probe", { candidateId, select: true });
export type CellOperation =
  | { kind: "source"; cellId: string; expectedSource: string; source: string }
  | { kind: "insert"; cellId: string; after: string | null; cellType: string }
  | { kind: "delete"; cellId: string }
  | { kind: "move"; cellId: string; before: string | null }
  | { kind: "cell_type"; cellId: string; cellType: string }
  | { kind: "rename"; title: string };

export function createCalculationPage(root: string, activePageId: string, position: PagePosition, title: string, continuePageId: string | null) {
  return invoke<StructureHistoryResult>("create_calculation_page", {
    root, request: { activePageId, position, title, continuePageId, modifiedAt: new Date().toISOString() },
  });
}
export function readCalculation(root: string, pageId: string) {
  return invoke<CalculationResult>("read_calculation", { root, pageId });
}
export function editCalculation(root: string, pageId: string, operation: CellOperation) {
  return invoke<CalculationResult>("edit_calculation", { root, pageId, operation, modifiedAt: new Date().toISOString() });
}
export function calculationHistory(root: string, pageId: string, undo: boolean, pageScope: boolean) {
  return invoke<CalculationResult>("calculation_history", { root, pageId, undo, pageScope, modifiedAt: new Date().toISOString() });
}

export type CellLayout = { id: string; sourceSvg: string; outputSvg: string | null; sourceHeightPt: number; outputHeightPt: number; stale: boolean };
export type CalculationLayout = { revision: number; widthPt: number; heightPt: number; cells: CellLayout[] };
export function layoutCalculation(root: string, pageId: string, uiLanguage: string) {
  return invoke<CalculationLayout>("layout_calculation", { root, pageId, uiLanguage });
}

export type PythonRuntimeStatus = { installed: boolean; installing: boolean; downloadsAllowed: boolean; progress: string; downloadBytes: number };
export const pythonRuntimeStatus = () => invoke<PythonRuntimeStatus>("python_runtime_status");
export const installPythonRuntime = () => invoke<void>("python_runtime_install");
export const cancelPythonInstall = () => invoke<void>("python_runtime_cancel");
/** Forget the selected copy of an installed Python and end live sessions. True when a copy was removed. */
export const forgetLocalPython = () => invoke<boolean>("python_runtime_forget_local");
export const endCalculationSession = (root: string, calculationId: string) => invoke<void>("end_calculation_session", { root, calculationId });
export const runCalculationCell = (root: string, pageId: string, cellId: string, uiLanguage: string) => invoke<CalculationResult>("run_calculation_cell", {
  root, pageId, cellId, uiLanguage, modifiedAt: new Date().toISOString(),
});

export type CalculationStep = { pageId: string; cellId: string; partId: string };
export const calculationExecutionPlan = (root: string, pageId: string) => invoke<{ revision: number; steps: CalculationStep[] }>("calculation_execution_plan", { root, pageId });

export const importCalculationNotebook = (root: string, activePageId: string, position: PagePosition, title: string) => invoke<StructureHistoryResult | null>("import_calculation_notebook", {
  root, request: { activePageId, position, title, continuePageId: null, modifiedAt: new Date().toISOString() }, uiLanguage: get(locale),
});
