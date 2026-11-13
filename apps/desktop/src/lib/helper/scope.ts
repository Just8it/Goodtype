import { helperAnswerScope } from "../ipc/helper";
export async function answerScope(requestId: string, answer: "once" | "session" | "deny" | "request") {
  try { await helperAnswerScope(requestId, answer); }
  catch (error) { if (!/unknown scope request|scope request expired/.test(String(error))) throw error; }
}

import type { ScopeRequest } from "../ipc/helper";
/** Identity and draft revision both matter: an away-and-back draft is a new intent. */
export function matchesScope(request: ScopeRequest, conversationId: string | null, requestId: string | null, root: string, version: number, capturedVersion: number | undefined) {
  return Boolean(request.conversationId && request.conversationId === conversationId && request.contextRequestId === requestId && request.root === root && capturedVersion === version);
}
