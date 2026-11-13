import { notice } from "../i18n";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { PagePosition } from "../model";
import type { StructureHistoryResult, HistoryResult } from "./types";
export type Usage = "low" | "balanced" | "detailed";
export type CropPt = { x: number; y: number; width: number; height: number };
export type SessionInfo = { id: string; clientName: string; clientVersion: string; approved: boolean; connectedAt: number };
export type SharePayload = { root: string; pageId: string; objectIds: string[]; strokeIds: string[]; cropPt: CropPt; sessionIds: string[] | null };
export type ShareInfo = { pageId: string; pageNumber: number; objectCount: number; cropPt: CropPt; sessionIds: string[]; sharedAt: number };
export type HelperStatus = { enabled: boolean; listening: boolean; relayPath: string | null; relayProblem: { code: "relay_unavailable"; params: Record<string, string> } | null; skillPath: string | null; usage: Usage; sessions: SessionInfo[]; share: ShareInfo | null };
export type ContextReply = { requestId: string; status: "ok" | "busy" | "flush_failed" | "no_notebook"; root?: string; pageId?: string; editGeneration?: number };
export type ActivityEntry = { usageOverride: boolean; at: number; sessionId: string; clientName: string; tool: string; grant: "cells" | "none" | "selection" | "page" | "notebook"; outcome: "ok" | "denied" | "error" | "limit" | "pending_approval"; handedOver: { sourceBytes: number; strokes: number; images: { width: number; height: number }[] }; detailCode: "limit" | "pending_approval" | "changed" | "busy" | "denied" | "failed" | null };
export type ScopeRequest = { requestId: string; sessionId: string; clientName: string; scope: { kind: "pages"; pages: number[] } | { kind: "presets" } | { kind: "page_names" } | { kind: "notebook" }; reason: string; automatic?: boolean; conversationId?: string; contextRequestId?: string; root?: string; epoch?: string; pageIds?: string[]; targets?: string[] };
export const helperStatus = () => invoke<HelperStatus>("helper_status");
export const helperInstallRelay = () => invoke<HelperStatus>("helper_install_relay");
export const helperShare = (share: SharePayload) => invoke<ShareInfo>("helper_share", { share });
export const helperUnshare = () => invoke<void>("helper_unshare");
export const helperContextReply = (reply: ContextReply) => invoke<void>("helper_context_reply", { reply });
export const helperAnswerConnection = (sessionId: string, allow: boolean) => invoke<void>("helper_answer_connection", { sessionId, allow });
export const helperAnswerScope = (requestId: string, answer: "once" | "session" | "deny" | "request") => invoke<void>("helper_answer_scope", { requestId, answer });
export const helperRevoke = (sessionId: string) => invoke<void>("helper_revoke", { sessionId });
export const helperActivity = () => invoke<ActivityEntry[]>("helper_activity");
export type HelperEvents = { "codex-run": CodexRunEvent; "codex-delta": { runId: string; text: string; itemId?: string }; "codex-tool": CodexToolEvent; proposal: Proposal; "context-request": { requestId: string }; "connection-request": SessionInfo; "scope-request": ScopeRequest; "scope-granted": ScopeRequest; "scope-expired": { requestId: string }; status: HelperStatus; activity: ActivityEntry };
export function listenHelper<K extends keyof HelperEvents>(event: K, handler: (payload: HelperEvents[K]) => void) {
  return listen<HelperEvents[K]>(`helper://${event}`, event => handler(event.payload));
}

export type ProposalItem = { object: import("../model").PageObject; source: string; hash: string; svg: string; beforeSource: string | null; beforeSvg: string | null; widthPt: number; heightPt: number; placement: "beside_selection" | "below_selection" | {x:number;y:number} };
/** `page_text_edit` reviews existing Page text: one full-page item, a read-only managed header and overflow. */
export type ProposalKind = "page_create" | "cells_append" | "cell_edit" | "cell_insert" | "blocks" | "block_edit" | "page_text_edit" | "preset_install";
export type PresetReview = { source: string; name: string; description: string | null; sourceUrl: string | null; candidateHash: string; dependencyHashes: Record<string,string>; check: { form: "function" | "preamble" | null; sampleRevision: string; rendered: boolean; widthPt: number | null; heightPt: number | null; diagnostics: { severity: string; message: string }[] }; svg: string; png: string; filename: string; epoch: number; requestId: string };
export type PageReview = { targetPageId?: string | null; pageType: "blank" | "page_text" | "python"; title: string; position: PagePosition; source: string; cells: {cellType:"code"|"markdown";source:string}[]; svg: string | null };
export type Proposal = { newPage?: PageReview; cell?: CellProposal; preset?: PresetReview; overflow?: boolean; kind?: ProposalKind; managedHeader?: string | null; pageNumber?: number; anchor: CropPt; id: string; sessionId: string; root: string; pageId: string; version: number; status: "pending" | "inserted" | "dismissed" | "change_requested" | "stale" | "installed" | "already_installed"; text: string | null; changed: {dependency:"object"|"page"|"pageText"|"preset"|"sharedStyle"|"file";id?:string;change:string}[]; summary: string; assumptions: string[]; edit: boolean; items: ProposalItem[] };
export const helperProposals = () => invoke<Proposal[]>("helper_proposals");
/** Rust builds the candidate from the committed page; the caller flushes its own edits first. */
export const helperInsertProposal = (root: string, pageId: string, proposalId: string, proposalVersion: number) =>
  invoke<HistoryResult>("insert_helper_proposal", { root, pageId, proposal: { proposalId, proposalVersion } });
export const helperDismissProposal = (id:string) => invoke<void>("helper_dismiss_proposal",{id});
export const helperRequestChange = (id:string,text:string) => invoke<void>("helper_request_change",{id,text});
export const helperEditProposal = (id:string,proposalVersion:number,sources:string[]) => invoke<Proposal>("helper_edit_proposal",{id,proposalVersion,sources});

export type CodexProblem = { code: string; params: Record<string, string> };
export type CodexStatus = {
  available: boolean; path: string | null; version: string | null;
  account: "chatgpt" | "none" | "other" | "unknown";
  problem: CodexProblem | null;
  models: { id: string; label: string; image: boolean }[];
  mcpServers: { name: string; allowed: boolean }[];
};
export type CodexRunRequest = { usageOverride?: "balanced"; prompt: string; model: string | null; webSearch: boolean; conversationId: string | null } & ({ intent: RequestContextIntent; share?: never } | { share: SharePayload; intent?: never });
export type CodexRunState = "starting" | "reading" | "preparing" | "checking" | "ready" | "done" | "failed" | "stopped";
export type CodexRunEvent = { runId: string; conversationId: string; state: CodexRunState; problem?: CodexProblem };
export type CodexToolEvent = { runId: string; server: string; tool: string; phase: "start" | "done" | "error"; external: boolean };
export const helperCodexStatus = (refresh = false) => invoke<CodexStatus>("helper_codex_status", { refresh });
export const helperCodexRun = (request: CodexRunRequest) => invoke<{ runId: string; requestId: string; conversationId: string }>("helper_codex_run", { request });
export const helperCodexStop = (runId: string) => invoke<void>("helper_codex_stop", { runId });
export const helperCodexClose = (conversationId: string) => invoke<void>("helper_codex_close", { conversationId });

/** Draft context only. Authority is granted by Rust at Send, never by this value. */
export type RequestContextIntent = {
  sharePageNames?: boolean;
  cells?: CellGrantIntent | null;
  destination: { root: string; pageId: string };
  selection: SharePayload | null;
  pageIds: string[];
  presets: boolean;
  /** Monotonically changes on draft edits, including A ? B ? A. */
  intentVersion: string;
};

/** Immediate removal from a live request, including pending launch admission. */
/** Reserved revocation target; this is never a notebook page ID. */
export const helperRevokePresets = (conversationId: string, requestId: string) =>
  helperRevokeAttachment(conversationId, requestId, "@presets");
export const helperRevokeAttachment = (conversationId: string, requestId: string, pageId: string) =>
  invoke<void>("helper_revoke_attachment", { conversationId, requestId, pageId });
/** Call on notebook close; Hide must not call this. */
export const helperRevokeNotebook = (root: string) => invoke<void>("helper_revoke_notebook", { root });

export type PresetInstallInput = { proposalId: string; proposalVersion: number; candidateHash: string; sessionId: string; root: string };
export type PresetInstallResult = { status: "installed" | "already_installed"; path: string; wrote: boolean };
export const helperInstallPreset = (input: PresetInstallInput) => invoke<PresetInstallResult>("helper_install_preset", { input });

/** Disclosure predicate; does not grant authority. Empty IDs plus a valid crop mean a region. */
export function helperSelectionNeedsPixels(objectTypes: readonly string[], pdfBackground: boolean, region: boolean): boolean {
  return region || pdfBackground || objectTypes.some(type => ["ink_group", "image", "pdf_material"].includes(type));
}

export type CellGrantIntent = { calculationId: string; partId: string; cellIds: string[]; includeOutputs: boolean };
export type CellProposal = { calculationId: string; partId: string; cellId: string; afterCellId: string | null; cellType: string; oldSource: string | null; newSource: string };
export type ApplyCellResult = { cellId: string; calculationRevision: number; result: import("./calculation").CalculationResult };
export const helperApplyCellProposal = (proposalId: string, version: number) => invoke<ApplyCellResult>("apply_helper_cell_proposal", { input: { proposalId, version } });
export const helperRevokeCells = (conversationId: string, requestId: string) => helperRevokeAttachment(conversationId, requestId, "@cells");

export const cellProblemKeys = {
  cell_grant_denied: "helper.cell_grant_denied",
  cell_proposal_revoked: "helper.cell_proposal_revoked",
  cell_proposal_duplicate: "helper.cell_proposal_duplicate",
  cell_proposal_changed: "helper.cell_proposal_changed",
  cell_proposal_destination: "helper.cell_proposal_destination",
  cell_proposal_stale: "helper.cell_proposal_stale",
  cell_proposal_persist_failed: "helper.cell_proposal_persist_failed",
} as const;

/** Render structured cell problems through the locale catalogs. */
export function helperCellProblem(problem: CodexProblem) {
  const key = cellProblemKeys[problem.code as keyof typeof cellProblemKeys] ?? cellProblemKeys.cell_proposal_persist_failed;
  return notice(key, problem.params);
}

export const helperCreatePage = (root:string, proposalId:string, proposalVersion:number, position:PagePosition) => invoke<StructureHistoryResult & {calculation?: import("./calculation").CalculationResult}>("apply_helper_page_proposal", {root,input:{proposalId,proposalVersion,position,modifiedAt:new Date().toISOString()}});
