/**
 * The Typst compiler, in the same process as Rust (ADR 0006).
 *
 * Caching is not done here on purpose. `typstCache` is keyed on source and width, which is a
 * decision about when a preview is still good enough to reuse — a policy the caller owns, not
 * something the transport should quietly apply on its behalf.
 */
import { invoke } from "@tauri-apps/api/core";

import type { TypstCompileResult } from "../editor/typst";

/**
 * Mirrors `CompileBlockRequest` in `src-tauri/src/typst.rs`, and deliberately carries no package
 * policy. Whether a compile may reach the network is Rust's decision, read from settings — the
 * internal `CompileRequest` has an `allow_remote_packages` field that the command fills in
 * itself. Accepting one here would let the frontend override a user's choice.
 */
export type TypstCompileRequest = {
  source: string;
  sharedStyle?: string | null;
  widthPt: number;
  generation: number;
};

export function compileTypst(
  root: string,
  request: TypstCompileRequest,
): Promise<TypstCompileResult> {
  return invoke<TypstCompileResult>("compile_typst", { root, request });
}

export type TypstCompletionItem = {
  kind: string;
  symbol: string | null;
  label: string;
  apply: string | null;
  detail: string | null;
  /** Byte offset into the UTF-8 source where the replacement starts. */
  offset: number;
};

export function completeTypst(
  root: string,
  source: string,
  cursor: number,
  explicit: boolean,
): Promise<TypstCompletionItem[]> {
  return invoke<TypstCompletionItem[]>("complete_typst", { root, source, cursor, explicit });
}

export type TypstHover = { value: string; code: boolean };

export function hoverTypst(
  root: string,
  source: string,
  cursor: number,
): Promise<TypstHover | null> {
  return invoke<TypstHover | null>("hover_typst", { root, source, cursor });
}

export function formatTypst(root: string, source: string): Promise<string> {
  return invoke<string>("format_typst", { root, source });
}

export type TypstHighlight = { kind: string; modifiers: string[]; from: number; to: number };
export type TypstDiagnostic = {
  severity: "error" | "warning" | "info";
  message: string;
  from: number;
  to: number;
};
export type TypstAnalysis = {
  highlights: TypstHighlight[];
  diagnostics: TypstDiagnostic[];
  /**
   * Whether an analyzer answered at all. Semantic highlighting has no in-process fallback, so
   * an empty analysis with `available: false` means "nothing is analysing", not "nothing to
   * highlight" — and the colours already on screen should stay rather than being cleared.
   */
  available: boolean;
};

export function analyzeTypst(root: string, source: string): Promise<TypstAnalysis> {
  return invoke<TypstAnalysis>("analyze_typst", { root, source });
}
