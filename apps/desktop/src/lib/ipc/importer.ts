import { invoke } from "@tauri-apps/api/core";

// The optional Goodnotes importer (ADR 0029). Rust owns the download, the installation, the
// source path and the process; this side only asks and shows.

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

export const importerStatus = () => invoke<ImporterStatus>("goodnotes_importer_status");

/** Starts the download of the pinned release. Only ever from the user's explicit action. */
export const installImporter = () => invoke<void>("goodnotes_importer_install");
export const checkImporterUpdate = () => invoke<void>("goodnotes_importer_check_update");
export const updateImporter = () => invoke<void>("goodnotes_importer_update");

export const cancelImporter = () => invoke<void>("goodnotes_importer_cancel");

export const dismissImporter = () => invoke<void>("goodnotes_importer_dismiss");

/** Native file dialog in Rust; the chosen path never comes here. */
export const pickImportSource = () =>
  invoke<{ name: string; sizeBytes: number } | null>("goodnotes_import_pick_source");

/**
 * Imports the picked source into a new batch folder inside the Library folder `parent`, or with
 * `into` adds what is missing to an earlier batch.
 */
export const startImport = (parent: string, allowDerived: boolean, into: string | null = null) =>
  invoke<void>("goodnotes_import_start", { parent, allowDerived, into });

/** True while a download, install or import is running. */
export function importerBusy(activity: ImporterActivity): boolean {
  return ["checking", "downloading", "installing", "importing", "publishing"].includes(activity.kind);
}

export function megabytes(bytes: number): string {
  const value = bytes / (1024 * 1024);
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} MB`;
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
