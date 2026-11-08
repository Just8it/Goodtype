import { describe, expect, it, vi } from "vitest";
const { load } = vi.hoisted(() => ({ load: vi.fn() }));
vi.mock("../ipc/workspace", () => ({ loadAppSettings: load, saveAppSettings: vi.fn() }));
import { DEFAULT_SETTINGS, loadSettings } from "../settings";
import { searchSettings, settingGroups } from "../workspace/settingsSchema";
describe("helper settings migration", () => {
  it("keeps one AI category and exposes only enable until opted in", () => {
    const off = settingGroups().filter(group => group.id === "aiHelper");
    expect(off).toHaveLength(0);
    expect(settingGroups().find(group => group.id === "appearance")?.items.map(item => item.id)).toContain("helperEnabled");
    expect(settingGroups(undefined, [], true).find(group => group.id === "aiHelper")?.items.map(item => item.id)).toEqual(["helperEnabled", "helperUsage", "helperWebSearch", "helperPermissionPages", "helperPermissionPresets", "helperPermissionNames"]);
    expect(searchSettings("web", settingGroups()).some(hit => hit.group.id === "aiHelper")).toBe(false);
  });
  it("defaults to disabled, balanced and web search enabled", async () => { expect((await loadSettings(false)).aiHelper).toEqual({ enabled: false, usage: "balanced", webSearch: true, codexPath: null, codexModel: null, allowedMcpServers: [], permissions: { pages: "ask", presets: "ask", pageNames: "allow" } }); });
  it("loads old settings", async () => { const { aiHelper: _, ...old } = DEFAULT_SETTINGS; load.mockResolvedValue(old); expect((await loadSettings(true)).aiHelper).toEqual(DEFAULT_SETTINGS.aiHelper); });
  it("preserves choices and fills missing fields", async () => { load.mockResolvedValue({ ...DEFAULT_SETTINGS, aiHelper: { enabled: true, usage: "low" } }); expect((await loadSettings(true)).aiHelper).toEqual({ enabled: true, usage: "low", webSearch: true, codexPath: null, codexModel: null, allowedMcpServers: [], permissions: { pages: "ask", presets: "ask", pageNames: "allow" } }); });
  it("keeps saved permissions and fills missing ones", async () => { load.mockResolvedValue({ ...DEFAULT_SETTINGS, aiHelper: { permissions: { presets: "never" } } }); expect((await loadSettings(true)).aiHelper.permissions).toEqual({ pages: "ask", presets: "never", pageNames: "allow" }); });
  it("searches catalog keywords without exposing disabled AI categories", () => { expect(searchSettings("MCP").some(h => h.item.id === "helperEnabled")).toBe(true); });
});

 it.each([undefined, null, "", "   "])("normalizes absent/default model %s to null", async codexModel => {
   load.mockResolvedValue({ ...DEFAULT_SETTINGS, aiHelper: { codexModel } });
   expect((await loadSettings(true)).aiHelper.codexModel).toBeNull();
 });
 it.each(["gpt-6-terra", "gpt-6-sol"])("preserves explicit saved model %s", async codexModel => {
   load.mockResolvedValue({ ...DEFAULT_SETTINGS, aiHelper: { codexModel } });
   expect((await loadSettings(true)).aiHelper.codexModel).toBe(codexModel);
 });
