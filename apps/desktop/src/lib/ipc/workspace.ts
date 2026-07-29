/**
 * Everything around the open notebook rather than inside it: where it lives, that it was opened,
 * what it exports to, and how it looks on a shelf.
 *
 * Spread across `workspace.rs`, `settings.rs`, `export.rs` and `library.rs` on the Rust side.
 * Grouped here by what the frontend is doing rather than by which Rust file answers, because a
 * caller reaching for "record that this opened" should not have to know which module owns it.
 */
import { invoke } from "@tauri-apps/api/core";
import type { RecentNotebook } from "../settings";

/** The default notebook directory, created if absent. Used when there are no recents. */
export function defaultNotebookRoot(): Promise<string> {
  return invoke<string>("phase0_notebook_root");
}

export function listRecentNotebooks(): Promise<RecentNotebook[]> {
  return invoke<RecentNotebook[]>("list_recent_notebooks");
}

export function recordNotebookPage(root: string, pageId: string): Promise<void> {
  return invoke("record_notebook_page", { root, pageId });
}

export function recordNotebookOpened(
  root: string,
  title: string,
  openedAt: string,
): Promise<void> {
  return invoke("record_notebook_opened", { root, title, openedAt });
}

export function exportNotebookPdf(root: string, outputName: string): Promise<string> {
  return invoke<string>("export_notebook_pdf", { root, outputName });
}

/** See the note in `storePastedImage` about why the bytes are converted here. */
export function writeNotebookCover(root: string, png: Uint8Array): Promise<void> {
  return invoke("write_notebook_cover", { root, png: Array.from(png) });
}

export function writeMetrics(root: string, metrics: unknown): Promise<void> {
  return invoke("write_phase0_metrics", { root, metrics });
}
