import { beforeEach, describe, expect, it, vi } from "vitest";
import { render } from "svelte/server";
const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
import { canSend, composerKey, codexProblem, withMcpServer } from "./codex";
import { helperCodexStatus, helperCodexRun, helperCodexStop, helperCodexClose, listenHelper, type CodexStatus, type CodexRunState } from "../ipc/helper";
import { DEFAULT_SETTINGS } from "../settings";
import { codexStatus } from "./state";
import { renderMessage, setLanguage, translate } from "../i18n";
import PageAttachments from "./PageAttachments.svelte";
import CodexComposer from "./CodexComposer.svelte";
import CodexRunPanel from "./CodexRunPanel.svelte";
import CodexSettings from "./CodexSettings.svelte";
import SelectionActions from "../notebook/SelectionActions.svelte";

const status: CodexStatus = { available: true, path: "codex.exe", version: "0.166", account: "chatgpt", problem: null,
  models: [{ id: "m", label: "Model M", image: true }], mcpServers: [{ name: "CAD", allowed: false }] };
beforeEach(() => { vi.clearAllMocks(); setLanguage("en"); codexStatus.set(status); });

describe("Codex composer", () => {
  it("sends on Enter, keeps Shift+Enter, ignores composition and closes on Escape", () => {
    const send = vi.fn(), close = vi.fn(), preventDefault = vi.fn(), stopPropagation = vi.fn();
    const event = { key: "Enter", shiftKey: false, isComposing: false, preventDefault, stopPropagation };
    composerKey(event, send, close); expect(send).toHaveBeenCalledTimes(1); expect(preventDefault).toHaveBeenCalledTimes(1);
    composerKey({ ...event, shiftKey: true }, send, close);
    composerKey({ ...event, isComposing: true }, send, close); expect(send).toHaveBeenCalledTimes(1);
    composerKey({ ...event, key: "Escape" }, send, close); expect(close).toHaveBeenCalledOnce();
    expect(stopPropagation).toHaveBeenCalledTimes(2);
  });
  it("requires nonblank input, caps at 4000 and blocks a busy run", () => {
    expect(canSend("  ")).toBe(false); expect(canSend("x".repeat(4000))).toBe(true);
    expect(canSend("x".repeat(4001))).toBe(false); expect(canSend("hello", true)).toBe(false);
  });
  it("discloses provider, account, experiment, attachments and all input controls without autofocus", () => {
    const html = render(CodexComposer, { props: { status, count: 2, async onSend() { return true; }, onClose() {} } }).body;
    for (const text of ["Codex sends this to OpenAI with your account.", "Experimental", "not fully sandboxed", "2 selected items attached", "Model M", "Web search", 'maxlength="4000"', "packages.typst.org", "not your notebook content"]) expect(html).toContain(text);
    expect(html).not.toContain("autofocus");
  });
  it("shows the selection action only when provided", () => {
    const props = { subject: "ink" as const, left: 0, top: 0, ready: true };
    expect(render(SelectionActions, { props }).body).not.toContain('aria-label="Ask AI"');
    expect(render(SelectionActions, { props: { ...props, onAI() {} } }).body).toContain('aria-label="Ask AI"');
  });
  it("shows the attached block excerpt as escaped content", () => {
    const html = render(CodexComposer, { props: { status,count:1,selectionLabel:"Second block <script>",async onSend(){return true;},onClose(){} } }).body;
    expect(html).toContain("Second block &lt;script>");
    expect(html).not.toContain("Second block <script>");
  });
});

describe("Codex run panel", () => {
  it.each(["en", "de"] as const)("shows retry counts, Stop, and a manual retry only for startup connection failures in %s", language => {
    setLanguage(language);
    for (const attempt of [1, 5]) {
      const html = render(CodexRunPanel, { props: { state: "starting", text: "", tools: [], onStop() {}, onRetry() {},
        problem: { code: "connection_retrying", params: { attempt: String(attempt), max: "5", seconds: "15" } } } }).body;
      expect(html).toContain(`(${attempt}/5)`);
      expect(html).toContain(translate("codex.stop"));
      expect(html).not.toContain('role="alert"');
      expect(html).not.toContain(`>${translate("codex.retry")}</button>`);
    }
    for (const code of ["provider_unreachable", "confinement_failed", "codex_crashed"]) {
      const html = render(CodexRunPanel, { props: { state: "failed", text: "", tools: [], onStop() {}, onRetry() {},
        problem: { code, params: { detail: "<private>" } } } }).body;
      expect(html.includes(`>${translate("codex.retry")}</button>`)).toBe(code === "provider_unreachable");
      expect(html).toContain("&lt;private>");
    }
  });

  for (const state of ["starting", "reading", "preparing", "checking", "ready", "done", "failed", "stopped"] as CodexRunState[]) {
    it(`renders ${state} and Stop only for active runs`, () => {
      const html = render(CodexRunPanel, { props: { state, text: "Answer <script>", tools: [], onStop() {}, onClose() {} } }).body;
      expect(html).toContain(translate(`codex.${state}`)); expect(html.includes(">Stop</button>")).toBe(!["done", "failed", "stopped"].includes(state));
      expect(html).toContain("&lt;script>"); expect(html).not.toContain("Close conversation");
    });
  }
  it("highlights external calls and keeps Goodtype calls ordinary", () => {
    const html = render(CodexRunPanel, { props: { state: "reading", text: "", tools: [
      { runId: "r", server: "CAD", tool: "draw", phase: "start", external: true },
      { runId: "r", server: "goodtype", tool: "render", phase: "done", external: false },
    ], onStop() {}, onClose() {} } }).body;
    expect(html.match(/outside Goodtype/g)).toHaveLength(1); expect(html).toMatch(/class="[^"]*external/); expect(html).toContain("CAD: draw");
  });
  it("keeps the streamed response visible while tool activity is collapsed", () => {
    const html = render(CodexRunPanel, { props: { state: "reading", text: "Working on the translation", tools: [
      { runId:"r", server:"goodtype", tool:"get_context", phase:"start", external:false },
      { runId:"r", server:"goodtype", tool:"get_context", phase:"done", external:false },
    ], onStop() {} } }).body;
    expect(html.indexOf("Working on the translation")).toBeLessThan(html.indexOf("<details"));
    expect(html).toContain("1 tool step");
    expect(html).not.toMatch(/<details[^>]*\bopen\b/);
    expect(html.match(/goodtype: get_context/g)).toHaveLength(1);
  });
});

describe("Codex settings", () => {
  it("defaults all external servers off and changes only the named server", () => {
    expect(DEFAULT_SETTINGS.aiHelper.allowedMcpServers).toEqual([]);
    expect(withMcpServer([], "CAD", true)).toEqual(["CAD"]);
    expect(withMcpServer(["CAD"], "CAD", true)).toEqual(["CAD"]);
    expect(withMcpServer(["CAD", "other"], "CAD", false)).toEqual(["other"]);
    const off = render(CodexSettings, { props: { settings: DEFAULT_SETTINGS, onChange() {} } }).body;
    expect(off).toContain('role="switch"'); expect(off).not.toContain("checked");
    const on = render(CodexSettings, { props: { settings: { ...DEFAULT_SETTINGS, aiHelper: { ...DEFAULT_SETTINGS.aiHelper, allowedMcpServers: ["CAD"] } }, onChange() {} } }).body;
    expect(on).toContain("checked"); expect(on).toContain("outside Goodtype"); expect(on).toContain("Account type: ChatGPT");
    expect(on).not.toContain("email");
  });
  it("localizes every contract problem code and falls back for unknown errors", () => {
    for (const lang of ["en", "de"] as const) {
      setLanguage(lang);
      for (const code of ["codex_not_found", "codex_version_unsupported", "codex_signed_out", "codex_not_chatgpt", "confinement_failed", "codex_crashed", "run_busy", "share_required"]) {
        const text = renderMessage(codexProblem({ code, params: {} }), translate);
        expect(text).not.toContain(`codex.${code}`); expect(text.length).toBeGreaterThan(15);
      }
    }
    expect(codexProblem(new Error("private detail"))).toEqual(codexProblem({ code: "codex_crashed", params: {} }));
    setLanguage("en");
  });
});

describe("Codex IPC", () => {
  it("uses contract envelopes including follow-up conversation and explicit web choice", () => {
    const request = { prompt: "Explain", share: { root: "notes", pageId: "p", objectIds: ["a"], strokeIds: [], cropPt: { x: 1, y: 2, width: 30, height: 40 }, sessionIds: null }, model: "m", webSearch: false, conversationId: "c" };
    helperCodexStatus(); helperCodexStatus(true); helperCodexRun(request); helperCodexStop("r"); helperCodexClose("c");
    expect(invoke.mock.calls).toEqual([["helper_codex_status", { refresh: false }], ["helper_codex_status", { refresh: true }], ["helper_codex_run", { request }], ["helper_codex_stop", { runId: "r" }], ["helper_codex_close", { conversationId: "c" }]]);
  });
  it("unwraps all three streaming events", () => {
    const handler = vi.fn();
    for (const name of ["codex-run", "codex-delta", "codex-tool"] as const) listenHelper(name, handler);
    expect(listen.mock.calls.map(call => call[0])).toEqual(["helper://codex-run", "helper://codex-delta", "helper://codex-tool"]);
    for (const call of listen.mock.calls) call[1]({ payload: { runId: "r" } });
    expect(handler).toHaveBeenCalledTimes(3); expect(handler).toHaveBeenCalledWith({ runId: "r" });
  });
});


it("maps every request-context refusal to translated actionable copy", () => {
  for (const code of ["attachment_limit", "invalid_context_intent", "invalid_selection", "context_root_denied", "context_page_unavailable", "computational_context_unavailable", "context_dependencies_unavailable", "context_limit", "context_unavailable", "context_changed", "presets_not_supported", "stale_request"]) {
    const message = renderMessage(codexProblem({ code, params: { detail: "PRIVATE RUST DETAIL" } }), translate);
    expect(message).not.toContain("PRIVATE RUST DETAIL");
    expect(message).not.toContain(code);
    expect(message.length).toBeGreaterThan(25);
  }
});


it("collapses eight attachments to two chips and keeps all picker entries", () => {
  const pages = Array.from({ length: 10 }, (_, i) => ({ id: `page-${i}`, number: i + 1, title: `Title ${i}`, calculation: i === 9 }));
  const html = render(PageAttachments, { props: { pages, selected: pages.slice(0, 8).map(page => page.id), currentPageId: "page-0", onToggle() {} } }).body;
  expect(html).toContain("6 more pages");
  expect(html).toContain('class="page-chip more');
  expect(html).not.toContain("<small>Notebook page</small>");
  expect(html.match(/<span class="page-chip/g)).toHaveLength(2);
  expect(html.match(/role="checkbox"/g)).toHaveLength(10);
  // Python pages are ordinary attachments now: selectable, labelled with what they share.
  expect(html).not.toContain('aria-disabled="true"');
  expect(html).toContain("Python cells and saved outputs");
  expect(html).toContain('aria-label="Remove page 1"');
});

it("omits the selection row for page-only requests", () => {
  const html = render(CodexComposer, { props: { status, count: 0, async onSend() { return true; }, onClose() {} } }).body;
  expect(html).not.toContain("0 selected items attached");
  expect(html).not.toContain('class="context-row');
  for (const language of ["en", "de"] as const) {
    setLanguage(language);
    expect(translate("codex.pageOnlyHint")).not.toContain("codex.");
  }
  setLanguage("en");
});

 it.each(["en", "de"] as const)("names the actual default and actionable unavailable error in %s", language => {
   setLanguage(language);
   expect(translate("codex.defaultModel")).toContain("Luna");
   const html = render(CodexSettings, { props: { settings: DEFAULT_SETTINGS, onChange() {} } }).body;
   expect(html).toContain(translate("codex.defaultModel"));
   const message = renderMessage(codexProblem({ code: "model_unavailable", params: { detail: "gpt-6-luna" } }), translate);
   expect(message).toContain("gpt-6-luna");
   expect(message).toContain(language === "en" ? "choose an available model" : "ein verf\u00fcgbares Modell");
 });

it.each(["en", "de"] as const)("distinguishes actionable capture errors in %s", language => {
 setLanguage(language);
 const codes = ["invalid_context_intent", "selection_changed", "selection_too_large", "selection_crop_invalid", "context_changed", "stale_request"];
 const messages = codes.map(code => renderMessage(codexProblem({ code }), translate));
 expect(new Set(messages).size).toBe(codes.length);
 for (const message of messages) expect(message).not.toBe(translate("codex.contextChanged"));
});
