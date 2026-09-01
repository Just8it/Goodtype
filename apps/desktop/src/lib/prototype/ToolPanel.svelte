<script lang="ts">
  import { PEN_TYPES, penType, type PenPreset, type PenTypeId } from "../settings";

  // Quick settings for one tool slot, opened by double-pressing its tile. The two pen tiles are
  // assignable slots rather than fixed pens: choose a nib from the library, then tune it.
  let {
    preset,
    kind = "pen",
    label,
    smoothing,
    opacity = 1,
    straighten = false,
    behindInk = true,
    onChange,
    onSmoothingChange,
    onOpacityChange,
    onStraightenChange,
    onBehindInkChange,
    onClose,
  }: {
    preset: PenPreset;
    kind?: "pen" | "highlighter";
    label: string;
    smoothing: number;
    opacity?: number;
    straighten?: boolean;
    behindInk?: boolean;
    onChange: (preset: PenPreset) => void;
    onSmoothingChange: (smoothing: number) => void;
    onOpacityChange?: (opacity: number) => void;
    onStraightenChange?: (straighten: boolean) => void;
    onBehindInkChange?: (behindInk: boolean) => void;
    onClose: () => void;
  } = $props();

  const SMOOTHING_STEPS = [
    { label: "None", value: 0 },
    { label: "Low", value: 0.15 },
    { label: "Medium", value: 0.35 },
    { label: "High", value: 0.6 },
  ];

  const nearestSmoothing = $derived(
    SMOOTHING_STEPS.reduce((best, step) =>
      Math.abs(step.value - smoothing) < Math.abs(best.value - smoothing) ? step : best,
    ),
  );

  /// Switching nib adopts that nib's character; the colour is the writer's and is kept.
  function chooseType(id: PenTypeId) {
    const type = penType(id);
    onChange({ ...preset, type: id, pressure: type.pressure, widthPt: type.widthPt });
    onSmoothingChange(type.smoothing);
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.stopPropagation();
      onClose();
    }
  }
</script>

<div class="tool-panel" role="dialog" tabindex="-1" aria-label={`${label} settings`} onkeydown={keydown}>
  <div class="head">
    <span class="name">{label}</span>
    <span class="value">{preset.widthPt.toFixed(2)} pt</span>
  </div>

  <!-- A stroke drawn with the current settings, so the choice is visible rather than numeric. -->
  <div class="preview">
    <svg viewBox="0 0 220 46" aria-hidden="true">
      <path
        d="M10 30 C 55 -4, 75 40, 110 24 S 180 6, 210 18"
        fill="none"
        stroke={preset.color}
        stroke-width={preset.widthPt * (kind === "highlighter" ? 3 : 1.6)}
        stroke-linecap="round"
        opacity={kind === "highlighter" ? opacity : 1}
      />
    </svg>
  </div>

  {#if kind === "pen"}
    <div class="label">Nib</div>
    <div class="types">
      {#each PEN_TYPES as type (type.id)}
        <button
          type="button"
          class="type"
          class:current={preset.type === type.id}
          aria-pressed={preset.type === type.id}
          title={type.description}
          onclick={() => chooseType(type.id)}
        >
          <span class="type-label">{type.label}</span>
          <span class="type-note">{type.description}</span>
        </button>
      {/each}
    </div>

    <div class="switch">
      <div>
        <div class="label plain">Pressure response</div>
        <div class="note">Stylus force varies the stroke width</div>
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={preset.pressure}
        aria-label="Pressure response"
        class:on={preset.pressure}
        class="toggle"
        onclick={() => onChange({ ...preset, pressure: !preset.pressure })}
      ><span></span></button>
    </div>
  {:else}
    <div class="label">Opacity</div>
    <input
      type="range"
      min="0.1"
      max="1"
      step="0.05"
      value={opacity}
      aria-label="Highlighter opacity"
      oninput={(event) => onOpacityChange?.(Number(event.currentTarget.value))}
    />

    <div class="switch">
      <div class="label plain">Straighten to lines</div>
      <button
        type="button"
        role="switch"
        aria-checked={straighten}
        aria-label="Straighten to lines"
        class:on={straighten}
        class="toggle"
        onclick={() => onStraightenChange?.(!straighten)}
      ><span></span></button>
    </div>
    <div class="switch">
      <div class="label plain">Draw behind ink</div>
      <button
        type="button"
        role="switch"
        aria-checked={behindInk}
        aria-label="Draw behind ink"
        class:on={behindInk}
        class="toggle"
        onclick={() => onBehindInkChange?.(!behindInk)}
      ><span></span></button>
    </div>
  {/if}

  <div class="label">Smoothing</div>
  <div class="steps" role="group" aria-label="Smoothing">
    {#each SMOOTHING_STEPS as step (step.label)}
      <button
        type="button"
        class="step"
        class:current={nearestSmoothing.label === step.label}
        aria-pressed={nearestSmoothing.label === step.label}
        onclick={() => onSmoothingChange(step.value)}
      >{step.label}</button>
    {/each}
  </div>
</div>

<style>
  .tool-panel {
    display: flex;
    width: 246px;
    flex-direction: column;
    gap: 7px;
    padding: 12px;
    border: 1px solid var(--edge);
    border-radius: var(--radius-lg);
    background: #23272f;
    box-shadow: 0 18px 44px rgb(0 0 0 / 55%);
  }

  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
  }

  .name {
    color: #e9ebee;
    font-size: var(--text-md);
  }

  .value {
    color: #aeb5be;
    font: 11px/1 "Cascadia Mono", Consolas, monospace;
  }

  .preview {
    padding: 4px 6px;
    border-radius: var(--radius);
    background: #fcfcfa;
  }

  .preview svg {
    display: block;
    width: 100%;
    height: 44px;
  }

  .label {
    color: #6a727c;
    font-size: var(--text-xs);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .label.plain {
    color: #e9ebee;
    font-size: var(--text-md);
    letter-spacing: 0;
    text-transform: none;
  }

  .note {
    color: #6a727c;
    font-size: var(--text-xs);
  }

  .types {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .type {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 1px;
    padding: 6px 8px;
    border: 1px solid transparent;
    border-radius: var(--radius);
    background: transparent;
    color: #e9ebee;
    text-align: left;
    cursor: pointer;
  }

  .type:hover {
    background: var(--wash);
  }

  .type.current {
    border-color: rgb(76 141 240 / 60%);
    background: rgb(76 141 240 / 16%);
  }

  .type-label {
    font-size: var(--text-md);
  }

  .type-note {
    color: #6a727c;
    font-size: var(--text-xs);
  }

  .switch {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }

  .toggle {
    width: 34px;
    height: 19px;
    flex: none;
    padding: 2px;
    border: 0;
    border-radius: var(--radius-lg);
    background: #3a414c;
    cursor: pointer;
  }

  .toggle span {
    display: block;
    width: 15px;
    height: 15px;
    border-radius: 50%;
    background: #aeb5be;
    transition: transform 120ms ease;
  }

  .toggle.on {
    background: #4c8df0;
  }

  .toggle.on span {
    background: #fff;
    transform: translateX(15px);
  }

  .steps {
    display: grid;
    gap: 3px;
    grid-template-columns: repeat(4, 1fr);
  }

  .step {
    padding: 5px 0;
    border: 1px solid rgb(255 255 255 / 10%);
    border-radius: var(--radius);
    background: transparent;
    color: #aeb5be;
    font-size: var(--text-xs);
    cursor: pointer;
  }

  .step.current {
    border-color: rgb(76 141 240 / 60%);
    background: rgb(76 141 240 / 16%);
    color: #e9ebee;
  }

  input[type="range"] {
    width: 100%;
    accent-color: #4c8df0;
  }

  button:focus-visible,
  input:focus-visible {
    outline: 2px solid #7fb0f7;
    outline-offset: 1px;
  }
</style>
