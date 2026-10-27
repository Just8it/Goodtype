export type NotebookFailureCode = "external_change" | "revision_conflict" | "recovery_limit" | "operation_failed";

/** Stable Rust codes decide recovery behavior. External diagnostic prose is never inspected. */
export function notebookFailureCode(error: unknown): NotebookFailureCode {
  if (error && typeof error === "object" && "code" in error) {
    const code = error.code;
    if (code === "external_change" || code === "revision_conflict" || code === "recovery_limit") return code;
  }
  return "operation_failed";
}

export function errorDetails(error: unknown): string {
  if (error && typeof error === "object" && "details" in error && typeof error.details === "string") return error.details;
  return error instanceof Error ? error.message : String(error);
}
