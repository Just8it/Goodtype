// The shape of an overflow menu.
//
// Menus are described as data rather than markup so that adding an entry is one object in a list,
// not another branch in a template. That matters here specifically: the page menu is the surface
// every future page-level feature has to land in — templates, bookmarks, rotation, outline — and
// each of those should cost an entry, not a rewrite.
//
// The three kinds cover what the menu needs to express. Resist adding a fourth without a reason:
// a menu that can render anything stops being a menu.

export type MenuEntry = MenuAction | MenuToggle | MenuNumber;

type Shared = {
  /** Stable identity for the keyed each block. */
  id: string;
  label: string;
  /** Right-aligned secondary text: a shortcut, a current value, a count. */
  hint?: string;
  disabled?: boolean;
};

/** Runs something and closes the menu. */
export type MenuAction = Shared & {
  kind: "action";
  /** Red styling. Reserve it for entries that destroy work the writer cannot retype. */
  destructive?: boolean;
  onSelect: () => void;
};

/** A setting that flips in place. The menu stays open, since flipping one often precedes another. */
export type MenuToggle = Shared & {
  kind: "toggle";
  value: boolean;
  onChange: (value: boolean) => void;
};

/** A bounded number committed on Enter or blur — a page to jump to, a count, a size. */
export type MenuNumber = Shared & {
  kind: "number";
  value: number;
  min: number;
  max: number;
  onCommit: (value: number) => void;
};

export type MenuSection = {
  /** Shown above the group. Omit on the first section, which reads as the subject's own actions. */
  title?: string;
  entries: MenuEntry[];
};

/** Drops empty sections so a caller can build entries conditionally without leaving stray headings. */
export function populated(sections: MenuSection[]): MenuSection[] {
  return sections.filter((section) => section.entries.length > 0);
}
