import { describe, expect, it } from "vitest";
import { render } from "svelte/server";
import SelectionActions from "../notebook/SelectionActions.svelte";
import ShareChip from "./ShareChip.svelte";
import HelperSetup from "./HelperSetup.svelte";
import { helper } from "./state";
describe("helper surfaces", () => {
  it("offers sharing behind More only when available", () => { const props = { subject: "ink" as const, left: 0, top: 0, ready: true }; expect(render(SelectionActions, { props }).body).not.toContain('aria-label="More actions"'); expect(render(SelectionActions, { props: { ...props, onShare() {} } }).body).toContain('aria-label="More actions"'); });
  it("labels recipients self-reported and offers a keyboard stop button", () => { helper.set({ enabled: true, listening: true, relayPath: "relay.exe", relayProblem: null, skillPath: "skills", usage: "balanced", sessions: [{ id: "s", clientName: "Codex", clientVersion: "1", approved: true, connectedAt: 0 }], share: { pageId: "p", pageNumber: 4, objectCount: 3, cropPt: { x: 1, y: 2, width: 3, height: 4 }, sessionIds: ["s"], sharedAt: 0 } }); const html = render(ShareChip, { props: { pageId: "p", width: 100, height: 100, onError() {} } }).body; expect(html).toContain("3 objects"); expect(html).toContain("self-reported"); expect(html).toContain('aria-label="Stop sharing"'); });
  it("renders quoted Codex registration and skill instructions", () => { const html = render(HelperSetup).body; expect(html).toContain('codex mcp add goodtype -- "relay.exe"'); expect(html).toContain("~/.agents/skills/"); });
});

import { setLanguage } from "../i18n";
it("translates Rust relay codes through both catalogs", () => {
  helper.update(s => s ? { ...s, relayProblem: { code: "relay_unavailable", params: {} } } : s);
  setLanguage("en"); expect(render(HelperSetup).body).toContain("The relay could not be installed");
  setLanguage("de"); expect(render(HelperSetup).body).toContain("Relay-Installation");
  setLanguage("en");
});
