import { describe, expect, it, vi } from "vitest";
import { contextReply } from "./context";
const make = () => ({ root: "notes", activePageId: "p1", editGeneration: 3, persist: vi.fn(async () => true) });
describe("helper context responder", () => {
  it("flushes and returns the exact page and generation", async () => { const s = make(); expect(await contextReply("r", () => s, () => false)).toEqual({ requestId: "r", status: "ok", root: "notes", pageId: "p1", editGeneration: 3 }); expect(s.persist).toHaveBeenCalledOnce(); });
  it("returns busy after two seconds without flushing", async () => { const s = make(), pause = vi.fn(async () => {}); expect((await contextReply("r", () => s, () => true, pause)).status).toBe("busy"); expect(pause).toHaveBeenCalledTimes(80); expect(s.persist).not.toHaveBeenCalled(); });
  it("continues after pen-up", async () => { const s = make(); let down = true; expect((await contextReply("r", () => s, () => down, async () => { down = false; })).status).toBe("ok"); });
  it.each([false, "throw"])("reports flush_failed for %s", async failure => { const s = make(); s.persist.mockImplementation(async () => { if (failure === "throw") throw Error(); return false; }); expect((await contextReply("r", () => s, () => false)).status).toBe("flush_failed"); });
  it("reports no_notebook", async () => { expect((await contextReply("r", () => null, () => false)).status).toBe("no_notebook"); const s = make(); s.root = ""; expect((await contextReply("r", () => s, () => false)).status).toBe("no_notebook"); });
  it("rejects edits during persistence", async () => { const s = make(); s.persist.mockImplementation(async () => { s.editGeneration++; return true; }); expect((await contextReply("r", () => s, () => false)).status).toBe("busy"); });
  it("rejects page switches during persistence", async () => { const s = make(); s.persist.mockImplementation(async () => { s.activePageId = "p2"; return true; }); expect((await contextReply("r", () => s, () => false)).status).toBe("busy"); });
});
