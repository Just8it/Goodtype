<script lang="ts">
  import { centredDash } from "./dash";
  import type { PaletteDock } from "./palette";
  import type { StrokePattern } from "../model";
  import { patternDashArray } from "../shape/geometry";
  import { NIB_PATHS } from "./instruments";
  import StepSlider from "./StepSlider.svelte";
  import {
    MM_PER_PT,
    PEN_TYPES,
    penType,
    type PenPreset,
    type PenTypeId,
  } from "../settings";

  // Tool behaviour lives here; width, pattern and colour are edited at their rail controls.
  let {
    dock,
    tool,
    label,
    preset,
    smoothing,
    opacity = 1,
    straighten = false,
    behindInk = true,
    taperEnds = false,
    onChange,
    onSmoothingChange,
    onOpacityChange,
    onStraightenChange,
    onBehindInkChange,
    onTaperEndsChange,
    onClose,
  }: {
    /** Which edge the rail is on, so the pointer faces back toward the tile that opened this. */
    dock: PaletteDock;
    tool: "pen" | "highlighter";
    label: string;
    preset: PenPreset;
    smoothing: number;
    opacity?: number;
    straighten?: boolean;
    behindInk?: boolean;
    taperEnds?: boolean;
    onChange: (preset: PenPreset) => void;
    onSmoothingChange: (smoothing: number) => void;
    onOpacityChange?: (opacity: number) => void;
    onStraightenChange?: (straighten: boolean) => void;
    onBehindInkChange?: (behindInk: boolean) => void;
    onTaperEndsChange?: (taperEnds: boolean) => void;
    onClose: () => void;
  } = $props();

  const SMOOTHING_STEPS = [
    { label: "None", value: 0 },
    { label: "Low", value: 0.15 },
    { label: "Med", value: 0.35 },
    { label: "High", value: 0.6 },
  ];

  const mm = (pt: number) => `${(pt * MM_PER_PT).toFixed(2)} mm`;
  const nearestSmoothing = $derived(
    SMOOTHING_STEPS.reduce((best, step) =>
      Math.abs(step.value - smoothing) < Math.abs(best.value - smoothing) ? step : best,
    ),
  );

  /// Switching nib adopts that nib's character; the colour is the writer's and is kept.
  function chooseNib(id: PenTypeId) {
    const type = penType(id);
    onChange({ ...preset, type: id, pressure: type.pressure, widthPt: type.widthPt });
    onSmoothingChange(type.smoothing);
  }

  function keydown(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    event.stopPropagation();
    onClose();
  }
</script>

{#snippet sectionLabel(title: string, value?: string, hint?: string)}
  <div class="section-label">
    <span class="eyebrow">
      {title}{#if hint}<span class="hint"> · {hint}</span>{/if}
    </span>
    {#if value}<span class="value">{value}</span>{/if}
  </div>
{/snippet}

{#snippet toggleRow(name: string, checked: boolean, onToggle: () => void, note?: string)}
  <div class="row">
    <span class="row-name">
      {name}
      {#if note}<span class="row-note">{note}</span>{/if}
    </span>
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={name}
      class:on={checked}
      class="toggle"
      onclick={onToggle}
    ><span></span></button>
  </div>
{/snippet}

{#snippet choiceRow(name: string, options: { label: string; value: number }[], current: number, onPick: (value: number) => void)}
  <div class="row">
    <span class="row-name">{name}</span>
    <div class="segmented" role="group" aria-label={name}>
      {#each options as option (option.label)}
        <button
          type="button"
          class:current={option.value === current}
          aria-pressed={option.value === current}
          onclick={() => onPick(option.value)}
        >{option.label}</button>
      {/each}
    </div>
  </div>
{/snippet}

<div
  class="tool-card"
  class:point-down={dock === "bottom"}
  class:point-up={dock === "top"}
  class:point-left={dock === "left"}
  class:point-right={dock === "right"}
  role="dialog"
  tabindex="-1"
  aria-label={`${label} settings`}
  onkeydown={keydown}
>
  <span class="arrow" aria-hidden="true"></span>
  <header>
    <span class="name">{label}</span>
    <span class="meta">{mm(preset.widthPt)}</span>
    <button type="button" class="chrome" aria-label="Close settings" onclick={onClose}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
    </button>
  </header>

    <div class="body">
      <!-- The choice made visible rather than numeric: the same stroke the pen will lay down. -->
      <div class="preview">
        <svg viewBox="0 0 250 42" aria-hidden="true">
          <path
            d="M12 28 C 62 -4, 92 40, 132 22 S 208 4, 240 18"
            fill="none"
            stroke={preset.color}
            stroke-width={preset.widthPt * (tool === "highlighter" ? 3 : 1.6)}
            stroke-linecap="round"
            use:centredDash={tool === "pen"
              ? patternDashArray(preset.pattern, preset.widthPt * 1.6)
              : null}
            opacity={tool === "highlighter" ? opacity : 1}
          />
        </svg>
      </div>

      {#if tool === "pen"}
        <section>
          {@render sectionLabel("Nib")}
          <div class="nibs" role="group" aria-label="Pen nib">
            {#each PEN_TYPES as type (type.id)}
              <button
                type="button"
                class="nib"
                class:current={preset.type === type.id}
                aria-pressed={preset.type === type.id}
                aria-label={type.label}
                title={`${type.label} — ${type.description}`}
                onclick={() => chooseNib(type.id)}
              >
                <svg viewBox="0 0 24 24" aria-hidden="true"><path d={NIB_PATHS[type.id]} transform="rotate(45 12 12)" /></svg>
                <span>{type.label}</span>
              </button>
            {/each}
          </div>
        </section>
      {/if}

      <div class="divider"></div>

      <!-- One line per setting. New switches land here; see AGENT_DOC §3.9. -->
      <section class="rows">
        {#if tool === "pen"}
          {@render toggleRow("Pressure", preset.pressure, () => onChange({ ...preset, pressure: !preset.pressure }), "Stylus force varies the width")}
          {#if penType(preset.type).taper > 0}
            <StepSlider name="Maximum taper" value={preset.taperLengthWidths ?? 3} min={0} max={6} step={0.25}
              valueText={`${preset.taperLengthWidths ?? 3} × line width`} label="Maximum taper length"
              spokenValue={`${preset.taperLengthWidths ?? 3} times line width`}
              onInput={(taperLengthWidths) => onChange({ ...preset, taperLengthWidths })} />
          {/if}
        {:else}
          <StepSlider name="Opacity" value={opacity} min={0.1} max={1} step={0.05}
            valueText={`${Math.round(opacity * 100)}%`} onInput={(next) => onOpacityChange?.(next)} />
          {@render toggleRow("Straighten to lines", straighten, () => onStraightenChange?.(!straighten))}
          {@render toggleRow("Draw behind ink", behindInk, () => onBehindInkChange?.(!behindInk))}
          {@render toggleRow("Tapered ends", taperEnds, () => onTaperEndsChange?.(!taperEnds), "Off keeps the full width to both ends")}
        {/if}
        {@render choiceRow("Stabiliser", SMOOTHING_STEPS, nearestSmoothing.value, onSmoothingChange)}
      </section>
    </div>
</div>

<style>
  /* Fixed width in every state. New settings extend the body, which scrolls, so the card can
     never push itself off the canvas the way a growing palette column did. */
  .tool-card {
    position: relative;
    display: flex;
    width: 296px;
    /* Sized against the canvas it hangs over, not the window: the workspace loses the command
       strip, the status strip and the chrome's own padding before the card gets any room. */
    max-height: min(520px, calc(100vh - 200px));
    flex-direction: column;
    border: 1px solid var(--edge);
    border-radius: var(--radius-lg);
    background: var(--panel);
    box-shadow: 0 22px 54px rgb(0 0 0 / 58%);
    color: var(--text);
  }

  /* A rotated square with two of its borders kept, so the pointer reads as a corner of the card
     rather than a separate shape sitting next to it. */
  .arrow {
    position: absolute;
    width: 10px;
    height: 10px;
    background: var(--panel);
    transform: rotate(45deg);
  }

  .point-down .arrow {
    bottom: -6px;
    left: clamp(12px, calc(var(--arrow, 148px) - 5px), 274px);
    border-right: 1px solid var(--edge);
    border-bottom: 1px solid var(--edge);
  }

  .point-up .arrow {
    top: -6px;
    left: clamp(12px, calc(var(--arrow, 148px) - 5px), 274px);
    border-top: 1px solid var(--edge);
    border-left: 1px solid var(--edge);
  }

  .point-left .arrow {
    top: clamp(12px, calc(var(--arrow, 150px) - 5px), calc(100% - 22px));
    left: -6px;
    border-bottom: 1px solid var(--edge);
    border-left: 1px solid var(--edge);
  }

  .point-right .arrow {
    top: clamp(12px, calc(var(--arrow, 150px) - 5px), calc(100% - 22px));
    right: -6px;
    border-top: 1px solid var(--edge);
    border-right: 1px solid var(--edge);
  }

  header {
    position: relative;
    display: flex;
    height: var(--control);
    flex: none;
    align-items: center;
    padding: 0 6px 0 12px;
    border-bottom: 1px solid var(--edge-soft);
    gap: 8px;
  }

  .name {
    overflow: hidden;
    flex: 1;
    font-size: var(--text-md);
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    flex: none;
    color: var(--quiet);
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
  }

  .chrome {
    display: grid;
    width: var(--control-dense);
    height: var(--control-dense);
    flex: none;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    place-items: center;
  }

  .chrome:hover { background: var(--wash); color: var(--text); }
  .chrome svg {
    width: var(--icon-dense);
    height: var(--icon-dense);
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-width: var(--stroke-dense);
  }

  .body {
    display: flex;
    min-height: 0;
    flex-direction: column;
    padding: 12px;
    gap: 13px;
    overflow-y: auto;
  }

  section {
    display: flex;
    flex-direction: column;
    gap: 7px;
  }

  .section-label {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
  }

  /* Sentence case, like every other label in the palette menus. */
  .eyebrow {
    color: var(--muted);
    font-size: var(--text-sm);
  }

  .eyebrow .hint { color: var(--quiet); }

  .section-label .value {
    color: var(--muted);
    font-size: var(--text-sm);
    font-variant-numeric: tabular-nums;
  }

  .preview {
    display: grid;
    height: 56px;
    flex: none;
    border-radius: var(--radius);
    background: var(--paper);
    place-items: center;
    overflow: hidden;
  }

  .preview svg { width: 100%; height: 42px; }

  .nibs { display: grid; grid-template-columns: repeat(3, 1fr); gap: 6px; }

  .nib {
    display: flex;
    min-height: 64px;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 8px 4px;
    gap: 4px;
    border: 1px solid var(--edge);
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
    place-items: center;
  }

  .nib svg {
    width: var(--icon);
    height: var(--icon);
    fill: none;
    stroke: currentColor;
    stroke-linejoin: round;
    stroke-width: var(--stroke);
  }

  .nib:hover { background: var(--wash); }
  .nib.current { border-color: var(--blueprint); background: rgb(76 141 240 / 14%); color: var(--text); }

  .divider { height: 1px; flex: none; background: var(--edge-soft); }

  /* One setting per band, divided the same way in the pen and highlighter cards. */
  .rows { gap: 10px; }
  /* Global so a StepSlider, whose root is styled in its own component, is divided too. */
  .rows > :global(:not(:first-child)) { padding-top: 10px; border-top: 1px solid var(--edge-soft); }

  .row {
    display: flex;
    min-height: var(--control-dense);
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }

  .row-name {
    display: flex;
    min-width: 0;
    flex-direction: column;
    font-size: var(--text-md);
    gap: 2px;
  }

  .row-note { color: var(--quiet); font-size: var(--text-sm); }

  .toggle {
    position: relative;
    width: 34px;
    height: 20px;
    flex: none;
    padding: 0;
    border: 0;
    border-radius: var(--radius-pill);
    background: rgb(255 255 255 / 14%);
    cursor: pointer;
  }

  .toggle span {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--muted);
    transition: transform 120ms ease, background 120ms ease;
  }

  .toggle.on { background: var(--blueprint); }
  .toggle.on span { background: #fff; transform: translateX(14px); }

  .segmented {
    display: flex;
    flex: none;
    padding: 2px;
    border-radius: var(--radius);
    background: rgb(0 0 0 / 22%);
    gap: 2px;
  }

  .segmented button {
    padding: 4px 7px;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    font-size: var(--text-sm);
    cursor: pointer;
  }

  .segmented button.current { background: var(--blueprint); color: #10141a; font-weight: 600; }


  .nib:focus-visible,
  .toggle:focus-visible,
  .chrome:focus-visible,
  .segmented button:focus-visible {
    outline: 2px solid var(--blueprint-light);
    outline-offset: 1px;
  }

  @media (prefers-reduced-motion: reduce) {
    .toggle span { transition: none; }
  }
</style>
