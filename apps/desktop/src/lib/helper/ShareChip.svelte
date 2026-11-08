<script lang="ts">
  import { helper } from "./state";
  import { helperUnshare } from "../ipc/helper";
  import { t } from "../i18n";
  let { pageId, width, height, onError }: { pageId: string; width: number; height: number; onError: () => void } = $props();
  let hover = $state(false), focus = $state(false);
  const share = $derived($helper?.share?.pageId === pageId ? $helper.share : null);
  async function stop() { try { await helperUnshare(); helper.update(s => s ? { ...s, share: null } : s); } catch { onError(); } }
</script>
{#if share}
  <div role="group" aria-label={$t("helper.share")} class="chip" onpointerenter={() => hover = true} onpointerleave={() => hover = false} onfocusin={() => focus = true} onfocusout={event => focus = event.currentTarget.contains(event.relatedTarget as Node)}>
    <span>{$t("helper.shared", { count: share.objectCount, page: share.pageNumber })}<small>{$t("helper.names", { names: $helper?.sessions.filter(s => share.sessionIds.includes(s.id)).map(s => s.clientName).join(", ") ?? "" })}</small></span>
    <button type="button" aria-label={$t("helper.unshare")} onclick={stop}>×</button>
  </div>
  {#if hover || focus}<svg class="crop" viewBox={`0 0 ${width} ${height}`} aria-hidden="true"><rect x={share.cropPt.x} y={share.cropPt.y} width={share.cropPt.width} height={share.cropPt.height} /></svg>{/if}
{/if}
<style>
.chip { position: absolute; z-index: 26; top: 8px; left: 8px; display: flex; align-items: center; max-width: calc(100% - 16px); padding: 4px 8px; border: 1px solid var(--edge); border-radius: var(--radius); background: var(--panel); color: var(--text); } small { display: block; } button { min-width: 44px; min-height: 44px; background: transparent; border: 0; color: var(--text); } button:focus-visible { outline: 2px solid var(--blueprint-light); } .crop { position: absolute; inset: 0; width: 100%; height: 100%; pointer-events: none; z-index: 25; overflow: visible; } rect { fill: none; stroke: var(--blueprint); stroke-width: 2; stroke-dasharray: 6 4; vector-effect: non-scaling-stroke; }
</style>
