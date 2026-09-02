<script lang="ts">
  import type { AppSettings } from "../settings";
  import { PALETTE_TOOLS, type PaletteCommand } from "./palette";

  type Props = {
    settings: AppSettings;
    activeCommands: readonly PaletteCommand[];
    expandedCommand: PaletteCommand | null;
    horizontal: boolean;
    /** The tile comes back with the command so a card can be anchored to what was tapped. */
    onActivate: (command: PaletteCommand, tile: HTMLElement) => void;
  };

  let { settings, activeCommands, expandedCommand, horizontal, onActivate }: Props = $props();
</script>

{#each PALETTE_TOOLS as definition (definition.id)}
  {#if definition.dividerBefore}
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
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M15.5 3.5l5 5-9.5 9.5-5.5 1.5 1.5-5.5 9.5-9.5z"></path><path d="M6.5 19.5l1.3-3.6"></path></svg>
      <span class="tool-state" aria-hidden="true" style:height={`${Math.max(2, Math.min(settings.penPresets[0].widthPt * 1.4, 6))}px`} style:background={settings.penPresets[0].color}></span>
    {:else if definition.id === "pen-2"}
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14 4l6 6-9 9-5 1 1-5 7-11z"></path><path d="M12.5 6.5l5 5"></path></svg>
      <span class="tool-state" aria-hidden="true" style:height={`${Math.max(2, Math.min(settings.penPresets[1].widthPt * 1.4, 6))}px`} style:background={settings.penPresets[1].color}></span>
    {:else if definition.id === "highlighter"}
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 15l7-9 5 4-6 9-4 1-2-5z"></path><path d="M8 20h8"></path></svg>
      <span class="tool-state" aria-hidden="true" style:height={`${Math.max(2, Math.min(settings.highlighter.widthPt * 1.2, 6))}px`} style:background={settings.highlighter.color} style:opacity={settings.highlighterOpacity}></span>
    {:else if definition.id === "eraser"}
      <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3.5" y="12" width="13" height="7" rx="1.6" transform="rotate(-38 10 15)"></rect><path d="M9 21h11"></path></svg>
      <span class="eraser-tool-state" class:medium={settings.eraserSize === "medium"} class:large={settings.eraserSize === "large"} aria-hidden="true"></span>
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
    width: var(--icon);
    height: var(--icon);
    fill: none;
    stroke: currentColor;
    stroke-width: var(--stroke);
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .tool-tile svg circle { fill: currentColor; stroke: none; }
  .tool-state {
    position: absolute;
    right: 4px;
    bottom: 4px;
    width: 10px;
    min-height: 2px;
    border-radius: var(--radius-pill);
    box-shadow: 0 0 0 1px rgb(255 255 255 / 55%);
    pointer-events: none;
  }
  .eraser-tool-state {
    position: absolute;
    right: 3px;
    bottom: 3px;
    box-sizing: border-box;
    width: 5px;
    height: 5px;
    border: 1px solid currentColor;
    border-radius: 50%;
    pointer-events: none;
  }
  .eraser-tool-state.medium { width: 7px; height: 7px; }
  .eraser-tool-state.large { width: 9px; height: 9px; }
  .palette-divider { width: 26px; height: 1px; margin: 1px 0; background: var(--edge); }
  .palette-divider.horizontal { width: 1px; height: 26px; margin: 0 3px; }
</style>
