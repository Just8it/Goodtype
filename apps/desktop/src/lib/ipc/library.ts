/**
 * The library command surface, one function per Rust command.
 *
 * Mirrors `apps/desktop/src-tauri/src/library.rs`. Every call names its arguments and its return
 * type once, here, instead of at each `invoke` site — the argument names are part of the contract
 * and a typo in one produces a runtime error rather than a compile error.
 *
 * Every `path` below is relative to the library root, forward-slashed, with the empty string
 * meaning the root itself. An absolute path never crosses this boundary in either direction, with
 * the single exception of the two calls that hand one back for a notebook about to be opened:
 * downstream, a notebook is named by its real root and knows nothing about libraries.
 *
 * These wrappers deliberately do not catch. Whether a failure is fatal, retried, or shown as
 * status text is the caller's decision.
 */
import { invoke } from "@tauri-apps/api/core";

import type { LibraryEntry, LibraryListing } from "../library/library";

/** The chosen library, or null when none has been chosen yet. */
export function libraryRoot(): Promise<string | null> {
  return invoke<string | null>("library_root");
}

/** Choose a library with the native dialog. Null when the dialog was dismissed. */
export function pickLibraryRoot(): Promise<string | null> {
  return invoke<string | null>("pick_library_root");
}

export function listLibrary(path: string): Promise<LibraryListing> {
  return invoke<LibraryListing>("list_library", { path });
}

/** Admit a notebook inside the library and answer with its absolute root. */
export function openLibraryNotebook(path: string): Promise<string> {
  return invoke<string>("open_library_notebook", { path });
}

/** Pick a notebook from outside the library. Null when the dialog was dismissed. */
export function pickNotebookRoot(): Promise<string | null> {
  return invoke<string | null>("pick_notebook_root");
}

export function createLibraryFolder(parent: string, name: string): Promise<string> {
  return invoke<string>("create_library_folder", { parent, name });
}

/** Makes an empty directory and answers with its absolute root, for the caller to fill. */
export function createLibraryNotebook(parent: string, name: string): Promise<string> {
  return invoke<string>("create_library_notebook", { parent, name });
}

export function renameLibraryEntry(path: string, name: string): Promise<string> {
  return invoke<string>("rename_library_entry", { path, name });
}

export function moveLibraryEntry(path: string, destination: string): Promise<string> {
  return invoke<string>("move_library_entry", { path, destination });
}

/** Moves to the library's trash rather than deleting outright. */
export function deleteLibraryEntry(path: string): Promise<void> {
  return invoke("delete_library_entry", { path });
}

export function libraryFavourites(): Promise<string[]> {
  return invoke<string[]>("library_favourites");
}

export function setLibraryFavourite(path: string, favourite: boolean): Promise<string[]> {
  return invoke<string[]>("set_library_favourite", { path, favourite });
}

export function listLibraryFavourites(): Promise<LibraryEntry[]> {
  return invoke<LibraryEntry[]>("list_library_favourites");
}

/** A notebook's stored cover as a data URL, or null when it has never been given one. */
export function libraryCover(path: string): Promise<string | null> {
  return invoke<string | null>("library_cover", { path });
}
