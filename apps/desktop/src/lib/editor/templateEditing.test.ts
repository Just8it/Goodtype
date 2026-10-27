import { afterEach, describe, expect, it, vi } from "vitest";
import { createTemplateEditing } from "./templateEditing.svelte";

const ipc = vi.hoisted(() => ({ read: vi.fn(), save: vi.fn() }));
vi.mock("../ipc/presets", () => ({ readTypstPreset: ipc.read, saveTypstPreset: ipc.save }));

afterEach(() => { vi.useRealTimers(); vi.resetAllMocks(); });

function editor() {
  let context = { root: "notes", generation: 1 };
  const saved = vi.fn(), report = vi.fn(), show = vi.fn();
  const editing = createTemplateEditing({ context: () => context, saved, report, show });
  return { editing, saved, report, show, switchRoot: () => { context = { root: "other", generation: 2 }; } };
}

describe("template editing lifecycle", () => {
  it("retains failed and conflicting drafts and refuses close until retry succeeds", async () => {
    vi.useFakeTimers();
    ipc.read.mockResolvedValue("// Name: Original\n#set text(size: 12pt)");
    ipc.save.mockRejectedValueOnce({ code: "external_change", details: "changed" }).mockResolvedValue(undefined);
    const { editing, saved } = editor();
    await editing.open("/styles/default.typ", "Original");
    editing.update("// Name: Edited\n#set text(size: 13pt)");
    expect(await editing.close()).toBe(false);
    expect(editing.draft?.source).toContain("Edited");
    expect(editing.draft?.dirty).toBe(true);
    expect(editing.error).toBe("changed");
    expect(await editing.close()).toBe(true);
    expect(editing.draft).toBeNull();
    expect(saved).toHaveBeenCalledOnce();
    expect(ipc.save).toHaveBeenLastCalledWith("notes", "/styles/default.typ",
      "// Name: Edited\n#set text(size: 13pt)", "// Name: Original\n#set text(size: 12pt)");
  });

  it("discards a delayed open after root invalidation and cancels its timer on disposal", async () => {
    vi.useFakeTimers();
    let finish!: (source: string) => void;
    ipc.read.mockImplementationOnce(() => new Promise<string>(resolve => { finish = resolve; }));
    const { editing, show, switchRoot } = editor();
    const opening = editing.open("/styles/a.typ", "A");
    await Promise.resolve();
    switchRoot(); finish("original"); await opening;
    expect(editing.draft).toBeNull(); expect(show).not.toHaveBeenCalled();
    ipc.read.mockResolvedValue("original");
    await editing.open("/styles/a.typ", "A");
    editing.update("edited"); editing.dispose();
    await vi.advanceTimersByTimeAsync(1_000);
    expect(ipc.save).not.toHaveBeenCalled();
  });
});
