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
  widthPt: number;
  generation: number;
};

export function compileTypst(
  root: string,
  request: TypstCompileRequest,
): Promise<TypstCompileResult> {
  return invoke<TypstCompileResult>("compile_typst", { root, request });
}
