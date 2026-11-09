// What an export covers, turned into what Rust is asked for.
//
// Kept out of the menu so the page list parsing and the naming can be tested without a DOM.

export type ExportScope = "notebook" | "page" | "pages";

/** Mirrors `ExportInclude` in the desktop crate. Everything is on unless switched off. */
export type ExportInclude = {
  handwriting: boolean;
  highlighter: boolean;
  typst: boolean;
  pageText: boolean;
  images: boolean;
  shapes: boolean;
  background: boolean;
  calculations: boolean;
};

export const INCLUDE_EVERYTHING: ExportInclude = {
  handwriting: true,
  highlighter: true,
  typst: true,
  pageText: true,
  images: true,
  shapes: true,
  background: true,
  calculations: true,
};

export type ExportChoice = {
  scope: ExportScope;
  /** A page list as typed, e.g. "1-3, 7". Read only for `pages`. */
  pages: string;
  include: ExportInclude;
  /** Cut calculation pages into A4 sheets instead of one tall page each. */
  splitCalculationsA4: boolean;
};

export type ExportTarget = {
  /** Undefined exports every page, which also covers pages added after the menu was opened. */
  pageIds?: string[];
  outputName: string;
  pageCount: number;
};

/** One entry of a page list: a page number, or a range written with a hyphen or an en dash. */
const PAGE_LIST_ENTRY = /^(\d+)(?:[-–](\d+))?$/;
const PAGE_LIST_SEPARATORS = /[,;\s]+/;

/**
 * Whether a page list is written right, before anything knows how many pages there are: numbers
 * and ranges separated by commas or spaces. Empty counts as written right; it means every page.
 */
export function isPageListSyntax(text: string): boolean {
  return text.split(PAGE_LIST_SEPARATORS).filter(Boolean).every((part) => PAGE_LIST_ENTRY.test(part));
}

/**
 * Reads a print-dialog page list: numbers and ranges separated by commas or spaces, one-based.
 * A range may be written backwards. Returns the pages in notebook order without repeats, or null
 * when anything in it is not a page of this notebook, so the menu can say so instead of guessing.
 */
export function parsePageList(text: string, pageCount: number): number[] | null {
  const parts = text.split(PAGE_LIST_SEPARATORS).filter(Boolean);
  if (!parts.length) return null;
  const pages = new Set<number>();
  for (const part of parts) {
    const match = PAGE_LIST_ENTRY.exec(part);
    if (!match) return null;
    const a = Number(match[1]);
    const b = Number(match[2] ?? match[1]);
    const [from, to] = a <= b ? [a, b] : [b, a];
    if (from < 1 || to > pageCount) return null;
    for (let page = from; page <= to; page += 1) pages.add(page);
  }
  return [...pages].sort((x, y) => x - y);
}

export function exportTarget(choice: ExportChoice, pageIds: readonly string[], activePageId: string): ExportTarget | null {
  const suffix = choice.splitCalculationsA4 && choice.include.calculations ? "-a4" : "";
  if (choice.scope === "notebook" || pageIds.length === 0) {
    return { outputName: `notebook${suffix}.pdf`, pageCount: pageIds.length };
  }
  const numbers =
    choice.scope === "page"
      ? [Math.max(1, pageIds.indexOf(activePageId) + 1)]
      : parsePageList(choice.pages, pageIds.length);
  if (!numbers) return null;
  return {
    pageIds: numbers.map((number) => pageIds[number - 1]!),
    outputName: `${numbers.length === 1 ? "page" : "pages"}-${describe(numbers)}${suffix}.pdf`,
    pageCount: numbers.length,
  };
}

/** "1-3_7" for pages 1, 2, 3 and 7; long lists shorten so the file name stays readable. */
function describe(numbers: number[]): string {
  const runs: string[] = [];
  for (let index = 0; index < numbers.length; ) {
    let end = index;
    while (end + 1 < numbers.length && numbers[end + 1] === numbers[end]! + 1) end += 1;
    runs.push(end > index ? `${numbers[index]}-${numbers[end]}` : `${numbers[index]}`);
    index = end + 1;
  }
  const joined = runs.join("_");
  return joined.length <= 40 ? joined : `${numbers[0]}-${numbers[numbers.length - 1]}-selection`;
}
