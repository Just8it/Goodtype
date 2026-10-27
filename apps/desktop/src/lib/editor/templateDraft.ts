import { translate } from "../i18n";
import { saveTypstPreset } from "../ipc/presets";

/** One file's unsaved buffer and ordered confirmations, retained after a failed write. */
export function createTemplateDraft(root: string, path: string, name: string, source: string) {
  let queue: Promise<void> = Promise.resolve();
  const draft = {
    root, path, name, source, confirmedSource: source,
    get dirty() { return this.source !== this.confirmedSource; },
    get savedSource() { return this.confirmedSource; },
    flush(): Promise<void> {
      queue = queue.catch(() => {}).then(async () => {
        while (this.source !== this.confirmedSource) {
          const pending = this.source;
          if (!pending.trim()) throw new Error(translate("editor.theEmptyTemplateHasNotBeenSaved"));
          await saveTypstPreset(root, path, pending, this.confirmedSource);
          this.confirmedSource = pending;
        }
      });
      return queue;
    },
  };
  return draft;
}

export type TemplateDraft = ReturnType<typeof createTemplateDraft>;
