import { invoke } from "@tauri-apps/api/core";
import type { ImporterStatus } from "./importer";

export type InkPolylines = { widthPt: number; points: [number, number][] }[];
export type RecognitionCapabilities = { recognizers: string[]; math: ImporterStatus };
export type MathRecognition = { typst: string | null; unsupported: string | null; latex: string };

export const recognitionCapabilities = () => invoke<RecognitionCapabilities>("recognition_capabilities");
export const mathRecognizerStatus = () => invoke<ImporterStatus>("importer_status", { tool: "math" });
export const installMathRecognizer = () => invoke<void>("importer_install", { tool: "math" });
export const removeMathRecognizer = () => invoke<void>("remove_math_recognizer");
export const cancelMathRecognizerDownload = () => invoke<void>("importer_cancel", { tool: "math" });
export const prepareMathRecognition = () => invoke<void>("prepare_math_recognition");
export function recognizeInkText(strokes: InkPolylines, recognizer: string | null): Promise<{ text: string }> {
  return invoke("recognize_ink_text", { selection: { strokes: strokes.map(s => s.points) }, recognizer });
}
export function recognizeInkMath(strokes: InkPolylines): Promise<MathRecognition> {
  return invoke("recognize_ink_math", { selection: { strokes } });
}
