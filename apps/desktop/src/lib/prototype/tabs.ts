export type NotebookTab = { root: string; title: string };

export function openedTab(tabs: NotebookTab[], tab: NotebookTab): NotebookTab[] {
  const existing = tabs.findIndex((entry) => entry.root === tab.root);
  if (existing < 0) return [...tabs, tab];
  return tabs.map((entry, index) => (index === existing ? tab : entry));
}

export function closedTab(
  tabs: NotebookTab[],
  root: string,
): { tabs: NotebookTab[]; nextRoot: string | null } {
  const index = tabs.findIndex((entry) => entry.root === root);
  if (index < 0) return { tabs, nextRoot: null };
  const remaining = tabs.filter((entry) => entry.root !== root);
  return {
    tabs: remaining,
    nextRoot: remaining[index]?.root ?? remaining[index - 1]?.root ?? null,
  };
}

export function cycledTab(tabs: NotebookTab[], root: string, offset: -1 | 1): string | null {
  if (tabs.length < 2) return null;
  const index = Math.max(0, tabs.findIndex((entry) => entry.root === root));
  return tabs[(index + offset + tabs.length) % tabs.length].root;
}
