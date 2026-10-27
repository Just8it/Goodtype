import { expect, it, vi } from "vitest";
import { createTemplateDraft } from "./templateDraft";
import { saveTypstPreset } from "../ipc/presets";

vi.mock("../ipc/presets", () => ({ saveTypstPreset: vi.fn() }));

it("orders writes, keeps newer edits, and stays bound to the original notebook", async () => {
  vi.mocked(saveTypstPreset).mockReset();
  let release!: () => void;
  vi.mocked(saveTypstPreset).mockImplementationOnce(() => new Promise<void>(resolve => { release = resolve; }))
    .mockResolvedValue(undefined);
  const draft = createTemplateDraft("notebook A", "/styles/a.typ", "A", "original");
  draft.source = "first";
  const first = draft.flush();
  await vi.waitFor(() => expect(saveTypstPreset).toHaveBeenCalledTimes(1));
  draft.source = "second";
  const second = draft.flush();
  release();
  await Promise.all([first, second]);
  expect(vi.mocked(saveTypstPreset).mock.calls).toEqual([
    ["notebook A", "/styles/a.typ", "first", "original"],
    ["notebook A", "/styles/a.typ", "second", "first"],
  ]);
  expect(draft.dirty).toBe(false);
});

it("retains a failed or empty buffer and retries against the last confirmed source", async () => {
  vi.mocked(saveTypstPreset).mockReset().mockRejectedValueOnce(new Error("external change")).mockResolvedValue(undefined);
  const draft = createTemplateDraft("A", "/styles/a.typ", "A", "original");
  draft.source = "unsaved";
  await expect(draft.flush()).rejects.toThrow("external change");
  expect(draft.source).toBe("unsaved");
  expect(draft.savedSource).toBe("original");
  expect(draft.dirty).toBe(true);
  await draft.flush();
  expect(saveTypstPreset).toHaveBeenLastCalledWith("A", "/styles/a.typ", "unsaved", "original");
  draft.source = "";
  await expect(draft.flush()).rejects.toThrow("empty template");
  expect(draft.savedSource).toBe("unsaved");
});
