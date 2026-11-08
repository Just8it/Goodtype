import type { ContextReply } from "../ipc/helper";
export type ContextSession = { readonly root: string; readonly activePageId: string; readonly editGeneration: number; readonly notebookGeneration?: number; persist: () => Promise<boolean> };
/** Poll only while requested; pointer samples and live feedback never wait on this work. */
export async function contextReply(requestId: string, current: () => ContextSession | null, gestureActive: () => boolean,
  pause = (ms: number) => new Promise<void>(resolve => setTimeout(resolve, ms))): Promise<ContextReply> {
  const fail = (status: ContextReply["status"]): ContextReply => ({ requestId, status });
  if (!current()?.root) return fail("no_notebook");
  for (let elapsed = 0; gestureActive(); elapsed += 25) {
    if (elapsed >= 2000) return fail("busy");
    await pause(25);
  }
  const session = current();
  if (!session?.root || !session.activePageId) return fail("no_notebook");
  const { root, activePageId: pageId, editGeneration, notebookGeneration } = session;
  try { if (!await session.persist()) return fail("flush_failed"); }
  catch { return fail("flush_failed"); }
  if (current() !== session || session.root !== root || session.activePageId !== pageId || session.editGeneration !== editGeneration || session.notebookGeneration !== notebookGeneration || gestureActive()) return fail("busy");
  return { requestId, status: "ok", root, pageId, editGeneration };
}
