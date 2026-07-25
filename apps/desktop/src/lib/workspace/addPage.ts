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

/** One thing a new page can be made from. Blank today; templates, images, and imports next. */
export type AddPageSource = {
  /** Stable identity for the keyed each block. */
  id: string;
  label: string;
  /** Second line: what the source is, or why it cannot be used yet. */
  detail?: string;
  disabled?: boolean;
  /**
   * Optional inline SVG markup drawn as a preview tile. Templates render their own definition
   * here, so a preview never has to be a stored bitmap that can go stale.
   */
  preview?: string;
  onSelect: (position: PagePosition) => void;
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
