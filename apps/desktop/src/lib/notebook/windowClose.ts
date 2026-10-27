/** Native close is allowed only after settings and every notebook save have confirmed. */
export function createWindowCloseHandler(options: {
  saveSettings: () => Promise<boolean>;
  saveNotebook: () => Promise<boolean>;
  rememberPage: () => Promise<void>;
  destroy: () => Promise<void>;
  failed: (error: unknown) => void;
}) {
  let closing = false;
  return {
    get closing() { return closing; },
    async handle(event: { preventDefault(): void }) {
      event.preventDefault();
      if (closing) return;
      closing = true;
      try {
        if (!(await options.saveSettings())) return;
        if (!(await options.saveNotebook())) return;
        await options.rememberPage();
        // close() would re-enter the native event. Destroy only after all writes confirm.
        await options.destroy();
      } catch (error) {
        options.failed(error);
      } finally {
        closing = false;
      }
    },
  };
}
