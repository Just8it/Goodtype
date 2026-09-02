/**
 * The library as the frontend sees it: relative paths and nothing else.
 *
 * Every path here is relative to the library root, forward-slashed, with the empty string
 * meaning the root itself. An absolute path never reaches this side — Rust resolves and contains
 * one on every call — so nothing in the UI can name a folder outside the library even by
 * accident.
 *
 * The mirror of `apps/desktop/src-tauri/src/library.rs`.
 */

import type { PageDefaults } from "../model";

export type LibraryFolder = {
  kind: "folder";
  name: string;
  path: string;
  modifiedMs: number | null;
  childCount: number;
};

export type LibraryNotebook = {
  kind: "notebook";
  name: string;
  path: string;
  modifiedMs: number | null;
  /** Null when the manifest could not be read; the notebook still belongs on the shelf. */
  pageCount: number | null;
  /**
   * The paper the notebook is written on, so a tile draws its real ruling before any cover of
   * its contents exists. Null when the manifest could not be read, where a tile falls back to
   * blank paper rather than to nothing.
   */
  paper: PageDefaults | null;
};

export type LibraryEntry = LibraryFolder | LibraryNotebook;

export type LibraryListing = {
  path: string;
  entries: LibraryEntry[];
};

/** One step of the path, with the path that navigates to it. */
export type Crumb = { name: string; path: string };

/**
 * The trail from the library root down to `path`, root included.
 *
 * This is the whole of the navigation model. There is deliberately no folder tree beside it: at
 * the root a tree lists exactly what the grid already shows, it needs expand state and a cached
 * listing per node that every rename and move has to invalidate, and a 13px row with a
 * disclosure triangle is the one control on this surface you would have to put the pen down for.
 * Jumping to a distant branch is what favourites, recents and search are for.
 */
export function breadcrumb(path: string, rootName = "Bibliothek"): Crumb[] {
  const crumbs: Crumb[] = [{ name: rootName, path: "" }];
  let walked = "";
  for (const segment of path.split("/").filter(Boolean)) {
    walked = walked ? `${walked}/${segment}` : segment;
    crumbs.push({ name: segment, path: walked });
  }
  return crumbs;
}

/** The folder holding `path`, or null at the root, where there is nowhere further up. */
export function parentPath(path: string): string | null {
  if (!path) return null;
  const cut = path.lastIndexOf("/");
  return cut === -1 ? "" : path.slice(0, cut);
}

export function childPath(parent: string, name: string): string {
  return parent ? `${parent}/${name}` : name;
}

/**
 * Whether dropping `source` into `destination` can produce a different valid shelf location.
 *
 * This only keeps impossible/no-op targets from lighting up in the UI. Rust remains the
 * authority: it resolves both paths inside the selected root and repeats every containment and
 * collision check before moving anything.
 */
export function canMoveLibraryEntry(source: string, destination: string): boolean {
  if (!source || source === destination) return false;
  if (parentPath(source) === destination) return false;
  return !destination.startsWith(`${source}/`);
}

/** All members move or none do; this mirrors the Rust batch command's contract. */
export function canMoveLibraryEntries(sources: string[], destination: string): boolean {
  return sources.length > 0 && sources.every((source) => canMoveLibraryEntry(source, destination));
}

/**
 * Names Goodtype will create a folder or notebook under.
 *
 * Deliberately narrow rather than merely legal. These names become directory names on someone
 * else's filesystem the moment a library is synced or shared, so the rule is the intersection of
 * what Windows, macOS and Linux all accept — not what the running platform happens to allow.
 * Leading and trailing dots and spaces are out because Windows silently strips them, which turns
 * two distinct names into one folder.
 */
const FORBIDDEN = /[\\/:*?"<>|]/;
/**
 * The same superset `is_windows_reserved` refuses in `crates/goodtype-core/src/paths.rs`, which
 * is the rule that actually governs: this copy only makes the message immediate. `com0`/`lpt0`
 * are not real device names but are refused on both sides so neither can accept a name the
 * other rejects.
 */
const RESERVED = /^(con|prn|aux|nul|com|lpt|com[0-9]|lpt[0-9])$/i;

/** Mirrors `MAX_NAME_CHARS` in `apps/desktop/src-tauri/src/library.rs`. */
const MAX_NAME_CHARS = 80;

export function nameProblem(name: string): string | null {
  const trimmed = name.trim();
  if (!trimmed) return "A name is needed";
  if (trimmed !== name) return "Names cannot start or end with a space";
  // Counted the way Rust counts it — `length` is UTF-16 units, so an emoji or an accented
  // character would otherwise be measured differently on each side of the boundary.
  if ([...name].length > MAX_NAME_CHARS) return "That name is too long";
  if (FORBIDDEN.test(name)) return 'A name cannot contain \\ / : * ? " < > |';
  if (name.startsWith(".")) return "A name starting with a dot would be hidden";
  if (name.endsWith(".")) return "Names cannot end with a dot";
  if (RESERVED.test(name)) return "That name is reserved by Windows";
  return null;
}

export type SortOrder = "name" | "modified";

/**
 * Compare within a band, never across one.
 *
 * Folders and notebooks are sorted and shown separately, so this never has to decide whether a
 * folder outranks a notebook — the bands answer that, and the sort only orders each in turn.
 */
export function compareEntries(
  a: LibraryEntry,
  b: LibraryEntry,
  order: SortOrder,
): number {
  if (order === "modified") {
    // Newest first: the reason to sort by date is to find what you just had open.
    const byDate = (b.modifiedMs ?? 0) - (a.modifiedMs ?? 0);
    if (byDate !== 0) return byDate;
  }
  return a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" });
}

/**
 * Split a listing into the two bands the shelf draws.
 *
 * Numeric collation is what puts `Serie 2` before `Serie 10`, which plain string order does not,
 * and coursework is numbered far more often than it is named.
 */
export function bands(
  entries: LibraryEntry[],
  order: SortOrder,
): { folders: LibraryFolder[]; notebooks: LibraryNotebook[] } {
  const folders = entries.filter((entry): entry is LibraryFolder => entry.kind === "folder");
  const notebooks = entries.filter(
    (entry): entry is LibraryNotebook => entry.kind === "notebook",
  );
  return {
    folders: [...folders].sort((a, b) => compareEntries(a, b, order)),
    notebooks: [...notebooks].sort((a, b) => compareEntries(a, b, order)),
  };
}
