/**
 * Close a popout when the writer presses somewhere else or hits Escape.
 *
 * Shared because getting it wrong is the bug every popout in this app has had at least once, in
 * two flavours: a menu that outlives the press that should have dismissed it, and a menu that
 * dismisses itself on the very press that opened it. Both fixes live here, so a new popout gets
 * them by using the action rather than by remembering them.
 */
export function dismissable(node: HTMLElement, onClose: () => void) {
  let close = onClose;

  // Pointer rather than click, and capture rather than bubble, so the menu is gone before the
  // press lands on whatever is underneath it.
  const dismiss = (event: PointerEvent) => {
    if (!node.contains(event.target as Node)) close();
  };
  const key = (event: KeyboardEvent) => {
    if (event.key === "Escape") close();
  };

  // Deferred a frame: the press that opened this is still propagating when the action runs.
  const timer = setTimeout(() => window.addEventListener("pointerdown", dismiss, true));
  window.addEventListener("keydown", key);

  return {
    update(next: () => void) {
      close = next;
    },
    destroy() {
      clearTimeout(timer);
      window.removeEventListener("pointerdown", dismiss, true);
      window.removeEventListener("keydown", key);
    },
  };
}
