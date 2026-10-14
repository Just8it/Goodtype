<script lang="ts">
  import { centredDashArray } from "./dash";
  import { colorName, MM_PER_PT, type EraserSize } from "../settings";
  import type { StrokePattern } from "../model";
  import { paletteWidthIndex } from "./palette";

  /**
   * The quick controls at the foot of the rail: sizes and favourite colours for the tool being
   * held, always visible, one tap each.
   *
   * It replaces a second full-height column that used to open beside the tools and push the
   * palette sideways. Two 2×2 grids cost the rail about ninety pixels of height and nothing at
   * all in width, which is what lets the rail stay one fixed column in every state.
   *
   * Colours grow with the writer: three by default, a `+` in the next free cell, and at six —
   * the swatch ceiling — the last colour takes the `+`'s place rather than starting a row for it.
   * Press the selected width or colour again to edit it in place. Nib settings stay on the tool.
   */
  let {
    kind,
    horizontal = false,
    widths = [],
    activeWidth = 0,
    colors = [],
    activeColor = "",
    opacity = 1,
    eraserSizes = [],
    eraserSize,
    onPickWidth,
    onPickColor,
    onEditWidth,
    onEditColor,
    pattern = "solid",
    patternOf,
    expandedEditor = null,
    addingColor = false,
    onPickEraserSize,
    canAddColor = true,
    onAddColor,
    onOpenCard,
  }: {
    kind: "ink" | "eraser";
    horizontal?: boolean;
    widths?: number[];
    activeWidth?: number;
    colors?: string[];
    activeColor?: string;
    /** Highlighter chips are drawn at the ink's real translucency, not as solid dots. */
    opacity?: number;
    eraserSizes?: { id: EraserSize; label: string; diameter: number }[];
    eraserSize?: EraserSize;
    onPickWidth?: (widthPt: number, tile: HTMLElement) => void;
    onPickColor?: (color: string, tile: HTMLElement) => void;
    onEditWidth?: (index: number, tile: HTMLElement) => void;
    onEditColor?: (index: number, tile: HTMLElement) => void;
    pattern?: StrokePattern;
    /** Each chip's own pattern; the active pattern stands in for a custom width off the row. */
    patternOf?: (widthPt: number) => StrokePattern;
    expandedEditor?: "width" | "color" | null;
    addingColor?: boolean;
    onPickEraserSize?: (size: EraserSize) => void;
    /** False at the swatch ceiling, where the last colour stands where the `+` would have. */
    canAddColor?: boolean;
    /** Opens a colour picker attached to the add button. */
    onAddColor?: (tile: HTMLElement) => void;
    /** Absent for tools whose card holds nothing the pocket does not already show. */
    onOpenCard?: () => void;
  } = $props();

  const mm = (pt: number) => `${(pt * MM_PER_PT).toFixed(2)} mm`;

  // Custom nib widths get their own live control rather than falsely selecting a nearby preset.
  const activeWidthIndex = $derived(paletteWidthIndex(widths, activeWidth));
  const visibleWidths = $derived(
    activeWidthIndex < 0 && Number.isFinite(activeWidth) && activeWidth > 0
      ? [...widths, activeWidth]
      : widths,
  );
  const activeColorIndex = $derived(colors.findIndex((color) => color.toLowerCase() === activeColor.toLowerCase()));
  const visibleColors = $derived(activeColor && activeColorIndex < 0 ? [...colors, activeColor] : colors);
  const chipPattern = (width: number, index: number): StrokePattern =>
    index < widths.length && patternOf ? patternOf(width) : pattern;
  /** The chip rule is a fixed 18 units long, so its centred pattern needs no measuring. */
  const CHIP_RULE_LENGTH = 18;
  const chipDash = (chip: StrokePattern) =>
    chip === "dashed"
      ? centredDashArray(CHIP_RULE_LENGTH, 5, 4)
      : chip === "dotted"
        ? centredDashArray(CHIP_RULE_LENGTH, 0.01, 5)
        : null;

</script>

<div class:horizontal class="pocket">
  {#if kind === "ink"}
    <div class="grid widths" role="group" aria-label="Stroke size">
      {#each visibleWidths as width, index (width)}
        {@const selected = Math.abs(width - activeWidth) < 0.001}
        {@const linePattern = chipPattern(width, index)}
        <button
          type="button"
          class:current={selected}
          aria-pressed={selected}
          aria-expanded={selected && onEditWidth ? expandedEditor === "width" : undefined}
          aria-label={`${mm(width)}, ${linePattern}${selected ? " — edit stroke" : ""}`}
          title={`${mm(width)} · ${linePattern}${selected ? " — press again to edit stroke" : ""}`}
          onclick={(event) => selected && onEditWidth
            ? onEditWidth(index < widths.length ? index : -1, event.currentTarget)
            : onPickWidth?.(width, event.currentTarget)}
        >
          <svg class="rule" viewBox="0 0 24 12" aria-hidden="true">
            <path d="M3 6H21" fill="none" stroke="currentColor" stroke-linecap="round"
              stroke-width={Math.max(1, Math.min(width * 1.4, linePattern === "solid" ? 8 : 3))}
              stroke-dasharray={chipDash(linePattern)?.dasharray}
              stroke-dashoffset={chipDash(linePattern)?.dashoffset} />
          </svg>
        </button>
      {/each}
    </div>

    <div class="grid colors" role="group" aria-label="Ink colour">
      {#each visibleColors as color, index}
        {@const selected = color.toLowerCase() === activeColor.toLowerCase()}
        <button
          type="button"
          class="dot"
          class:current={selected}
          aria-pressed={selected}
          aria-expanded={selected && onEditColor ? expandedEditor === "color" : undefined}
          aria-label={`${colorName(color)}${selected ? " — edit colour" : ""}`}
          title={`${colorName(color)}${selected ? " — press again to edit colour" : ""}`}
          onclick={(event) => selected && onEditColor
            ? onEditColor(index < colors.length ? index : -1, event.currentTarget)
            : onPickColor?.(color, event.currentTarget)}
        ><span class="swatch" style:background={color} style:opacity></span></button>
      {/each}
      {#if canAddColor && onAddColor}
        <button type="button" class="dot add" aria-label="Add a colour" title="Add a colour" aria-expanded={addingColor} onclick={(event) => onAddColor?.(event.currentTarget)}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 6v12M6 12h12" /></svg>
        </button>
      {/if}
    </div>
  {:else}
    <div class="grid erasers" role="radiogroup" aria-label="Eraser size">
      {#each eraserSizes as option (option.id)}
        <button
          type="button"
          role="radio"
          aria-checked={eraserSize === option.id}
          aria-label={`${option.label} eraser`}
          title={option.label}
          class:current={eraserSize === option.id}
          onclick={() => onPickEraserSize?.(option.id)}
        >
          <span
            class="ring"
            style:width={`${option.diameter}px`}
            style:height={`${option.diameter}px`}
          ></span>
        </button>
      {/each}
    </div>
  {/if}

  {#if onOpenCard}
    <button type="button" class="more" aria-label="All settings for this tool" title="All settings" onclick={onOpenCard}>
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M4 8h8M17 8h3M4 16h3M12 16h8" />
        <circle cx="14.5" cy="8" r="2.2" />
        <circle cx="9.5" cy="16" r="2.2" />
      </svg>
    </button>
  {/if}
</div>

<style>
  .pocket {
    display: flex;
    flex-direction: column;
    padding-top: 7px;
    margin-top: 3px;
    border-top: 1px solid var(--edge);
    gap: 6px;
  }

  /* Docked along an edge the rail is a row, so the pocket runs beside the tools instead of
     under them and the border moves to the leading side. */
  .pocket.horizontal {
    flex-direction: row;
    align-items: center;
    padding-top: 0;
    padding-left: 7px;
    margin-top: 0;
    margin-left: 3px;
    border-top: 0;
    border-left: 1px solid var(--edge);
  }

  /* Two wide on a vertical rail, growing downward; two tall on a horizontal one, growing to the
     right. The rail's thickness is fixed either way, so the axis the pocket grows along has to
     be the one the bar is already long in — otherwise a sixth swatch bursts out of the bar. */
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 2px;
    place-items: center;
  }

  .horizontal .grid {
    grid-auto-flow: column;
    grid-template-columns: none;
    grid-template-rows: 1fr 1fr;
  }

  /* Three rings on one line, never two: they are a size ramp, and a ramp that wraps stops
     reading as one. The largest ring is 26px, so these cells are wider than the 20px chips. */
  .erasers {
    grid-auto-flow: row;
    grid-template-columns: 1fr;
  }

  .horizontal .erasers {
    grid-auto-flow: column;
    grid-template-columns: none;
    grid-template-rows: 1fr;
  }

  .erasers button {
    width: 30px;
    height: 30px;
  }

  button {
    display: grid;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 0;
    border-radius: var(--radius);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    place-items: center;
  }

  button:hover { background: var(--wash); }
  button:focus-visible { outline: 2px solid var(--blueprint); outline-offset: 1px; }

  .widths button.current, .erasers button.current {
    background: rgb(76 141 240 / 18%);
    outline: 1px solid var(--blueprint);
  }

  .rule { width: 24px; height: 12px; color: var(--text); }

  .dot {
    border-radius: 50%;
  }

  .swatch {
    width: 18px;
    height: 18px;
    border: 1px solid rgb(255 255 255 / 22%);
    border-radius: 50%;
  }

  .dot.current { outline: 1.5px solid var(--blueprint); outline-offset: 2px; }

  /* Dashed, like every other control in this app that opens an editor rather than setting a
     value. It vanishes at the ceiling, where the sixth colour stands in its cell. */
  .dot.add {
    border: 1px dashed rgb(255 255 255 / 30%);
    background: transparent;
  }

  .dot.add svg {
    width: 10px;
    height: 10px;
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-width: var(--stroke-dense);
  }

  .ring {
    box-sizing: border-box;
    border: 1.4px solid currentColor;
    border-radius: 50%;
  }

  /* Full width of the rail: it is the way out of the pocket, not one more chip in it. */
  .more {
    width: 100%;
    height: 22px;
    background: rgb(255 255 255 / 5%);
  }

  .horizontal .more { width: 22px; height: 100%; min-height: 22px; }

  .more:hover { background: var(--wash); color: var(--text); }
  .more svg {
    width: 15px;
    height: 15px;
    fill: none;
    stroke: currentColor;
    stroke-linecap: round;
    stroke-width: var(--stroke-dense);
  }
</style>
