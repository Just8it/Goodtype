/**
 * The shapes that cross the Rust boundary.
 *
 * These mirror `NotebookSnapshot`, `HistoryResult` and `StoredFile` in
 * `crates/goodtype-core/src/storage/mod.rs`, serialised camelCase. They are the wire format
 * rather than component state, which is why they live beside the calls that carry them: a
 * component that redeclares one locally is a component that can disagree with Rust silently.
 */
import type { InkLayer, NotebookManifest, Page } from "../model";

/** Bytes as Rust hands them over — `Vec<u8>` arrives as a number array, not a `Uint8Array`. */
export type StoredFile = { path: string; bytes: number[] };

export type NotebookSnapshot = {
  manifest: NotebookManifest;
  page: Page;
  blocks: StoredFile[];
  assets: StoredFile[];
  inkLayers: InkLayer[];
};

/** A snapshot plus whether the page it belongs to can still be stepped backward or forward. */
export type HistoryResult = {
  snapshot: NotebookSnapshot;
  canUndo: boolean;
  canRedo: boolean;
};

/** The same shape for manifest page-list history; kept distinct for command clarity. */
export type StructureHistoryResult = HistoryResult;

/** Which direction `runHistory` steps. The strings are the Rust command names. */
export type HistoryCommand = "undo_notebook" | "redo_notebook";
