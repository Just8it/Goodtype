<script lang="ts">
  import { t } from "../i18n";
  import { MAX_TYPST_SCALE, MIN_TYPST_SCALE, MIN_TYPST_WIDTH_PT, POINTS_PER_MM, typstScale, typstWidthFromMm } from "./typstSizing";

  let { scale, layoutWidthPt, onChange }: {
    scale: number;
    layoutWidthPt: number;
    onChange: (change: { scale: number } | { layoutWidthPt: number }) => void;
  } = $props();
  let size = $state<number | undefined>(100);
  let width = $state<number | undefined>(0);
  const widthMm = $derived(layoutWidthPt * scale / POINTS_PER_MM);
  const minWidthMm = $derived(MIN_TYPST_WIDTH_PT * scale / POINTS_PER_MM);
  $effect(() => { size = Math.round(scale * 100); });
  $effect(() => { width = Math.round(widthMm * 10) / 10; });

  function setSize(value: number | undefined) {
    if (value !== undefined && Number.isFinite(value) && value > 0) {
      const next = typstScale(value / 100);
      if (next !== scale && value !== Math.round(scale * 100)) onChange({ scale: next });
      size = Math.round(next * 100);
    } else size = Math.round(scale * 100);
  }
  function setWidth(value: number | undefined) {
    if (value !== undefined && Number.isFinite(value) && value > 0) {
      // Avoid changing exact stored geometry just by focusing and leaving the rounded readout.
      const next = typstWidthFromMm(value, scale);
      if (Number.isFinite(next) && value !== Math.round(widthMm * 10) / 10) onChange({ layoutWidthPt: next });
    }
    width = Math.round(widthMm * 10) / 10;
  }
  function fieldKey(event: KeyboardEvent, reset: () => void) {
    if (event.key === "Escape") {
      event.preventDefault(); event.stopPropagation(); reset();
      (event.currentTarget as HTMLInputElement).blur();
    } else if (event.key === "Enter") {
      event.preventDefault(); event.stopPropagation();
      (event.currentTarget as HTMLInputElement).blur();
    }
  }
</script>

<div class="typst-sizing" role="group" aria-label={$t("workspace.blockDimensions")} title={$t("workspace.blockSizingHint")}>
  <div class="dimension"><label for="typst-size">{$t("workspace.blockSize")}</label>
    <div class="value"><input id="typst-size" type="number" min="10" max="800" step="1" bind:value={size} onchange={() => setSize(size)} onkeydown={e => fieldKey(e, () => { size = Math.round(scale * 100); })} /><span>{$t("workspace.percentageUnit")}</span>
      <div class="arrows">
        <button type="button" aria-label={$t("workspace.increaseBlockSize")} disabled={scale >= MAX_TYPST_SCALE} onclick={() => setSize(Math.round(scale * 100) + 1)}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m7 14 5-5 5 5" /></svg></button>
        <button type="button" aria-label={$t("workspace.decreaseBlockSize")} disabled={scale <= MIN_TYPST_SCALE} onclick={() => setSize(Math.round(scale * 100) - 1)}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m7 10 5 5 5-5" /></svg></button>
      </div>
    </div>
  </div>
  <div class="dimension"><label for="typst-line-width">{$t("workspace.blockLineWidth")}</label>
    <div class="value"><input id="typst-line-width" type="number" min={minWidthMm} step="0.1" bind:value={width} onchange={() => setWidth(width)} onkeydown={e => fieldKey(e, () => { width = Math.round(widthMm * 10) / 10; })} /><span>{$t("workspace.millimetreUnit")}</span>
      <div class="arrows">
        <button type="button" aria-label={$t("workspace.increaseBlockWidth")} onclick={() => setWidth(Math.round(widthMm * 10 + 1) / 10)}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m7 14 5-5 5 5" /></svg></button>
        <button type="button" aria-label={$t("workspace.decreaseBlockWidth")} disabled={layoutWidthPt <= MIN_TYPST_WIDTH_PT} onclick={() => setWidth(Math.round(widthMm * 10 - 1) / 10)}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m7 10 5 5 5-5" /></svg></button>
      </div>
    </div>
  </div>
</div>

<style>
  .typst-sizing { display: flex; flex-wrap: wrap; gap: 6px 12px; padding: 4px; }
  .dimension { display: flex; flex: 1; align-items: center; justify-content: space-between; gap: 6px; color: var(--muted); font-size: var(--text-sm); white-space: nowrap; }
  .value { display: flex; align-items: center; gap: 3px; border-radius: var(--radius); background: var(--wash); }
  input { box-sizing: border-box; width: 46px; height: var(--control-dense); padding: 0 3px; border: 0; border-radius: var(--radius); background: transparent; color: var(--text); font: inherit; font-size: var(--text-md); font-variant-numeric: tabular-nums; text-align: right; appearance: textfield; }
  input::-webkit-inner-spin-button, input::-webkit-outer-spin-button { margin: 0; appearance: none; }
  input:focus { outline: 2px solid var(--blueprint); outline-offset: -2px; background: var(--panel); }
  .arrows { display: flex; flex-direction: column; }
  button { display: grid; width: var(--control-dense); height: calc(var(--control-dense) / 2); padding: 0; border: 0; border-radius: var(--radius); background: transparent; color: var(--muted); cursor: pointer; place-items: center; }
  button:hover:not(:disabled) { background: var(--wash); color: var(--text); }
  button:disabled { opacity: 0.35; cursor: default; }
  button:focus-visible { outline: 2px solid var(--blueprint); outline-offset: -1px; }
  svg { width: var(--icon-dense); height: var(--icon-dense); fill: none; stroke: currentColor; stroke-width: var(--stroke-dense); stroke-linecap: round; stroke-linejoin: round; }
  @media (pointer: coarse) { input { min-height: var(--control-touch); } button { width: var(--control-touch); height: calc(var(--control-touch) / 2); } }
</style>
