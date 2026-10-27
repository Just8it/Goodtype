<script lang="ts">
  import { t, formatNumber, displayLocale } from "../i18n";
  import { centredDash } from "./dash";
  import { untrack } from "svelte";
  import { MM_PER_PT } from "../settings";
  import { dismissable } from "../workspace/dismiss";
  import type { StrokePattern } from "../model";
  import { MAX_PATTERN_SPACING, MIN_PATTERN_SPACING } from "../ink/pattern";
  import { patternDashArray } from "../shape/geometry";
  import StepSlider from "./StepSlider.svelte";

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
    adding = false,
    taken = () => false,
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
    /**
     * A new chip rather than an existing one. Nothing reaches the row or the pen until Add is
     * pressed — pattern and spacing are held here as a draft — so closing the panel leaves no
     * trace, and there is no half-made chip to clean up.
     */
    adding?: boolean;
    /** Whether another chip already draws this width; the row would merge the two. */
    taken?: (widthPt: number) => boolean;
    /** Takes the new chip's width in points with its pattern and spacing. */
    onAdd?: (widthPt: number, pattern: StrokePattern, spacing: number) => void;
    /** Takes a width in points, the unit everything downstream is in. */
    onCommit: (widthPt: number) => void;
    onRemove?: () => void;
    onClose: () => void;
  } = $props();

  // Seeded once and then owned by the field. A fresh panel is a fresh component — the caller
  // mounts it behind `{#if}` — so following `widthPt` afterwards would fight the hand on the
  // slider with the value the slider is in the middle of changing.
  // A new chip starts at the nearest width not already on the row — one step thicker, else
  // thinner — so Add is ready at once instead of opening on a refusal.
  let mm = $state(untrack(() => (adding ? freeWidth(round(widthPt * MM_PER_PT)) : round(widthPt * MM_PER_PT))));

  function freeWidth(start: number): number {
    for (let steps = 1; steps <= 400; steps += 1) {
      for (const candidate of [start + steps * 0.05, start - steps * 0.05]) {
        const value = round(candidate);
        if (value >= minimumMm && value <= maximumMm && !taken(value / MM_PER_PT)) return value;
      }
    }
    return start;
  }

  function round(value: number): number {
    return Math.round(value * 100) / 100;
  }

  function clamp(value: number): number {
    return Math.min(Math.max(value, minimumMm), maximumMm);
  }

  const PATTERNS: { id: StrokePattern; label: string; dasharray: number[] | null }[] = [
    { id: "solid", label: $t("workspace.solid"), dasharray: null },
    { id: "dashed", label: $t("workspace.dashed"), dasharray: [7, 5] },
    { id: "dotted", label: $t("workspace.dotted"), dasharray: [0.01, 5] },
  ];

  // The draft a new chip is built in; an existing chip edits the real pattern live instead.
  let draftPattern = $state(untrack(() => pattern));
  let draftSpacing = $state(untrack(() => spacing));
  const shownPattern = $derived(adding ? draftPattern : pattern);
  const shownSpacing = $derived(adding ? draftSpacing : spacing);

  const preview = $derived(clamp(mm) / MM_PER_PT);
  const duplicate = $derived(taken(round(clamp(mm)) / MM_PER_PT));
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
    if (duplicate) return;
    if (adding) onAdd?.(mm / MM_PER_PT, draftPattern, draftSpacing);
    else onCommit(mm / MM_PER_PT);
  }
</script>

<!-- Embedded, the card owns dismissal: the outside-press listener would fire on the card's own
     header and close the sub-view out from under the writer. -->
<aside use:dismissable={embedded ? () => {} : onClose} class:embedded class="width-panel" aria-label={$t("workspace.strokeWidth")}>
  <!-- Drawn at the width it will be, so the number is checked against a mark rather than trusted. -->
  <div class="sample">
    <svg viewBox="0 0 220 42" aria-hidden="true"><path d="M20 28C48 3 66 4 91 24S133 39 153 20S181 5 200 17" fill="none" stroke="currentColor" stroke-linecap="round"
      stroke-width={sampleWidth}
      use:centredDash={patternDashArray(shownPattern, sampleWidth, shownSpacing)} /></svg>
  </div>

  {#if onPatternChange}
    <div class="segments" role="group" aria-label={$t("workspace.linePattern")}>
      {#each PATTERNS as choice (choice.id)}
        <button type="button" class:current={shownPattern === choice.id} aria-pressed={shownPattern === choice.id}
          aria-label={choice.label} title={choice.label}
          onclick={() => adding ? (draftPattern = choice.id) : onPatternChange?.(choice.id)}>
          <svg viewBox="0 0 40 12" aria-hidden="true"><path d="M5 6H35" use:centredDash={choice.dasharray} /></svg>
        </button>
      {/each}
    </div>
  {/if}

  <div class="settings">
    <!-- Logarithmic: everyday nibs (0.1–1 mm) get half the track instead of a few pixels. -->
    <StepSlider name="Width" value={mm} min={minimumMm} max={maximumMm} step={0.05} log
      valueText={$t("workspace.mm2", { "value1": formatNumber(mm, { minimumFractionDigits: 2, maximumFractionDigits: 2 }, $displayLocale) })} label={$t("workspace.strokeWidth")} spokenValue={$t("workspace.millimetres", { "value1": formatNumber(mm, { minimumFractionDigits: 2, maximumFractionDigits: 2 }, $displayLocale) })} onInput={(next) => (mm = next)} />
    {#if onSpacingChange && shownPattern !== "solid"}
      <StepSlider name="Spacing" label={$t("workspace.patternSpacing")} value={shownSpacing} min={MIN_PATTERN_SPACING} max={MAX_PATTERN_SPACING} step={0.05}
        valueText={`${Math.round(shownSpacing * 100)}%`}
        onInput={(next) => adding ? (draftSpacing = next) : onSpacingChange?.(next)} />
    {/if}
  </div>

  {#if duplicate}
    <p class="note" role="status">{$t("workspace.mmIsAlreadyOnTheRowPick", { "value1": formatNumber(mm, { minimumFractionDigits: 2, maximumFractionDigits: 2 }, $displayLocale) })}</p>
  {/if}

  <div class="actions">
    {#if canRemove}
      <button type="button" class="remove" onclick={() => onRemove?.()}>{$t("workspace.remove")}</button>
    {/if}
    <span class="grow"></span>
    <button type="button" class="primary" disabled={duplicate} onclick={commit}>{adding ? $t("workspace.add") : $t("workspace.apply")}</button>
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

  .settings { display: grid; gap: 12px; }

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
    white-space: nowrap;
    cursor: pointer;
  }

  .primary {
    background: var(--blueprint);
    color: #0e1b31;
    font-weight: 600;
  }

  .primary:disabled { opacity: 0.45; cursor: default; }

  .note { margin: 0; color: var(--muted); font-size: var(--text-sm); }

  /* Destructive, so it reads as a quiet word rather than a button competing with Apply. */
  .remove {
    padding: 0 6px;
    background: transparent;
    color: var(--oxide);
  }
  .remove:hover { background: var(--wash); }

  button:focus-visible {
    outline: 2px solid var(--blueprint);
    outline-offset: 2px;
  }
</style>
