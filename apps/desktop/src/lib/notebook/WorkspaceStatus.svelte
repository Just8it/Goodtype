<script lang="ts">
  import { t, renderMessage, type UiText } from "../i18n";
  let { toolLabel, toolDetail, status, transactionFailed, pageNumber, pageCount, zoom, saveNeedsAttention, savePending, overviewOpen, overviewId, onZoom, onToggleOverview, onSave }: {
    toolLabel: string; toolDetail: string; status: UiText; transactionFailed: boolean;
    pageNumber: number; pageCount: number; zoom: number; saveNeedsAttention: boolean; savePending: boolean;
    overviewOpen: boolean; overviewId: string;
    onZoom: (zoom: number) => void; onToggleOverview: () => void; onSave: () => void;
  } = $props();
</script>

  <footer class="status-strip">
    <div class="tool-status"><strong>{toolLabel}</strong><span>{toolDetail}</span></div>
    <div class:failure={transactionFailed} class="operation-status" title={renderMessage(status, $t)}>{renderMessage(status, $t)}</div>
    <!-- The page count is where you look to learn where you are, so it also opens every page. -->
    <button
      type="button"
      id="page-overview-toggle"
      class="page-count"
      class:open={overviewOpen}
      title={$t("pageOverview.show")}
      aria-expanded={overviewOpen}
      aria-controls={overviewId}
      onclick={onToggleOverview}
    >{$t("workspace.pageOf", { "value2": pageNumber, "value4": pageCount })}</button>
    <span class="footer-divider"></span>
    <!-- Zoom reads and is changed in one place. It used to be a floating pill over the canvas
         *and* a number down here, so neither looked like the answer. -->
    <span class="zoom-group">
      <button type="button" aria-label={$t("workspace.zoomOut")} onclick={() => onZoom(zoom / 1.4)}>−</button>
      <button type="button" class="zoom-value" aria-label={$t("workspace.resetZoomTo")} title={$t("workspace.resetZoom")} onclick={() => onZoom(1)}>{Math.round(zoom * 100)}%</button>
      <button type="button" aria-label={$t("workspace.zoomIn")} onclick={() => onZoom(zoom * 1.4)}>+</button>
    </span>
    <span class="footer-divider"></span><button type="button" class:failure={saveNeedsAttention} class="local-state" title={$t("workspace.saveNow")} onclick={onSave}>{saveNeedsAttention ? $t("workspace.needsAttention") : savePending ? $t("workspace.localSaving") : $t("workspace.localSaved")}</button>
  </footer>


<style>
  button { border: 0; }
  button:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: 2px; }
  .tool-status { display: flex; align-items: center; }
  .status-strip {
    z-index: 30;
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 12px;
    padding: 0 16px;
    border-top: 1px solid var(--edge-soft);
    background: var(--charcoal);
    color: var(--muted);
    font-size: var(--text-sm);
  }

  .tool-status { flex: none; gap: 6px; }
  .tool-status strong { color: var(--text); font-weight: 600; }
  .operation-status { overflow: hidden; flex: 1; color: var(--quiet); text-align: center; text-overflow: ellipsis; white-space: nowrap; }
  .operation-status.failure, .status-strip .local-state.failure { color: var(--oxide); }
  .footer-divider { width: 1px; height: 15px; flex: none; background: var(--edge); }
  .status-strip button { padding: 3px 5px; border-radius: var(--radius); background: transparent; color: var(--muted); cursor: pointer; }
  .status-strip button:hover { background: var(--wash); }
  .local-state { flex: none; }

  .page-count {
    color: var(--quiet);
    font-size: var(--text-xs);
    font-variant-numeric: tabular-nums;
  }
  .status-strip button.page-count { color: var(--quiet); }
  .status-strip button.page-count:hover { color: var(--text); }
  .status-strip button.page-count.open { background: var(--accent-fill); color: var(--text); }

  .zoom-group { display: flex; align-items: center; gap: 1px; }
  .zoom-group .zoom-value { min-width: 42px; font-variant-numeric: tabular-nums; text-align: center; }


  /* The page count stays: it is the way into the page overview. */
  @media (max-width: 800px) { .operation-status { display: none; } }
</style>
