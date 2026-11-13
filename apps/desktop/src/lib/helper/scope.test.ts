import { expect, it, vi } from "vitest";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { answerScope } from "./scope";
it("dismisses unknown and expired scope cards locally but retains retryable failures", async () => {
  for (const error of ["unknown scope request", "scope request expired"]) {
    invoke.mockRejectedValueOnce(error);
    await expect(answerScope("r", "deny")).resolves.toBeUndefined();
  }
  invoke.mockRejectedValueOnce("transport failed");
  await expect(answerScope("r", "once")).rejects.toBe("transport failed");
});

import { matchesScope } from "./scope";
it("matches only the current request, notebook and unchanged draft including A B A", () => {
 const value = {requestId:"approval",sessionId:"s",clientName:"Codex",conversationId:"c",contextRequestId:"r",root:"A",scope:{kind:"presets" as const},reason:"need"};
 expect(matchesScope(value,"c","r","A",1,1)).toBe(true);
 for (const args of [["c","new","A",1,1],["other","r","A",1,1],["c","r","B",1,1],["c","r","A",3,1],["c","r","A",1,undefined]] as const) expect(matchesScope(value,args[0],args[1],args[2],args[3],args[4])).toBe(false);
});
it("uses explicit request-only approval without changing terminal answers", async () => {
 invoke.mockResolvedValueOnce(undefined); await answerScope("approval","request");
 expect(invoke).toHaveBeenLastCalledWith("helper_answer_scope",{requestId:"approval",answer:"request"});
});
