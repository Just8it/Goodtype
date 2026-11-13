import { describe, expect, it, vi } from "vitest";
const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
import * as api from "../ipc/helper";
describe("helper IPC contract", () => {
  it("uses exact commands and camelCase envelopes", () => {
    const share: api.SharePayload = { root: "notes", pageId: "p", objectIds: ["a"], strokeIds: ["ink"], cropPt: { x: 1, y: 2, width: 3, height: 4 }, sessionIds: null };
    const reply: api.ContextReply = { requestId: "r", status: "busy" };
    api.helperStatus(); api.helperInstallRelay(); api.helperShare(share); api.helperUnshare(); api.helperContextReply(reply); api.helperAnswerConnection("s", false); api.helperAnswerScope("r", "once"); api.helperRevoke("s"); api.helperActivity();
    expect(invoke.mock.calls).toEqual([["helper_status"], ["helper_install_relay"], ["helper_share", { share }], ["helper_unshare"], ["helper_context_reply", { reply }], ["helper_answer_connection", { sessionId: "s", allow: false }], ["helper_answer_scope", { requestId: "r", answer: "once" }], ["helper_revoke", { sessionId: "s" }], ["helper_activity"]]);
  });
  it("listens to every event and unwraps payloads", () => {
    const handler = vi.fn(); for (const name of ["context-request", "connection-request", "scope-request", "scope-expired", "status", "activity"] as const) api.listenHelper(name, handler);
    expect(listen.mock.calls.map(c => c[0])).toEqual(["helper://context-request", "helper://connection-request", "helper://scope-request", "helper://scope-expired", "helper://status", "helper://activity"]);
    listen.mock.calls[0][1]({ payload: { requestId: "r" } }); expect(handler).toHaveBeenCalledWith({ requestId: "r" });
  });
});

it("sends page-only intent and exact request revocation identities", () => {
  invoke.mockClear();
  const intent: api.RequestContextIntent = { destination: { root: "book", pageId: "destination" }, selection: null, pageIds: ["context"], presets: false, intentVersion: "9007199254740993" };
  const request: api.CodexRunRequest = { prompt: "Summarize", intent, model: null, webSearch: false, conversationId: null };
  api.helperCodexRun(request);
  api.helperRevokeAttachment("conversation", "request", "context");
  api.helperRevokeNotebook("book");
  expect(invoke.mock.calls).toEqual([
    ["helper_codex_run", { request }],
    ["helper_revoke_attachment", { conversationId: "conversation", requestId: "request", pageId: "context" }],
    ["helper_revoke_notebook", { root: "book" }],
  ]);
});
