// The shape of the add-page menu.
//
// Two independent choices: *where* the page goes and *what it is made of*. Keeping them separate
// is what stops this menu growing combinatorially — a template, an image, or an imported PDF is
// one more source, not three more entries per position.
//
// Sources are described as data for the same reason `menu.ts` describes entries as data: the
// template picker, image placement, and PDF import all have to land here, and each should cost
// an object in a list rather than another branch of markup.

import type { PagePosition } from "../model";

/** The writer's choice, before it is resolved against whichever page is open. */
export type AddPageWhere = "before" | "after" | "last";

/** One thing a new page can be made from: a paper template, or a document brought in. */
export type AddPageSource = {
  /** Stable identity for the keyed each block. */
  id: string;
  label: string;
  /** Second line: what the source is, or why it cannot be used yet. */
  detail?: string;
  disabled?: boolean;
  /** A short full-width row for external document sources rather than a paper swatch. */
  compact?: boolean;
  /**
   * Optional inline SVG markup drawn as a preview tile. Templates render their own definition
   * here, so a preview never has to be a stored bitmap that can go stale.
   */
  preview?: string;
  /**
   * Commit. `count` is how many of this page to make; a source that cannot honour it — an import
   * brings whatever the file holds — is free to ignore it.
   */
  onSelect: (position: PagePosition, count: number) => void;
};

/**
 * One shelf of the picker. Grouping is the menu's business, not the page's: nothing on disk
 * changes when the shelves get rearranged.
 */
export type AddPageGroup = {
  id: string;
  title: string;
  /**
   * Which half of the menu the shelf belongs to. `blank` shelves are built here, from the size,
   * orientation and paper chosen above them; `import` shelves bring a document in and take its
   * geometry from the file, so those controls do not apply and are not shown beside them.
   *
   * `current` is the page you are on: a starting point rather than a kind of ruling, so it is
   * pinned beside the import instead of sitting among the paper filters.
   *
   * A lane rather than a position in the list: an image importer should land next to the PDF one
   * by saying what it is, not by being inserted at the right index.
   */
  lane?: "blank" | "current" | "import";
  sources: AddPageSource[];
};

/**
 * Turn the writer's choice into the argument the command takes. Falls back to appending when
 * there is no page to be relative to, which is the only sensible reading of "before" in an
 * empty notebook.
 */
export function resolvePosition(where: AddPageWhere, currentPageId: string): PagePosition {
  if (where === "last" || !currentPageId) return { kind: "last" };
  return { kind: where, pageId: currentPageId };
}
