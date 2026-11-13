import { translate } from "../i18n";
import type { EditorState } from "@codemirror/state";
export type EditorFocus = { fromLine: number; toLine: number; text: string };
export function extractFocus(state: EditorState): EditorFocus | null {
  const { from, to, empty } = state.selection.main;
  return empty ? null : { fromLine: state.doc.lineAt(from).number, toLine: state.doc.lineAt(Math.max(from, to - 1)).number, text: state.sliceDoc(from, Math.min(to, from + 2000)) };
}
export function focusPrompt(prompt: string, focus: EditorFocus | null): string {
  if (!focus) return prompt;
  const fence = "`".repeat(Math.max(3, ...Array.from(focus.text.matchAll(/`+/g), m => m[0].length + 1)));
  return `${translate("aiEntry.focusNote", { from: focus.fromLine, to: focus.toLine })}\n${fence}typst\n${focus.text}\n${fence}\n\n${prompt}`;
}

import { selectionCrop } from "./crop";
import type { TypstBlockState } from "../notebook/pageView";
import type { SharePayload, CropPt } from "../ipc/helper";
export function editorShare(root: string, pageId: string, mode: string, block: TypstBlockState | { id: string } | null, pageText: boolean, pageBounds: CropPt): SharePayload | null {
  if (mode !== "edit" || !block) return null;
  const cropPt = pageText ? pageBounds : "transform" in block ? selectionCrop([block.id], [], [], [], [block]) : null;
  return cropPt ? { root, pageId, objectIds: pageText ? [] : [block.id], strokeIds: [], cropPt, sessionIds: null } : null;
}
