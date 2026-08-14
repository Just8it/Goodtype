/**
 * The notebook command surface, one function per Rust command.
 *
 * Mirrors `apps/desktop/src-tauri/src/notebook.rs`. Every call names its arguments and its return
 * type once, here, instead of at each `invoke` site — the argument names are part of the contract
 * (serde renames them to camelCase) and a typo in one produces a runtime error rather than a
 * compile error, which is exactly the class of mistake a boundary module exists to remove.
 *
 * These wrappers deliberately do not catch. Whether a failure is fatal, retried, or shown as
 * status text is the caller's decision, and swallowing it here would take that away.
 */
import { invoke } from "@tauri-apps/api/core";

import type { PageBackground, PageGeometry, PagePosition } from "../model";
import type { PresetChoice } from "../page/presets";
import type {
  HistoryCommand,
  HistoryResult,
  NotebookSnapshot,
  StructureHistoryResult,
} from "./types";

export function createNotebook(
  root: string,
  snapshot: NotebookSnapshot,
  presetChoice: PresetChoice = { kind: "none" },
): Promise<void> {
  return invoke("create_notebook", { root, snapshot, presetChoice });
}

export function openNotebook(root: string): Promise<NotebookSnapshot> {
  return invoke<NotebookSnapshot>("open_notebook", { root });
}

export function openPage(root: string, pageId: string): Promise<NotebookSnapshot> {
  return invoke<NotebookSnapshot>("open_page", { root, pageId });
}

/** Move the edited page. Returns the page's own undo/redo availability, not the notebook's. */
export function focusPage(root: string, pageId: string): Promise<HistoryResult> {
  return invoke<HistoryResult>("focus_page", { root, pageId });
}

export function commitNotebook(
  root: string,
  snapshot: NotebookSnapshot,
): Promise<HistoryResult> {
  return invoke<HistoryResult>("commit_notebook", { root, snapshot });
}

export function runHistory(
  root: string,
  command: HistoryCommand,
  pageId: string,
): Promise<HistoryResult> {
  return invoke<HistoryResult>(command, { root, pageId });
}

export function createPage(
  root: string,
  modifiedAt: string,
  position: PagePosition,
  background: PageBackground | null,
  geometry: PageGeometry | null,
  activePageId: string,
): Promise<StructureHistoryResult> {
  return invoke<StructureHistoryResult>("create_page", {
    root,
    request: { modifiedAt, position, background, geometry, activePageId },
  });
}

/** Choose a PDF and preserve its original bytes under `references/`. */
export function pickPdfReference(root: string): Promise<string | null> {
  return invoke<string | null>("pick_pdf_reference", { root });
}

/** Read a contained PDF through Tauri's raw binary response path. */
export function readPdfReference(root: string, sourcePath: string): Promise<ArrayBuffer> {
  return invoke<ArrayBuffer>("read_pdf_reference", { root, sourcePath });
}

export function importPdfPages(
  root: string,
  modifiedAt: string,
  position: PagePosition,
  sourcePath: string,
  geometries: PageGeometry[],
  activePageId: string,
): Promise<StructureHistoryResult> {
  return invoke<StructureHistoryResult>("import_pdf_pages", {
    root,
    request: { modifiedAt, position, sourcePath, geometries, activePageId },
  });
}

export function duplicatePage(
  root: string,
  pageId: string,
  modifiedAt: string,
): Promise<StructureHistoryResult> {
  return invoke<StructureHistoryResult>("duplicate_page", { root, pageId, modifiedAt });
}

export function deletePage(
  root: string,
  pageId: string,
  modifiedAt: string,
): Promise<StructureHistoryResult> {
  return invoke<StructureHistoryResult>("delete_page", { root, pageId, modifiedAt });
}

export function reorderPages(
  root: string,
  orderedIds: string[],
  modifiedAt: string,
  activePageId: string,
): Promise<StructureHistoryResult> {
  return invoke<StructureHistoryResult>("reorder_pages", {
    root,
    orderedIds,
    modifiedAt,
    activePageId,
  });
}

export function runStructureHistory(
  root: string,
  command: "undo_page_structure" | "redo_page_structure",
  modifiedAt: string,
): Promise<StructureHistoryResult> {
  return invoke<StructureHistoryResult>(command, { root, modifiedAt });
}

/**
 * Store pasted bytes under `assets/` and return the notebook-relative path.
 *
 * Rust expects a plain number array; handing it a `Uint8Array` serialises to an object of
 * indices, which fails far from here. Converting at the boundary keeps that detail in one place.
 */
export function storePastedImage(
  root: string,
  filename: string,
  bytes: Uint8Array,
): Promise<string> {
  return invoke<string>("store_pasted_image", {
    root,
    filename,
    bytes: Array.from(bytes),
  });
}
