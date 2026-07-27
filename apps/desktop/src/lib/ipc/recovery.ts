/**
 * Interrupted-transaction candidates retained by the store.
 *
 * Also `notebook.rs` on the Rust side, kept separate here because it is a different job: these
 * are offers to the writer about work that was interrupted, not operations on the open notebook.
 */
import { invoke } from "@tauri-apps/api/core";

import type { RecoveryCandidate } from "../settings";
import type { HistoryResult } from "./types";

export function listRecoveryCandidates(root: string): Promise<RecoveryCandidate[]> {
  return invoke<RecoveryCandidate[]>("list_recovery_candidates", { root });
}

/** Land a candidate as a new committed revision. It becomes an ordinary undoable commit. */
export function restoreRecoveryCandidate(
  root: string,
  fileName: string,
): Promise<HistoryResult> {
  return invoke<HistoryResult>("restore_recovery_candidate", { root, fileName });
}

export function discardRecoveryCandidate(root: string, fileName: string): Promise<void> {
  return invoke("discard_recovery_candidate", { root, fileName });
}
