import { keepOnPage } from "../geometry/placement";
import { transcriptionPlacement } from "../notebook/transcription";
import type { Proposal } from "../ipc/helper";

/** A failed follow-up must leave the current proposal usable and the revision editor open. */
export async function sendProposalRevision(proposal: Proposal, text: string, send: (prompt: string) => Promise<boolean>): Promise<void> {
  if (!await send(`Revise proposal ${proposal.id} as requested: ${text}`)) throw new Error("revision_failed");
}
export async function acceptProposal(proposal:Proposal, env:{current:()=>{root:string;pageId:string};persist:()=>Promise<boolean>;submit:(p:Proposal)=>Promise<void>}) {
  const captured={...env.current()}, version=proposal.version;
  if(captured.root!==proposal.root || (!proposal.newPage && captured.pageId!==proposal.pageId)) throw new Error("wrong_page");
  if(!await env.persist()) throw new Error("flush_failed");
  const now=env.current();
  if(now.root!==captured.root || now.pageId!==captured.pageId || version!==proposal.version) throw new Error("wrong_page");
  await env.submit({...proposal,version});
}

/** Validate the stored placement with the same helpers as transcription; never allocate new IDs. */
export function validateProposalPlacement(proposal:Proposal,page:{widthPt:number;heightPt:number}) {
  if(proposal.edit)return;
  for(const [index,item] of proposal.items.entries()){
    const a=proposal.anchor;
    const below=transcriptionPlacement({left:a.x,top:a.y,right:a.x+a.width,bottom:a.y+a.height},page);
    const beside=item.placement==="beside_selection"&&a.x+a.width+6+item.widthPt<=page.widthPt;
    const wanted=proposal.version>1?item.object:typeof item.placement==="object"?item.placement:{x:beside?a.x+a.width+6:below.x,y:(beside?a.y:below.y)+index*(item.heightPt+6)};
    const position=keepOnPage(wanted,{widthPt:item.widthPt,heightPt:item.heightPt},page,Math.max(item.widthPt,item.heightPt));
    if(position.x!==item.object.x||position.y!==item.object.y)throw new Error("invalid_placement");
  }
}

/** Keep only live session proposals; final proposals do not need a discard notice. */
export function retainSessionProposals(proposals: Proposal[], sessions: { id: string }[]) {
  const ids = new Set(sessions.map(session => session.id));
  return {
    proposals: proposals.filter(proposal => ids.has(proposal.sessionId)),
    discarded: proposals.some(proposal => !ids.has(proposal.sessionId) &&
      ["pending", "stale", "change_requested"].includes(proposal.status)),
  };
}

export function reviewShortcut(
  event: Pick<KeyboardEvent, "key" | "ctrlKey" | "preventDefault" | "stopPropagation">,
  insert: () => Promise<void>, close: () => void,
) {
  if (event.key !== "Escape" && !(event.key === "Enter" && event.ctrlKey)) return;
  event.preventDefault();
  event.stopPropagation();
  if (event.key === "Escape") close();
  else void insert();
}

/** A minimal line diff for the review disclosure: unchanged, removed and added lines in order. */
export type DiffLine = { kind: "same" | "del" | "add"; text: string };
export function lineDiff(before: string, after: string): DiffLine[] {
  const a = before.split("\n"), b = after.split("\n");
  // Bound the quadratic table; very long sources fall back to removed-then-added blocks.
  if (a.length * b.length > 250_000) return [...a.map(text => ({ kind: "del" as const, text })), ...b.map(text => ({ kind: "add" as const, text }))];
  const lcs = Array.from({ length: a.length + 1 }, () => new Uint32Array(b.length + 1));
  for (let i = a.length - 1; i >= 0; i--) for (let j = b.length - 1; j >= 0; j--)
    lcs[i][j] = a[i] === b[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
  const out: DiffLine[] = [];
  let i = 0, j = 0;
  while (i < a.length || j < b.length) {
    if (i < a.length && j < b.length && a[i] === b[j]) { out.push({ kind: "same", text: a[i] }); i++; j++; }
    else if (i < a.length && (j === b.length || lcs[i + 1][j] >= lcs[i][j + 1])) out.push({ kind: "del", text: a[i++] });
    else out.push({ kind: "add", text: b[j++] });
  }
  return out;
}

/** Rust returns compile failures as a JSON diagnostics array; other failures stay plain text. */
export type ReviewDiagnostic = { message: string; line?: number; file?: string };
export function compileDiagnostics(error: string): ReviewDiagnostic[] | null {
  try {
    const value: unknown = JSON.parse(error);
    if (!Array.isArray(value) || !value.length) return null;
    return value.filter((d): d is ReviewDiagnostic => typeof d?.message === "string").slice(0, 5);
  } catch { return null; }
}

/** Review UI state that survives the card's re-mount when a new proposal version arrives. */
const reviewUi = new Map<string, { source: boolean; whole: boolean }>();
export function reviewState(id: string) {
  let state = reviewUi.get(id);
  if (!state) reviewUi.set(id, state = { source: false, whole: false });
  return state;
}

/**
 * Apply binds to the compiled, reviewed version. A Page text proposal also stays blocked while its
 * edited source is uncompiled or compiling, while it overflows the page and while the reviewer is
 * on another page.
 */
export function applyBlocked(proposal: Pick<Proposal, "kind" | "status" | "overflow">,
  review: { busy: boolean; dirty: boolean; stale: boolean; away: boolean }) {
  if (review.busy || proposal.kind === "page_create" && (review.dirty || Boolean(proposal.overflow) || review.away) || proposal.status !== "pending" || review.stale) return true;
  return proposal.kind === "preset_install" ? review.dirty || review.away : proposal.kind === "page_text_edit" && (review.dirty || Boolean(proposal.overflow) || review.away);
}

/** String transport stays internal; owned failures remain translated and actionable. */
export function presetError(error: string) {
  if (/collision|already exists/i.test(error)) return "proposal.presetCollision";
  if (/cached|dependency/i.test(error)) return "proposal.presetDependency";
  if (/preview.*required/i.test(error)) return "proposal.presetPreviewRequired";
  if (/limit/i.test(error)) return "proposal.presetLimit";
  if (/busy/i.test(error)) return "helper.detail_busy";
  if (/changed|final|ended|revoked|authorization|scope/i.test(error)) return "proposal.presetChanged";
  return "proposal.presetFailed";
}
