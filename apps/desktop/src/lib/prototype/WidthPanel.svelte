<script lang="ts">
  import { centredDash } from "./dash";
  import { untrack } from "svelte";
  import { MM_PER_PT } from "../settings";
  import { dismissable } from "../workspace/dismiss";
  import type { StrokePattern } from "../model";
  import { MAX_PATTERN_SPACING, MIN_PATTERN_SPACING } from "../ink/pattern";
  import { patternDashArray } from "../shape/geometry";

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
    pattern = "solid",
    onPatternChange,
    spacing = 1,
    onSpacingChange,
    onAdd,
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
    pattern?: StrokePattern;
    onPatternChange?: (pattern: StrokePattern) => void;
    /** Gap factor between dashes or dots, 0.5–3. */
    spacing?: number;
    onSpacingChange?: (spacing: number) => void;
    onAdd?: (widthPt: number) => void;
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

  const PATTERNS: { id: StrokePattern; label: string; dasharray: number[] | null }[] = [
    { id: "solid", label: "Solid", dasharray: null },
    { id: "dashed", label: "Dashed", dasharray: [7, 5] },
    { id: "dotted", label: "Dotted", dasharray: [0.01, 5] },
  ];

  const preview = $derived(clamp(mm) / MM_PER_PT);
  /** The sample is magnified to be readable; its pattern is spaced for that drawn width. */
  const sampleWidth = $derived(
    Math.max(1, Math.min(preview * (kind === "highlighter" ? 2.2 : 1.4), 26)),
  );

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
  <!-- Drawn at the width it will be, so the number is checked against a mark rather than trusted. -->
  <div class="sample">
    <svg viewBox="0 0 220 42" aria-hidden="true"><path d="M20 28C48 3 66 4 91 24S133 39 153 20S181 5 200 17" fill="none" stroke="currentColor" stroke-linecap="round"
      stroke-width={sampleWidth}
      use:centredDash={patternDashArray(pattern, sampleWidth, spacing)} /></svg>
  </div>

  {#if onPatternChange}
    <div class="segments" role="group" aria-label="Line pattern">
      {#each PATTERNS as choice (choice.id)}
        <button type="button" class:current={pattern === choice.id} aria-pressed={pattern === choice.id}
          aria-label={choice.label} title={choice.label}
          onclick={() => onPatternChange?.(choice.id)}>
          <svg viewBox="0 0 40 12" aria-hidden="true"><path d="M5 6H35" use:centredDash={choice.dasharray} /></svg>
        </button>
      {/each}
    </div>
  {/if}

  <div class="settings">
    <label class="setting">
      <span class="name">Width</span>
      <input bind:value={mm} type="range" min={minimumMm} max={maximumMm} step="0.05" aria-label="Stroke width" />
      <span class="value">
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
      </span>
    </label>
    {#if onSpacingChange && pattern !== "solid"}
      <label class="setting">
        <span class="name">Spacing</span>
        <input type="range" min={MIN_PATTERN_SPACING} max={MAX_PATTERN_SPACING} step="0.05"
          value={spacing} aria-label="Pattern spacing"
          oninput={(event) => onSpacingChange?.(Number(event.currentTarget.value))} />
        <output class="value">{Math.round(spacing * 100)}%</output>
      </label>
    {/if}
  </div>

  <div class="actions">
    {#if canRemove}
      <button type="button" class="remove" onclick={() => onRemove?.()}>Remove</button>
    {/if}
    <span class="grow"></span>
    {#if onAdd}
      <button type="button" class="secondary" title="Keep this width and add the new one to the row" onclick={() => {
        if (Number.isFinite(mm)) onAdd?.(round(clamp(mm)) / MM_PER_PT);
      }}>Add as new</button>
    {/if}
    <button type="button" class="primary" onclick={commit}>Apply</button>
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

  .sample {
    display: grid;
    height: 52px;
    border-radius: var(--radius);
    background: rgb(0 0 0 / 25%);
    place-items: center;
  }
  .sample svg { width: 100%; height: 42px; color: var(--text); }

  /* One strip, not three tiles: the choice is small, so the control is too. */
  .segments { display: grid; grid-template-columns: repeat(3, 1fr); padding: 3px; border: 1px solid var(--edge); border-radius: var(--radius); background: rgb(0 0 0 / 18%); gap: 3px; }
  .segments button { display: grid; height: 30px; place-items: center; border: 0; border-radius: calc(var(--radius) - 2px); background: transparent; color: var(--muted); cursor: pointer; }
  .segments button:hover { color: var(--text); background: var(--wash); }
  .segments button.current { background: rgb(76 141 240 / 22%); color: var(--text); box-shadow: inset 0 0 0 1px var(--blueprint); }
  .segments button:focus-visible { outline: 2px solid var(--blueprint); outline-offset: 1px; }
  .segments svg { width: 34px; height: 12px; fill: none; stroke: currentColor; stroke-width: 2.4; stroke-linecap: round; }

  .settings { display: grid; gap: 8px; }
  .setting { display: grid; grid-template-columns: 4.6em 1fr 6.2em; align-items: center; gap: 8px; }
  .setting .name { color: var(--muted); font-size: var(--text-sm); }
  .setting input[type="range"] { width: 100%; margin: 0; accent-color: var(--blueprint); }
  .setting .value { display: flex; align-items: center; justify-content: flex-end; gap: 4px; color: var(--text); font-size: var(--text-sm); font-variant-numeric: tabular-nums; }
  /* No spinner: its arrows covered the digits, and the slider and arrow keys already step it. */
  input[type="number"] { appearance: textfield; -moz-appearance: textfield; }
  input[type="number"]::-webkit-inner-spin-button,
  input[type="number"]::-webkit-outer-spin-button { margin: 0; -webkit-appearance: none; }
  input[type="number"] { box-sizing: border-box; width: 4.2em; padding: 4px 6px; border: 1px solid rgb(255 255 255 / 16%); border-radius: var(--radius); background: rgb(0 0 0 / 25%); color: var(--text); font: inherit; font-size: var(--text-sm); text-align: right; font-variant-numeric: tabular-nums; }
  input[type="number"]:focus-visible { outline: 2px solid var(--blueprint); outline-offset: 1px; }
  .unit { color: var(--quiet); }

  .actions {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .grow {
    flex: 1;
  }

  .primary,
  .secondary,
  .remove {
    height: 30px;
    padding: 0 11px;
    border: 0;
    border-radius: var(--radius);
    font: inherit;
    font-size: var(--text-md);
    white-space: nowrap;
    cursor: pointer;
  }

  .primary {
    background: var(--blueprint);
    color: #0e1b31;
    font-weight: 600;
  }

  .secondary {
    border: 1px solid var(--edge);
    background: transparent;
    color: var(--text);
  }
  .secondary:hover { background: var(--wash); }

  /* Destructive, so it reads as a quiet word rather than a button competing with Apply. */
  .remove {
    padding: 0 6px;
    background: transparent;
    color: var(--oxide);
  }
  .remove:hover { background: var(--wash); }

  button:focus-visible,
  input:focus-visible {
    outline: 2px solid var(--blueprint);
    outline-offset: 2px;
  }
</style>
