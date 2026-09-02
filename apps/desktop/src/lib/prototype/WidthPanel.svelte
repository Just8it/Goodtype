<script lang="ts">
  import { untrack } from "svelte";
  import { MM_PER_PT } from "../settings";
  import { dismissable } from "../workspace/dismiss";

  /**
   * Set a stroke width by hand, in millimetres.
   *
   * The bar carries three preset sizes, which covers most writing and none of the cases that
   * actually need a number: matching a printed rule, a hairline for construction lines, a broad
   * nib for headings. Presets are the fast path, not the only one.
   *
   * Millimetres because that is what a nib is sold in — a 0.35 is a 0.35 whatever the document
   * is set in — while the geometry underneath is points. The conversion lives in `settings.ts`,
   * so the status strip names the same nib this panel sets.
   */

  let {
    widthPt,
    kind = "pen",
    minimumMm = 0.05,
    maximumMm = 20,
    canRemove = false,
    embedded = false,
    onCommit,
    onRemove,
    onClose,
  }: {
    widthPt: number;
    kind?: "pen" | "highlighter";
    minimumMm?: number;
    maximumMm?: number;
    canRemove?: boolean;
    /** Rendered as a card sub-view rather than a popout: no chrome, and no self-dismissal. */
    embedded?: boolean;
    /** Takes a width in points, the unit everything downstream is in. */
    onCommit: (widthPt: number) => void;
    onRemove?: () => void;
    onClose: () => void;
  } = $props();

  // Seeded once and then owned by the field. A fresh panel is a fresh component — the caller
  // mounts it behind `{#if}` — so following `widthPt` afterwards would fight the hand on the
  // slider with the value the slider is in the middle of changing.
  let mm = $state(untrack(() => round(widthPt * MM_PER_PT)));

  function round(value: number): number {
    return Math.round(value * 100) / 100;
  }

  function clamp(value: number): number {
    return Math.min(Math.max(value, minimumMm), maximumMm);
  }

  const preview = $derived(clamp(mm) / MM_PER_PT);

  function commit() {
    if (!Number.isFinite(mm)) {
      mm = round(widthPt * MM_PER_PT);
      return;
    }
    mm = round(clamp(mm));
    onCommit(mm / MM_PER_PT);
  }
</script>

<!-- Embedded, the card owns dismissal: the outside-press listener would fire on the card's own
     header and close the sub-view out from under the writer. -->
<aside use:dismissable={embedded ? () => {} : onClose} class:embedded class="width-panel" aria-label="Stroke width">
  <div class="row">
    <input
      bind:value={mm}
      type="number"
      inputmode="decimal"
      step="0.05"
      min={minimumMm}
      max={maximumMm}
      aria-label="Stroke width in millimetres"
      onkeydown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          commit();
        }
      }}
    />
    <span class="unit">mm</span>
  </div>

  <input
    bind:value={mm}
    type="range"
    class="rail"
    min={minimumMm}
    max={maximumMm}
    step="0.05"
    aria-label="Stroke width"
  />

  <!-- Drawn at the width it will be, so the number is checked against a mark rather than trusted. -->
  <div class="sample">
    <span
      style:height={`${Math.max(1, Math.min(preview * (kind === "highlighter" ? 2.2 : 1.4), 26))}px`}
    ></span>
  </div>

  <div class="actions">
    {#if canRemove}
      <button type="button" class="remove" onclick={() => onRemove?.()}>Remove</button>
    {/if}
    <span class="grow"></span>
    <button type="button" class="primary" onclick={commit}>
      Use {round(clamp(mm)).toFixed(2)} mm
    </button>
  </div>
</aside>

<style>
  /* Positioned by `.palette-panel-anchor` in the palette, which already knows how to place a
     popout for all four docks. Positioning here as well offset it twice and left it behind when
     the bar moved to a side. */
  .width-panel {
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: 220px;
    padding: 12px;
    border: 1px solid var(--edge);
    border-radius: var(--radius-lg);
    background: var(--panel);
    box-shadow: 0 18px 44px rgb(0 0 0 / 55%);
  }

  .width-panel.embedded {
    width: auto;
    border: 0;
    border-radius: 0;
    background: transparent;
    box-shadow: none;
  }

  .row {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  input[type="number"] {
    box-sizing: border-box;
    width: 100%;
    padding: 8px 10px;
    border: 1px solid rgb(255 255 255 / 16%);
    border-radius: var(--radius);
    background: rgb(0 0 0 / 25%);
    color: var(--text);
    font: inherit;
    font-size: var(--text-lg);
  }

  .unit {
    color: var(--quiet);
    font-size: var(--text-md);
  }

  .rail {
    width: 100%;
    accent-color: var(--blueprint);
  }

  .sample {
    display: grid;
    height: 30px;
    border-radius: var(--radius);
    background: rgb(0 0 0 / 25%);
    place-items: center;
  }

  .sample span {
    display: block;
    width: 70%;
    border-radius: var(--radius-pill);
    background: var(--muted);
  }

  .actions {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .grow {
    flex: 1;
  }

  .primary,
  .remove {
    height: 30px;
    padding: 0 11px;
    border: 0;
    border-radius: var(--radius);
    font: inherit;
    font-size: var(--text-md);
    cursor: pointer;
  }

  .primary {
    background: var(--blueprint);
    color: #0e1b31;
    font-weight: 600;
  }

  .remove {
    background: var(--wash);
    color: var(--oxide);
  }

  button:focus-visible,
  input:focus-visible {
    outline: 2px solid var(--blueprint);
    outline-offset: 2px;
  }
</style>
