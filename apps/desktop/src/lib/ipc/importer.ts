import { get } from "svelte/store";
import { locale } from "../i18n";
import { translate, formatNumber } from "../i18n";
import { invoke } from "@tauri-apps/api/core";

// The optional Goodnotes and OneNote importers (ADR 0029, 0032). Rust owns the download, the
// installation, the source path and the process; this side only asks and shows.

export type NotebookOutcomeStatus = "imported" | "failed" | "needsArtwork" | "cancelled" | "alreadyThere";

export type NotebookOutcome = {
  source: string;
  status: NotebookOutcomeStatus;
  /** Library-relative path of the imported notebook. */
  path: string | null;
  pages: number;
  error: string | null;
  warnings: string[];
  /** Pages whose content became vector artwork with reduced editability. */
  artworkPages: number;
};

export type ImportResult = {
  /** Library-relative batch folder, when anything was imported into it. */
  batch: string | null;
  notebooks: NotebookOutcome[];
  cancelled: boolean;
  problem: string | null;
};

export type ImporterActivity =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "downloading"; received: number; total: number }
  | { kind: "installing" }
  | { kind: "installFailed"; message: string }
  | { kind: "importing"; done: number; total: number; cancelling: boolean }
  | { kind: "publishing" }
  | { kind: "finished"; result: ImportResult }
  | { kind: "importFailed"; message: string };

export type ImporterStatus = {
  platformSupported: boolean;
  /** The tested release this Goodtype version would download; null while none is published. */
  release: { version: string; sizeBytes: number } | null;
  installed: string | null;
  installedProblem: string | null;
  source: { name: string; sizeBytes: number } | null;
  activity: ImporterActivity;
  update: { version: string; sizeBytes: number } | null;
  updateChecked: boolean;
  updateProblem: string | null;
};

/** The optional importers. Each keeps its own installation, chosen file and job in Rust. */
export type ImporterId = "goodnotes" | "onenote";
export const IMPORTERS: ImporterId[] = ["goodnotes", "onenote"];

export const importerStatus = (tool: ImporterId) => invoke<ImporterStatus>("importer_status", { tool });

/** Starts the download of the pinned release. Only ever from the user's explicit action. */
export const installImporter = (tool: ImporterId) => invoke<void>("importer_install", { tool });
export const checkImporterUpdate = (tool: ImporterId) => invoke<void>("importer_check_update", { tool });
export const updateImporter = (tool: ImporterId) => invoke<void>("importer_update", { tool });

export const cancelImporter = (tool: ImporterId) => invoke<void>("importer_cancel", { tool });

export const dismissImporter = (tool: ImporterId) => invoke<void>("importer_dismiss", { tool });

/** Native file dialog in Rust; the chosen path never comes here. */
export const pickImportSource = (tool: ImporterId) =>
  invoke<{ name: string; sizeBytes: number } | null>("import_pick_source", { tool, uiLanguage: get(locale) });

/**
 * Imports the picked source into a new batch folder inside the Library folder `parent`, or with
 * `into` adds what is missing to an earlier batch.
 */
export const startImport = (
  tool: ImporterId,
  parent: string,
  allowDerived: boolean,
  into: string | null = null,
) => invoke<void>("import_start", { tool, parent, allowDerived, into });

/** Every importer's status, for the Library toolbar and for choosing which tab to open. */
export async function allImporterStatus(): Promise<Record<ImporterId, ImporterStatus>> {
  const [goodnotes, onenote] = await Promise.all(IMPORTERS.map(importerStatus));
  return { goodnotes, onenote };
}

/** True while a download, install or import is running. */
export function importerBusy(activity: ImporterActivity): boolean {
  return ["checking", "downloading", "installing", "importing", "publishing"].includes(activity.kind);
}

export function megabytes(bytes: number): string {
  const value = bytes / (1024 * 1024);
  return translate("ipc.mb", { "value1": formatNumber(value, { maximumFractionDigits: value < 10 ? 1 : 0 }) });
}

/** Counts for the result summary, in the order the sentence names them. */
export function summarize(result: ImportResult) {
  const count = (status: NotebookOutcomeStatus) =>
    result.notebooks.filter((notebook) => notebook.status === status).length;
  return {
    imported: count("imported"),
    alreadyThere: count("alreadyThere"),
    needsArtwork: count("needsArtwork"),
    failed: count("failed"),
    cancelled: count("cancelled"),
  };
}
