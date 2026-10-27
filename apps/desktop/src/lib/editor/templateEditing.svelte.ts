import { createTemplateDraft, type TemplateDraft } from "./templateDraft";
import { templateName } from "../page/presets";
import { readTypstPreset } from "../ipc/presets";
import { errorDetails } from "../ipc/errors";
import { notice, type UiText } from "../i18n";

/** One editor instance owns its draft and timer; closing the panel never discards failed edits. */
export function createTemplateEditing(options: {
  context: () => { root: string; generation: number };
  show: () => void;
  saved: (draft: TemplateDraft) => void;
  report: (message: UiText) => void;
}) {
  let draft = $state<TemplateDraft | null>(null);
  let error = $state("");
  let timer: ReturnType<typeof setTimeout> | undefined;
  let disposed = false;
  function cancelTimer() { clearTimeout(timer); timer = undefined; }
  async function flush(): Promise<boolean> {
    cancelTimer();
    const pending = draft;
    if (!pending) return true;
    try {
      await pending.flush();
      if (disposed || draft !== pending || options.context().root !== pending.root) return true;
      error = "";
      options.saved(pending);
      return true;
    } catch (cause) {
      if (!disposed && draft === pending) {
        error = errorDetails(cause);
        options.report(notice("workspace.theTemplateWasNotSaved", { value1: error }));
      }
      return false;
    }
  }
  async function close(): Promise<boolean> {
    const pending = draft;
    if (!pending) return true;
    if (!(await flush())) return false;
    if (draft === pending) draft = null;
    return true;
  }
  async function open(path: string, name: string) {
    const requested = options.context();
    if (!requested.root || !(await close())) return;
    try {
      const source = await readTypstPreset(requested.root, path);
      const current = options.context();
      if (disposed || current.root !== requested.root || current.generation !== requested.generation) return;
      draft = createTemplateDraft(requested.root, path, name, source);
      error = "";
      options.show();
    } catch (cause) {
      if (!disposed) options.report(notice("workspace.couldNotOpenTheTemplate", { value1: errorDetails(cause) }));
    }
  }
  function update(source: string) {
    if (!draft || disposed) return;
    draft.source = source;
    draft.name = templateName(source) ?? draft.name;
    error = "";
    cancelTimer();
    timer = setTimeout(() => void flush(), 500);
  }
  return {
    get draft() { return draft; }, get error() { return error; },
    open, update, flush, close,
    reset() { cancelTimer(); draft = null; error = ""; },
    dispose() { disposed = true; cancelTimer(); },
  };
}
