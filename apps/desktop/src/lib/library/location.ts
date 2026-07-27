/**
 * Where the shelf was left standing.
 *
 * Its own type because it outlives the component that uses it. `LibrarySurface` is unmounted for
 * as long as a notebook is open, so anything it remembers itself is gone the moment you open
 * something — and closing would put you back at the library root, three folders from where you
 * were. The workspace holds this instead and hands it back, so leaving a notebook returns you to
 * the shelf you opened it from.
 */
export type ShelfView = "library" | "favourites";

export type ShelfLocation = {
  view: ShelfView;
  /** Library-relative, empty at the root. Kept even while the favourites view is showing, so
   *  switching back to the library lands where you were rather than at the top. */
  path: string;
};

export const SHELF_ROOT: ShelfLocation = { view: "library", path: "" };
