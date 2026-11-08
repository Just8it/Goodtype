import { cellProblemKeys } from "../ipc/helper";
import type { CodexProblem } from "../ipc/helper";
import { notice } from "../i18n";

export const PROMPT_LIMIT = 4000;
export function canSend(prompt: string, busy = false) {
  return !busy && prompt.trim().length > 0 && prompt.length <= PROMPT_LIMIT;
}
export function composerKey(event: Pick<KeyboardEvent, "key" | "shiftKey" | "isComposing" | "preventDefault" | "stopPropagation">, send: () => void, close: () => void) {
  if (event.isComposing) return;
  if (event.key === "Escape") {
    event.preventDefault(); event.stopPropagation(); close();
  } else if (event.key === "Enter" && !event.shiftKey) {
    event.preventDefault(); event.stopPropagation(); send();
  }
}
const codes = ["codex_not_found", "codex_version_unsupported", "codex_signed_out", "codex_not_chatgpt", "confinement_failed", "codex_crashed", "run_busy", "share_required", "model_unavailable"] as const;
export function codexProblem(problem: CodexProblem | unknown) {
  const value = typeof problem === "object" && problem !== null && "code" in problem ? problem as CodexProblem : null;
  if (value && value.code in cellProblemKeys) return notice(cellProblemKeys[value.code as keyof typeof cellProblemKeys]);
  const contextCodes = {
    attachment_limit: "attachmentLimit", invalid_context_intent: "intentInvalid", invalid_selection: "selectionChanged",
    selection_changed: "selectionChanged", selection_too_large: "selectionTooLarge", selection_crop_invalid: "selectionCropInvalid",
    context_root_denied: "contextDenied", context_page_unavailable: "pageUnavailable",
    computational_context_unavailable: "calculationUnavailable", context_dependencies_unavailable: "contextUnavailable",
    context_limit: "contextLimit", context_unavailable: "contextUnavailable", context_changed: "notebookChanged",
    presets_not_supported: "presetsUnavailable", stale_request: "requestExpired",
  } as const;
  if (value && value.code in contextCodes) {
    const key = contextCodes[value.code as keyof typeof contextCodes];
    return key === "calculationUnavailable" ? notice("helper.calculationUnavailable") : notice(`codex.${key}`, { ...value.params, count: "8" });
  }
  const code = codes.find(code => code === value?.code) ?? "codex_crashed";
  return notice(`codex.${code}`, value?.params ?? {});
}
export function withMcpServer(servers: string[], name: string, allowed: boolean) {
  return allowed ? [...new Set([...servers, name])] : servers.filter(server => server !== name);
}
