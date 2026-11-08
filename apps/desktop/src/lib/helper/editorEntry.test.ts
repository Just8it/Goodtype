import { describe, expect, it } from "vitest";
import { EditorState } from "@codemirror/state";
import { editorShare, extractFocus, focusPrompt } from "./editorEntry";
import type { TypstBlockState } from "../notebook/pageView";
const block = { id: "open", transform: { x: 12, y: 24, layoutWidthPt: 100, scale: 2, rotation: 0 }, result: { widthPt: 100, heightPt: 30 } } as TypstBlockState;
const bounds = { x: 0, y: 0, width: 595, height: 842 };
describe("editor AI context", () => {
 it("ignores a caret and bounds selected text", () => {
   expect(extractFocus(EditorState.create({ doc: "abc" }))).toBeNull();
   expect(extractFocus(EditorState.create({ doc: "a\nb\nc", selection: { anchor: 0, head: 4 } }))).toEqual({ fromLine: 1, toLine: 2, text: "a\nb\n" });
   expect(extractFocus(EditorState.create({ doc: "x".repeat(3000), selection: { anchor: 3000, head: 0 } }))?.text.length).toBe(2000);
 });
 it("quotes focus safely without changing the user's prompt", () => {
   const prompt = "Explain this";
   expect(focusPrompt(prompt, null)).toBe(prompt);
   expect(focusPrompt(prompt, { fromLine: 12, toLine: 18, text: "```" })).toBe("Focus on lines 12–18 of the selected block (quoted source):\n````typst\n```\n````\n\nExplain this");
   expect(prompt).toBe("Explain this");
 });
 it("captures the open block alone with canonical transformed bounds", () => {
   expect(editorShare("root", "page", "edit", block, false, bounds)).toEqual({ root: "root", pageId: "page", objectIds: ["open"], strokeIds: [], cropPt: { x: 12, y: 24, width: 200, height: 60 }, sessionIds: null });
 });
 it("page text has no block grant and uses page bounds", () => {
   expect(editorShare("root", "page", "edit", block, true, bounds)).toMatchObject({ objectIds: [], cropPt: bounds });
 });
 it("excludes style, away and missing targets", () => {
   for (const mode of ["style", "away", "none"]) expect(editorShare("root", "page", mode, block, false, bounds)).toBeNull();
   expect(editorShare("root", "page", "edit", null, false, bounds)).toBeNull();
 });
});
