import { describe, expect, it, vi } from "vitest";
import { createWindowCloseHandler } from "./windowClose";

describe("native window close ordering", () => {
  it("flushes settings and notebook before remembering the page and destroying once", async () => {
    const calls: string[] = [];
    let confirm!: (value: boolean) => void;
    const handler = createWindowCloseHandler({
      saveSettings: async () => { calls.push("settings"); return true; },
      saveNotebook: () => { calls.push("notebook"); return new Promise(resolve => { confirm = resolve; }); },
      rememberPage: async () => { calls.push("position"); },
      destroy: async () => { calls.push("destroy"); }, failed: vi.fn(),
    });
    const event = { preventDefault: vi.fn() };
    const closing = handler.handle(event); await Promise.resolve();
    expect(handler.closing).toBe(true); expect(calls).toEqual(["settings", "notebook"]);
    await handler.handle(event); confirm(true); await closing;
    expect(calls).toEqual(["settings", "notebook", "position", "destroy"]);
    expect(handler.closing).toBe(false); expect(event.preventDefault).toHaveBeenCalledTimes(2);
  });

  it.each(["settings", "notebook", "exception"])("leaves the window open on %s failure", async failure => {
    const destroy = vi.fn(), rememberPage = vi.fn(), failed = vi.fn();
    const handler = createWindowCloseHandler({
      saveSettings: async () => failure !== "settings",
      saveNotebook: async () => { if (failure === "exception") throw new Error("disk full"); return false; },
      destroy, rememberPage, failed,
    });
    await handler.handle({ preventDefault() {} });
    expect(destroy).not.toHaveBeenCalled(); expect(rememberPage).not.toHaveBeenCalled();
    expect(handler.closing).toBe(false);
    expect(failed).toHaveBeenCalledTimes(failure === "exception" ? 1 : 0);
  });
});
