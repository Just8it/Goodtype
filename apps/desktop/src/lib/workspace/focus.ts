const FOCUSABLE = [
  "button:not(:disabled)",
  "input:not(:disabled)",
  "select:not(:disabled)",
  "textarea:not(:disabled)",
  "a[href]",
  '[tabindex]:not([tabindex="-1"])',
].join(",");

/** Keep keyboard focus inside a modal and return it to the control that opened the modal. */
export function focusTrap(node: HTMLElement) {
  const opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;

  const keydown = (event: KeyboardEvent) => {
    if (event.key !== "Tab") return;
    const focusable = [...node.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
      (element) => element.getClientRects().length > 0,
    );
    if (focusable.length === 0) {
      event.preventDefault();
      node.focus();
      return;
    }
    const first = focusable[0];
    const last = focusable.at(-1);
    if (event.shiftKey && (document.activeElement === first || !node.contains(document.activeElement))) {
      event.preventDefault();
      last?.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first?.focus();
    }
  };

  node.addEventListener("keydown", keydown);
  return {
    destroy() {
      node.removeEventListener("keydown", keydown);
      if (opener?.isConnected) requestAnimationFrame(() => opener.focus());
    },
  };
}
