<script lang="ts">
  import ColorPanel from "./ColorPanel.svelte";
  import WidthPanel from "./WidthPanel.svelte";
  import type { PaletteDock } from "./palette";
  import {
    MM_PER_PT,
    PEN_TYPES,
    colorName,
    penType,
    type PenPreset,
    type PenTypeId,
  } from "../settings";

  /**
   * Everything one tool can be set to, in a single card anchored to the tile you tapped.
   *
   * The pocket on the rail is where a width or colour is *chosen*; this card is where one is
   * changed or removed. Splitting the two that way is what let the second-tap-to-edit gesture go:
   * you no longer have to adopt a colour in order to delete it.
   *
   * It replaces a second full-height column on the palette plus three satellite popouts. The
   * palette used to grow in two directions and still could not hold a pen's settings; now the
   * rail is a fixed width and the options live off it. The width and colour editors open *inside*
   * this card rather than beside it, because a popout hanging off a popout is the clutter this
   * card exists to remove.
   *
   * Adding a setting is meant to cost one line. The rows below are snippets — `toggleRow`,
   * `choiceRow`, `sliderRow` — so a new switch is a `{@render toggleRow(...)}` and nothing else.
   * See AGENT_DOC §3.9 for the settings queued to land here.
   */
  let {
    dock,
    initialView = "main",
    tool,
    label,
    preset,
    smoothing,
    opacity = 1,
    straighten = false,
    behindInk = true,
    widths,
    colors,
    recentColors = [],
    canAddWidth = true,
    canAddColor = true,
    canRemoveWidth = false,
    canRemoveColor = false,
    widthBounds,
    onChange,
    onSmoothingChange,
    onOpacityChange,
    onStraightenChange,
    onBehindInkChange,
    onCommitWidth,
    onRemoveWidth,
    onCommitColor,
    onPreviewColor,
    onRemoveColor,
    onClose,
  }: {
    /** Which edge the rail is on, so the pointer faces back toward the tile that opened this. */
    dock: PaletteDock;
    /** `add-colour` opens straight into the picker, for the pocket's `+`. */
    initialView?: "main" | "add-colour";
    tool: "pen" | "highlighter";
    label: string;
    preset: PenPreset;
    smoothing: number;
    opacity?: number;
    straighten?: boolean;
    behindInk?: boolean;
    widths: number[];
    colors: string[];
    recentColors?: string[];
    canAddWidth?: boolean;
    canAddColor?: boolean;
    canRemoveWidth?: boolean;
    canRemoveColor?: boolean;
    widthBounds: { minimum: number; maximum: number };
    onChange: (preset: PenPreset) => void;
    onSmoothingChange: (smoothing: number) => void;
    onOpacityChange?: (opacity: number) => void;
    onStraightenChange?: (straighten: boolean) => void;
    onBehindInkChange?: (behindInk: boolean) => void;
    /** Write a width back to the chip at `index`, or append when `index` is -1. */
    onCommitWidth: (index: number, widthPt: number) => void;
    onRemoveWidth: (index: number) => void;
    onCommitColor: (index: number, color: string) => void;
    onPreviewColor: (index: number, color: string) => void;
    onRemoveColor: (index: number) => void;
    onClose: () => void;
  } = $props();

  type View = { kind: "main" } | { kind: "width"; index: number } | { kind: "color"; index: number };

  const SMOOTHING_STEPS = [
    { label: "None", value: 0 },
    { label: "Low", value: 0.15 },
    { label: "Med", value: 0.35 },
    { label: "High", value: 0.6 },
  ];

  /// Drawn rather than named: at 20px a nib reads faster as its own silhouette than as a word.
  const NIB_ICONS: Record<PenTypeId, string> = {
    fountain: "M15.5 3.5l5 5-9.5 9.5-5.5 1.5 1.5-5.5 9.5-9.5z",
    ballpoint: "M14 4l6 6-9 9-5 1 1-5 7-11z",
    pencil: "M4 20l3.5-1 12-12-2.5-2.5-12 12L4 20zM14 7l3 3",
    marker: "M7 14l7-9 5 4-6 9H8l-1-4z",
    technical: "M12 3.5 15 10v8.5H9V10zM10.5 13.5h3",
  };

  let view = $state<View>({ kind: "main" });

  /// Seeded from the caller rather than fixed at mount, so the pocket's `+` still lands on the
  /// picker when the card is already open on something else.
  $effect(() => {
    if (initialView === "add-colour") view = { kind: "color", index: -1 };
  });

  const mm = (pt: number) => `${(pt * MM_PER_PT).toFixed(2)} mm`;
  const activeWidthIndex = $derived(
    widths.findIndex((width) => Math.abs(width - preset.widthPt) < 0.001),
  );
  const activeColorIndex = $derived(
    colors.findIndex((color) => color.toLowerCase() === preset.color.toLowerCase()),
  );
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

  function back() {
    view = { kind: "main" };
  }

  function keydown(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    event.stopPropagation();
    if (view.kind === "main") onClose();
    else back();
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

{#snippet sliderRow(name: string, value: number, min: number, max: number, step: number, onInput: (value: number) => void)}
  <div class="slider">
    {@render sectionLabel(name, `${Math.round(value * 100)}%`)}
    <input
      type="range"
      {min}
      {max}
      {step}
      {value}
      aria-label={name}
      oninput={(event) => onInput(Number(event.currentTarget.value))}
    />
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
    {#if view.kind !== "main"}
      <button type="button" class="chrome" aria-label="Back to {label} settings" onclick={back}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14 6l-6 6 6 6" /></svg>
      </button>
    {/if}
    <span class="name">{view.kind === "width" ? "Width" : view.kind === "color" ? "Colour" : label}</span>
    {#if view.kind === "main"}<span class="meta">{mm(preset.widthPt)}</span>{/if}
    <button type="button" class="chrome" aria-label="Close settings" onclick={onClose}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
    </button>
  </header>

  {#if view.kind === "width"}
    <WidthPanel
      embedded
      widthPt={view.index === -1 ? preset.widthPt : widths[view.index]}
      kind={tool}
      minimumMm={widthBounds.minimum}
      maximumMm={widthBounds.maximum}
      canRemove={view.index !== -1 && canRemoveWidth}
      onCommit={(next) => {
        onCommitWidth(view.kind === "width" ? view.index : -1, next);
        back();
      }}
      onRemove={() => {
        if (view.kind === "width" && view.index !== -1) onRemoveWidth(view.index);
        back();
      }}
      onClose={back}
    />
  {:else if view.kind === "color"}
    <ColorPanel
      embedded
      value={view.index === -1 ? preset.color : colors[view.index]}
      recent={recentColors}
      mode={view.index === -1 ? "add" : "edit"}
      canRemove={view.index !== -1 && canRemoveColor}
      onPick={(color) => {
        // Opened by the pocket's `+`, the card was only ever a means to add a colour, so it
        // gets out of the way once there is one rather than parking itself over the page.
        const fromPocket = initialView === "add-colour" && view.kind === "color" && view.index === -1;
        onCommitColor(view.kind === "color" ? view.index : -1, color);
        if (fromPocket) onClose();
        else back();
      }}
      onChange={(color) => onPreviewColor(view.kind === "color" ? view.index : -1, color)}
      onRemove={() => {
        if (view.kind === "color" && view.index !== -1) onRemoveColor(view.index);
        back();
      }}
      onClose={back}
    />
  {:else}
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
            opacity={tool === "highlighter" ? opacity : 1}
          />
        </svg>
      </div>

      {#if tool === "pen"}
        <section>
          {@render sectionLabel("Nib")}
          <div class="nibs">
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
                <svg viewBox="0 0 24 24" aria-hidden="true"><path d={NIB_ICONS[type.id]} /></svg>
              </button>
            {/each}
          </div>
        </section>
      {/if}

      <section>
        {@render sectionLabel("Width", mm(preset.widthPt), "tap to edit")}
        <div class="chips">
          {#each widths as width, index (width)}
            <button
              type="button"
              class="chip"
              class:current={index === activeWidthIndex}
              title={`${mm(width)} — tap to change or remove`}
              aria-label={index === activeWidthIndex ? `Edit ${mm(width)}, in use` : `Edit ${mm(width)}`}
              onclick={() => (view = { kind: "width", index })}
            >
              <span
                class="rule"
                style:height={`${Math.max(1, Math.min(width * (tool === "highlighter" ? 2.2 : 1.4), 9))}px`}
                style:background={tool === "highlighter" ? `${preset.color}99` : "var(--text)"}
              ></span>
            </button>
          {/each}
          {#if canAddWidth}
            <button
              type="button"
              class="chip open"
              aria-label="Set an exact width"
              title="Set an exact width"
              onclick={() => (view = { kind: "width", index: -1 })}
            >mm</button>
          {/if}
        </div>
      </section>

      <section>
        {@render sectionLabel("Colour", colorName(preset.color), "tap to edit")}
        <div class="swatches">
          {#each colors as color, index (color)}
            <button
              type="button"
              class="swatch"
              class:current={index === activeColorIndex}
              style:background={color}
              style:opacity={tool === "highlighter" ? opacity : 1}
              aria-label={index === activeColorIndex
                ? `Edit ${colorName(color)}, in use`
                : `Edit ${colorName(color)}`}
              title={`${colorName(color)} — tap to change or remove`}
              onclick={() => (view = { kind: "color", index })}
            ></button>
          {/each}
          {#if canAddColor}
            <button
              type="button"
              class="swatch open"
              aria-label="Add a colour"
              title="Add a colour"
              onclick={() => (view = { kind: "color", index: -1 })}
            >
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 6v12M6 12h12" /></svg>
            </button>
          {/if}
        </div>
      </section>

      <div class="divider"></div>

      <!-- One line per setting. New switches land here; see AGENT_DOC §3.9. -->
      <section class="rows">
        {#if tool === "pen"}
          {@render toggleRow("Pressure", preset.pressure, () => onChange({ ...preset, pressure: !preset.pressure }), "Stylus force varies the width")}
        {:else}
          {@render sliderRow("Opacity", opacity, 0.1, 1, 0.05, (next) => onOpacityChange?.(next))}
          {@render toggleRow("Straighten to lines", straighten, () => onStraightenChange?.(!straighten))}
          {@render toggleRow("Draw behind ink", behindInk, () => onBehindInkChange?.(!behindInk))}
        {/if}
        {@render choiceRow("Stabiliser", SMOOTHING_STEPS, nearestSmoothing.value, onSmoothingChange)}
      </section>
    </div>
  {/if}
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
    font-size: var(--text-sm);
    letter-spacing: 0.09em;
    text-overflow: ellipsis;
    text-transform: uppercase;
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

  .eyebrow {
    color: var(--quiet);
    font-size: var(--text-xs);
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .eyebrow .hint {
    color: var(--quiet);
    text-transform: none;
    letter-spacing: 0;
  }

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

  .nibs { display: grid; grid-template-columns: repeat(5, 1fr); gap: 5px; }

  .nib {
    display: grid;
    height: 40px;
    border: 1px solid var(--edge);
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
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

  .chips { display: flex; gap: 5px; }

  .chip {
    display: grid;
    height: 38px;
    flex: 1;
    border: 1px solid var(--edge);
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    place-items: center;
  }

  .chip .rule { width: 22px; border-radius: 4px; }

  /* Dashed, because it opens an editor rather than setting a value — the same distinction the
     palette already draws between a swatch and the control that makes one. */
  .chip.open,
  .swatch.open {
    flex: none;
    border-style: dashed;
    font-size: var(--text-sm);
  }

  .chip.open { width: 38px; }

  .nib:hover, .chip:hover, .swatch:hover { background: var(--wash); }

  .nib.current, .chip.current {
    border-color: var(--blueprint);
    background: rgb(76 141 240 / 14%);
    color: var(--text);
  }

  .swatches { display: grid; grid-template-columns: repeat(7, 1fr); gap: 6px; }

  .swatch {
    display: grid;
    width: 100%;
    aspect-ratio: 1;
    border: 1px solid var(--edge);
    border-radius: 50%;
    color: var(--quiet);
    cursor: pointer;
    place-items: center;
  }

  .swatch.current { outline: 1.5px solid var(--blueprint); outline-offset: 2px; }
  .swatch.open { background: transparent; }
  .swatch.open svg {
    width: 12px;
    height: 12px;
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-width: var(--stroke-dense);
  }

  .divider { height: 1px; flex: none; background: var(--edge-soft); }

  .rows { gap: 2px; }

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

  .slider { display: flex; flex-direction: column; gap: 6px; }
  .slider input { width: 100%; accent-color: var(--blueprint); }

  .nib:focus-visible,
  .chip:focus-visible,
  .swatch:focus-visible,
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
