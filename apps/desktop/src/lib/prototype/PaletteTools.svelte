<script lang="ts">
  import { centredDash } from "./dash";
  import type { StrokePattern } from "../model";
  import type { AppSettings, PenPreset } from "../settings";
  import {
    ERASER_PATH,
    HIGHLIGHTER_PATH,
    INSTRUMENT_TRANSFORM,
    NIB_PATHS,
  } from "./instruments";
  import { visiblePaletteTools, type PaletteCommand } from "./palette";

  type Props = {
    settings: AppSettings;
    activeCommands: readonly PaletteCommand[];
    expandedCommand: PaletteCommand | null;
    horizontal: boolean;
    /** The tile comes back with the command so a card can be anchored to what was tapped. */
    onActivate: (command: PaletteCommand, tile: HTMLElement) => void;
  };

  let { settings, activeCommands, expandedCommand, horizontal, onActivate }: Props = $props();

  // The mark under each instrument is the line it will actually lay down: the pen's colour,
  // thickness and pattern, the highlighter's translucent band. That, more than the silhouette,
  // is what tells a pen from a highlighter at a glance.
  const lineWidth = (preset: PenPreset) => Math.max(1.2, Math.min(preset.widthPt * 0.9, 3));
  const bandHeight = (preset: PenPreset) => Math.max(3, Math.min(preset.widthPt * 0.9, 5));
  const dasharray = (pattern: StrokePattern) =>
    pattern === "dashed" ? [3.2, 2.2] : pattern === "dotted" ? [0.01, 2.6] : null;
  const ERASER_RADIUS = { small: 1.4, medium: 2, large: 2.7 } as const;
</script>

{#snippet pen(preset: PenPreset)}
  <svg viewBox="0 0 24 24" aria-hidden="true">
    <path class="ink-halo" d="M4.5 21.2h15" stroke-width={lineWidth(preset) + 1.1} use:centredDash={dasharray(preset.pattern)} />
    <path class="ink" d="M4.5 21.2h15" stroke={preset.color} stroke-width={lineWidth(preset)} use:centredDash={dasharray(preset.pattern)} />
    <path d={NIB_PATHS[preset.type]} transform={INSTRUMENT_TRANSFORM} />
  </svg>
{/snippet}

{#each visiblePaletteTools(settings.hiddenTools) as { tool: definition, dividerBefore } (definition.id)}
  {#if dividerBefore}
    <span class:horizontal class="palette-divider"></span>
  {/if}
  {@const active = activeCommands.includes(definition.id)}
  <button
    class:active
    class="tool-tile"
    type="button"
    aria-label={definition.label}
    aria-pressed={definition.action ? undefined : active}
    aria-expanded={definition.context && active ? expandedCommand === definition.id : undefined}
    title={definition.title}
    onclick={(event) => onActivate(definition.id, event.currentTarget)}
  >
    {#if definition.id === "pen-1"}
      {@render pen(settings.penPresets[0])}
    {:else if definition.id === "pen-2"}
      {@render pen(settings.penPresets[1])}
    {:else if definition.id === "highlighter"}
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <rect class="band" x="3.5" y={22.2 - bandHeight(settings.highlighter)} width="17" height={bandHeight(settings.highlighter)} rx="1"
          fill={settings.highlighter.color} opacity={Math.max(settings.highlighterOpacity, 0.45)} />
        <path d={HIGHLIGHTER_PATH} transform={INSTRUMENT_TRANSFORM} />
      </svg>
    {:else if definition.id === "eraser"}
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d={ERASER_PATH} transform={INSTRUMENT_TRANSFORM} />
        <circle class="eraser-size" cx="19" cy="20" r={ERASER_RADIUS[settings.eraserSize]} />
      </svg>
    {:else if definition.id === "shape"}
      {#if settings.shapeKind === "line"}
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 18 19 6"></path><circle cx="5" cy="18" r="1.5"></circle><circle cx="19" cy="6" r="1.5"></circle></svg>
      {:else if settings.shapeKind === "rectangle"}
        <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="4.5" y="5.5" width="15" height="13" rx="1.5"></rect></svg>
      {:else if settings.shapeKind === "ellipse"}
        <svg viewBox="0 0 24 24" aria-hidden="true"><ellipse cx="12" cy="12" rx="8" ry="6.5"></ellipse></svg>
      {:else}
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 17C7 5 13 5 20 14"></path><circle cx="4" cy="17" r="1.4"></circle><circle cx="20" cy="14" r="1.4"></circle></svg>
      {/if}
    {:else if definition.id === "lasso"}
      <svg viewBox="0 0 24 24" aria-hidden="true"><ellipse cx="12" cy="10" rx="8" ry="6" stroke-dasharray="3 2.6"></ellipse><path d="M9 16c0 2 1 4 3 4"></path><circle cx="12" cy="20" r="1.4"></circle></svg>
    {:else if definition.id === "page-text"}
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 3.5h14v17H5z"></path><path d="M8 8h8M8 11h8M8 14h6"></path></svg>
    {:else}
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7.5V6h11v1.5M9.5 6v13M7 19h5"></path><path d="M18.5 4.5v6M15.5 7.5h6"></path></svg>
    {/if}
  </button>
{/each}

<style>
  .tool-tile {
    position: relative;
    display: grid;
    width: var(--control-touch);
    height: var(--control-touch);
    flex: none;
    padding: 0;
    border: 0;
    border-radius: var(--radius-lg);
    background: transparent;
    color: #c4cad2;
    cursor: pointer;
    place-items: center;
  }
  .tool-tile:hover { background: var(--wash); }
  .tool-tile.active { background: var(--blueprint); color: #fff; }
  .tool-tile:focus-visible { outline: 2px solid var(--blueprint); outline-offset: 1px; }
  .tool-tile svg {
    /* A touch larger than chrome icons: the tip is the detail that names the instrument. */
    width: calc(var(--icon) + 4px);
    height: calc(var(--icon) + 4px);
    fill: none;
    stroke: currentColor;
    stroke-width: var(--stroke);
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .tool-tile svg circle { fill: currentColor; stroke: none; }
  /* Ink sits on a pale halo so graphite stays visible on the dark rail and on the blue tile. */
  .tool-tile svg .ink-halo { stroke: rgb(255 255 255 / 62%); }
  .tool-tile svg .band { stroke: none; }
  .tool-tile svg circle.eraser-size { fill: none; stroke: currentColor; stroke-width: 1.2; }
  .palette-divider { width: 26px; height: 1px; margin: 1px 0; background: var(--edge); }
  .palette-divider.horizontal { width: 1px; height: 26px; margin: 0 3px; }
</style>
