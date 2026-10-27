<script lang="ts">
  import { t, formatNumber, displayLocale } from "../i18n";
  import { centredDash } from "./dash";
  import type { StrokePattern } from "../model";
  import { MAX_PATTERN_SPACING, MIN_PATTERN_SPACING } from "../ink/pattern";
  import { colorName, MM_PER_PT } from "../settings";
  import { patternDashArray } from "../shape/geometry";
  import type { ShapeKind } from "../shape/geometry";
  import type { PaletteDock } from "./palette";
  import StepSlider from "./StepSlider.svelte";

  let {
    dock,
    kind,
    fill,
    constrain,
    drawAndHold,
    minimumLineLengthPt = 6,
    onMinimumLineLengthChange,
    strokeColor,
    strokeWidthPt,
    pattern,
    spacing,
    onKindChange,
    onFillChange,
    onConstrainChange,
    onDrawAndHoldChange,
    onPatternChange,
    onSpacingChange,
    onClose,
  }: {
    dock: PaletteDock;
    kind: ShapeKind;
    fill: boolean;
    constrain: boolean;
    drawAndHold: boolean;
    minimumLineLengthPt?: number;
    onMinimumLineLengthChange?: (length: number) => void;
    strokeColor: string;
    strokeWidthPt: number;
    pattern: StrokePattern;
    spacing: number;
    onKindChange: (kind: ShapeKind) => void;
    onFillChange: (fill: boolean) => void;
    onConstrainChange: (constrain: boolean) => void;
    onDrawAndHoldChange: (enabled: boolean) => void;
    onPatternChange: (pattern: StrokePattern) => void;
    onSpacingChange: (spacing: number) => void;
    onClose: () => void;
  } = $props();

  const PATTERNS: { id: StrokePattern; label: string; dasharray: number[] | null }[] = $derived([
    { id: "solid", label: $t("workspace.solid"), dasharray: null },
    { id: "dashed", label: $t("workspace.dashed"), dasharray: [10, 7] },
    { id: "dotted", label: $t("workspace.dotted"), dasharray: [0.01, 7] },
  ]);
  const previewWidth = $derived(Math.max(1.5, strokeWidthPt * 1.6));
  const previewDasharray = $derived(patternDashArray(pattern, previewWidth, spacing));

  const SHAPES: { kind: ShapeKind; label: string; hint: string }[] = $derived([
    { kind: "line", label: $t("workspace.line"), hint: $t("workspace.dragFromStartToEnd") },
    { kind: "rectangle", label: $t("workspace.rectangle"), hint: $t("workspace.dragOppositeCorners") },
    { kind: "ellipse", label: $t("workspace.ellipse"), hint: $t("workspace.lockProportionsForACircle") },
    { kind: "spline", label: $t("workspace.spline"), hint: $t("workspace.drawASmoothEditableCurve") },
  ]);

  const isClosed = $derived(kind === "rectangle" || kind === "ellipse");
  /// What the constraint does depends on what is being drawn, and it governs a promoted line
  /// whatever the tool is set to, so it is never inert.
  const constraintHint = $derived(
    kind === "line"
      ? $t("workspace.snapToAngles")
      : kind === "spline"
        ? $t("workspace.snapAPromotedLineToAngles")
        : $t("workspace.squareOrCircleAndLines"),
  );
  const widthLabel = $derived($t("workspace.mm2", { "value1": formatNumber(strokeWidthPt * MM_PER_PT, { minimumFractionDigits: 2, maximumFractionDigits: 2 }, $displayLocale) }));

  function keydown(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    event.stopPropagation();
    onClose();
  }
</script>

{#snippet icon(shape: ShapeKind)}
  {#if shape === "line"}
    <path d="M5 18 19 6"></path><circle cx="5" cy="18" r="1.4"></circle><circle cx="19" cy="6" r="1.4"></circle>
  {:else if shape === "rectangle"}
    <rect x="4.5" y="5.5" width="15" height="13" rx="1.5"></rect>
  {:else if shape === "ellipse"}
    <ellipse cx="12" cy="12" rx="8" ry="6.5"></ellipse>
  {:else}
    <path d="M4 17C7 5 13 5 20 14"></path><circle cx="4" cy="17" r="1.3"></circle><circle cx="20" cy="14" r="1.3"></circle>
  {/if}
{/snippet}

{#snippet toggle(name: string, note: string, checked: boolean, disabled: boolean, change: () => void)}
  <div class:disabled class="row">
    <span class="row-name">{name}<span>{note}</span></span>
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={name}
      {disabled}
      class:on={checked}
      class="toggle"
      onclick={change}
    ><span></span></button>
  </div>
{/snippet}

<div
  class="shape-card"
  class:point-down={dock === "bottom"}
  class:point-up={dock === "top"}
  class:point-left={dock === "left"}
  class:point-right={dock === "right"}
  role="dialog"
  tabindex="-1"
  aria-label={$t("workspace.shapeSettings")}
  onkeydown={keydown}
>
  <span class="arrow" aria-hidden="true"></span>
  <header>
    <span class="name">{$t("workspace.shapes")}</span>
    <span class="meta">{widthLabel}</span>
    <button type="button" class="chrome" aria-label={$t("workspace.closeShapeSettings")} onclick={onClose}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18"></path></svg>
    </button>
  </header>

  <div class="body">
    <div class="preview" style:--shape-colour={strokeColor}>
      <svg viewBox="0 0 250 54" aria-hidden="true">
        <path d="M22 38C66 5 91 46 130 25S196 7 228 29" fill="none" stroke={strokeColor} stroke-width={previewWidth} stroke-linecap="round" use:centredDash={previewDasharray}></path>
      </svg>
      <span>{$t("workspace.currentPenStroke", { "value1": colorName(strokeColor) })}</span>
    </div>

    <section>
      <span class="section-label">{$t("workspace.draw")}</span>
      <div class="shape-grid" role="radiogroup" aria-label={$t("workspace.shapeType")}>
        {#each SHAPES as shape (shape.kind)}
          <button
            type="button"
            role="radio"
            class:current={kind === shape.kind}
            aria-checked={kind === shape.kind}
            title={shape.hint}
            onclick={() => onKindChange(shape.kind)}
          >
            <svg viewBox="0 0 24 24" aria-hidden="true">{@render icon(shape.kind)}</svg>
            <span>{shape.label}</span>
          </button>
        {/each}
      </div>
    </section>

    <section>
      <span class="section-label">{$t("workspace.pattern")}</span>
      <div class="pattern-grid" role="radiogroup" aria-label={$t("workspace.linePattern")}>
        {#each PATTERNS as choice (choice.id)}
          <button
            type="button"
            role="radio"
            class:current={pattern === choice.id}
            aria-checked={pattern === choice.id}
            onclick={() => onPatternChange(choice.id)}
          >
            <svg viewBox="0 0 60 12" aria-hidden="true"><path d="M6 6H54" use:centredDash={choice.dasharray} /></svg>
            <span>{choice.label}</span>
          </button>
        {/each}
      </div>
      {#if pattern !== "solid"}
        <div class="spacing">
          <StepSlider name="Spacing" label={$t("workspace.patternSpacing")} value={spacing} min={MIN_PATTERN_SPACING} max={MAX_PATTERN_SPACING} step={0.05}
            valueText={`${Math.round(spacing * 100)}%`} onInput={onSpacingChange} />
        </div>
      {/if}
    </section>

    <div class="divider"></div>
    <section class="rows">
      {@render toggle(
        $t("workspace.keepProportions"),
        constraintHint,
        constrain,
        false,
        () => onConstrainChange(!constrain),
      )}
      {@render toggle(
        $t("workspace.fillClosedShapes"),
        $t("workspace.aQuietTintUsingTheStrokeColour"),
        fill,
        !isClosed,
        () => onFillChange(!fill),
      )}
      {@render toggle(
        $t("workspace.drawAndHold"),
        $t("workspace.promoteDeliberatePenMarksIntoShapes"),
        drawAndHold,
        false,
        () => onDrawAndHoldChange(!drawAndHold),
      )}
      {#if drawAndHold}
        <StepSlider name="Held line minimum" value={minimumLineLengthPt} min={4} max={96} step={1} log
          valueText={$t("workspace.mm2", { "value1": formatNumber(minimumLineLengthPt * MM_PER_PT, { minimumFractionDigits: 1, maximumFractionDigits: 1 }, $displayLocale) })} label={$t("workspace.minimumHeldLineLength")}
          spokenValue={$t("workspace.millimetres", { "value1": formatNumber(minimumLineLengthPt * MM_PER_PT, { minimumFractionDigits: 1, maximumFractionDigits: 1 }, $displayLocale) })}
          onInput={(next) => onMinimumLineLengthChange?.(next)} />
      {/if}
    </section>
  </div>
</div>

<style>
  .shape-card {
    position: relative;
    display: flex;
    width: 296px;
    max-height: min(520px, calc(100vh - 200px));
    flex-direction: column;
    border: 1px solid var(--edge);
    border-radius: var(--radius-lg);
    background: var(--panel);
    box-shadow: 0 22px 54px rgb(0 0 0 / 58%);
    color: var(--text);
  }

  .arrow { position: absolute; width: 10px; height: 10px; background: var(--panel); transform: rotate(45deg); }
  .point-down .arrow { bottom: -6px; left: clamp(12px, calc(var(--arrow, 148px) - 5px), 274px); border-right: 1px solid var(--edge); border-bottom: 1px solid var(--edge); }
  .point-up .arrow { top: -6px; left: clamp(12px, calc(var(--arrow, 148px) - 5px), 274px); border-top: 1px solid var(--edge); border-left: 1px solid var(--edge); }
  .point-left .arrow { top: clamp(12px, calc(var(--arrow, 150px) - 5px), calc(100% - 22px)); left: -6px; border-bottom: 1px solid var(--edge); border-left: 1px solid var(--edge); }
  .point-right .arrow { top: clamp(12px, calc(var(--arrow, 150px) - 5px), calc(100% - 22px)); right: -6px; border-top: 1px solid var(--edge); border-right: 1px solid var(--edge); }

  header { display: flex; height: var(--control); flex: none; align-items: center; padding: 0 6px 0 12px; border-bottom: 1px solid var(--edge-soft); gap: 8px; }
  .name { flex: 1; font-size: var(--text-md); font-weight: 600; }
  .meta { color: var(--quiet); font-size: var(--text-sm); font-variant-numeric: tabular-nums; }
  .chrome { display: grid; width: var(--control-dense); height: var(--control-dense); padding: 0; border: 0; border-radius: var(--radius); background: transparent; color: var(--muted); cursor: pointer; place-items: center; }
  .chrome:hover { background: var(--wash); color: var(--text); }
  .chrome svg { width: var(--icon-dense); height: var(--icon-dense); fill: none; stroke: currentColor; stroke-linecap: round; stroke-width: var(--stroke-dense); }

  .body { display: flex; min-height: 0; flex-direction: column; padding: 12px; gap: 13px; overflow-y: auto; }
  section { display: flex; flex-direction: column; gap: 7px; }
  .section-label { color: var(--muted); font-size: var(--text-sm); }
  .preview { position: relative; display: grid; height: 64px; border-radius: var(--radius); background: var(--paper); color: #59616b; place-items: center; overflow: hidden; }
  .preview svg { position: absolute; inset: 0; width: 100%; height: 54px; }
  .preview path { stroke-linecap: round; }
  .preview span { position: absolute; right: 8px; bottom: 5px; font-size: 10px; }

  .shape-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; }
  .shape-grid button { display: flex; min-height: var(--control-touch); align-items: center; padding: 0 10px; border: 1px solid var(--edge); border-radius: var(--radius); background: transparent; color: var(--muted); cursor: pointer; gap: 8px; transition: background 140ms ease, border-color 140ms ease, color 140ms ease; }
  .shape-grid button:hover { background: var(--wash); color: var(--text); }
  .shape-grid button.current { border-color: var(--blueprint); background: rgb(76 141 240 / 14%); color: var(--text); }
  .shape-grid svg { width: var(--icon); height: var(--icon); flex: none; fill: none; stroke: currentColor; stroke-linecap: round; stroke-linejoin: round; stroke-width: var(--stroke); }
  .shape-grid svg circle { fill: currentColor; stroke: none; }
  .shape-grid button span { font-size: var(--text-md); }
  .pattern-grid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 6px; }
  .pattern-grid button { display: grid; min-height: var(--control-touch); place-items: center; padding: 6px 4px; border: 1px solid var(--edge); border-radius: var(--radius); background: transparent; color: var(--muted); cursor: pointer; gap: 2px; font: inherit; font-size: var(--text-sm); }
  .pattern-grid button:hover { background: var(--wash); color: var(--text); }
  .pattern-grid button.current { border-color: var(--blueprint); background: rgb(76 141 240 / 14%); color: var(--text); }
  .pattern-grid button:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: 1px; }
  .spacing { margin-top: 8px; }
  .pattern-grid svg { width: 100%; height: 12px; fill: none; stroke: currentColor; stroke-width: 2; stroke-linecap: round; }

  .divider { height: 1px; background: var(--edge-soft); }
  .rows { gap: 2px; }
  .row { display: flex; min-height: var(--control-touch); align-items: center; justify-content: space-between; gap: 10px; }
  .row.disabled { opacity: 0.45; }
  .row-name { display: flex; min-width: 0; flex-direction: column; font-size: var(--text-md); gap: 2px; }
  .row-name span { color: var(--quiet); font-size: var(--text-sm); }
  .toggle { position: relative; width: 34px; height: 20px; flex: none; padding: 0; border: 0; border-radius: var(--radius-pill); background: rgb(255 255 255 / 14%); cursor: pointer; }
  .toggle span { position: absolute; top: 3px; left: 3px; width: 14px; height: 14px; border-radius: 50%; background: var(--muted); transition: transform 120ms ease, background 120ms ease; }
  .toggle.on { background: var(--blueprint); }
  .toggle.on span { background: #fff; transform: translateX(14px); }

  .chrome:focus-visible,
  .shape-grid button:focus-visible,
  .toggle:focus-visible { outline: 2px solid var(--blueprint-light); outline-offset: 1px; }

  @media (prefers-reduced-motion: reduce) {
    .shape-grid button, .toggle span { transition: none; }
  }
</style>
