import type { CellGrantIntent, ApplyCellResult, Proposal } from "../ipc/helper";
export type CellEntry = { root: string; pageId: string; grant: CellGrantIntent; ordered: string[]; focus: import("../helper/editorEntry").EditorFocus | null; version: number };
export function cellGrant(calculationId: string, partId: string, ordered: string[], active: string): CellGrantIntent {
  if (!ordered.includes(active)) throw new Error("cell_grant_denied");
  return { calculationId, partId, cellIds: [active], includeOutputs: true };
}
export function cellsAbove(grant: CellGrantIntent, ordered: string[]): CellGrantIntent {
  if (grant.cellIds.some(id => !ordered.includes(id)) || new Set(ordered).size !== ordered.length) throw new Error("cell_grant_denied");
  const end = Math.max(...grant.cellIds.map(id => ordered.indexOf(id)));
  const cellIds = ordered.slice(0, end + 1);
  if (!cellIds.length || cellIds.length > 100) throw new Error("cell_grant_denied");
  return { ...grant, cellIds };
}
let apply: ((proposal: Proposal, result: ApplyCellResult, run: boolean) => Promise<void>) | undefined;
let prepare: (proposal: Proposal) => Promise<void> = async () => {};
let label: (id: string) => string = id => id;
export const cellLabel = (id: string) => label(id);
export function registerCellApply(handler: NonNullable<typeof apply>, labels: (id: string) => string = id => id, preflight: typeof prepare = async () => {}) { apply = handler; label = labels; prepare = preflight; return () => { if (apply === handler) apply = undefined; }; }
export async function applyAndRun(proposal: Proposal, run: boolean, submit: () => Promise<ApplyCellResult>) {
  if (!apply) throw new Error("cell_proposal_destination");
  const handler = apply;
  await prepare(proposal);
  const result = await submit();
  await handler(proposal, result, run);
}

export function validateCellContext(cells: CellGrantIntent) {
 if (!cells.cellIds.length || cells.cellIds.length > 100 || new Set(cells.cellIds).size !== cells.cellIds.length) throw new Error("cell_grant_denied");
 return cells;
}
